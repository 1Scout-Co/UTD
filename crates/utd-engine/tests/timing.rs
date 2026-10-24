//! Mốc đo `engine.timing` trong log (đo từng pha của một lượt tải): mỗi pha một dòng, đúng thứ
//! tự, thời gian tăng dần. Logger là toàn cục nên bài này nằm riêng một file.

mod kit;

use std::sync::{Arc, Mutex, Once};

use kit::*;
use utd_engine::{DownloadControl, DownloadRequest, DownloadTask, TaskKind};

struct Capture(Mutex<Vec<String>>);

impl log::Log for Capture {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            self.0.lock().expect("log").push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

static CAPTURE: Capture = Capture(Mutex::new(Vec::new()));
static INSTALL: Once = Once::new();

/// Cài logger thu dòng log một lần cho cả file (các bài chạy song song).
fn capture() {
    INSTALL.call_once(|| {
        log::set_logger(&CAPTURE).expect("logger");
        log::set_max_level(log::LevelFilter::Info);
    });
}

/// Các dòng `engine.timing` thuộc task này.
fn lines(task_id: &str) -> Vec<String> {
    let task = format!("task={task_id} ");
    CAPTURE
        .0
        .lock()
        .expect("log")
        .iter()
        .filter(|line| line.starts_with("engine.timing ") && line.contains(&task))
        .cloned()
        .collect()
}

/// `(pha, ms)` của các dòng `engine.timing` thuộc task này.
fn marks(task_id: &str) -> Vec<(String, u64)> {
    lines(task_id)
        .iter()
        .map(|line| {
            let field = |name: &str| {
                line.split_whitespace()
                    .find_map(|part| part.strip_prefix(name))
                    .unwrap_or_else(|| panic!("missing {name} in {line}"))
                    .to_string()
            };
            (field("phase="), field("ms=").parse().expect("ms"))
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn marks_each_phase_once_in_order() {
    capture();
    let data = payload(2 * MIB);
    let server = Server::start(file_handler(Arc::new(data), FileOpts::default())).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));
    let mut request = DownloadRequest::new(server.url("/timed.bin"), downloads(directory.path()));
    request.file_name = Some("timed.bin".into());

    let outcome = engine
        .download(request, DownloadControl::new())
        .await
        .expect("download");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");

    let marks = marks(outcome.task_id());
    let phases: Vec<&str> = marks.iter().map(|(phase, _)| phase.as_str()).collect();
    assert_eq!(
        phases,
        ["probe", "first_byte", "downloaded", "synced", "done"]
    );
    assert!(
        marks.windows(2).all(|pair| pair[0].1 <= pair[1].1),
        "times go backwards: {marks:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn child_marks_name_the_parent() {
    // Task con của media: mốc mang id task cha để ghép với `media.timing` của app không cần journal.
    capture();
    let data = payload(MIB);
    let server = Server::start(file_handler(Arc::new(data), FileOpts::default())).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), options(1, 1));
    let parent = DownloadTask::new(
        server.url("/watch"),
        "Video.mp4",
        downloads(directory.path()),
        1,
    )
    .expect("parent")
    .with_kind(TaskKind::Media);
    journal.save_task(&parent).expect("save parent");
    let mut request = DownloadRequest::new(server.url("/part.bin"), downloads(directory.path()));
    request.file_name = Some("Video.f399.mp4".into());
    request.parent_id = Some(parent.id.clone());

    let outcome = engine
        .download(request, DownloadControl::new())
        .await
        .expect("download");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");

    let lines = lines(outcome.task_id());
    assert_eq!(lines.len(), 5, "expected one line per phase: {lines:?}");
    let parent_field = format!(" parent={} ", parent.id);
    assert!(
        lines.iter().all(|line| line.contains(&parent_field)),
        "a mark misses the parent: {lines:?}"
    );
}
