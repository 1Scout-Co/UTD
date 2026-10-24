//! Nhịp của một lượt tải: ghi mốc thời gian từng pha vào log (`engine.timing`) để đo đúng chỗ
//! chậm, và nhớ lúc nhận byte gần nhất để một kết nối biết mình treo trong khi kết nối khác vẫn chạy.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::model::now_ms;

pub(crate) struct Pulse {
    /// `task=<id>`, task con của media thì kèm `parent=<id>` để ghép với mốc `media.timing` của app.
    label: String,
    started: Instant,
    /// µs tính từ `started` của lần ghi byte gần nhất, cộng 1 (0 = chưa nhận byte nào).
    last_byte: AtomicU64,
    /// Số kết nối đang chờ byte (trong `Watch::wait`): mở mà im, hoặc đang nhận dở.
    waiting: AtomicU32,
}

/// Giữ một chỗ trong số kết nối đang chờ byte; trả lại khi drop (kể cả khi bị huỷ giữa chừng).
pub(crate) struct Waiting<'a>(&'a Pulse);

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.waiting.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Pulse {
    pub fn new(task: &str, parent: Option<&str>) -> Arc<Self> {
        let label = match parent {
            Some(parent) => format!("task={task} parent={parent}"),
            None => format!("task={task}"),
        };
        Arc::new(Self {
            label,
            started: Instant::now(),
            last_byte: AtomicU64::new(0),
            waiting: AtomicU32::new(0),
        })
    }

    /// Một dòng log cho mỗi mốc: `ms` tính từ lúc bắt đầu lượt, `at` là giờ máy (ms) để ghép với
    /// mốc của app.
    pub fn mark(&self, phase: &str) {
        log::info!(
            "engine.timing {} phase={phase} ms={} at={}",
            self.label,
            self.started.elapsed().as_millis(),
            now_ms()
        );
    }

    /// Gọi sau mỗi lần ghi được byte vào file; lần đầu ghi mốc `first_byte`.
    pub fn beat(&self) {
        let now = self.offset(Instant::now());
        if self.last_byte.fetch_max(now, Ordering::AcqRel) == 0 {
            self.mark("first_byte");
        }
    }

    /// Có kết nối nào ghi byte sau `since` không.
    pub fn heard_since(&self, since: Instant) -> bool {
        self.last_byte.load(Ordering::Acquire) > self.offset(since)
    }

    /// Đánh dấu một kết nối đang chờ byte, tới khi trả guard.
    pub fn waiting(&self) -> Waiting<'_> {
        self.waiting.fetch_add(1, Ordering::AcqRel);
        Waiting(self)
    }

    /// Ngoài kết nối đang hỏi còn kết nối khác đang chờ byte.
    pub fn others_waiting(&self) -> bool {
        self.waiting.load(Ordering::Acquire) > 1
    }

    fn offset(&self, at: Instant) -> u64 {
        let micros = at.saturating_duration_since(self.started).as_micros();
        u64::try_from(micros).unwrap_or(u64::MAX - 1) + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hears_only_bytes_written_after_the_silence_began() {
        let pulse = Pulse::new("task", None);
        pulse.beat();
        let silent_from = Instant::now();
        assert!(!pulse.heard_since(silent_from));
        std::thread::sleep(std::time::Duration::from_millis(2));
        pulse.beat();
        assert!(pulse.heard_since(silent_from));
    }

    #[test]
    fn counts_other_waiting_connections_until_their_guard_drops() {
        let pulse = Pulse::new("task", None);
        let mine = pulse.waiting();
        assert!(!pulse.others_waiting());
        let other = pulse.waiting();
        assert!(pulse.others_waiting());
        drop(other);
        assert!(!pulse.others_waiting());
        drop(mine);
    }
}
