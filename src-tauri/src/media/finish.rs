//! Phần chung của mọi lượt tải media: báo giao diện, dừng/lỗi, và bước cuối (file nằm trong
//! thư mục đã chọn, Download Guard, Mark of the Web) cho cả yt-dlp lẫn đường tải thẳng.

use std::path::Path;

use tauri::{AppHandle, Emitter};
use utd_engine::{
    now_ms, validate_file_name, DownloadTask, ErrorClass, HashState, Journal, ProgressSnapshot,
    TaskError, TaskStatus,
};

use crate::events::{PROGRESS_EVENT, TASK_EVENT};

/// Nơi nhận sự kiện của task media (giao diện; bài thử ghi lại).
pub(crate) trait Sink: Send + Sync {
    fn task(&self, task: &DownloadTask);
    fn progress(&self, progress: ProgressSnapshot);
}

impl Sink for AppHandle {
    fn task(&self, task: &DownloadTask) {
        let _ = self.emit(TASK_EVENT, task);
    }

    fn progress(&self, progress: ProgressSnapshot) {
        let _ = self.emit(PROGRESS_EVENT, progress);
    }
}

pub(super) fn emit_task(sink: &dyn Sink, task: &DownloadTask) {
    sink.task(task);
}

pub(super) fn emit_progress(sink: &dyn Sink, task: &DownloadTask, downloaded: u64, speed: u64) {
    sink.progress(ProgressSnapshot {
        task_id: task.id.clone(),
        status: task.status,
        downloaded_bytes: downloaded,
        total_bytes: task.total_bytes,
        bytes_per_second: speed,
        connections: 1,
        segments_done: 0,
        segments_total: 0,
        hash_state: task.hash_state,
    });
}

pub(super) fn error(class: ErrorClass, code: &str, message: &str) -> TaskError {
    TaskError {
        class,
        code: code.into(),
        message: message.into(),
    }
}

pub(super) fn clamp(downloaded: u64, total: Option<u64>) -> u64 {
    total.map_or(downloaded, |total| downloaded.min(total))
}

pub(super) fn fail(
    sink: &dyn Sink,
    journal: &Journal,
    task: &mut DownloadTask,
    task_error: TaskError,
) -> Result<(), String> {
    task.status = TaskStatus::Failed;
    task.error = Some(task_error);
    task.updated_at = now_ms();
    journal.save_task(task).map_err(|error| error.to_string())?;
    emit_task(sink, task);
    Ok(())
}

pub(super) fn pause(
    sink: &dyn Sink,
    journal: &Journal,
    task: &mut DownloadTask,
    downloaded: u64,
) -> Result<(), String> {
    task.status = TaskStatus::Paused;
    task.downloaded_bytes = clamp(downloaded, task.total_bytes);
    task.updated_at = now_ms();
    task.error = None;
    journal.save_task(task).map_err(|error| error.to_string())?;
    emit_task(sink, task);
    Ok(())
}

pub(super) fn set_status(
    sink: &dyn Sink,
    journal: &Journal,
    task: &mut DownloadTask,
    status: TaskStatus,
) -> Result<(), String> {
    task.status = status;
    task.error = None;
    task.updated_at = now_ms();
    journal.save_task(task).map_err(|error| error.to_string())?;
    emit_task(sink, task);
    Ok(())
}

/// File cuối `path` đã nằm trong `destination`: kiểm chỗ ghi, rỗng, Guard (cách ly nếu nguy
/// hiểm), gắn Mark of the Web rồi đánh dấu xong. Tên file của task theo tên thật trên đĩa.
pub(super) fn finalize(
    sink: &dyn Sink,
    journal: &Journal,
    task: &mut DownloadTask,
    destination: &Path,
    path: &Path,
) -> Result<(), String> {
    let destination_canonical = destination
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let file_canonical = path.canonicalize().map_err(|error| error.to_string())?;
    if !file_canonical.starts_with(&destination_canonical) {
        return fail(
            sink,
            journal,
            task,
            error(
                ErrorClass::Security,
                "media.path_escape",
                "File media được ghi ra ngoài thư mục đã chọn.",
            ),
        );
    }
    let size = std::fs::metadata(&file_canonical)
        .map_err(|error| error.to_string())?
        .len();
    if size == 0 {
        return fail(
            sink,
            journal,
            task,
            error(
                ErrorClass::Permanent,
                "media.empty_output",
                "File media tải về rỗng.",
            ),
        );
    }
    let actual_name = file_canonical
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&task.file_name)
        .to_string();
    if validate_file_name(&actual_name).is_ok() {
        task.file_name = actual_name;
    }
    let Ok(guard) = utd_engine::safety::inspect(&file_canonical, &task.file_name) else {
        return fail(
            sink,
            journal,
            task,
            error(
                ErrorClass::Permanent,
                "guard.read",
                "Không đọc được file để kiểm tra an toàn.",
            ),
        );
    };
    if guard.action == utd_engine::safety::GuardAction::Quarantine {
        let quarantine = crate::ipc::data_dir()
            .map(|directory| directory.join("quarantine"))
            .ok();
        if utd_engine::safety::quarantine(&file_canonical, &task.file_name, quarantine.as_deref())
            .is_err()
        {
            return fail(
                sink,
                journal,
                task,
                error(
                    ErrorClass::Permanent,
                    "guard.quarantine_failed",
                    "Không chuyển được file nguy hiểm vào khu cách ly.",
                ),
            );
        }
        task.status = TaskStatus::Quarantined;
        task.total_bytes = Some(size);
        task.downloaded_bytes = size;
        task.error = Some(TaskError {
            class: ErrorClass::Security,
            code: guard.code,
            message: guard.reason,
        });
        task.updated_at = now_ms();
        journal.save_task(task).map_err(|error| error.to_string())?;
        emit_task(sink, task);
        return Ok(());
    }
    let _ = utd_engine::safety::mark_of_the_web(&file_canonical);
    task.status = TaskStatus::Completed;
    task.total_bytes = Some(size);
    task.downloaded_bytes = size;
    task.hash_state = HashState::Unavailable;
    task.completed_at = Some(now_ms());
    task.updated_at = now_ms();
    task.error = None;
    journal.save_task(task).map_err(|error| error.to_string())?;
    emit_task(sink, task);
    if let Ok(metadata) = std::fs::metadata(&task.temp_path)
        && metadata.is_file()
    {
        let _ = std::fs::remove_file(&task.temp_path);
    }
    Ok(())
}
