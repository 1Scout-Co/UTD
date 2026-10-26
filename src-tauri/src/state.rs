use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio_util::sync::CancellationToken;
use utd_engine::{DownloadEngine, Journal};

use crate::queue::DownloadQueue;

pub struct AppState {
    pub engine: Arc<DownloadEngine>,
    pub journal: Arc<Journal>,
    pub running: Arc<RunningTasks>,
    /// Mọi download (HTTP, media) chạy qua hàng đợi chung: giới hạn số chạy cùng lúc.
    pub queue: Arc<DownloadQueue>,
    /// Cookie đăng nhập của trình duyệt, chỉ ở RAM (src/auth).
    pub auth: Arc<crate::auth::AuthStore>,
}

#[derive(Default)]
pub struct RunningTasks {
    tokens: Mutex<HashMap<String, CancellationToken>>,
}

impl RunningTasks {
    /// Chỉ thêm khi task chưa chạy; đã có thì giữ nguyên token cũ (không làm Tạm dừng mất tác dụng).
    pub fn insert(&self, task_id: &str, token: CancellationToken) -> bool {
        use std::collections::hash_map::Entry;
        match self.tokens.lock() {
            Ok(mut tokens) => match tokens.entry(task_id.to_string()) {
                Entry::Vacant(slot) => {
                    slot.insert(token);
                    true
                }
                Entry::Occupied(_) => false,
            },
            Err(_) => false,
        }
    }

    pub fn contains(&self, task_id: &str) -> bool {
        self.tokens
            .lock()
            .map(|tokens| tokens.contains_key(task_id))
            .unwrap_or(false)
    }

    pub fn cancel(&self, task_id: &str) -> bool {
        match self.tokens.lock() {
            Ok(tokens) => match tokens.get(task_id) {
                Some(token) => {
                    token.cancel();
                    true
                }
                None => false,
            },
            Err(_) => false,
        }
    }

    pub fn remove(&self, task_id: &str) {
        if let Ok(mut tokens) = self.tokens.lock() {
            tokens.remove(task_id);
        }
    }

    /// Không có download nào đang chạy (macOS dùng để nhả App Nap: app_nap.rs).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.tokens.lock().map(|tokens| tokens.is_empty()).unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_insert_keeps_the_running_token() {
        let running = RunningTasks::default();
        let first = CancellationToken::new();
        assert!(running.insert("t", first.clone()));
        assert!(!running.insert("t", CancellationToken::new()));
        assert!(running.cancel("t"));
        assert!(first.is_cancelled(), "Tạm dừng phải huỷ đúng lượt tải đang chạy");
        assert!(!running.is_empty());
        running.remove("t");
        assert!(running.is_empty());
    }
}
