use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcConfig {
    pub token: String,
    pub pipe: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PipeRequest {
    token: String,
    url: String,
    suggested_name: Option<String>,
    /// Trang đang mở khi chuột phải; native host cũ không gửi trường này.
    page_url: Option<String>,
}

/// Việc làm với một yêu cầu qua pipe. Token và giới hạn kiểm trước, rồi mới xem có phải trang video.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Route {
    Reject(&'static str),
    /// Trang video: không tạo task HTTP (chỉ được trang HTML), mở hộp Tải media trong UTD.
    Media,
    /// Trang kênh: cùng sự kiện với trang video, giao diện nhận ra link kênh và mở hộp Tải cả kênh.
    Channel,
    Download,
}

fn route(request: &PipeRequest, token: &str) -> Route {
    if !secure_eq(&request.token, token) {
        Route::Reject("Token IPC không hợp lệ.")
    } else if request.url.len() > 8192 {
        Route::Reject("URL quá dài.")
    } else if crate::media_pages::is_media_page(&request.url) {
        Route::Media
    } else if crate::media_pages::is_channel_page(&request.url) {
        Route::Channel
    } else {
        Route::Download
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PipeResponse {
    ok: bool,
    task_id: Option<String>,
    message: Option<String>,
}

impl PipeResponse {
    fn ok(task_id: String) -> Self {
        Self {
            ok: true,
            task_id: Some(task_id),
            message: None,
        }
    }

    /// Đã nhận nhưng không có task (link video chuyển sang hộp Tải media).
    fn note(message: &str) -> Self {
        Self {
            ok: true,
            task_id: None,
            message: Some(message.into()),
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            task_id: None,
            message: Some(message.into()),
        }
    }
}

/// `UTD_DATA_DIR` (đường dẫn tuyệt đối): bản chạy thử dùng thư mục dữ liệu riêng, chạy song song với bản
/// thật mà không đụng `UTD-Data` (journal, cài đặt, công cụ, tích hợp trình duyệt). Native host do
/// trình duyệt chạy không có biến này nên luôn nói chuyện với bản thật.
pub fn isolated_data_dir() -> Option<PathBuf> {
    std::env::var_os("UTD_DATA_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

pub fn data_dir() -> Result<PathBuf, String> {
    if let Some(directory) = isolated_data_dir() {
        return Ok(directory);
    }
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return Ok(PathBuf::from(local).join("UTD-Data"));
    }
    #[cfg(not(windows))]
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".local").join("share").join("UTD-Data"));
    }
    Err("Không tìm thấy thư mục dữ liệu ứng dụng.".into())
}

pub fn create_config() -> IpcConfig {
    IpcConfig {
        token: format!(
            "{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        ),
        pipe: format!("utd-desktop-{}", Uuid::new_v4().simple()),
    }
}

pub fn write_config(directory: &Path, config: &IpcConfig) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let path = directory.join("ipc.json");
    let payload = serde_json::to_vec_pretty(config).map_err(|error| error.to_string())?;
    let temporary = directory.join("ipc.json.tmp");
    std::fs::write(&temporary, payload).map_err(|error| error.to_string())?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    std::fs::rename(&temporary, &path).map_err(|error| error.to_string())
}

pub fn read_config() -> Result<(IpcConfig, PathBuf), String> {
    let directory = data_dir()?;
    let path = directory.join("ipc.json");
    let bytes = std::fs::read(&path).map_err(|_| {
        "UTD chưa chạy hoặc chưa sẵn sàng nhận link. Mở UTD rồi thử lại.".to_string()
    })?;
    let config: IpcConfig =
        serde_json::from_slice(&bytes).map_err(|_| "Tệp IPC của UTD không hợp lệ.".to_string())?;
    if !valid_config(&config) {
        return Err("Tệp IPC của UTD không hợp lệ.".into());
    }
    Ok((config, path))
}

fn valid_config(config: &IpcConfig) -> bool {
    config.token.len() >= 32
        && config.token.chars().all(|c| c.is_ascii_hexdigit())
        && config.pipe.starts_with("utd-desktop-")
        && config
            .pipe
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[cfg(windows)]
pub fn pipe_path(pipe: &str) -> String {
    format!(r"\\.\pipe\{pipe}")
}

#[cfg(windows)]
pub fn spawn_server(
    app: tauri::AppHandle,
    engine: Arc<utd_engine::DownloadEngine>,
    running: Arc<crate::state::RunningTasks>,
    config: IpcConfig,
) {
    tauri::async_runtime::spawn(async move {
        accept_loop(app, engine, running, config).await;
    });
}

#[cfg(windows)]
async fn accept_loop(
    app: tauri::AppHandle,
    engine: Arc<utd_engine::DownloadEngine>,
    running: Arc<crate::state::RunningTasks>,
    config: IpcConfig,
) {
    use tokio::net::windows::named_pipe::{PipeMode, ServerOptions};

    let name = pipe_path(&config.pipe);
    loop {
        let server = match ServerOptions::new()
            .pipe_mode(PipeMode::Byte)
            .create(&name)
        {
            Ok(server) => server,
            Err(error) => {
                log::error!("IPC pipe create failed: {error}");
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };
        if let Err(error) = server.connect().await {
            log::warn!("IPC pipe connect failed: {error}");
            continue;
        }
        let app = app.clone();
        let engine = engine.clone();
        let running = running.clone();
        let token = config.token.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = handle_connection(server, app, engine, running, &token).await {
                log::warn!("IPC request rejected: {error}");
            }
        });
    }
}

#[cfg(windows)]
async fn handle_connection(
    mut server: tokio::net::windows::named_pipe::NamedPipeServer,
    app: tauri::AppHandle,
    engine: Arc<utd_engine::DownloadEngine>,
    running: Arc<crate::state::RunningTasks>,
    token: &str,
) -> Result<(), String> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    let mut line = String::new();
    {
        let mut reader = BufReader::new((&mut server).take(64 * 1024));
        let read = tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
            .await
            .map_err(|_| "IPC timeout".to_string())?
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Ok(());
        }
    }

    let request: PipeRequest =
        serde_json::from_str(&line).map_err(|_| "IPC payload không hợp lệ".to_string())?;
    let routed = route(&request, token);
    let response = match routed {
        Route::Reject(message) => PipeResponse::error(message),
        Route::Media | Route::Channel => {
            let note = if routed == Route::Channel {
                crate::media_pages::CHANNEL_PAGE_NOTE
            } else {
                crate::media_pages::MEDIA_PAGE_NOTE
            };
            let media = crate::events::MediaRequest {
                url: request.url.trim().to_string(),
                page_url: crate::media_pages::clean_page_url(request.page_url),
            };
            crate::media_pages::open_media_flow(&app, media);
            PipeResponse::note(note)
        }
        Route::Download => {
            let input = crate::commands::AddDownloadInput {
                url: request.url,
                dest_dir: None,
                file_name: request.suggested_name,
                connections: None,
                expected_sha256: None,
            };
            match crate::commands::enqueue_download(app, engine, running, input) {
                Ok(task) => PipeResponse::ok(task.id),
                Err(message) => PipeResponse::error(message),
            }
        }
    };

    let payload = serde_json::to_string(&response).map_err(|error| error.to_string())?;
    server
        .write_all(format!("{payload}\n").as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    server.flush().await.map_err(|error| error.to_string())
}

fn secure_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn request(token: &str, url: &str) -> PipeRequest {
        serde_json::from_value(serde_json::json!({ "token": token, "url": url })).unwrap()
    }

    #[test]
    fn video_page_from_the_browser_opens_the_media_flow() {
        let watch = "https://www.youtube.com/watch?v=09OGSRptb0M";
        assert_eq!(route(&request(TOKEN, watch), TOKEN), Route::Media);
        assert_eq!(
            route(&request(TOKEN, "https://vt.tiktok.com/ZSabc123/"), TOKEN),
            Route::Media
        );
        let file = "https://example.com/releases/app-1.2.3.zip";
        assert_eq!(route(&request(TOKEN, file), TOKEN), Route::Download);
        assert_eq!(
            route(&request(TOKEN, "https://www.youtube.com/"), TOKEN),
            Route::Download,
            "trang chủ vẫn đi đường tải thường"
        );
        assert_eq!(
            route(&request(TOKEN, "https://www.tiktok.com/@tiktok"), TOKEN),
            Route::Channel
        );
    }

    #[test]
    fn token_and_length_are_checked_before_the_media_branch() {
        let watch = "https://www.youtube.com/watch?v=09OGSRptb0M";
        let wrong = "f".repeat(TOKEN.len());
        assert_eq!(
            route(&request(&wrong, watch), TOKEN),
            Route::Reject("Token IPC không hợp lệ.")
        );
        let long = format!("{watch}&x={}", "a".repeat(8192));
        assert_eq!(route(&request(TOKEN, &long), TOKEN), Route::Reject("URL quá dài."));
    }

    #[test]
    fn page_url_is_optional_and_note_is_ok() {
        let parsed: PipeRequest = serde_json::from_str(
            r#"{"token":"t","url":"https://youtu.be/x","suggestedName":null,"pageUrl":"https://www.youtube.com/"}"#,
        )
        .unwrap();
        assert_eq!(parsed.page_url.as_deref(), Some("https://www.youtube.com/"));
        assert_eq!(request(TOKEN, "https://youtu.be/x").page_url, None);

        let note = serde_json::to_value(PipeResponse::note("Đã mở")).unwrap();
        assert_eq!(
            note,
            serde_json::json!({ "ok": true, "taskId": null, "message": "Đã mở" })
        );
    }
}
