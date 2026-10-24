//! Hàng đợi chung của mọi download: thêm lẻ, hàng loạt, từ trình duyệt và media (kể cả phiên tải cả
//! kênh gọi `add_media_download` hàng chục lần). Chạy cùng lúc tối đa `limit` task (Cài đặt, mặc định
//! 3); task còn lại nằm ở trạng thái `queued`, một lượt chạy kết thúc (xong, dừng, lỗi) thì chạy task kế.
//!
//! - Thứ tự chờ lưu trong bảng settings (`queue_order`) mỗi lần đổi: mở lại app thì chạy tiếp đúng thứ tự.
//! - Mở app: task đang chạy lúc app tắt (bị ngắt) hoặc dừng để cài bản mới lên đầu hàng, rồi tới hàng cũ.
//! - Thêm lẻ, Tiếp tục, "Bắt đầu ngay" ⇒ đầu hàng (`Place::Front`, giữ thứ tự trong lô); "Thêm vào hàng
//!   đợi" và lô tự động (tải cả kênh) ⇒ cuối hàng (`Place::Back`).
//! - Tạm dừng task đang chờ: bỏ khỏi hàng, trạng thái `paused`. Task đang chạy thì người gọi huỷ token.
//! - Giảm giới hạn không dừng task đang chạy; chỉ chưa chạy thêm cho tới khi dưới giới hạn.
//! - Task con ẩn của media (luồng hình/tiếng) không qua đây: task cha giữ một chỗ.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use utd_engine::{DownloadTask, Journal, TaskKind, TaskStatus};

use crate::events::TASK_EVENT;
use crate::state::AppState;

pub const DEFAULT_LIMIT: usize = 3;
/// Cài đặt cho chọn 1–6, 8, 10 (`settings-queue.tsx`).
pub const MAX_LIMIT: usize = 10;
const LIMIT_KEY: &str = "queue_limit";
const ORDER_KEY: &str = "queue_order";
/// Task đang bận (đổi tên, chuyển thư mục): thử chạy lại sau chừng này.
const BUSY_RETRY: Duration = Duration::from_millis(500);

/// Một task trong hàng. `rename`: chưa tải byte nào và người dùng không đặt tên ⇒ lấy tên server đề xuất.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rename: bool,
}

impl Entry {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            rename: false,
        }
    }
}

/// Chỗ chen vào hàng.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Front,
    Back,
}

/// Kết quả khi hàng đợi gọi chạy một task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    Started,
    /// Task đang bận (đổi tên, chuyển thư mục…): để lại đầu hàng, thử lại sau.
    Busy,
}

/// Phần chạy thật (Tauri) hoặc phần giả của bài thử.
pub trait Runtime: Send + Sync {
    /// Chạy task; khi lượt chạy kết thúc (xong, dừng, lỗi) phải gọi [`DownloadQueue::release`].
    fn launch(&self, task: &DownloadTask, entry: &Entry) -> Launch;
    /// Trạng thái task đổi do hàng đợi (vào hàng, dừng khi đang chờ): báo giao diện.
    fn changed(&self, task: &DownloadTask);
    /// Gọi lại [`DownloadQueue::pump`] sau một lúc (task bận).
    fn retry_later(&self);
}

#[derive(Default)]
struct State {
    limit: usize,
    waiting: VecDeque<Entry>,
    /// Task đang giữ chỗ (từ lúc gọi chạy tới `release`).
    active: Vec<Entry>,
    /// Đang cài bản mới: không chạy thêm task nào.
    frozen: bool,
}

impl State {
    fn holds(&self, id: &str) -> bool {
        self.active.iter().any(|entry| entry.id == id)
            || self.waiting.iter().any(|entry| entry.id == id)
    }
}

pub struct DownloadQueue {
    journal: Arc<Journal>,
    state: Mutex<State>,
    runtime: OnceLock<Arc<dyn Runtime>>,
}

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

impl DownloadQueue {
    pub fn new(journal: Arc<Journal>) -> Arc<Self> {
        let limit = journal
            .get_setting(LIMIT_KEY)
            .ok()
            .flatten()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .map_or(DEFAULT_LIMIT, |value| value.clamp(1, MAX_LIMIT));
        Arc::new(Self {
            journal,
            state: Mutex::new(State {
                limit,
                ..State::default()
            }),
            runtime: OnceLock::new(),
        })
    }

    /// Gắn phần chạy rồi chạy các task đang chờ. Trước đó task chỉ được xếp hàng (IPC tới sớm).
    pub fn start(&self, runtime: Arc<dyn Runtime>) {
        let _ = self.runtime.set(runtime);
        self.pump();
    }

    pub fn limit(&self) -> usize {
        self.lock().limit
    }

    /// Lưu giới hạn (1..=`MAX_LIMIT`), trả giá trị đã dùng; tăng thì chạy thêm ngay.
    pub fn set_limit(&self, limit: usize) -> Result<usize, String> {
        let limit = limit.clamp(1, MAX_LIMIT);
        self.journal
            .set_setting(LIMIT_KEY, &limit.to_string())
            .map_err(text)?;
        self.lock().limit = limit;
        self.pump();
        Ok(limit)
    }

    /// Xếp task vào hàng (bỏ task đã xong, không có, hoặc đã trong hàng/đang chạy); trả task đã xếp.
    pub fn submit(&self, entries: Vec<Entry>, place: Place) -> Result<Vec<DownloadTask>, String> {
        let mut queued = Vec::new();
        {
            let mut state = self.lock();
            let mut fresh: Vec<Entry> = Vec::new();
            for entry in entries {
                if state.holds(&entry.id) || fresh.iter().any(|item| item.id == entry.id) {
                    continue;
                }
                let Some(mut task) = self.journal.load_task(&entry.id).map_err(text)? else {
                    continue;
                };
                if task.status == TaskStatus::Completed || task.parent_id.is_some() {
                    continue;
                }
                if task.status != TaskStatus::Queued {
                    self.journal
                        .set_task_status(&task.id, TaskStatus::Queued)
                        .map_err(text)?;
                    task.status = TaskStatus::Queued;
                }
                queued.push(task);
                fresh.push(entry);
            }
            match place {
                Place::Back => state.waiting.extend(fresh),
                Place::Front => {
                    for entry in fresh.into_iter().rev() {
                        state.waiting.push_front(entry);
                    }
                }
            }
            self.persist(&state);
        }
        if let Some(runtime) = self.runtime.get() {
            for task in &queued {
                runtime.changed(task);
            }
        }
        self.pump();
        Ok(queued)
    }

    /// Chạy task đầu hàng tới khi đủ giới hạn. Không giữ khoá lúc gọi chạy: lượt tải xong rất nhanh
    /// gọi `release` ngay.
    pub fn pump(&self) {
        let Some(runtime) = self.runtime.get() else {
            return;
        };
        while let Some((task, entry)) = self.next_ready() {
            if runtime.launch(&task, &entry) == Launch::Busy {
                let mut state = self.lock();
                state.active.retain(|item| item.id != entry.id);
                state.waiting.push_front(entry);
                self.persist(&state);
                drop(state);
                runtime.retry_later();
                return;
            }
        }
    }

    /// Lấy task kế còn chạy được (vẫn `queued` trong journal) và giữ chỗ cho nó.
    fn next_ready(&self) -> Option<(DownloadTask, Entry)> {
        let mut state = self.lock();
        let mut picked = None;
        let mut popped = false;
        while !state.frozen && state.active.len() < state.limit {
            let Some(entry) = state.waiting.pop_front() else {
                break;
            };
            popped = true;
            // Đã xoá, đã xong hoặc đổi trạng thái ngoài hàng đợi ⇒ bỏ.
            if let Ok(Some(task)) = self.journal.load_task(&entry.id)
                && task.status == TaskStatus::Queued
            {
                state.active.push(entry.clone());
                picked = Some((task, entry));
                break;
            }
        }
        if popped {
            self.persist(&state);
        }
        picked
    }

    /// Lượt chạy của task kết thúc: nhả chỗ, chạy task kế.
    pub fn release(&self, id: &str) {
        let removed = {
            let mut state = self.lock();
            let before = state.active.len();
            state.active.retain(|entry| entry.id != id);
            let removed = state.active.len() != before;
            if removed {
                self.persist(&state);
            }
            removed
        };
        if removed {
            self.pump();
        }
    }

    /// Tạm dừng task chưa chạy: bỏ khỏi hàng, `paused`. `false` ⇒ task đang chạy (người gọi huỷ token)
    /// hoặc không có trong hàng.
    pub fn pause(&self, id: &str) -> Result<bool, String> {
        let task = {
            let mut state = self.lock();
            let position = state.waiting.iter().position(|entry| entry.id == id);
            // `queued` trong journal mà không nằm trong hàng (lệch sau lỗi): vẫn cho dừng.
            let stray = position.is_none()
                && !state.active.iter().any(|entry| entry.id == id)
                && self
                    .journal
                    .load_task(id)
                    .ok()
                    .flatten()
                    .is_some_and(|task| task.status == TaskStatus::Queued);
            if position.is_none() && !stray {
                return Ok(false);
            }
            if let Some(position) = position {
                state.waiting.remove(position);
                self.persist(&state);
            }
            self.journal
                .set_task_status(id, TaskStatus::Paused)
                .map_err(text)?;
            self.journal.load_task(id).map_err(text)?
        };
        if let (Some(task), Some(runtime)) = (task, self.runtime.get()) {
            runtime.changed(&task);
        }
        Ok(true)
    }

    /// Bỏ task khỏi hàng (trước khi xoá khỏi danh sách); không đổi trạng thái.
    pub fn forget(&self, id: &str) {
        let mut state = self.lock();
        state.waiting.retain(|entry| entry.id != id);
        self.persist(&state);
    }

    /// Thứ tự chờ hiện tại (id), đầu hàng trước.
    #[cfg(test)]
    pub fn waiting(&self) -> Vec<String> {
        self.lock()
            .waiting
            .iter()
            .map(|entry| entry.id.clone())
            .collect()
    }

    /// Đang cài bản mới: không chạy thêm task (task bị dừng để cài không nhường chỗ cho task khác).
    pub fn freeze(&self) {
        self.lock().frozen = true;
    }

    pub fn thaw(&self) {
        self.lock().frozen = false;
        self.pump();
    }

    /// Dựng lại hàng lúc mở app: `front` (bị ngắt giữa chừng, dừng để cài bản mới) lên đầu, rồi task
    /// `queued` theo thứ tự đã lưu, cuối cùng task `queued` không có trong thứ tự (journal cũ) theo giờ tạo.
    pub fn restore(&self, front: &[String]) -> Result<(), String> {
        let saved: Vec<Entry> = self
            .journal
            .get_setting(ORDER_KEY)
            .map_err(text)?
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default();
        let rename_of = |id: &str| saved.iter().any(|entry| entry.id == id && entry.rename);
        let mut order: Vec<Entry> = Vec::new();
        for id in front {
            let Some(task) = self.journal.load_task(id).map_err(text)? else {
                continue;
            };
            let resumable = task.parent_id.is_none()
                && !matches!(task.status, TaskStatus::Completed | TaskStatus::Quarantined);
            if !resumable || order.iter().any(|entry| entry.id == *id) {
                continue;
            }
            if task.status != TaskStatus::Queued {
                self.journal
                    .set_task_status(id, TaskStatus::Queued)
                    .map_err(text)?;
            }
            order.push(Entry {
                id: id.clone(),
                rename: rename_of(id),
            });
        }
        let mut queued: Vec<DownloadTask> = self
            .journal
            .list_tasks()
            .map_err(text)?
            .into_iter()
            .filter(|task| task.status == TaskStatus::Queued)
            .collect();
        queued.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then(left.id.cmp(&right.id))
        });
        let known = |order: &[Entry], id: &str| order.iter().any(|entry| entry.id == id);
        for entry in saved
            .iter()
            .filter(|entry| queued.iter().any(|task| task.id == entry.id))
        {
            if !known(&order, &entry.id) {
                order.push(entry.clone());
            }
        }
        for task in &queued {
            if !known(&order, &task.id) {
                order.push(Entry::new(task.id.clone()));
            }
        }
        let mut state = self.lock();
        state.waiting = order.into();
        self.persist(&state);
        Ok(())
    }

    /// Ghi thứ tự (đang chạy rồi đang chờ): app bị tắt ngang thì task vừa giữ chỗ vẫn đứng đầu.
    fn persist(&self, state: &State) {
        let order: Vec<&Entry> = state.active.iter().chain(state.waiting.iter()).collect();
        let saved = serde_json::to_string(&order)
            .map_err(text)
            .and_then(|value| self.journal.set_setting(ORDER_KEY, &value).map_err(text));
        if let Err(error) = saved {
            log::warn!("Queue order save failed: {error}");
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Task top-level đang chạy dở trong journal (app tắt ngang): gọi trước `Journal::mark_interrupted`.
pub fn interrupted(journal: &Journal) -> Vec<String> {
    let mut tasks: Vec<DownloadTask> = journal
        .list_tasks()
        .unwrap_or_default()
        .into_iter()
        .filter(|task| {
            matches!(
                task.status,
                TaskStatus::Resolving | TaskStatus::Downloading | TaskStatus::Verifying
            )
        })
        .collect();
    tasks.sort_by_key(|task| task.created_at);
    tasks.into_iter().map(|task| task.id).collect()
}

/// Chạy thật trong app: HTTP qua engine, media qua runner; báo giao diện bằng `utd://task`.
pub struct AppRuntime(pub AppHandle);

impl Runtime for AppRuntime {
    fn launch(&self, task: &DownloadTask, entry: &Entry) -> Launch {
        let Some(state) = self.0.try_state::<AppState>() else {
            return Launch::Busy;
        };
        let started = if task.kind == TaskKind::Media {
            crate::media::spawn_media_run(
                self.0.clone(),
                state.journal.clone(),
                state.running.clone(),
                task.id.clone(),
            )
        } else {
            crate::commands::spawn_run(
                self.0.clone(),
                state.engine.clone(),
                state.running.clone(),
                task.id.clone(),
                !entry.rename,
                entry.rename,
            )
        };
        if started.is_ok() {
            Launch::Started
        } else {
            Launch::Busy
        }
    }

    fn changed(&self, task: &DownloadTask) {
        let _ = self.0.emit(TASK_EVENT, task);
    }

    fn retry_later(&self) {
        let app = self.0.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(BUSY_RETRY).await;
            if let Some(state) = app.try_state::<AppState>() {
                state.queue.pump();
            }
        });
    }
}

/// Lượt chạy của task kết thúc (gọi ở cuối `spawn_run` / `spawn_media_run`).
pub(crate) fn finished(app: &AppHandle, task_id: &str) {
    if let Some(state) = app.try_state::<AppState>() {
        state.queue.release(task_id);
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueSettings {
    pub limit: usize,
    pub max_limit: usize,
}

#[tauri::command]
pub fn queue_settings(state: tauri::State<'_, AppState>) -> QueueSettings {
    QueueSettings {
        limit: state.queue.limit(),
        max_limit: MAX_LIMIT,
    }
}

#[tauri::command]
pub fn set_queue_limit(
    state: tauri::State<'_, AppState>,
    limit: usize,
) -> Result<QueueSettings, String> {
    let limit = state.queue.set_limit(limit)?;
    Ok(QueueSettings {
        limit,
        max_limit: MAX_LIMIT,
    })
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
