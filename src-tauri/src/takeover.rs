//! Tự bắt link tải của trình duyệt (AC-15; notes/auth-plan.md §4.4). Tiện ích đã tạm dừng download của
//! trình duyệt rồi hỏi UTD (`extensions/*/takeover.js`). UTD chỉ nhận khi dò được đúng một file (kèm cookie
//! của cú bấm nếu trang dùng đăng nhập) và không cần hỏi người dùng điều gì; còn lại trả lời "không nhận" để
//! trình duyệt tải tiếp. Nhờ vậy không mất download nào: POST, link dùng một lần, trang web, file cần xác
//! nhận (S-22) đều về lại trình duyệt.

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use utd_engine::{DownloadEngine, RemoteFile};

use crate::auth::AuthPayload;
use crate::events::{TAKEN_OVER_EVENT, TakenOver};
use crate::queue::DownloadQueue;

/// Tiện ích chờ trả lời tối đa 20 s rồi tự cho trình duyệt tải tiếp; UTD dò xong trước đó.
const PROBE_BUDGET: Duration = Duration::from_secs(12);

/// Câu trả khi không nhận (tiện ích không hiện, chỉ cho trình duyệt tải tiếp).
pub(crate) const DECLINED: &str = "[takeover.declined] Để trình duyệt tải file này.";

/// Một download trình duyệt vừa bắt đầu. `page_url`: trang chứa link (referrer).
pub(crate) struct Offer {
    pub url: String,
    pub page_url: Option<String>,
    pub suggested_name: Option<String>,
    pub auth: Option<AuthPayload>,
}

/// Nhận download: trả mã task, hoặc lý do (chỉ ghi log) để trình duyệt tải tiếp.
pub(crate) async fn take(
    app: &AppHandle,
    engine: &Arc<DownloadEngine>,
    queue: &Arc<DownloadQueue>,
    offer: Offer,
) -> Result<String, &'static str> {
    let url = offer.url.trim().to_string();
    if crate::media_pages::is_media_page(&url)
        || crate::media_pages::is_channel_page(&url)
        || crate::media_pages::is_drive_folder(&url)
    {
        return Err("page link");
    }
    let page_url = crate::media_pages::clean_page_url(offer.page_url);
    let urls: Vec<&str> = std::iter::once(url.as_str())
        .chain(page_url.as_deref())
        .collect();
    // Cú bấm tải trên trang là hành động của người dùng: cookie tiện ích đọc lúc đó đi theo đúng luật đăng nhập.
    let grant = crate::auth::flow::grant_from_click(app, offer.auth, &urls);
    let cookies = grant.as_ref().map(|grant| grant.jar.clone());
    let file = tokio::time::timeout(PROBE_BUDGET, engine.inspect_signed(&url, cookies))
        .await
        .map_err(|_| "probe timeout")?
        .map_err(|_| "probe failed")?;
    let name = accepted_name(&url, &file, offer.suggested_name.as_deref())?;

    // Đúng file của download đang chờ link mới: gắn vào đó, giữ phần đã tải.
    let task_id = match crate::recover::capture_link(
        app,
        engine,
        queue,
        &url,
        Some(&name),
        grant.as_ref(),
    )
    .await
    {
        Some(task_id) => task_id,
        None => {
            let input = crate::commands::AddDownloadInput {
                url: url.clone(),
                dest_dir: None,
                file_name: Some(name.clone()),
                connections: None,
                expected_sha256: None,
                later: false,
                auth_grant: grant.map(|grant| grant.id.clone()),
            };
            crate::commands::enqueue_download(app.clone(), engine.clone(), queue.clone(), input)
                .map_err(|_| "enqueue failed")?
                .id
        }
    };
    let notice = TakenOver {
        task_id: task_id.clone(),
        file_name: name,
    };
    if let Err(error) = app.emit(TAKEN_OVER_EVENT, notice) {
        log::warn!("taken-over event failed: {error}");
    }
    Ok(task_id)
}

/// Tên file UTD sẽ lưu nếu nhận: tên trình duyệt gợi ý (đã hợp lệ), không thì tên máy chủ đưa
/// (Content-Disposition) hoặc lấy từ link. Trang web hoặc file có cảnh báo trước khi tải (S-22, cần
/// người dùng xem) ⇒ không nhận.
fn accepted_name(
    url: &str,
    file: &RemoteFile,
    suggested: Option<&str>,
) -> Result<String, &'static str> {
    if file.page {
        return Err("web page");
    }
    let name = suggested
        .map(str::trim)
        .filter(|name| utd_engine::validate_file_name(name).is_ok())
        .unwrap_or(&file.file_name)
        .to_string();
    let link = crate::preflight::LinkInput {
        url: url.to_string(),
        final_url: Some(file.url.clone()),
        file_name: Some(name.clone()),
        content_type: file.content_type.clone(),
    };
    if !crate::preflight::warnings_of(&link).is_empty() {
        return Err("needs confirmation");
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(url: &str, name: &str, content_type: &str, page: bool) -> RemoteFile {
        RemoteFile {
            url: url.into(),
            file_name: name.into(),
            total_bytes: Some(5 << 20),
            content_type: Some(content_type.into()),
            page,
        }
    }

    #[test]
    fn takes_plain_files_under_the_browser_name() {
        let url = "https://cdn.example.com/d/8f1c";
        let file = remote(url, "8f1c", "application/zip", false);
        assert_eq!(accepted_name(url, &file, Some(" bao-cao.zip ")), Ok("bao-cao.zip".into()));
        assert_eq!(accepted_name(url, &file, None), Ok("8f1c".into()), "tên máy chủ / link");
        assert_eq!(
            accepted_name(url, &file, Some("../x")),
            Ok("8f1c".into()),
            "tên lạ từ trình duyệt thì bỏ"
        );
    }

    /// Không làm mất download: thứ UTD không tải đúng được, hoặc phải hỏi người dùng, về lại trình duyệt.
    #[test]
    fn leaves_pages_and_files_that_need_a_question_to_the_browser() {
        let url = "https://example.com/export";
        let page = remote(url, "export.html", "text/html", true);
        assert_eq!(accepted_name(url, &page, Some("report.csv")), Err("web page"));
        let exe = remote("https://example.com/setup.exe", "setup.exe", "application/octet-stream", false);
        assert_eq!(
            accepted_name("https://example.com/setup.exe", &exe, None),
            Err("needs confirmation")
        );
        let plain = remote("http://mirror.example.org/a.iso", "a.iso", "application/octet-stream", false);
        assert_eq!(
            accepted_name("http://mirror.example.org/a.iso", &plain, None),
            Err("needs confirmation"),
            "http: trình duyệt tự cảnh báo"
        );
        let spoofed = remote(url, "hoa-don.pdf.exe", "application/pdf", false);
        assert_eq!(accepted_name(url, &spoofed, None), Err("needs confirmation"));
    }
}
