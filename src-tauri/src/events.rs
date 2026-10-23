use serde::Serialize;
use tauri::{AppHandle, Emitter};
use utd_engine::EngineEvent;

pub const TASK_EVENT: &str = "utd://task";
pub const PROGRESS_EVENT: &str = "utd://progress";
pub const ERROR_EVENT: &str = "utd://error";
pub const TOOL_PROGRESS_EVENT: &str = "utd://tool-progress";
pub const UPDATE_PROGRESS_EVENT: &str = "utd://update-progress";
pub const UPDATE_AVAILABLE_EVENT: &str = "utd://update-available";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeError {
    pub task_id: String,
    pub message: String,
}

/// Tiến độ tải/cài công cụ media (yt-dlp, ffmpeg).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolProgress {
    pub tool: String,
    /// checksum · download · extract · check · install
    pub stage: &'static str,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

/// Tiến độ tải/cài bản UTD mới.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    /// download · install (đang dừng download, sao lưu, chạy bộ cài)
    pub stage: &'static str,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

pub fn emit_engine_event(app: &AppHandle, event: EngineEvent) {
    match event {
        EngineEvent::Task { task } => {
            let _ = app.emit(TASK_EVENT, *task);
        }
        EngineEvent::Progress { progress } => {
            let _ = app.emit(PROGRESS_EVENT, progress);
        }
    }
}
