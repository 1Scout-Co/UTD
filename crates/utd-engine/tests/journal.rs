use std::path::{Path, PathBuf};

use rusqlite::Connection;
use tempfile::TempDir;
use utd_engine::{
    now_ms, plan_segments, DownloadTask, Error, ErrorClass, HashState, Journal, MediaJob, Recovery,
    RecoveryWait, SegmentStatus, TaskError, TaskStatus,
};

fn setup() -> (TempDir, Journal) {
    let directory = tempfile::tempdir().expect("tempdir");
    let journal = Journal::open(directory.path().join("utd.db")).expect("journal");
    (directory, journal)
}

fn sample_task(directory: &Path, index: usize) -> DownloadTask {
    let downloads = directory.join("downloads");
    DownloadTask::new(
        format!("https://example.com/file-{index}.bin"),
        format!("file-{index}.bin"),
        downloads.to_string_lossy().into_owned(),
        8,
    )
    .expect("valid task")
}

fn completed_task(mut task: DownloadTask, total: u64) -> DownloadTask {
    task.total_bytes = Some(total);
    task.downloaded_bytes = total;
    task.status = TaskStatus::Completed;
    task.hash_state = HashState::Unavailable;
    task.completed_at = Some(now_ms());
    task
}

#[test]
fn opens_in_wal_mode_with_schema_and_clean_integrity() {
    let (_directory, journal) = setup();
    assert_eq!(journal.schema_version().expect("version"), 4);
    assert!(journal.integrity_check().expect("integrity").is_empty());

    let connection = Connection::open(journal.path()).expect("open raw");
    let mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("journal mode");
    assert_eq!(mode, "wal");
    let owners: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('tasks', 'segments', 'settings', 'schema_migrations', 'media_jobs')",
            [],
            |row| row.get(0),
        )
        .expect("schema");
    assert_eq!(owners, 5);
}

#[test]
fn round_trips_tasks_and_updates_in_place() {
    let (directory, journal) = setup();
    let mut task = sample_task(directory.path(), 1);
    task.etag = Some("\"abc123\"".into());
    task.last_modified = Some("Wed, 21 Oct 2026 07:28:00 GMT".into());
    journal.save_task(&task).expect("save");

    let loaded = journal.load_task(&task.id).expect("load").expect("present");
    assert_eq!(loaded, task);
    assert_eq!(journal.list_tasks().expect("list").len(), 1);

    task.status = TaskStatus::Paused;
    task.updated_at = now_ms() + 1;
    journal.save_task(&task).expect("update");

    let tasks = journal.list_tasks().expect("list");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].status, TaskStatus::Paused);
    assert_eq!(journal.load_task("missing").expect("load"), None);
}

#[test]
fn persists_error_classification() {
    let (directory, journal) = setup();
    let mut task = sample_task(directory.path(), 2);
    task.status = TaskStatus::Failed;
    task.error = Some(
        TaskError::new(
            ErrorClass::Security,
            "guard.motw",
            "File came from the internet and is quarantined",
        )
        .expect("error"),
    );
    journal.save_task(&task).expect("save");

    let loaded = journal.load_task(&task.id).expect("load").expect("present");
    let error = loaded.error.expect("error present");
    assert_eq!(error.class, ErrorClass::Security);
    assert_eq!(error.code, "guard.motw");
    assert!(!error.is_retryable());
}

#[test]
fn stores_plan_and_checkpoints_progress() {
    let (directory, journal) = setup();
    let total = 8 * 1024 * 1024;
    let mut task = sample_task(directory.path(), 3);
    task.total_bytes = Some(total);
    journal.save_task(&task).expect("save");

    let mut segments = plan_segments(total, 8);
    assert_eq!(segments.len(), 8);
    journal.save_segments(&task.id, &segments).expect("plan");

    segments[0].downloaded = segments[0].byte_len();
    segments[0].status = SegmentStatus::Done;
    segments[1].downloaded = 512;
    segments[1].status = SegmentStatus::Active;
    let downloaded = 512 + segments[0].byte_len();
    journal
        .checkpoint(&task.id, &segments, downloaded)
        .expect("checkpoint");

    let loaded = journal.load_segments(&task.id).expect("segments");
    assert_eq!(loaded, segments);
    let task = journal.load_task(&task.id).expect("load").expect("present");
    assert_eq!(task.downloaded_bytes, downloaded);
    assert_eq!(task.progress_ratio(), downloaded as f64 / total as f64);
}

#[test]
fn checkpoints_tasks_without_known_length() {
    let (directory, journal) = setup();
    let task = sample_task(directory.path(), 4);
    journal.save_task(&task).expect("save");
    journal.checkpoint(&task.id, &[], 4096).expect("checkpoint");
    let loaded = journal.load_task(&task.id).expect("load").expect("present");
    assert_eq!(loaded.downloaded_bytes, 4096);
    assert_eq!(loaded.progress_ratio(), 0.0);
}

#[test]
fn rejects_checkpoint_that_disagrees_with_segments() {
    let (directory, journal) = setup();
    let total = 4 * 1024 * 1024;
    let mut task = sample_task(directory.path(), 5);
    task.total_bytes = Some(total);
    journal.save_task(&task).expect("save");
    let segments = plan_segments(total, 4);
    journal.save_segments(&task.id, &segments).expect("plan");
    assert!(journal.checkpoint(&task.id, &segments, 1).is_err());
}

#[test]
fn keeps_previous_plan_when_new_plan_is_invalid() {
    let (directory, journal) = setup();
    let total = 8 * 1024 * 1024;
    let mut task = sample_task(directory.path(), 6);
    task.total_bytes = Some(total);
    journal.save_task(&task).expect("save");
    journal
        .save_segments(&task.id, &plan_segments(total, 8))
        .expect("plan");

    let wrong = plan_segments(total / 2, 4);
    assert!(journal.save_segments(&task.id, &wrong).is_err());
    assert_eq!(journal.load_segments(&task.id).expect("segments").len(), 8);
}

#[test]
fn mark_interrupted_pauses_active_tasks_only() {
    let (directory, journal) = setup();

    let mut downloading = sample_task(directory.path(), 7);
    downloading.status = TaskStatus::Downloading;
    downloading.total_bytes = Some(100);
    downloading.downloaded_bytes = 40;
    let mut resolving = sample_task(directory.path(), 8);
    resolving.status = TaskStatus::Resolving;
    let queued = sample_task(directory.path(), 9);
    let completed = completed_task(sample_task(directory.path(), 10), 10);
    for task in [&downloading, &resolving, &queued, &completed] {
        journal.save_task(task).expect("save");
    }

    assert_eq!(journal.mark_interrupted().expect("interrupt"), 2);
    assert_eq!(
        journal.load_task(&downloading.id).expect("load").expect("present").status,
        TaskStatus::Paused
    );
    assert_eq!(
        journal.load_task(&resolving.id).expect("load").expect("present").status,
        TaskStatus::Paused
    );
    assert_eq!(
        journal.load_task(&completed.id).expect("load").expect("present").status,
        TaskStatus::Completed
    );

    let recoverable = journal.recoverable_tasks().expect("recoverable");
    assert_eq!(recoverable.len(), 3);
}

/// Lịch tự thử lại nằm trong journal: mở lại app vẫn chờ đúng giờ, đúng lý do; giao diện nhận qua JSON.
#[test]
fn keeps_the_recovery_schedule_and_lists_waiting_tasks() {
    let (directory, journal) = setup();
    let mut waiting = sample_task(directory.path(), 1);
    waiting.status = TaskStatus::Paused;
    let mut recovery = Recovery::new(RecoveryWait::Retry);
    recovery.retry_at = Some(now_ms() + 60_000);
    recovery.attempts = 2;
    waiting.recovery = Some(recovery.clone());
    let mut paused = sample_task(directory.path(), 2);
    paused.status = TaskStatus::Paused;
    let mut link = sample_task(directory.path(), 3);
    link.status = TaskStatus::Failed;
    link.recovery = Some(Recovery::new(RecoveryWait::Link));
    for task in [&waiting, &paused, &link] {
        journal.save_task(task).expect("save");
    }
    let path = journal.path().to_path_buf();
    drop(journal);

    let journal = Journal::open(&path).expect("reopen");
    let loaded = journal.load_task(&waiting.id).expect("load").expect("task");
    assert_eq!(loaded.recovery, Some(recovery));
    let ids: Vec<String> = journal
        .waiting_tasks()
        .expect("waiting")
        .into_iter()
        .map(|task| task.id)
        .collect();
    assert_eq!(ids, vec![waiting.id.clone()], "chỉ task tạm dừng có lịch");
    let json = serde_json::to_string(&loaded).expect("json");
    assert!(json.contains("\"recovery\":{\"wait\":\"retry\""), "{json}");
    assert!(!serde_json::to_string(&paused).expect("json").contains("recovery"));
}

#[test]
fn delete_cascades_to_segments() {
    let (directory, journal) = setup();
    let mut task = sample_task(directory.path(), 11);
    task.total_bytes = Some(1024 * 1024);
    journal.save_task(&task).expect("save");
    journal
        .save_segments(&task.id, &plan_segments(1024 * 1024, 2))
        .expect("plan");

    journal.delete_task(&task.id).expect("delete");
    assert!(journal.load_segments(&task.id).expect("segments").is_empty());
    assert!(matches!(
        journal.delete_task(&task.id),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn settings_round_trip() {
    let (_directory, journal) = setup();
    assert_eq!(journal.get_setting("language").expect("get"), None);
    journal.set_setting("language", "vi").expect("set");
    journal.set_setting("language", "en").expect("update");
    assert_eq!(
        journal.get_setting("language").expect("get"),
        Some("en".into())
    );
    journal.delete_setting("language").expect("delete");
    assert_eq!(journal.get_setting("language").expect("get"), None);
    journal.delete_setting("language").expect("delete twice");
}

#[test]
fn persists_tasks_across_reopen() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("utd.db");
    let task = {
        let journal = Journal::open(&path).expect("open");
        let task = sample_task(directory.path(), 12);
        journal.save_task(&task).expect("save");
        task
    };

    let journal = Journal::open(&path).expect("reopen");
    assert_eq!(
        journal.load_task(&task.id).expect("load").expect("present"),
        task
    );
}

#[test]
fn backs_up_legacy_database_before_migrating() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("utd.db");
    {
        let connection = Connection::open(&path).expect("legacy db");
        connection
            .execute_batch(
                "CREATE TABLE legacy_data (value TEXT);
                 INSERT INTO legacy_data (value) VALUES ('keep-me');",
            )
            .expect("legacy schema");
    }

    let journal = Journal::open(&path).expect("migrate");
    assert_eq!(journal.schema_version().expect("version"), 4);

    let backup = PathBuf::from(format!("{}.bak", path.display()));
    assert!(backup.exists(), "backup was not created");
    let connection = Connection::open(&backup).expect("backup db");
    let value: String = connection
        .query_row("SELECT value FROM legacy_data", [], |row| row.get(0))
        .expect("legacy value");
    assert_eq!(value, "keep-me");
}

#[test]
fn rejects_a_newer_schema() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("utd.db");
    {
        let connection = Connection::open(&path).expect("db");
        connection
            .pragma_update(None, "user_version", 99)
            .expect("version");
    }
    match Journal::open(&path) {
        Err(Error::UnsupportedSchema { found, max }) => {
            assert_eq!(found, 99);
            assert_eq!(max, 4);
        }
        other => panic!("expected UnsupportedSchema, got {other:?}"),
    }
}

#[test]
fn handles_a_thousand_tasks() {
    let (directory, journal) = setup();
    for index in 0..1000 {
        let task = completed_task(sample_task(directory.path(), index + 100), 1024);
        journal.save_task(&task).expect("save");
    }

    assert_eq!(journal.list_tasks().expect("list").len(), 1000);
    let counts = journal.status_counts().expect("counts");
    assert_eq!(counts.len(), TaskStatus::ALL.len());
    let completed = counts
        .into_iter()
        .find(|(status, _)| *status == TaskStatus::Completed)
        .expect("completed count");
    assert_eq!(completed.1, 1000);
    assert_eq!(journal.mark_interrupted().expect("interrupt"), 0);
}

#[test]
fn checkpoints_single_stream_progress_with_known_total() {
    let (directory, journal) = setup();
    let mut task = sample_task(directory.path(), 14);
    task.total_bytes = Some(4096);
    journal.save_task(&task).expect("save");
    journal
        .checkpoint(&task.id, &[], 1024)
        .expect("single stream checkpoint");
    let loaded = journal.load_task(&task.id).expect("load").expect("present");
    assert_eq!(loaded.downloaded_bytes, 1024);
}

#[test]
fn reset_progress_clears_the_segment_plan() {
    let (directory, journal) = setup();
    let total = 4 * 1024 * 1024;
    let mut task = sample_task(directory.path(), 15);
    task.total_bytes = Some(total);
    journal.save_task(&task).expect("save");
    journal
        .save_segments(&task.id, &plan_segments(total, 4))
        .expect("plan");
    journal.reset_task_progress(&task.id).expect("reset");
    assert!(journal.load_segments(&task.id).expect("segments").is_empty());
    let loaded = journal.load_task(&task.id).expect("load").expect("present");
    assert_eq!(loaded.downloaded_bytes, 0);
}

#[test]
fn stores_media_jobs_and_cascades_on_delete() {
    let (directory, journal) = setup();
    let task = sample_task(directory.path(), 16);
    journal.save_task(&task).expect("save task");
    let job = MediaJob {
        task_id: task.id.clone(),
        url: "https://www.youtube.com/watch?v=example".into(),
        page_url: Some("https://www.youtube.com/watch?v=example".into()),
        selector: "bv*+ba/b".into(),
        audio_format: None,
        title: "Video thử nghiệm".into(),
        created_at: now_ms(),
    };
    journal.save_media_job(&job).expect("save media job");
    assert_eq!(
        journal.load_media_job(&task.id).expect("load"),
        Some(job)
    );
    journal.delete_task(&task.id).expect("delete");
    assert_eq!(journal.load_media_job(&task.id).expect("load"), None);
}

#[test]
fn copies_a_live_journal_including_wal_without_overwriting() {
    let (directory, journal) = setup();
    let task = completed_task(sample_task(directory.path(), 7), 2048);
    journal.save_task(&task).expect("save");
    journal.set_setting("download_dir", "D:/Tải về").expect("setting");

    // Journal nguồn vẫn đang mở (dữ liệu còn nằm trong WAL) khi chép.
    let target = directory.path().join("moi").join("Dữ liệu").join("utd.db");
    utd_engine::db::copy_database(journal.path(), &target).expect("copy");

    let copied = Journal::open(&target).expect("open copy");
    assert_eq!(copied.load_task(&task.id).expect("load"), Some(task));
    assert_eq!(
        copied.get_setting("download_dir").expect("get").as_deref(),
        Some("D:/Tải về")
    );
    assert!(copied.integrity_check().expect("integrity").is_empty());
    drop(copied);

    let error = utd_engine::db::copy_database(journal.path(), &target).expect_err("no overwrite");
    assert!(matches!(error, Error::InvalidInput(_)));
}
