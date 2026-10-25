//! Bài thử tự cứu ở mức app: xếp loại sau một lượt chạy, vòng hẹn giờ, chọn task nhận link từ trình duyệt.

use std::path::PathBuf;
use std::sync::Mutex;

use utd_engine::{HashState, Recovery};

use super::*;

struct Fixture {
    root: PathBuf,
    journal: Journal,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("utd-recover-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let journal = Journal::open(root.join("utd.db")).unwrap();
        Self { root, journal }
    }

    fn failed(&self, name: &str, class: ErrorClass, code: &str) -> DownloadTask {
        let dir = self.root.to_string_lossy().into_owned();
        let mut task =
            DownloadTask::new(format!("https://example.com/{name}"), name, dir, 16).unwrap();
        task.total_bytes = Some(1000);
        task.downloaded_bytes = 400;
        task.status = TaskStatus::Failed;
        task.error = Some(TaskError::new(class, code, "x").unwrap());
        self.journal.save_task(&task).unwrap();
        task
    }

    fn load(&self, id: &str) -> DownloadTask {
        self.journal.load_task(id).unwrap().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Ổ giả: (trống, tổng) đổi được giữa chừng.
struct FakeDisk(Mutex<Option<(u64, u64)>>);

impl FakeDisk {
    fn new(space: Option<(u64, u64)>) -> Self {
        Self(Mutex::new(space))
    }

    fn set(&self, space: Option<(u64, u64)>) {
        *self.0.lock().unwrap() = space;
    }
}

impl Disk for FakeDisk {
    fn space(&self, _folder: &Path) -> Option<(u64, u64)> {
        *self.0.lock().unwrap()
    }
}

const NOW: i64 = 1_800_000_000_000;

#[test]
fn a_server_error_waits_then_returns_to_the_queue_on_time() {
    let fx = Fixture::new();
    let disk = FakeDisk::new(Some((u64::MAX, u64::MAX)));
    let task = fx.failed("a.iso", ErrorClass::Transient, "http.503");

    let settled = settle(&fx.journal, &task.id, None, true, &disk, NOW).expect("changed");
    assert_eq!(settled.status, TaskStatus::Paused, "không ra trạng thái lỗi");
    let recovery = settled.recovery.clone().expect("lịch thử lại");
    assert_eq!((recovery.wait, recovery.attempts), (RecoveryWait::Retry, 1));
    let at = recovery.retry_at.expect("giờ thử lại");
    assert!(at > NOW && at <= NOW + 6_000, "{}", at - NOW);
    assert_eq!(settled.error.as_ref().unwrap().code, "http.503", "giữ lý do để hiện");

    assert!(due(&fx.journal, true, &disk, true, at - 1).is_empty(), "chưa tới giờ");
    assert_eq!(due(&fx.journal, true, &disk, true, at), vec![task.id.clone()]);
    let resumed = fx.load(&task.id);
    assert_eq!(resumed.recovery.as_ref().unwrap().retry_at, None, "đã dùng giờ hẹn");
    assert_eq!(resumed.recovery.as_ref().unwrap().attempts, 1, "giữ số lần thử");
    assert!(due(&fx.journal, true, &disk, true, at + 10_000).is_empty(), "không chạy hai lần");
}

#[test]
fn offline_waits_for_the_network_and_runs_as_soon_as_it_returns() {
    let fx = Fixture::new();
    let disk = FakeDisk::new(Some((u64::MAX, u64::MAX)));
    let task = fx.failed("b.iso", ErrorClass::Transient, "network.connect");
    let settled = settle(&fx.journal, &task.id, None, false, &disk, NOW).expect("changed");
    assert_eq!(settled.recovery.as_ref().unwrap().wait, RecoveryWait::Network);
    assert!(due(&fx.journal, false, &disk, true, NOW + 1_000).is_empty());
    assert_eq!(due(&fx.journal, true, &disk, true, NOW + 1_000), vec![task.id]);
}

#[test]
fn a_full_disk_pauses_until_there_is_room() {
    let fx = Fixture::new();
    let disk = FakeDisk::new(Some((10, u64::MAX)));
    let task = fx.failed("c.iso", ErrorClass::User, "disk.full");
    let settled = settle(&fx.journal, &task.id, None, true, &disk, NOW).expect("changed");
    assert_eq!(settled.status, TaskStatus::Paused);
    let need = settled.recovery.as_ref().unwrap().need_bytes.expect("cần bao nhiêu");
    assert_eq!(need, 600 + recovery::DISK_MARGIN);

    assert!(due(&fx.journal, true, &disk, true, NOW + 60_000).is_empty());
    disk.set(Some((need, u64::MAX)));
    assert!(due(&fx.journal, true, &disk, false, NOW + 60_000).is_empty(), "chỉ hỏi ổ mỗi 10 s");
    assert_eq!(due(&fx.journal, true, &disk, true, NOW + 60_000), vec![task.id]);
}

#[test]
fn a_disk_smaller_than_the_file_is_reported_not_awaited() {
    let fx = Fixture::new();
    let disk = FakeDisk::new(Some((0, 100)));
    let task = fx.failed("d.iso", ErrorClass::User, "disk.full");
    let settled = settle(&fx.journal, &task.id, None, true, &disk, NOW).expect("changed");
    assert_eq!(settled.status, TaskStatus::Failed);
    assert_eq!(settled.error.unwrap().code, "disk.too_small");
    assert!(settled.recovery.is_none());
}

#[test]
fn an_unplugged_drive_resumes_when_it_comes_back() {
    let fx = Fixture::new();
    let disk = FakeDisk::new(None);
    let task = fx.failed("e.iso", ErrorClass::User, "disk.unplugged");
    settle(&fx.journal, &task.id, None, true, &disk, NOW).expect("changed");
    assert!(due(&fx.journal, true, &disk, true, NOW).is_empty());
    disk.set(Some((1, u64::MAX)));
    assert_eq!(due(&fx.journal, true, &disk, true, NOW), vec![task.id]);
}

#[test]
fn an_expired_link_stays_failed_and_waits_for_a_new_link() {
    let fx = Fixture::new();
    let disk = FakeDisk::new(Some((u64::MAX, u64::MAX)));
    let task = fx.failed("f.iso", ErrorClass::User, "http.403");
    let settled = settle(&fx.journal, &task.id, None, true, &disk, NOW).expect("changed");
    assert_eq!(settled.status, TaskStatus::Failed);
    assert_eq!(settled.recovery.as_ref().unwrap().wait, RecoveryWait::Link);
    assert!(due(&fx.journal, true, &disk, true, NOW + 86_400_000).is_empty());
    assert_eq!(
        link_candidates(&fx.journal, None, "https://cdn.example.com/x/F.ISO?sig=1", None),
        vec![task.id.clone()],
        "cùng tên file (không phân biệt hoa thường)"
    );
    assert!(link_candidates(&fx.journal, None, "https://cdn.example.com/other.iso", None).is_empty());
    assert_eq!(
        link_candidates(&fx.journal, Some("open-dialog"), "https://x.example/f.iso", Some("f.iso")),
        vec!["open-dialog".to_string(), task.id],
        "hộp Cập nhật link đang mở thì thử task đó trước"
    );
}

#[test]
fn permanent_errors_and_the_retry_limit_end_as_errors() {
    let fx = Fixture::new();
    let disk = FakeDisk::new(Some((u64::MAX, u64::MAX)));
    let gone = fx.failed("g.iso", ErrorClass::Permanent, "http.404");
    assert!(settle(&fx.journal, &gone.id, None, true, &disk, NOW).is_none());
    assert_eq!(fx.load(&gone.id).status, TaskStatus::Failed);

    let busy = fx.failed("h.iso", ErrorClass::Transient, "http.503");
    let mut now = NOW;
    for _ in 0..recovery::RETRY_LIMIT {
        let settled = settle(&fx.journal, &busy.id, None, true, &disk, now).expect("changed");
        assert_eq!(settled.status, TaskStatus::Paused);
        now = settled.recovery.unwrap().retry_at.unwrap();
        // Lượt thử lại chạy rồi hỏng y như cũ, không thêm byte nào.
        let mut again = fx.load(&busy.id);
        again.status = TaskStatus::Failed;
        fx.journal.save_task(&again).unwrap();
    }
    let given_up = settle(&fx.journal, &busy.id, None, true, &disk, now).expect("changed");
    assert_eq!(given_up.status, TaskStatus::Failed, "hết lượt thì báo lỗi, có Doctor");
    assert!(given_up.recovery.is_none());
}

#[test]
fn a_run_that_ends_while_still_running_is_marked_failed() {
    let fx = Fixture::new();
    let disk = FakeDisk::new(Some((u64::MAX, u64::MAX)));
    let mut task = fx.failed("i.iso", ErrorClass::Transient, "x");
    task.status = TaskStatus::Downloading;
    task.error = None;
    fx.journal.save_task(&task).unwrap();
    let settled = settle(&fx.journal, &task.id, Some("database is locked"), true, &disk, NOW)
        .expect("changed");
    assert_ne!(settled.status, TaskStatus::Downloading, "không kẹt ở đang tải");
    assert_eq!(settled.error.as_ref().unwrap().code, "internal.run");
}

#[test]
fn completion_and_user_pause_drop_the_schedule() {
    let fx = Fixture::new();
    let disk = FakeDisk::new(Some((u64::MAX, u64::MAX)));
    let task = fx.failed("j.iso", ErrorClass::Transient, "http.503");
    settle(&fx.journal, &task.id, None, true, &disk, NOW).expect("changed");
    let forgotten = forget(&fx.journal, &task.id).expect("had a schedule");
    assert!(forgotten.recovery.is_none());
    assert!(forget(&fx.journal, &task.id).is_none());
    assert!(due(&fx.journal, true, &disk, true, NOW + 86_400_000).is_empty());

    let mut done = fx.load(&task.id);
    done.status = TaskStatus::Completed;
    done.downloaded_bytes = 1000;
    done.hash_state = HashState::Unavailable;
    done.completed_at = Some(NOW);
    done.error = None;
    done.recovery = Some(Recovery::new(RecoveryWait::Retry));
    fx.journal.save_task(&done).unwrap();
    let settled = settle(&fx.journal, &task.id, None, true, &disk, NOW).expect("changed");
    assert!(settled.recovery.is_none());
}

#[test]
fn decodes_url_file_names_for_matching() {
    assert_eq!(
        url_file_name("https://a.example/d/T%E1%BA%A3i%20v%E1%BB%81.zip?x=1").as_deref(),
        Some("Tải về.zip")
    );
    assert_eq!(percent_decode("bad%zz%4"), "bad%zz%4");
    assert_eq!(url_file_name("https://a.example/"), None);
}
