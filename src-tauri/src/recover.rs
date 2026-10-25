//! Tự cứu download, phần app (AC-02, AC-03, AC-28, SPEC mục 9); engine phân loại lỗi và gợi ý giờ
//! (`utd_engine::recovery`).
//!
//! - Lượt chạy kết thúc (`after_run`): lỗi tạm thời ⇒ hẹn giờ thử lại giãn dần, có giới hạn; mất mạng ⇒
//!   chờ có mạng; đầy ổ / ổ rút ⇒ chờ đủ chỗ / ổ cắm lại; link hết hạn (401/403/410) ⇒ chờ link mới. Task
//!   chờ nằm ở `paused` kèm `recovery` (không giữ chỗ chạy trong hàng đợi); task chờ link mới vẫn `failed`.
//! - Vòng hẹn giờ (`start`, mỗi 2 s, theo giờ máy nên máy ngủ dậy là chạy ngay): tới giờ / có mạng / đủ chỗ
//!   ⇒ vào lại đầu hàng đợi.
//! - Cập nhật link: dán link mới, hoặc tải lại đúng file đó từ trình duyệt (`capture_link`, gọi từ ipc.rs).
//!   Engine chỉ nhận khi cùng file (`DownloadEngine::replace_link`), giữ phần đã tải.
//! - Lượt chạy kết thúc mà task còn "đang chạy" trong journal (engine/runner trả lỗi): đánh lỗi, không kẹt.
//! - Người dùng Tạm dừng / Tiếp tục ⇒ bỏ lịch tự thử lại (`forget`).

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use utd_engine::recovery::{self, Plan};
use utd_engine::{
    now_ms, DownloadEngine, DownloadTask, ErrorClass, Journal, LinkError, RecoveryWait, TaskError,
    TaskStatus,
};

use crate::events::TASK_EVENT;
use crate::queue::{DownloadQueue, Entry, Place};
use crate::state::AppState;

/// Nhịp của vòng hẹn giờ.
const TICK: Duration = Duration::from_secs(2);
/// Hỏi dung lượng ổ mỗi chừng này nhịp (10 s).
const DISK_EVERY: u32 = 5;
/// Kết quả cập nhật link gửi từ trình duyệt cho hộp "Cập nhật link" đang mở.
pub const LINK_EVENT: &str = "utd://link-updated";

/// Dung lượng ổ chứa thư mục tải: (trống, tổng); `None` = thư mục hoặc ổ không còn.
pub(crate) trait Disk: Send + Sync {
    fn space(&self, folder: &Path) -> Option<(u64, u64)>;
}

pub(crate) struct SystemDisk;

impl Disk for SystemDisk {
    fn space(&self, folder: &Path) -> Option<(u64, u64)> {
        if !folder.is_dir() {
            return None;
        }
        Some((
            fs2::available_space(folder).ok()?,
            fs2::total_space(folder).ok()?,
        ))
    }
}

/// Task đang mở hộp "Cập nhật link": link tải tiếp theo từ trình duyệt thử gắn vào task này trước.
#[derive(Default)]
pub struct Recover {
    awaiting: Mutex<Option<String>>,
}

impl Recover {
    fn awaiting(&self) -> Option<String> {
        self.awaiting.lock().ok().and_then(|slot| slot.clone())
    }

    fn set_awaiting(&self, task_id: Option<String>) {
        if let Ok(mut slot) = self.awaiting.lock() {
            *slot = task_id;
        }
    }
}

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// Xếp loại task vừa chạy xong một lượt. Trả task nếu đã đổi (để báo giao diện).
pub(crate) fn settle(
    journal: &Journal,
    task_id: &str,
    run_error: Option<&str>,
    online: bool,
    disk: &dyn Disk,
    now: i64,
) -> Option<DownloadTask> {
    let mut task = journal.load_task(task_id).ok().flatten()?;
    if task.parent_id.is_some() {
        return None;
    }
    let mut changed = false;
    if task.status.is_active() || task.status == TaskStatus::Queued {
        log::warn!("recover: task={task_id} kết thúc lượt ở {:?}", task.status);
        task.status = TaskStatus::Failed;
        task.error = Some(stuck_error(run_error));
        changed = true;
    }
    match task.status {
        TaskStatus::Failed => {
            let error = task.error.clone()?;
            match recovery::plan(&task, &error, online, now) {
                Plan::GiveUp => {
                    changed |= task.recovery.take().is_some();
                }
                Plan::Wait(wait) => {
                    let impossible = wait.need_bytes.is_some_and(|need| {
                        disk.space(Path::new(&task.dest_dir))
                            .is_some_and(|(_, total)| total < need)
                    });
                    if impossible {
                        // Ổ nhỏ hơn phần còn phải tải: chờ bao lâu cũng không đủ chỗ.
                        task.error = Some(too_small());
                        task.recovery = None;
                    } else {
                        if wait.wait != RecoveryWait::Link {
                            task.status = TaskStatus::Paused;
                        }
                        task.recovery = Some(wait);
                    }
                    changed = true;
                }
            }
        }
        TaskStatus::Completed | TaskStatus::Quarantined => {
            changed |= task.recovery.take().is_some();
        }
        _ => {}
    }
    if !changed {
        return None;
    }
    task.updated_at = now.max(task.created_at);
    if let Err(error) = journal.save_task(&task) {
        log::warn!("recover: không lưu được task={task_id}: {error}");
        return None;
    }
    Some(task)
}

/// Task tới lượt tự chạy lại (đã bỏ giờ hẹn trong journal, giữ số lần thử), theo giờ tạo.
pub(crate) fn due(
    journal: &Journal,
    online: bool,
    disk: &dyn Disk,
    check_disk: bool,
    now: i64,
) -> Vec<String> {
    let mut ready = Vec::new();
    for mut task in journal.waiting_tasks().unwrap_or_default() {
        let Some(wait) = task.recovery.as_mut() else {
            continue;
        };
        let timed = wait.retry_at.is_some_and(|at| at <= now);
        let go = match wait.wait {
            RecoveryWait::Retry => timed,
            RecoveryWait::Network => online || timed,
            RecoveryWait::Disk => {
                check_disk
                    && disk
                        .space(Path::new(&task.dest_dir))
                        .is_some_and(|(free, _)| wait.need_bytes.is_none_or(|need| free >= need))
            }
            RecoveryWait::Link => false,
        };
        if !go {
            continue;
        }
        wait.retry_at = None;
        if journal.save_task(&task).is_ok() {
            ready.push(task.id);
        }
    }
    ready
}

/// Bỏ lịch tự cứu (người dùng Tạm dừng / Tiếp tục / Thử lại). Trả task nếu đã đổi.
pub(crate) fn forget(journal: &Journal, task_id: &str) -> Option<DownloadTask> {
    let mut task = journal.load_task(task_id).ok().flatten()?;
    task.recovery.take()?;
    task.updated_at = now_ms().max(task.created_at);
    journal.save_task(&task).ok()?;
    Some(task)
}

fn stuck_error(run_error: Option<&str>) -> TaskError {
    let message = run_error
        .map(|message| message.chars().take(400).collect::<String>())
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| "The download stopped unexpectedly.".into());
    TaskError {
        class: ErrorClass::Transient,
        code: "internal.run".into(),
        message,
    }
}

fn too_small() -> TaskError {
    TaskError {
        class: ErrorClass::User,
        code: "disk.too_small".into(),
        message: "The drive is smaller than the file.".into(),
    }
}

/// Gọi khi một lượt chạy (HTTP hoặc media) kết thúc, trước khi nhả chỗ trong hàng đợi.
pub(crate) fn after_run(app: &AppHandle, task_id: &str, run_error: Option<&str>) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let online = recovery::has_route();
    if let Some(task) = settle(&state.journal, task_id, run_error, online, &SystemDisk, now_ms()) {
        let _ = app.emit(TASK_EVENT, &task);
    }
}

/// Vòng hẹn giờ: tới giờ / có mạng / đủ chỗ ⇒ xếp lại đầu hàng đợi.
pub(crate) fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticks = 0u32;
        loop {
            tokio::time::sleep(TICK).await;
            ticks = ticks.wrapping_add(1);
            let Some(state) = app.try_state::<AppState>() else {
                continue;
            };
            let journal = state.journal.clone();
            let check_disk = ticks.is_multiple_of(DISK_EVERY);
            // Hỏi route và dung lượng ổ (ổ mạng có thể chậm) ngoài luồng async.
            let ids = tauri::async_runtime::spawn_blocking(move || {
                due(&journal, recovery::has_route(), &SystemDisk, check_disk, now_ms())
            })
            .await
            .unwrap_or_default();
            if ids.is_empty() {
                continue;
            }
            log::info!("recover: chạy lại {} task", ids.len());
            let entries = ids.into_iter().map(Entry::new).collect();
            if let Err(error) = state.queue.submit(entries, Place::Front) {
                log::warn!("recover: không xếp lại được: {error}");
            }
        }
    });
}

/// Lỗi lệnh có mã cho giao diện dịch (`[recover.…] câu`).
fn link_error(error: &LinkError) -> String {
    match error {
        LinkError::Invalid => "[recover.invalid] Link không hợp lệ.".into(),
        LinkError::NotAllowed => {
            "[recover.not_allowed] Download này không đổi link được lúc này.".into()
        }
        LinkError::Unreachable(_) => {
            "[recover.unreachable] Không mở được link mới. Kiểm tra link rồi thử lại.".into()
        }
        LinkError::Page => "[recover.page] Link mới là một trang web, không phải file.".into(),
        LinkError::Size { .. } => {
            "[recover.size] Link mới là file khác kích thước. Phần đã tải được giữ nguyên.".into()
        }
        LinkError::Changed => {
            "[recover.changed] Link mới trỏ tới một file khác. Phần đã tải được giữ nguyên.".into()
        }
        LinkError::Unverifiable => {
            "[recover.unverifiable] Không xác nhận được link mới là cùng file nên chưa nhận.".into()
        }
    }
}

/// Thay link rồi xếp task vào đầu hàng đợi.
async fn relink(
    engine: &DownloadEngine,
    queue: &DownloadQueue,
    task_id: &str,
    url: &str,
) -> Result<DownloadTask, LinkError> {
    let task = engine.replace_link(task_id, url, Vec::new()).await?;
    if let Err(error) = queue.submit(vec![Entry::new(task_id)], Place::Front) {
        log::warn!("recover: đã thay link nhưng không xếp được task={task_id}: {error}");
    }
    Ok(task)
}

/// Cập nhật link: dán link mới cho download lỗi/đang dừng, giữ phần đã tải nếu cùng file.
#[tauri::command]
pub async fn update_download_link(
    state: State<'_, AppState>,
    recover: State<'_, Recover>,
    task_id: String,
    url: String,
) -> Result<DownloadTask, String> {
    if state.running.contains(&task_id) {
        return Err(link_error(&LinkError::NotAllowed));
    }
    let engine = state.engine.clone();
    let queue = state.queue.clone();
    let task = relink(&engine, &queue, &task_id, &url)
        .await
        .map_err(|error| link_error(&error))?;
    if recover.awaiting().as_deref() == Some(task_id.as_str()) {
        recover.set_awaiting(None);
    }
    Ok(task)
}

/// Hộp "Cập nhật link" mở (`Some`) / đóng (`None`): trong lúc mở, link tải từ trình duyệt tới trước
/// tiên thử gắn vào task này.
#[tauri::command]
pub fn await_download_link(recover: State<'_, Recover>, task_id: Option<String>) {
    recover.set_awaiting(task_id);
}

/// "Tải lại từ đầu" (file trên máy chủ đã đổi, `.part` mất): bỏ phần đã tải rồi xếp đầu hàng đợi.
#[tauri::command]
pub fn restart_download(state: State<'_, AppState>, task_id: String) -> Result<(), String> {
    if state.running.contains(&task_id) {
        return Err("download is already running".into());
    }
    state.engine.reset_progress(&task_id).map_err(text)?;
    state
        .queue
        .submit(vec![Entry::new(task_id)], Place::Front)
        .map(|_| ())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LinkUpdate {
    task_id: String,
    /// `None` = đã nhận; có = mã lỗi `[recover.…]` để hộp thoại dịch.
    error: Option<String>,
}

/// Link tải từ trình duyệt: nếu đúng là file của một download đang chờ link mới (hộp "Cập nhật link"
/// đang mở, hoặc task lỗi 401/403/410 cùng tên file) và cùng file thì gắn vào task đó, trả id. Không
/// khớp ⇒ `None`, trình duyệt tạo download mới như thường.
pub(crate) async fn capture_link(
    app: &AppHandle,
    engine: &Arc<DownloadEngine>,
    queue: &Arc<DownloadQueue>,
    url: &str,
    suggested_name: Option<&str>,
) -> Option<String> {
    let state = app.try_state::<AppState>()?;
    let awaiting = app.try_state::<Recover>().and_then(|recover| recover.awaiting());
    let candidates = link_candidates(&state.journal, awaiting.as_deref(), url, suggested_name);
    for task_id in candidates {
        if state.running.contains(&task_id) {
            continue;
        }
        let result = relink(engine, queue, &task_id, url).await;
        let chosen = awaiting.as_deref() == Some(task_id.as_str());
        if chosen || result.is_ok() {
            let update = LinkUpdate {
                task_id: task_id.clone(),
                error: result.as_ref().err().map(link_error),
            };
            let _ = app.emit(LINK_EVENT, update);
        }
        if result.is_ok() {
            if chosen && let Some(recover) = app.try_state::<Recover>() {
                recover.set_awaiting(None);
            }
            log::info!("recover: link từ trình duyệt gắn vào task={task_id}");
            return Some(task_id);
        }
    }
    None
}

/// Task có thể nhận link này: task đang mở hộp "Cập nhật link" trước, rồi task chờ link mới cùng tên file.
fn link_candidates(
    journal: &Journal,
    awaiting: Option<&str>,
    url: &str,
    suggested_name: Option<&str>,
) -> Vec<String> {
    let mut ids: Vec<String> = awaiting.map(str::to_string).into_iter().collect();
    let names: Vec<String> = suggested_name
        .map(str::to_string)
        .into_iter()
        .chain(url_file_name(url))
        .map(|name| name.to_lowercase())
        .collect();
    if names.is_empty() {
        return ids;
    }
    for task in journal.list_tasks().unwrap_or_default() {
        let waits_for_link = task.status == TaskStatus::Failed
            && task
                .recovery
                .as_ref()
                .is_some_and(|recovery| recovery.wait == RecoveryWait::Link);
        if waits_for_link
            && names.contains(&task.file_name.to_lowercase())
            && !ids.contains(&task.id)
        {
            ids.push(task.id);
        }
    }
    ids
}

fn url_file_name(url: &str) -> Option<String> {
    let parsed = tauri::Url::parse(url).ok()?;
    let last = parsed.path_segments()?.next_back()?.to_string();
    let decoded = percent_decode(&last);
    (!decoded.is_empty()).then_some(decoded)
}

/// Giải `%xx` trong tên file của URL (chỉ để so tên).
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (bytes[index], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                index += 3;
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
#[path = "recover_tests.rs"]
mod tests;
