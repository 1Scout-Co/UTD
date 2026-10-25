//! Cập nhật link (kiểu IDM "Refresh download address"): link ký hết hạn, máy chủ đòi đăng nhập lại…
//! thì người dùng đưa link mới (dán vào, hoặc tải lại từ trình duyệt). Chỉ nhận khi link mới trỏ đúng
//! file đang tải dở — cùng kích thước và ETag/Last-Modified, không có thì so vài đoạn byte đã tải — để
//! giữ phần đã tải. Không bao giờ ghép đoạn của hai file khác nhau.

use std::fs::File;
use std::path::Path;
use std::time::Duration;

use reqwest::header::CONTENT_RANGE;
use reqwest::{Method, StatusCode};
use url::Url;

use super::content::{is_html_type, is_page_name};
use super::file::read_at;
use super::http::{parse_content_range, probe, Probe, Validators};
use super::transport::Transport;
use crate::model::{DownloadTask, Segment, TaskError};

/// Mỗi đoạn mẫu dài chừng này.
const SAMPLE: u64 = 64 * 1024;
/// So tối đa chừng này đoạn mẫu (đầu, giữa, cuối phần đã tải).
const SAMPLES: usize = 3;

/// Vì sao không nhận link mới.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// Không phải link http(s) hợp lệ, hoặc header không hợp lệ.
    Invalid,
    /// Task này không đổi link được (không còn, đã xong, đang chạy, là media).
    NotAllowed,
    /// Không dò được link mới (mạng, máy chủ vẫn từ chối…).
    Unreachable(TaskError),
    /// Link mới là một trang web, không phải file.
    Page,
    /// Link mới là file khác kích thước.
    Size { old: u64, new: Option<u64> },
    /// ETag/Last-Modified hoặc byte mẫu khác: file khác.
    Changed,
    /// Không có gì để so (không validator, máy chủ không cho đọc theo đoạn): không chắc cùng file.
    Unverifiable,
}

/// Dò `url` rồi so với file đang tải dở. `Ok` = cùng file, giữ được phần đã tải.
pub(crate) async fn verify(
    transport: &Transport,
    url: &Url,
    task: &DownloadTask,
    segments: &[Segment],
    timeout: Duration,
) -> Result<Probe, LinkError> {
    let probe = probe(transport, url, timeout)
        .await
        .map_err(|failure| LinkError::Unreachable(failure.error))?;
    let page = !probe.attachment
        && !is_page_name(&task.file_name)
        && probe.content_type.as_deref().is_some_and(is_html_type);
    if page {
        return Err(LinkError::Page);
    }
    if task.downloaded_bytes == 0 {
        return Ok(probe);
    }
    if let Some(old) = task.total_bytes
        && probe.total_bytes != Some(old)
    {
        return Err(LinkError::Size {
            old,
            new: probe.total_bytes,
        });
    }
    // Không tải tiếp theo đoạn được thì phần đã tải vô ích: báo, đừng âm thầm tải lại từ 0.
    if !probe.supports_ranges {
        return Err(LinkError::Unverifiable);
    }
    let old = Validators::of(task);
    let new = Validators {
        etag: probe.etag.clone(),
        last_modified: probe.last_modified.clone(),
    };
    // Link khác nên kiểm chặt hơn lúc tải tiếp cùng link: validator cái giống cái khác thì so byte.
    match old.agrees(&new) {
        Some(true) => return Ok(probe),
        Some(false) => return Err(LinkError::Changed),
        None => {}
    }
    let windows = sample_windows(segments);
    if windows.is_empty() {
        return Err(LinkError::Unverifiable);
    }
    for (offset, length) in windows {
        if !sample_matches(transport, &probe.url, &task.temp_path, offset, length, timeout).await? {
            return Err(LinkError::Changed);
        }
    }
    Ok(probe)
}

/// Các đoạn mẫu nằm trong phần đã tải: đầu đoạn đầu tiên có byte, giữa và cuối của đoạn có nhiều byte
/// nhất, không trùng nhau.
fn sample_windows(segments: &[Segment]) -> Vec<(u64, u64)> {
    let mut windows: Vec<(u64, u64)> = Vec::new();
    let mut push = |offset: u64, length: u64| {
        if length > 0
            && !windows
                .iter()
                .any(|(start, size)| offset < start + size && *start < offset + length)
        {
            windows.push((offset, length));
        }
    };
    let filled: Vec<&Segment> = segments.iter().filter(|segment| segment.downloaded > 0).collect();
    if let Some(first) = filled.first() {
        push(first.start, first.downloaded.min(SAMPLE));
    }
    if let Some(largest) = filled.iter().max_by_key(|segment| segment.downloaded) {
        let length = largest.downloaded.min(SAMPLE);
        push(largest.start + (largest.downloaded - length) / 2, length);
        push(largest.start + largest.downloaded - length, length);
    }
    windows.truncate(SAMPLES);
    windows
}

/// Đọc `[offset, offset + length)` từ link mới và so với `.part`.
async fn sample_matches(
    transport: &Transport,
    url: &Url,
    temp_path: &str,
    offset: u64,
    length: u64,
    timeout: Duration,
) -> Result<bool, LinkError> {
    let range = format!("bytes={offset}-{}", offset + length - 1);
    let response = transport
        .send(Method::GET, url, Some(&range), Some(timeout))
        .await
        .map_err(|error| LinkError::Unreachable(super::http::classify_error(&error).error))?;
    if response.status() != StatusCode::PARTIAL_CONTENT {
        return Err(LinkError::Unverifiable);
    }
    let matches_range = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range)
        .is_some_and(|range| range.start == offset && range.end == offset + length - 1);
    if !matches_range {
        return Err(LinkError::Unverifiable);
    }
    let remote = response
        .bytes()
        .await
        .map_err(|error| LinkError::Unreachable(super::http::classify_error(&error).error))?;
    if remote.len() as u64 != length {
        return Err(LinkError::Unverifiable);
    }
    let path = Path::new(temp_path).to_path_buf();
    let local = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        let file = File::open(path)?;
        let mut buffer = vec![0u8; length as usize];
        let read = read_at(&file, offset, &mut buffer)?;
        buffer.truncate(read);
        Ok(buffer)
    })
    .await
    .map_err(|_| LinkError::NotAllowed)?
    .map_err(|_| LinkError::NotAllowed)?;
    Ok(local == remote.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SegmentStatus;

    fn segment(index: u32, start: u64, end: u64, downloaded: u64) -> Segment {
        Segment {
            index,
            start,
            end,
            downloaded,
            status: SegmentStatus::Active,
        }
    }

    #[test]
    fn samples_stay_inside_downloaded_bytes_without_overlap() {
        let segments = vec![
            segment(0, 0, 999_999, 100_000),
            segment(1, 1_000_000, 1_999_999, 900_000),
            segment(2, 2_000_000, 2_999_999, 0),
        ];
        let windows = sample_windows(&segments);
        assert_eq!(windows.len(), 3);
        for (offset, length) in &windows {
            let inside = segments.iter().any(|segment| {
                *offset >= segment.start && offset + length <= segment.start + segment.downloaded
            });
            assert!(inside, "{offset}+{length}");
        }
        assert!(sample_windows(&[segment(0, 0, 9, 0)]).is_empty());
        assert_eq!(sample_windows(&[segment(0, 0, 9, 10)]), vec![(0, 10)], "đoạn nhỏ: một mẫu");
    }
}
