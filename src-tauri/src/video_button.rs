//! Nút "Tải video này" trên trang (tiện ích, GĐ3; notes/auth-plan.md): phân tích định dạng cho bảng chọn ngay trên
//! trang, rồi tải thẳng với định dạng người dùng chọn — không mở hộp Tải media. Kết quả phân tích nhớ 10 phút theo
//! (link, lần cho phép đăng nhập) để bảng mở lại tức thì và khỏi chạy yt-dlp lại.

use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::AppHandle;
use utd_engine::validate_url;

use crate::auth::Grant;
use crate::media::probe::MediaProbeResult;
use crate::media::AddMediaInput;

/// Nhớ kết quả phân tích chừng này.
const REMEMBER: Duration = Duration::from_secs(10 * 60);
/// Nhớ tối đa chừng này link (cũ nhất bỏ trước).
const MAX_REMEMBERED: usize = 64;
/// Tiêu đề dài hơn thì cắt (tên file đặt theo tiêu đề).
const MAX_TITLE_CHARS: usize = 300;

type Key = (String, Option<String>);

static REMEMBERED: LazyLock<Mutex<Vec<(Key, Instant, Value)>>> = LazyLock::new(|| Mutex::new(Vec::new()));

fn recall(key: &Key, now: Instant) -> Option<Value> {
    let mut remembered = REMEMBERED.lock().ok()?;
    remembered.retain(|(_, at, _)| now.duration_since(*at) < REMEMBER);
    remembered
        .iter()
        .find(|(known, _, _)| known == key)
        .map(|(_, _, value)| value.clone())
}

fn remember(key: Key, value: Value, now: Instant) {
    let Ok(mut remembered) = REMEMBERED.lock() else {
        return;
    };
    remembered.retain(|(known, _, _)| *known != key);
    if remembered.len() >= MAX_REMEMBERED {
        remembered.remove(0);
    }
    remembered.push((key, now, value));
}

/// Định dạng của video ở `url` cho bảng trên trang. yt-dlp không đọc được (không phải vì cần đăng nhập) mà `<video>`
/// có nguồn http(s) thật ⇒ một dòng "Tải file video" (`direct`) tải thẳng nguồn đó.
pub(crate) async fn formats(
    app: &AppHandle,
    url: &str,
    src: Option<&str>,
    grant: Option<Arc<Grant>>,
) -> Result<Value, String> {
    let url = validate_url(url.trim()).map_err(|error| error.to_string())?;
    // Trang file Google Drive (IPC đã đổi sang link tải thật): tải file gốc, không qua yt-dlp.
    if url.host_str() == Some("drive.usercontent.google.com") {
        return Ok(direct(url.as_str(), url.as_str()));
    }
    let key: Key = (url.to_string(), grant.as_ref().map(|grant| grant.id.clone()));
    if let Some(known) = recall(&key, Instant::now()) {
        return Ok(known);
    }
    match crate::media::analyze(app, url.as_str(), grant.as_deref()).await {
        Ok(result) => {
            let value = summary(&result, grant.is_some());
            remember(key, value.clone(), Instant::now());
            Ok(value)
        }
        Err(message) => match direct_source(src) {
            Some(src) if !crate::media::needs_sign_in(&message) => Ok(direct(url.as_str(), &src)),
            _ => Err(message),
        },
    }
}

/// Bảng trên trang: đủ mức chiều cao (`every_height`), mỗi dòng mang selector để tải đúng định dạng đó.
fn summary(result: &MediaProbeResult, signed: bool) -> Value {
    let formats: Vec<Value> = result
        .every_height
        .iter()
        .map(|option| {
            json!({
                "id": option.id,
                "sizeBytes": option.size_bytes,
                "selector": option.selector,
                "audioFormat": option.audio_format,
            })
        })
        .collect();
    json!({
        "title": result.title,
        "webpageUrl": result.webpage_url,
        "durationSeconds": result.duration_seconds,
        "signed": signed,
        "formats": formats,
    })
}

fn direct(url: &str, src: &str) -> Value {
    json!({
        "title": null,
        "webpageUrl": url,
        "signed": false,
        "formats": [{ "id": "file", "direct": src }],
    })
}

/// Nguồn của `<video>` tải thẳng được: http(s), không phải `blob:`/`data:` (luồng MSE chỉ trình phát hiểu).
fn direct_source(src: Option<&str>) -> Option<String> {
    let src = src?.trim();
    validate_url(src).ok().map(|url| url.to_string())
}

/// Tải với định dạng đã chọn trên trang: tạo task media rồi xếp đầu hàng đợi (như bấm Tải trong hộp Tải media).
pub(crate) async fn download(
    app: AppHandle,
    request: PageDownload,
    grant: Option<Arc<Grant>>,
) -> Result<String, String> {
    let title: String = request
        .title
        .unwrap_or_default()
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_TITLE_CHARS)
        .collect();
    let input = AddMediaInput {
        url: request.url,
        title,
        selector: request.selector,
        quality: None,
        audio_format: request.audio_format,
        page_url: request.page_url,
        dest_dir: None,
        later: false,
        subfolder: None,
        channel: None,
        auth_grant: grant.map(|grant| grant.id.clone()),
    };
    let task = tauri::async_runtime::spawn_blocking(move || crate::media::enqueue_media(app, input))
        .await
        .map_err(|error| error.to_string())??;
    log::info!("video button: task={} queued", task.id);
    Ok(task.id)
}

/// Dòng người dùng bấm khi bảng lấy luồng thẳng từ trang (Tin Facebook…, yt-dlp không đọc được): UTD tải các luồng
/// đó (hình + tiếng thì ghép), không chạy yt-dlp, không cookie.
pub(crate) async fn download_streams(app: AppHandle, request: PageStreams) -> Result<String, String> {
    let page = validate_url(request.url.trim()).map_err(|error| error.to_string())?;
    let streams = crate::media::page_streams::checked(request.streams)?;
    let title: String = request
        .title
        .unwrap_or_default()
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_TITLE_CHARS)
        .collect();
    let input = AddMediaInput {
        url: page.to_string(),
        title,
        selector: crate::media::page_streams::SELECTOR.into(),
        quality: None,
        audio_format: None,
        page_url: Some(page.to_string()),
        dest_dir: None,
        later: false,
        subfolder: None,
        channel: None,
        auth_grant: None,
    };
    let task = tauri::async_runtime::spawn_blocking(move || crate::media::enqueue_page(app, input, &streams))
        .await
        .map_err(|error| error.to_string())??;
    log::info!("video button: task={} queued (page streams)", task.id);
    Ok(task.id)
}

/// Luồng tiện ích đọc từ trình phát của trang `url`.
pub(crate) struct PageStreams {
    pub url: String,
    pub title: Option<String>,
    pub streams: Vec<crate::media::page_streams::PageStream>,
}

/// Định dạng người dùng bấm trong bảng trên trang.
pub(crate) struct PageDownload {
    pub url: String,
    pub page_url: Option<String>,
    pub title: Option<String>,
    pub selector: String,
    pub audio_format: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_per_link_and_grant_for_ten_minutes() {
        let now = Instant::now();
        let link = "https://www.youtube.com/watch?v=remember-test".to_string();
        let plain: Key = (link.clone(), None);
        let signed: Key = (link, Some("grant-1".into()));
        remember(plain.clone(), json!({ "title": "A" }), now);
        assert_eq!(recall(&plain, now + Duration::from_secs(60)), Some(json!({ "title": "A" })));
        assert_eq!(recall(&signed, now), None, "kết quả có đăng nhập tách riêng");
        assert_eq!(recall(&plain, now + REMEMBER + Duration::from_secs(1)), None);
    }

    #[test]
    fn only_real_web_sources_download_directly() {
        assert_eq!(
            direct_source(Some(" https://cdn.example.com/clip.mp4 ")).as_deref(),
            Some("https://cdn.example.com/clip.mp4")
        );
        assert_eq!(direct_source(Some("blob:https://www.youtube.com/4c1a")), None);
        assert_eq!(direct_source(Some("data:video/mp4;base64,AAAA")), None);
        assert_eq!(direct_source(None), None);
        let value = direct("https://news.example/page", "https://cdn.example.com/clip.mp4");
        assert_eq!(value["formats"][0]["id"], "file");
        assert_eq!(value["formats"][0]["direct"], "https://cdn.example.com/clip.mp4");
    }
}
