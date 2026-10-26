//! Tự cứu download: lỗi nào tự thử lại, chờ bao lâu, khi nào chờ mạng / chỗ trống / link mới.
//! Engine phân loại lỗi và gợi ý giờ thử lại (Retry-After); app (src-tauri/src/recover.rs) giữ lịch,
//! theo dõi mạng và ổ đĩa, rồi xếp task vào lại hàng đợi.

use std::fmt;
use std::net::{IpAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use crate::auth::{AUTH_EXPIRED, AUTH_REQUIRED};
use crate::model::{DownloadTask, ErrorClass, Recovery, RecoveryWait, TaskError, TaskKind};

/// Lỗi tạm thời của mạng / máy chủ: tự thử lại chừng này lần liền nhau không thêm được byte nào.
pub const RETRY_LIMIT: u32 = 10;
/// Lỗi tạm thời không rõ nguyên nhân (vd yt-dlp báo chung chung): thử ít hơn.
const OTHER_LIMIT: u32 = 3;
/// Giãn dần giữa các lần thử (giây): 5 s … 30 phút, tổng 10 lần ≈ 1,6 giờ.
const BACKOFF_SECS: [u64; 9] = [5, 15, 30, 60, 120, 300, 600, 900, 1800];
/// Máy báo không có mạng: vẫn thử định kỳ (máy chỉ ra mạng qua proxy thì không có route mặc định).
pub const OFFLINE_POLL: Duration = Duration::from_secs(60);
/// Lượt tải vừa dừng vì mất mạng mà giờ đã có mạng lại: chạy lại gần như ngay.
const BACK_ONLINE: Duration = Duration::from_secs(1);
/// Máy chủ bảo chờ (Retry-After) lâu nhất chừng này; hơn nữa coi như máy chủ trả bừa.
pub const MAX_SERVER_WAIT: Duration = Duration::from_secs(6 * 60 * 60);
/// Ổ đầy: chừa thêm chừng này ngoài phần còn phải tải (journal, file tạm của hệ điều hành).
pub const DISK_MARGIN: u64 = 64 * 1024 * 1024;

/// Việc làm với một task vừa lỗi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Không tự cứu được: để nguyên lỗi cho người dùng (Doctor).
    GiveUp,
    /// Chờ rồi tự chạy lại (hoặc chờ người dùng gửi link mới với `RecoveryWait::Link`).
    Wait(Recovery),
}

/// Quyết định cho task vừa lỗi `error`. `online`: máy còn đường ra mạng (`has_route`); `now`: giờ máy
/// (ms). Gợi ý của engine (`task.recovery.retry_at`, từ Retry-After) được giữ nếu muộn hơn lịch giãn.
pub fn plan(task: &DownloadTask, error: &TaskError, online: bool, now: i64) -> Plan {
    let previous = task.recovery.as_ref();
    let code = error.code.as_str();
    if is_disk_wait(code) {
        let mut wait = Recovery::new(RecoveryWait::Disk);
        wait.need_bytes = (code == "disk.full").then(|| {
            task.remaining_bytes()
                .unwrap_or(0)
                .saturating_add(DISK_MARGIN)
        });
        wait.progress = task.downloaded_bytes;
        return Plan::Wait(wait);
    }
    let refused = is_link_refused(code) && task.kind == TaskKind::Http;
    if (refused || is_sign_in(code)) && task.parent_id.is_none() {
        let mut wait = Recovery::new(RecoveryWait::Link);
        wait.progress = task.downloaded_bytes;
        return Plan::Wait(wait);
    }
    if error.class != ErrorClass::Transient {
        return Plan::GiveUp;
    }
    let network = code.starts_with("network.") || code == "media.network";
    if network && !online {
        // Không có mạng: không đếm lần thử, chạy lại khi có mạng (app theo dõi) hoặc mỗi phút.
        let mut wait = carry(previous, RecoveryWait::Network, task.downloaded_bytes);
        wait.retry_at = Some(now + millis(OFFLINE_POLL));
        return Plan::Wait(wait);
    }
    if code == "network.offline" {
        // Lượt tải dừng vì mất mạng, giờ đã có lại: không tính là một lần thử hỏng.
        let mut wait = carry(previous, RecoveryWait::Retry, task.downloaded_bytes);
        wait.retry_at = Some(now + millis(BACK_ONLINE));
        return Plan::Wait(wait);
    }
    let moved = previous.is_none_or(|previous| task.downloaded_bytes > previous.progress);
    let attempts = if moved {
        1
    } else {
        previous.map_or(0, |previous| previous.attempts) + 1
    };
    let limit = if network || is_server_busy(code) {
        RETRY_LIMIT
    } else {
        OTHER_LIMIT
    };
    if attempts > limit {
        return Plan::GiveUp;
    }
    let delay = backoff(attempts, &task.id);
    let hinted = previous.and_then(|previous| previous.retry_at);
    let mut wait = Recovery::new(RecoveryWait::Retry);
    wait.attempts = attempts;
    wait.progress = task.downloaded_bytes;
    wait.retry_at = Some(hinted.unwrap_or(0).max(now + millis(delay)));
    Plan::Wait(wait)
}

/// Giữ số lần thử của lần chờ trước (chờ mạng không làm mất đếm).
fn carry(previous: Option<&Recovery>, wait: RecoveryWait, progress: u64) -> Recovery {
    let mut next = Recovery::new(wait);
    if let Some(previous) = previous {
        next.attempts = previous.attempts;
        next.progress = previous.progress.max(progress);
    } else {
        next.progress = progress;
    }
    next
}

/// Khoảng chờ lần thử thứ `attempt` (từ 1), lệch ±20% theo task để nhiều task không dồn một lúc.
pub fn backoff(attempt: u32, seed: &str) -> Duration {
    let index = (attempt.max(1) as usize - 1).min(BACKOFF_SECS.len() - 1);
    let base = BACKOFF_SECS[index] * 1000;
    let hash = seed
        .bytes()
        .fold(u64::from(attempt), |acc, byte| acc.wrapping_mul(31).wrapping_add(u64::from(byte)));
    let jitter = 80 + hash % 41;
    Duration::from_millis(base * jitter / 100)
}

/// Lỗi chờ ổ đĩa: đầy, hoặc ổ chứa thư mục tải đã rút.
pub fn is_disk_wait(code: &str) -> bool {
    matches!(code, "disk.full" | "disk.unplugged")
}

/// Máy chủ từ chối link (thường là link ký đã hết hạn, hoặc cần đăng nhập lại).
pub fn is_link_refused(code: &str) -> bool {
    matches!(code, "http.401" | "http.403" | "http.407" | "http.410")
}

/// Cần đăng nhập (lại): cookie bị từ chối, chưa có (mở lại app thì cookie ở RAM đã mất), hoặc yt-dlp đòi
/// đăng nhập. Chờ như link mới: người dùng cho phép lại trên trình duyệt (notes/auth-plan.md §8.1).
pub fn is_sign_in(code: &str) -> bool {
    matches!(code, AUTH_REQUIRED | AUTH_EXPIRED | "media.login")
}

fn is_server_busy(code: &str) -> bool {
    matches!(
        code,
        "http.408" | "http.425" | "http.429" | "media.rate_limited" | "media.network"
    ) || code.strip_prefix("http.5").is_some()
}

fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

/// Máy còn đường ra mạng không. Hỏi bảng định tuyến, không gửi gói nào (không telemetry, không ping
/// máy chủ lạ): `connect` một socket UDP tới địa chỉ dành cho tài liệu (RFC 5737, RFC 3849) chỉ chọn
/// đường đi; mất Wi-Fi/cáp ⇒ "network unreachable".
pub fn has_route() -> bool {
    route_address().is_some()
}

/// IP máy đang dùng để ra mạng. Đổi giữa hai lần hỏi ⇒ vừa đổi Wi-Fi↔4G, VPN, mạng khác.
pub fn route_address() -> Option<IpAddr> {
    probe_route("0.0.0.0:0", "192.0.2.1:9").or_else(|| probe_route("[::]:0", "[2001:db8::1]:9"))
}

fn probe_route(bind: &str, target: &str) -> Option<IpAddr> {
    let socket = UdpSocket::bind(bind).ok()?;
    socket.connect(target).ok()?;
    let address = socket.local_addr().ok()?.ip();
    (!address.is_unspecified() && !address.is_loopback()).then_some(address)
}

/// Cách hỏi đường ra mạng (mặc định `route_address`); bài thử thay bằng mạng giả để rớt mạng, đổi
/// mạng mà không phải rút cáp.
#[derive(Clone)]
pub struct RouteProbe(Arc<dyn Fn() -> Option<IpAddr> + Send + Sync>);

impl RouteProbe {
    pub fn new(probe: impl Fn() -> Option<IpAddr> + Send + Sync + 'static) -> Self {
        Self(Arc::new(probe))
    }

    pub fn address(&self) -> Option<IpAddr> {
        (self.0)()
    }
}

impl Default for RouteProbe {
    fn default() -> Self {
        Self::new(route_address)
    }
}

impl fmt::Debug for RouteProbe {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RouteProbe")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TaskStatus;

    fn task(downloaded: u64) -> DownloadTask {
        let mut task = DownloadTask::new(
            "https://example.com/a.iso",
            "a.iso",
            if cfg!(windows) { "C:\\Downloads" } else { "/tmp/downloads" },
            8,
        )
        .expect("task")
        .with_total_bytes(1000);
        task.downloaded_bytes = downloaded;
        task.status = TaskStatus::Failed;
        task
    }

    fn error(class: ErrorClass, code: &str) -> TaskError {
        TaskError::new(class, code, "x").expect("error")
    }

    fn waited(plan: Plan) -> Recovery {
        match plan {
            Plan::Wait(wait) => wait,
            Plan::GiveUp => panic!("expected a wait"),
        }
    }

    #[test]
    fn transient_errors_back_off_and_give_up_after_the_limit() {
        let mut task = task(100);
        let busy = error(ErrorClass::Transient, "http.503");
        let mut delays = Vec::new();
        for attempt in 1..=RETRY_LIMIT {
            let wait = waited(plan(&task, &busy, true, 0));
            assert_eq!(wait.wait, RecoveryWait::Retry);
            assert_eq!(wait.attempts, attempt);
            delays.push(wait.retry_at.expect("retry time"));
            task.recovery = Some(wait);
        }
        assert!(delays.windows(2).all(|pair| pair[0] <= pair[1]), "{delays:?}");
        assert!(delays[0] >= 4_000 && delays[0] <= 6_000, "{delays:?}");
        assert_eq!(plan(&task, &busy, true, 0), Plan::GiveUp);
    }

    #[test]
    fn new_bytes_restart_the_count() {
        let mut task = task(100);
        let dropped = error(ErrorClass::Transient, "network.body");
        for _ in 0..5 {
            task.recovery = Some(waited(plan(&task, &dropped, true, 0)));
        }
        assert_eq!(task.recovery.as_ref().unwrap().attempts, 5);
        task.downloaded_bytes = 500;
        assert_eq!(waited(plan(&task, &dropped, true, 0)).attempts, 1);
    }

    #[test]
    fn offline_waits_for_the_network_without_counting() {
        let mut task = task(100);
        let mut previous = Recovery::new(RecoveryWait::Retry);
        previous.attempts = 3;
        previous.progress = 100;
        task.recovery = Some(previous);
        let wait = waited(plan(&task, &error(ErrorClass::Transient, "network.connect"), false, 0));
        assert_eq!(wait.wait, RecoveryWait::Network);
        assert_eq!(wait.attempts, 3);
        assert_eq!(wait.retry_at, Some(60_000));
        // Lượt tải tự dừng vì mất mạng, nay có mạng lại: chạy lại gần như ngay, không đếm.
        let back = waited(plan(&task, &error(ErrorClass::Transient, "network.offline"), true, 0));
        assert_eq!((back.wait, back.attempts, back.retry_at), (RecoveryWait::Retry, 3, Some(1_000)));
    }

    #[test]
    fn retry_after_from_the_server_is_respected() {
        let mut task = task(0);
        let mut hint = Recovery::new(RecoveryWait::Retry);
        hint.retry_at = Some(3_600_000);
        task.recovery = Some(hint);
        let wait = waited(plan(&task, &error(ErrorClass::Transient, "http.429"), true, 0));
        assert_eq!(wait.retry_at, Some(3_600_000));
    }

    #[test]
    fn permanent_errors_disk_and_links_are_sorted() {
        let task = task(100);
        assert_eq!(
            plan(&task, &error(ErrorClass::Permanent, "http.404"), true, 0),
            Plan::GiveUp
        );
        assert_eq!(
            plan(&task, &error(ErrorClass::Permanent, "file.changed"), true, 0),
            Plan::GiveUp
        );
        let disk = waited(plan(&task, &error(ErrorClass::User, "disk.full"), true, 0));
        assert_eq!(disk.wait, RecoveryWait::Disk);
        assert_eq!(disk.need_bytes, Some(900 + DISK_MARGIN));
        let link = waited(plan(&task, &error(ErrorClass::User, "http.403"), true, 0));
        assert_eq!((link.wait, link.retry_at), (RecoveryWait::Link, None));
        let media = task.clone().with_kind(TaskKind::Media);
        assert_eq!(
            plan(&media, &error(ErrorClass::User, "http.403"), true, 0),
            Plan::GiveUp,
            "media tự hỏi yt-dlp link mới"
        );
        for code in [AUTH_REQUIRED, AUTH_EXPIRED, "media.login"] {
            for task in [&task, &media] {
                let wait = waited(plan(task, &error(ErrorClass::User, code), true, 0));
                assert_eq!((wait.wait, wait.retry_at), (RecoveryWait::Link, None), "{code}");
            }
        }
        let part = task.clone().with_parent("media-1");
        assert_eq!(plan(&part, &error(ErrorClass::User, AUTH_EXPIRED), true, 0), Plan::GiveUp);
        assert_eq!(
            plan(&task, &error(ErrorClass::User, "auth.pipe"), true, 0),
            Plan::GiveUp,
            "đăng nhập lại không sửa được pipe"
        );
        let vague = error(ErrorClass::Transient, "media.failed");
        let mut task = task;
        for _ in 0..OTHER_LIMIT {
            task.recovery = Some(waited(plan(&task, &vague, true, 0)));
        }
        assert_eq!(plan(&task, &vague, true, 0), Plan::GiveUp);
    }

    #[test]
    fn route_probe_sends_nothing_and_answers_quickly() {
        let started = std::time::Instant::now();
        let _ = route_address();
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
