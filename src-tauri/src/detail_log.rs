//! "Sao chép log" ở màn Chi tiết: một task dạng chữ thuần (tiếng Anh, như log máy) để dán vào báo lỗi.
//! Che thứ riêng tư (P-04): link bỏ query/fragment (token ký), thư mục người dùng thành `~`.

use std::fmt::Write as _;
use std::path::Path;

use crate::detail::DownloadDetail;

pub(crate) fn render(detail: &DownloadDetail, home: Option<&Path>) -> String {
    let task = &detail.task;
    let mut out = String::new();
    let _ = writeln!(out, "UTD {} download log", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(out, "file: {}", task.file_name);
    let _ = writeln!(out, "source: {}", redact_url(&task.url));
    if let Some(final_url) = task.final_url.as_deref().filter(|value| *value != task.url) {
        let _ = writeln!(out, "final: {}", redact_url(final_url));
    }
    let _ = writeln!(out, "saved to: {}", tidy_path(&detail.target_path, home));
    let _ = writeln!(
        out,
        "kind: {} · status: {} · hash: {} · connections: {}",
        task.kind.as_str(),
        task.status.as_str(),
        task.hash_state.as_str(),
        task.connections
    );
    let total = task.total_bytes.map_or("?".to_string(), |total| total.to_string());
    let _ = writeln!(out, "bytes: {} / {total}", task.downloaded_bytes);
    if let Some(error) = &task.error {
        let _ = writeln!(
            out,
            "error: {} [{}] {}",
            error.code,
            error.class.as_str(),
            redact_text(&error.message)
        );
    }
    let _ = writeln!(out, "history:");
    for event in &detail.events {
        let _ = write!(
            out,
            "  {} {} {}",
            iso_utc(event.at),
            event.status.as_str(),
            event.downloaded_bytes
        );
        if let Some(code) = &event.code {
            let _ = write!(out, " {code}");
        }
        if let Some(message) = &event.message {
            let _ = write!(out, " {}", redact_text(message));
        }
        out.push('\n');
    }
    if !detail.parts.is_empty() {
        let _ = writeln!(out, "parts:");
    }
    for part in &detail.parts {
        let done = part
            .segments
            .iter()
            .filter(|segment| segment.is_done())
            .count();
        let total = part.total_bytes.map_or("?".to_string(), |total| total.to_string());
        let _ = writeln!(
            out,
            "  {:?} {} {} {}/{total} segments {done}/{}",
            part.role,
            part.file_name,
            part.status.as_str(),
            part.downloaded_bytes,
            part.segments.len()
        );
    }
    out
}

/// Bỏ query, fragment (và tên/mật khẩu nếu có) của link: token ký thường nằm ở đó.
fn redact_url(raw: &str) -> String {
    match tauri::Url::parse(raw) {
        Ok(mut url) => {
            let hidden = url.query().is_some() || url.fragment().is_some();
            url.set_query(None);
            url.set_fragment(None);
            let _ = url.set_username("");
            let _ = url.set_password(None);
            let mut text = url.to_string();
            if hidden {
                text.push_str("?…");
            }
            text
        }
        Err(_) => "(invalid link)".into(),
    }
}

/// Câu lỗi có thể chứa link (vd câu của yt-dlp): che từng link trong câu.
fn redact_text(text: &str) -> String {
    text.split(' ')
        .map(|word| {
            if word.starts_with("http://") || word.starts_with("https://") {
                redact_url(word)
            } else {
                word.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn tidy_path(path: &str, home: Option<&Path>) -> String {
    let Some(home) = home else {
        return path.to_string();
    };
    match Path::new(path).strip_prefix(home) {
        Ok(rest) => Path::new("~").join(rest).to_string_lossy().into_owned(),
        Err(_) => path.to_string(),
    }
}

/// Giờ UTC dạng `2026-10-25T03:31:02Z` (không kéo thêm thư viện ngày giờ).
fn iso_utc(ms: i64) -> String {
    let seconds = ms.div_euclid(1000);
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Ngày dân dụng từ số ngày kể từ 1970-01-01 (thuật toán của Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detail::{DetailPart, PartRole};
    use utd_engine::{DownloadTask, ErrorClass, TaskError, TaskEvent, TaskStatus};

    #[test]
    fn formats_utc_dates() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_792_899_062_000), "2026-10-25T03:31:02Z");
        assert_eq!(iso_utc(951_782_400_000), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn hides_tokens_and_the_user_folder() {
        let home = if cfg!(windows) { r"C:\Users\Lan" } else { "/Users/lan" };
        let folder = Path::new(home).join("Downloads").join("UTD");
        let mut task = DownloadTask::new(
            "https://cdn.example.com/a.iso?token=SECRET#x",
            "a.iso",
            folder.to_string_lossy().into_owned(),
            8,
        )
        .unwrap();
        task.status = TaskStatus::Failed;
        task.error = Some(
            TaskError::new(
                ErrorClass::Permanent,
                "http.403",
                "Forbidden at https://cdn.example.com/a.iso?sig=SECRET2 now",
            )
            .unwrap(),
        );
        let detail = DownloadDetail {
            target_path: folder.join("a.iso").to_string_lossy().into_owned(),
            file_exists: None,
            media_title: None,
            parts: vec![DetailPart {
                id: task.id.clone(),
                role: PartRole::File,
                file_name: "a.iso".into(),
                status: TaskStatus::Failed,
                total_bytes: Some(10),
                downloaded_bytes: 4,
                segments: Vec::new(),
            }],
            events: vec![TaskEvent::of(&task)],
            task,
        };
        let log = render(&detail, Some(Path::new(home)));
        assert!(!log.contains("SECRET"), "{log}");
        assert!(!log.contains("Lan") && !log.contains("lan/"), "{log}");
        assert!(log.contains("source: https://cdn.example.com/a.iso?…"), "{log}");
        assert!(log.contains("error: http.403 [permanent] Forbidden at https://cdn.example.com/a.iso?… now"));
        assert!(log.contains(" failed 0 http.403 "), "{log}");
        assert!(log.contains("File a.iso failed 4/10 segments 0/0"), "{log}");
    }
}
