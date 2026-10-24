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
    /// Lô link ("Tải tất cả link bằng UTD"): mở hộp Thêm hàng loạt, `url` khi đó là trang đang mở.
    #[serde(default)]
    batch: Option<Vec<String>>,
}

/// Một dòng yêu cầu tối đa chừng này byte: lô `batch::MAX_LINKS` link (native host đã giới hạn 1 MiB).
const MAX_REQUEST_BYTES: u64 = 2 * 1024 * 1024;

/// Việc làm với một yêu cầu qua pipe. Token và giới hạn kiểm trước, rồi mới xem có phải trang video.
#[derive(Debug, PartialEq)]
enum Route {
    Reject(&'static str),
    /// Trang video: không tạo task HTTP (chỉ được trang HTML), mở hộp Tải media trong UTD.
    Media,
    /// Lô link đã kiểm: mở hộp Thêm hàng loạt, chưa tạo task nào.
    Batch(Vec<String>),
    /// Trang kênh: cùng sự kiện với trang video, giao diện nhận ra link kênh và mở hộp Tải cả kênh.
    Channel,
    Download,
}

fn route(request: &PipeRequest, token: &str) -> Route {
    if !secure_eq(&request.token, token) {
        Route::Reject("Token IPC không hợp lệ.")
    } else if let Some(links) = &request.batch {
        match crate::batch::clean_links(links) {
            Ok(links) => Route::Batch(links),
            Err(message) => Route::Reject(message),
        }
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
/// thật mà không đụng `UTD-Data` (journal, cài đặt, công cụ, tích hợp trình duyệt: nút cài báo lỗi, trừ
/// khi có `UTD_TEST_BROWSER_INTEGRATION=1`). Native host do trình duyệt chạy không có biến này nên luôn
/// nói chuyện với bản thật.
pub fn isolated_data_dir() -> Option<PathBuf> {
    std::env::var_os("UTD_DATA_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// Windows `%LOCALAPPDATA%\UTD-Data`, macOS `~/Library/Application Support/UTD-Data`. Không theo
/// `identifier` của Tauri: native host do trình duyệt chạy phải tự tìm ra đúng chỗ này.
pub fn data_dir() -> Result<PathBuf, String> {
    if let Some(directory) = isolated_data_dir() {
        return Ok(directory);
    }
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return Ok(PathBuf::from(local).join("UTD-Data"));
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()) {
        return Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("UTD-Data"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
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
    write_private(&temporary, &payload).map_err(|error| error.to_string())?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    std::fs::rename(&temporary, &path).map_err(|error| error.to_string())
}

/// Token chỉ chủ máy đọc được: Unix tạo file quyền 0600 ngay từ đầu (Windows dựa vào ACL của
/// `%LOCALAPPDATA%`).
fn write_private(path: &Path, payload: &[u8]) -> std::io::Result<()> {
    let _ = std::fs::remove_file(path);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    std::io::Write::write_all(&mut options.open(path)?, payload)
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

/// macOS: Unix domain socket trong thư mục dữ liệu (quyền 0600), vẫn kèm token của `ipc.json`.
/// Tên ngắn cố định: `sun_path` chỉ chứa 104 byte.
#[cfg(unix)]
pub fn socket_path(directory: &Path) -> PathBuf {
    directory.join("ipc.sock")
}

pub fn spawn_server(
    app: tauri::AppHandle,
    engine: Arc<utd_engine::DownloadEngine>,
    queue: Arc<crate::queue::DownloadQueue>,
    config: IpcConfig,
    directory: PathBuf,
) {
    tauri::async_runtime::spawn(async move {
        accept_loop(app, engine, queue, config, directory).await;
    });
}

#[cfg(unix)]
async fn accept_loop(
    app: tauri::AppHandle,
    engine: Arc<utd_engine::DownloadEngine>,
    queue: Arc<crate::queue::DownloadQueue>,
    config: IpcConfig,
    directory: PathBuf,
) {
    let listener = match bind_private(&socket_path(&directory)) {
        Ok(listener) => listener,
        Err(error) => {
            log::error!("IPC socket bind failed: {error}");
            return;
        }
    };
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(error) => {
                log::warn!("IPC socket accept failed: {error}");
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
        };
        let app = app.clone();
        let engine = engine.clone();
        let queue = queue.clone();
        let token = config.token.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = handle_connection(stream, app, engine, queue, &token).await {
                log::warn!("IPC request rejected: {error}");
            }
        });
    }
}

/// Socket sót lại (app bị tắt ngang) thì gỡ rồi tạo lại; chỉnh quyền 0600 trước khi nhận kết nối.
#[cfg(unix)]
fn bind_private(path: &Path) -> std::io::Result<tokio::net::UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let listener = tokio::net::UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

#[cfg(windows)]
async fn accept_loop(
    app: tauri::AppHandle,
    engine: Arc<utd_engine::DownloadEngine>,
    queue: Arc<crate::queue::DownloadQueue>,
    config: IpcConfig,
    _directory: PathBuf,
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
        let queue = queue.clone();
        let token = config.token.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = handle_connection(server, app, engine, queue, &token).await {
                log::warn!("IPC request rejected: {error}");
            }
        });
    }
}

/// Một yêu cầu một dòng JSON, trả một dòng JSON (named pipe trên Windows, Unix socket trên macOS).
async fn handle_connection<S>(
    mut server: S,
    app: tauri::AppHandle,
    engine: Arc<utd_engine::DownloadEngine>,
    queue: Arc<crate::queue::DownloadQueue>,
    token: &str,
) -> Result<(), String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    let mut line = String::new();
    {
        let mut reader = BufReader::new((&mut server).take(MAX_REQUEST_BYTES));
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
    let response = match route(&request, token) {
        Route::Reject(message) => PipeResponse::error(message),
        Route::Media => open_media(&app, request, crate::media_pages::MEDIA_PAGE_NOTE),
        Route::Channel => open_media(&app, request, crate::media_pages::CHANNEL_PAGE_NOTE),
        Route::Batch(urls) => {
            let batch = crate::events::BatchRequest {
                urls,
                page_url: crate::media_pages::clean_page_url(request.page_url),
            };
            crate::batch::open_batch_flow(&app, batch);
            PipeResponse::note(crate::batch::BATCH_NOTE)
        }
        Route::Download => {
            let input = crate::commands::AddDownloadInput {
                url: request.url,
                dest_dir: None,
                file_name: request.suggested_name,
                connections: None,
                expected_sha256: None,
                later: false,
            };
            match crate::commands::enqueue_download(app, engine, queue, input) {
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

/// Trang video hoặc trang kênh: cùng sự kiện `utd://media-request`, giao diện tự nhận ra link kênh.
fn open_media(app: &tauri::AppHandle, request: PipeRequest, note: &str) -> PipeResponse {
    let media = crate::events::MediaRequest {
        url: request.url.trim().to_string(),
        page_url: crate::media_pages::clean_page_url(request.page_url),
    };
    crate::media_pages::open_media_flow(app, media);
    PipeResponse::note(note)
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
    fn a_batch_opens_the_batch_dialog_after_the_token_check() {
        let batch = |token: &str, links: &[&str]| -> PipeRequest {
            serde_json::from_value(serde_json::json!({
                "token": token,
                "url": "https://example.com/gallery",
                "pageUrl": "https://example.com/gallery",
                "batch": links,
            }))
            .unwrap()
        };
        let links = ["https://example.com/a.jpg", "javascript:x", "https://example.com/a.jpg"];
        assert_eq!(
            route(&batch(TOKEN, &links), TOKEN),
            Route::Batch(vec!["https://example.com/a.jpg".into()])
        );
        let wrong = "f".repeat(TOKEN.len());
        assert_eq!(
            route(&batch(&wrong, &links), TOKEN),
            Route::Reject("Token IPC không hợp lệ.")
        );
        assert_eq!(
            route(&batch(TOKEN, &["mailto:x@example.com"]), TOKEN),
            Route::Reject("Không có link hợp lệ.")
        );
        assert_eq!(request(TOKEN, "https://youtu.be/x").batch, None, "native host cũ");
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

    #[test]
    fn config_round_trips_through_the_data_dir() {
        let directory = std::env::temp_dir().join(format!("utd-ipc-{}", Uuid::new_v4()));
        let config = create_config();
        assert!(valid_config(&config));
        write_config(&directory, &config).unwrap();
        write_config(&directory, &config).unwrap();
        let saved: IpcConfig =
            serde_json::from_slice(&std::fs::read(directory.join("ipc.json")).unwrap()).unwrap();
        assert_eq!(saved.token, config.token);
        assert!(!directory.join("ipc.json.tmp").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(directory.join("ipc.json")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "token chỉ chủ máy đọc được");
        }
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Socket sót lại thì thay; quyền 0600; native host nối được và nói một dòng mỗi chiều.
    #[cfg(unix)]
    #[test]
    fn unix_socket_is_private_and_replaces_a_stale_one() {
        use std::io::{BufRead, Write};
        use std::os::unix::fs::PermissionsExt;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        let directory = PathBuf::from("/tmp").join(format!("utd-sock-{}", &Uuid::new_v4().simple().to_string()[..8]));
        std::fs::create_dir_all(&directory).unwrap();
        let path = socket_path(&directory);
        std::fs::write(&path, b"cu").unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let listener = bind_private(&path).unwrap();
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
            let client = std::thread::spawn({
                let path = path.clone();
                move || {
                    let mut stream = std::os::unix::net::UnixStream::connect(&path).unwrap();
                    stream.write_all(b"xin chao\n").unwrap();
                    let mut line = String::new();
                    std::io::BufReader::new(stream).read_line(&mut line).unwrap();
                    line
                }
            });
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = tokio::io::BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            assert_eq!(line, "xin chao\n");
            reader.get_mut().write_all(b"da nhan\n").await.unwrap();
            assert_eq!(client.join().unwrap(), "da nhan\n");
        });
        let _ = std::fs::remove_dir_all(&directory);
    }
}
