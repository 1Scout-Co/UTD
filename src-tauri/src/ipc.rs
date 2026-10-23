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

    fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            task_id: None,
            message: Some(message.into()),
        }
    }
}

pub fn data_dir() -> Result<PathBuf, String> {
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
    let response = if !secure_eq(&request.token, token) {
        PipeResponse::error("Token IPC không hợp lệ.")
    } else if request.url.len() > 8192 {
        PipeResponse::error("URL quá dài.")
    } else {
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
