mod fast;
mod finish;
mod install;
mod mux;
mod output;
/// `parts::discard_parts`: dọn luồng con trước khi xoá task media khỏi danh sách.
pub(crate) mod parts;
mod probe;
mod probe_cache;
mod resolve;
mod rollup;
mod runner;
mod tools;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;
use utd_engine::{
    now_ms, validate_file_name, validate_url, DownloadTask, HashState, MediaJob, TaskKind,
    TaskStatus,
};

use utd_engine::Journal;
use crate::events::{BridgeError, ERROR_EVENT};
use crate::state::{AppState, RunningTasks};


#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddMediaInput {
    pub url: String,
    pub title: String,
    pub selector: String,
    pub audio_format: Option<String>,
    pub page_url: Option<String>,
}

/// Dò công cụ có chạy `--version` nên làm ngoài luồng giao diện.
#[tauri::command]
pub async fn media_tools_status() -> Result<tools::MediaToolsStatus, String> {
    let mut status = tauri::async_runtime::spawn_blocking(tools::detect)
        .await
        .map_err(|error| error.to_string())?;
    status.installing = install::current();
    Ok(status)
}

/// Tải yt-dlp hoặc ffmpeg chính chủ từ GitHub vào `<data>\bin` (người dùng đã bấm nút).
#[tauri::command]
pub async fn install_media_tool(
    app: AppHandle,
    tool: String,
) -> Result<tools::MediaToolsStatus, String> {
    install::install(app, &tool).await
}

#[tauri::command]
pub fn cancel_media_tool_install() -> bool {
    install::cancel()
}

#[tauri::command]
pub async fn media_probe(url: String) -> Result<probe::MediaProbeResult, String> {
    let url = validate_url(&url).map_err(|error| error.to_string())?;
    let tools = tauri::async_runtime::spawn_blocking(tools::resolve)
        .await
        .map_err(|error| error.to_string())??;
    probe::probe(&tools.yt_dlp, &tools, url.as_str()).await
}

/// Ngoài luồng giao diện: kiểm/tạo thư mục tải và đọc thư mục để tránh trùng tên đều chạm đĩa.
#[tauri::command]
pub async fn add_media_download(
    app: AppHandle,
    input: AddMediaInput,
) -> Result<DownloadTask, String> {
    let started = std::time::Instant::now();
    let task = tauri::async_runtime::spawn_blocking(move || enqueue_media(app, input))
        .await
        .map_err(|error| error.to_string())??;
    // Mốc đo: thời gian lệnh "Tải" (job đã chạy song song, log `media.timing … start`).
    let elapsed = started.elapsed().as_millis();
    log::info!("media.timing task={} queued ms={elapsed}", task.id);
    Ok(task)
}

fn enqueue_media(app: AppHandle, input: AddMediaInput) -> Result<DownloadTask, String> {
    let (journal, running) = {
        let state = app.state::<AppState>();
        (state.journal.clone(), state.running.clone())
    };
    let url = validate_url(&input.url).map_err(|error| error.to_string())?;
    if input.selector.trim().is_empty()
        || input.selector.len() > 512
        || input.selector.chars().any(char::is_control)
    {
        return Err("Bộ chọn định dạng không hợp lệ.".into());
    }
    if let Some(format) = &input.audio_format
        && !matches!(format.as_str(), "m4a" | "mp3")
    {
        return Err("Định dạng âm thanh không hợp lệ.".into());
    }
    if let Some(page_url) = &input.page_url {
        validate_url(page_url).map_err(|error| error.to_string())?;
    }

    let destination = crate::commands::suggested_download_dir(&app)?;
    std::fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    let title = input.title.trim();
    let title = if title.is_empty() { "Media" } else { title };
    let base = unique_base(&destination, &safe_base(title));
    let extension = input.audio_format.as_deref().unwrap_or("mp4");
    let file_name = format!("{base}.{extension}");
    validate_file_name(&file_name).map_err(|error| error.to_string())?;

    let mut task = DownloadTask::new(
        url.to_string(),
        file_name,
        destination.to_string_lossy().into_owned(),
        1,
    )
    .map_err(|error| error.to_string())?;
    task.kind = TaskKind::Media;
    task.status = TaskStatus::Queued;
    task.hash_state = HashState::Unavailable;
    task.validate().map_err(|error| error.to_string())?;
    journal.save_task(&task).map_err(|error| error.to_string())?;

    let job = MediaJob {
        task_id: task.id.clone(),
        url: url.to_string(),
        page_url: input.page_url,
        selector: input.selector.trim().to_string(),
        audio_format: input.audio_format,
        title: title.chars().take(300).collect(),
        created_at: now_ms(),
    };
    if let Err(error) = journal.save_media_job(&job) {
        let _ = journal.delete_task(&task.id);
        return Err(error.to_string());
    }
    if let Err(error) = spawn_media_run(
        app,
        journal.clone(),
        running,
        task.id.clone(),
    ) {
        let _ = journal.delete_task(&task.id);
        return Err(error);
    }
    Ok(task)
}

pub(crate) fn spawn_media_run(
    app: AppHandle,
    journal: Arc<Journal>,
    running: Arc<RunningTasks>,
    task_id: String,
) -> Result<(), String> {
    let cancel = CancellationToken::new();
    if !running.insert(&task_id, cancel.clone()) {
        return Err("download is already running".into());
    }
    tauri::async_runtime::spawn(async move {
        if let Err(message) = runner::run(app.clone(), journal, task_id.clone(), cancel).await {
            let _ = app.emit(
                ERROR_EVENT,
                BridgeError {
                    task_id: task_id.clone(),
                    message,
                },
            );
        }
        running.remove(&task_id);
    });
    Ok(())
}

pub(crate) fn std_command(program: &Path) -> Command {
    let mut command = Command::new(program);
    // Windows mặc định in theo bảng mã ANSI (cp1252…), làm hỏng tên file tiếng Việt.
    command.env("PYTHONIOENCODING", "utf-8").env("PYTHONUTF8", "1");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

pub(crate) fn tokio_command(program: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(program);
    command.env("PYTHONIOENCODING", "utf-8").env("PYTHONUTF8", "1");
    #[cfg(windows)]
    {
        command.creation_flags(0x0800_0000);
    }
    command
}

pub(crate) fn ffmpeg_location(tools: &tools::MediaTools) -> Option<String> {
    tools::ffmpeg_directory(tools.ffmpeg.as_deref())
}

fn safe_base(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                ' '
            } else {
                character
            }
        })
        .collect();
    let mut base = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    if base.chars().count() > 100 {
        base = base.chars().take(100).collect::<String>();
        base = base.trim_end_matches(['.', ' ']).to_string();
    }
    if base.is_empty() {
        base = "media".into();
    }
    if validate_file_name(&format!("{base}.mp4")).is_err() {
        base = format!("_{base}");
    }
    if validate_file_name(&format!("{base}.mp4")).is_err() {
        base = "media".into();
    }
    base
}

fn unique_base(destination: &Path, base: &str) -> String {
    for counter in 0..1000 {
        let candidate = if counter == 0 {
            base.to_string()
        } else {
            format!("{base} ({counter})")
        };
        if !conflicts(destination, &candidate) {
            return candidate;
        }
    }
    format!("{base} {}", now_ms())
}

fn conflicts(destination: &Path, base: &str) -> bool {
    let prefix = format!("{base}.");
    let variant_prefix = format!("{base} (");
    let Ok(entries) = std::fs::read_dir(destination) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        name == base || name.starts_with(&prefix) || name.starts_with(&variant_prefix)
    })
}
