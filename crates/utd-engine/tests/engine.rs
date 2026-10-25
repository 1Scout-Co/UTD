mod support;

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use support::{ServerConfig, TestServer};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use utd_engine::{
    now_ms, plan_segments, DownloadControl, DownloadEngine, DownloadOutcome, DownloadRequest,
    DownloadTask, EngineEvent, EngineOptions, HashState, Journal, SegmentStatus, TaskStatus,
};

fn payload(size: usize) -> Vec<u8> {
    (0..size)
        .map(|index| ((index.wrapping_mul(31).wrapping_add(7)) % 251) as u8)
        .collect()
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        write!(&mut hex, "{byte:02x}").expect("hex");
    }
    hex
}

fn options(initial: u16, max: u16) -> EngineOptions {
    EngineOptions {
        initial_connections: initial,
        large_initial_connections: initial,
        max_connections: max,
        host_connections: 8,
        global_connections: 16,
        max_attempts: 5,
        connect_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(5),
        // Nhỏ hơn idle_timeout như cấu hình thật: luật "kết nối treo" chạy trong mọi bài.
        stall_timeout: Duration::from_secs(2),
        sample_interval: Duration::from_millis(50),
        checkpoint_interval: Duration::from_millis(100),
        balance_interval: Duration::from_millis(200),
        user_agent: "utd-test/0.1".into(),
        quarantine_dir: None,
    }
}

fn engine(directory: &Path, options: EngineOptions) -> (Arc<Journal>, DownloadEngine) {
    let journal = Arc::new(Journal::open(directory.join("utd.db")).expect("journal"));
    let engine = DownloadEngine::new(journal.clone(), options).expect("engine");
    (journal, engine)
}

fn request(server: &TestServer, directory: &Path, name: &str) -> DownloadRequest {
    let downloads = directory.join("downloads");
    let mut request = DownloadRequest::new(
        server.url("/file.bin"),
        downloads.to_string_lossy().into_owned(),
    );
    request.file_name = Some(name.into());
    request.connections = 4;
    request
}

fn final_path(task: &DownloadTask) -> std::path::PathBuf {
    Path::new(&task.dest_dir).join(&task.file_name)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_segmented_file_and_verifies_hash() {
    let data = payload(4 * 1024 * 1024);
    let server = TestServer::start(data.clone(), ServerConfig::default()).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), options(2, 4));

    let mut request = request(&server, directory.path(), "segmented.bin");
    request.expected_sha256 = Some(sha256(&data));
    let outcome = engine
        .download(request, DownloadControl::new())
        .await
        .expect("download");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    let task = outcome.task();
    assert_eq!(task.status, TaskStatus::Completed);
    assert_eq!(task.hash_state, HashState::Verified);
    assert_eq!(task.downloaded_bytes, data.len() as u64);
    assert_eq!(std::fs::read(final_path(task)).expect("file"), data);
    assert!(!Path::new(&task.temp_path).exists());

    let segments = journal.load_segments(&task.id).expect("segments");
    assert!(!segments.is_empty());
    assert!(
        segments
            .iter()
            .all(|segment| segment.status == SegmentStatus::Done
                && segment.downloaded == segment.byte_len())
    );
    assert!(server.range_requests.load(Ordering::SeqCst) >= 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn falls_back_to_single_stream_without_ranges() {
    let data = payload(1024 * 1024);
    let server = TestServer::start(
        data.clone(),
        ServerConfig {
            ranges: false,
            ..ServerConfig::default()
        },
    )
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), options(2, 4));

    let outcome = engine
        .download(
            request(&server, directory.path(), "single.bin"),
            DownloadControl::new(),
        )
        .await
        .expect("download");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    let task = outcome.task();
    assert_eq!(std::fs::read(final_path(task)).expect("file"), data);
    assert!(journal.load_segments(&task.id).expect("segments").is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resumes_after_connection_drops() {
    let data = payload(2 * 1024 * 1024);
    let server = TestServer::start(
        data.clone(),
        ServerConfig {
            drops: 3,
            drop_bytes: 32 * 1024,
            ..ServerConfig::default()
        },
    )
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));

    let outcome = engine
        .download(
            request(&server, directory.path(), "flaky.bin"),
            DownloadControl::new(),
        )
        .await
        .expect("download");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    assert_eq!(
        std::fs::read(final_path(outcome.task())).expect("file"),
        data
    );
    assert!(server.requests.load(Ordering::SeqCst) >= 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn honors_retry_after() {
    let data = payload(256 * 1024);
    let server = TestServer::start(
        data.clone(),
        ServerConfig {
            retry_after_first: 1,
            ..ServerConfig::default()
        },
    )
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(1, 1));
    let mut request = request(&server, directory.path(), "slow.bin");
    request.connections = 1;

    let started = Instant::now();
    let outcome = engine
        .download(request, DownloadControl::new())
        .await
        .expect("download");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    assert!(started.elapsed() >= Duration::from_millis(900));
    assert_eq!(
        std::fs::read(final_path(outcome.task())).expect("file"),
        data
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn detects_hash_mismatch_and_keeps_temp_file() {
    let data = payload(512 * 1024);
    let server = TestServer::start(data, ServerConfig::default()).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));
    let mut request = request(&server, directory.path(), "tampered.bin");
    request.expected_sha256 = Some("0".repeat(64));

    let outcome = engine
        .download(request, DownloadControl::new())
        .await
        .expect("download");
    let DownloadOutcome::Failed(task) = outcome else {
        panic!("expected failure");
    };
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(task.hash_state, HashState::Mismatch);
    assert_eq!(
        task.error.as_ref().expect("error").code,
        "verify.hash_mismatch"
    );
    assert!(Path::new(&task.temp_path).exists());
    assert!(!final_path(&task).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_then_resume_completes() {
    let data = payload(4 * 1024 * 1024);
    let server = TestServer::start(
        data.clone(),
        ServerConfig {
            chunk_delay: Duration::from_millis(4),
            ..ServerConfig::default()
        },
    )
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), options(2, 4));
    let engine = Arc::new(engine);
    let (events, mut receiver) = mpsc::unbounded_channel();
    let control = DownloadControl {
        cancel: CancellationToken::new(),
        events: Some(events),
        initial_connections: None,
    };
    let cancel = control.cancel.clone();
    let request = request(&server, directory.path(), "paused.bin");
    let handle = tokio::spawn({
        let engine = engine.clone();
        async move { engine.download(request, control).await }
    });

    let watched = async {
        while let Some(event) = receiver.recv().await {
            if let EngineEvent::Progress { progress } = event
                && progress.downloaded_bytes >= 512 * 1024
            {
                cancel.cancel();
                break;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), watched)
        .await
        .expect("progress before timeout");
    let outcome = tokio::time::timeout(Duration::from_secs(10), handle)
        .await
        .expect("pause before timeout")
        .expect("download task")
        .expect("download result");
    let DownloadOutcome::Paused(task) = outcome else {
        panic!("expected paused outcome");
    };
    assert_eq!(task.status, TaskStatus::Paused);
    assert!(task.downloaded_bytes > 0);
    let task_id = task.id.clone();
    assert!(journal.load_task(&task_id).expect("load").is_some());

    let resumed = tokio::time::timeout(
        Duration::from_secs(20),
        engine.resume(&task_id, DownloadControl::new()),
    )
    .await
    .expect("resume before timeout")
    .expect("resume");
    assert!(resumed.is_completed(), "unexpected outcome: {resumed:?}");
    assert_eq!(std::fs::read(final_path(resumed.task())).expect("file"), data);
}

/// AC-04: file đổi trên máy chủ lúc đang tải dở ⇒ dừng, báo `file.changed`, không ghép đoạn cũ; người
/// dùng chọn "Tải lại từ đầu" (`reset_progress`) thì tải đúng file mới.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stops_when_etag_changes_and_restarts_on_request() {
    let data = payload(1024 * 1024);
    let total = data.len() as u64;
    let server = TestServer::start(
        data.clone(),
        ServerConfig {
            etag: "\"v2\"".into(),
            ..ServerConfig::default()
        },
    )
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let downloads = directory.path().join("downloads");
    std::fs::create_dir_all(&downloads).expect("downloads");
    let (journal, engine) = engine(directory.path(), options(2, 4));

    let mut task = DownloadTask::new(
        server.url("/file.bin"),
        "changed.bin",
        downloads.to_string_lossy().into_owned(),
        4,
    )
    .expect("task");
    task.etag = Some("\"v1\"".into());
    task.total_bytes = Some(total);
    task.downloaded_bytes = total / 2;
    task.status = TaskStatus::Paused;
    task.updated_at = now_ms();
    {
        let mut file = std::fs::File::create(&task.temp_path).expect("temp");
        file.write_all(&vec![0xAA; total as usize / 2]).expect("old bytes");
        file.set_len(total).expect("preallocate");
    }
    journal.save_task(&task).expect("save task");
    let mut segments = plan_segments(total, 4);
    for segment in segments.iter_mut().take(2) {
        segment.downloaded = segment.byte_len();
        segment.status = SegmentStatus::Done;
    }
    journal.save_segments(&task.id, &segments).expect("save plan");
    let before = journal.load_task(&task.id).expect("load").expect("task").downloaded_bytes;
    assert!(before > 0);

    let outcome = engine
        .resume(&task.id, DownloadControl::new())
        .await
        .expect("resume");
    let DownloadOutcome::Failed(failed) = &outcome else {
        panic!("unexpected outcome: {outcome:?}");
    };
    assert_eq!(failed.error.as_ref().map(|error| error.code.as_str()), Some("file.changed"));
    assert_eq!(failed.downloaded_bytes, before, "không âm thầm xoá phần đã tải");
    assert!(!final_path(failed).exists());

    engine.reset_progress(&task.id).expect("reset");
    let outcome = engine
        .resume(&task.id, DownloadControl::new())
        .await
        .expect("resume");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    assert_eq!(std::fs::read(final_path(outcome.task())).expect("file"), data);
    let stored = journal
        .load_task(&task.id)
        .expect("load")
        .expect("present");
    assert_eq!(stored.etag.as_deref(), Some("\"v2\""));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refuses_to_overwrite_an_existing_file() {
    let data = payload(128 * 1024);
    let server = TestServer::start(data, ServerConfig::default()).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let downloads = directory.path().join("downloads");
    std::fs::create_dir_all(&downloads).expect("downloads");
    std::fs::write(downloads.join("existing.bin"), b"keep me").expect("existing file");
    let (_journal, engine) = engine(directory.path(), options(2, 4));

    let outcome = engine
        .download(
            request(&server, directory.path(), "existing.bin"),
            DownloadControl::new(),
        )
        .await
        .expect("download");
    let DownloadOutcome::Failed(task) = outcome else {
        panic!("expected failure");
    };
    assert_eq!(task.error.as_ref().expect("error").code, "file.exists");
    assert_eq!(
        std::fs::read(downloads.join("existing.bin")).expect("file"),
        b"keep me"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn keeps_a_file_that_appears_after_the_download_finished() {
    // Tên còn trống lúc bắt đầu; người dùng tạo đúng tên đó khi đã tải xong, lúc engine đang kiểm
    // hash. Đưa vào chỗ phải báo trùng tên chứ không ghi đè, và giữ file tạm đã tải.
    let data = payload(4 * 1024 * 1024);
    let server = TestServer::start(data.clone(), ServerConfig::default()).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let target = directory.path().join("downloads").join("late.bin");
    let (_journal, engine) = engine(directory.path(), options(2, 4));
    let mut request = request(&server, directory.path(), "late.bin");
    request.expected_sha256 = Some(sha256(&data));

    let (events, mut received) = mpsc::unbounded_channel();
    let watcher = {
        let target = target.clone();
        tokio::spawn(async move {
            while let Some(event) = received.recv().await {
                if let EngineEvent::Task { task } = event
                    && task.status == TaskStatus::Verifying
                {
                    let mut file = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&target)
                        .expect("the engine placed its file before verification ended");
                    file.write_all(b"user file").expect("user file");
                    return true;
                }
            }
            false
        })
    };
    let outcome = engine
        .download(request, DownloadControl::with_events(events))
        .await
        .expect("download");
    assert!(watcher.await.expect("watcher"), "never reached verification");
    let DownloadOutcome::Failed(task) = outcome else {
        panic!("expected failure, got {outcome:?}");
    };
    assert_eq!(task.error.as_ref().expect("error").code, "file.exists");
    assert_eq!(std::fs::read(&target).expect("user file"), b"user file");
    assert_eq!(
        std::fs::read(&task.temp_path).expect("part file"),
        data,
        "the finished part was lost"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completes_an_empty_file() {
    let server = TestServer::start(Vec::new(), ServerConfig::default()).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));
    let outcome = engine
        .download(
            request(&server, directory.path(), "empty.bin"),
            DownloadControl::new(),
        )
        .await
        .expect("download");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    let task = outcome.task();
    assert_eq!(task.total_bytes, Some(0));
    assert_eq!(std::fs::read(final_path(task)).expect("file"), Vec::<u8>::new());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn handles_unknown_length_streaming() {
    let data = payload(512 * 1024);
    let server = TestServer::start(
        data.clone(),
        ServerConfig {
            ranges: false,
            omit_length: true,
            ..ServerConfig::default()
        },
    )
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));
    let outcome = engine
        .download(
            request(&server, directory.path(), "unknown.bin"),
            DownloadControl::new(),
        )
        .await
        .expect("download");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    let task = outcome.task();
    assert_eq!(task.total_bytes, Some(data.len() as u64));
    assert_eq!(std::fs::read(final_path(task)).expect("file"), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quarantines_an_executable_disguised_as_pdf() {
    let mut data = payload(256 * 1024);
    data[0] = b'M';
    data[1] = b'Z';
    let server = TestServer::start(data.clone(), ServerConfig::default()).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let quarantine = directory.path().join("quarantine");
    let mut engine_options = options(2, 4);
    engine_options.quarantine_dir = Some(quarantine.to_string_lossy().into_owned());
    let (_journal, engine) = engine(directory.path(), engine_options);
    let mut request = request(&server, directory.path(), "invoice.pdf");
    request.expected_sha256 = Some(sha256(&data));

    let outcome = engine
        .download(request, DownloadControl::new())
        .await
        .expect("download");
    let DownloadOutcome::Quarantined(task) = outcome else {
        panic!("expected quarantined outcome");
    };
    assert_eq!(task.status, TaskStatus::Quarantined);
    assert_eq!(
        task.error.as_ref().expect("error").code,
        "guard.extension_mismatch"
    );
    assert!(!final_path(&task).exists());
    let files = std::fs::read_dir(&quarantine)
        .expect("quarantine dir")
        .flatten()
        .count();
    assert_eq!(files, 1);
}
