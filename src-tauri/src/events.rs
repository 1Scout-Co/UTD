use serde::Serialize;
use tauri::{AppHandle, Emitter};
use utd_engine::EngineEvent;

pub const TASK_EVENT: &str = "utd://task";
pub const PROGRESS_EVENT: &str = "utd://progress";
pub const ERROR_EVENT: &str = "utd://error";
pub const TOOL_PROGRESS_EVENT: &str = "utd://tool-progress";
/// Trạng thái tự tải/cập nhật công cụ media (media/auto.rs).
pub const TOOLS_AUTO_EVENT: &str = "utd://tools-auto";
pub const UPDATE_PROGRESS_EVENT: &str = "utd://update-progress";
pub const UPDATE_AVAILABLE_EVENT: &str = "utd://update-available";
/// Link trang video gửi từ trình duyệt: giao diện mở hộp Tải media với link này (media_pages.rs).
pub const MEDIA_REQUEST_EVENT: &str = "utd://media-request";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRequest {
    pub url: String,
    /// Trang đang mở khi chuột phải (nếu trình duyệt gửi kèm).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_url: Option<String>,
}

/// Lô link gửi từ trình duyệt ("Tải tất cả link bằng UTD"): giao diện mở hộp Thêm hàng loạt (batch.rs).
pub const BATCH_REQUEST_EVENT: &str = "utd://batch-request";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRequest {
    /// Đã kiểm (`batch::clean_links`): http/https, không trùng, giữ thứ tự trên trang.
    pub urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeError {
    pub task_id: String,
    pub message: String,
}

/// Tiến độ tải/cài công cụ media (yt-dlp, deno, ffmpeg).
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
