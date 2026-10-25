//! Journal v2 (bản đang chạy trên máy người dùng) mở được bằng bản mới: lên v3, sao lưu `.bak`,
//! giữ nguyên mọi dòng.

use std::path::PathBuf;

use rusqlite::{Connection, params};
use utd_engine::{DownloadTask, Journal, SegmentStatus, TaskKind, TaskStatus, now_ms};

/// Lược đồ v1 + v2 đúng như bản 0.1.0 đã phát hành.
const V2_SCHEMA: &str = r#"
    CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
    CREATE TABLE tasks (
        id TEXT PRIMARY KEY,
        url TEXT NOT NULL,
        final_url TEXT,
        file_name TEXT NOT NULL,
        dest_dir TEXT NOT NULL,
        temp_path TEXT NOT NULL,
        kind TEXT NOT NULL,
        status TEXT NOT NULL,
        total_bytes INTEGER CHECK (total_bytes IS NULL OR total_bytes >= 0),
        downloaded_bytes INTEGER NOT NULL DEFAULT 0 CHECK (downloaded_bytes >= 0),
        etag TEXT,
        last_modified TEXT,
        expected_sha256 TEXT,
        hash_state TEXT NOT NULL,
        connections INTEGER NOT NULL CHECK (connections BETWEEN 1 AND 32),
        error_json TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        completed_at INTEGER
    );
    CREATE INDEX idx_tasks_status ON tasks(status);
    CREATE INDEX idx_tasks_created ON tasks(created_at DESC);
    CREATE TABLE segments (
        task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
        idx INTEGER NOT NULL,
        start_byte INTEGER NOT NULL,
        end_byte INTEGER NOT NULL,
        downloaded INTEGER NOT NULL DEFAULT 0 CHECK (downloaded >= 0),
        status TEXT NOT NULL,
        PRIMARY KEY (task_id, idx)
    );
    CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at INTEGER NOT NULL);
    CREATE TABLE media_jobs (
        task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
        url TEXT NOT NULL,
        page_url TEXT,
        selector TEXT NOT NULL,
        audio_format TEXT,
        title TEXT NOT NULL,
        created_at INTEGER NOT NULL
    );
    INSERT INTO schema_migrations (version, applied_at) VALUES (1, 1), (2, 2);
    PRAGMA user_version = 2;
"#;

fn insert_task(connection: &Connection, id: &str, dir: &str, kind: &str, status: &str, total: i64) {
    let now = now_ms();
    connection
        .execute(
            "INSERT INTO tasks (id, url, file_name, dest_dir, temp_path, kind, status, total_bytes,
                downloaded_bytes, hash_state, connections, created_at, updated_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 8, ?11, ?11, ?12)",
            params![
                id,
                format!("https://example.com/{id}.bin"),
                format!("{id}.bin"),
                dir,
                format!("{dir}{}{id}.bin.{id}.part", std::path::MAIN_SEPARATOR),
                kind,
                status,
                total,
                if status == "completed" {
                    total
                } else {
                    total.min(1024)
                },
                if status == "completed" {
                    "unavailable"
                } else {
                    "pending"
                },
                now,
                if status == "completed" {
                    Some(now)
                } else {
                    None
                },
            ],
        )
        .expect("insert task");
}

#[test]
fn migrates_a_v2_journal_to_v3_and_keeps_every_row() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("utd.db");
    let dir = directory
        .path()
        .join("downloads")
        .to_string_lossy()
        .into_owned();
    {
        let connection = Connection::open(&path).expect("v2 db");
        connection.execute_batch(V2_SCHEMA).expect("v2 schema");
        insert_task(&connection, "paused-1", &dir, "http", "paused", 4096);
        insert_task(&connection, "done-1", &dir, "http", "completed", 2048);
        insert_task(&connection, "media-1", &dir, "media", "failed", 0);
        connection
            .execute(
                "INSERT INTO segments (task_id, idx, start_byte, end_byte, downloaded, status)
                 VALUES ('paused-1', 0, 0, 2047, 1024, 'active'), ('paused-1', 1, 2048, 4095, 0, 'pending')",
                [],
            )
            .expect("segments");
        connection
            .execute(
                "INSERT INTO media_jobs (task_id, url, selector, title, created_at)
                 VALUES ('media-1', 'https://www.youtube.com/watch?v=x', 'bv*+ba/b', 'Video', 1)",
                [],
            )
            .expect("media job");
        connection
            .execute(
                "INSERT INTO settings (key, value, updated_at) VALUES ('theme', 'dark', 1)",
                [],
            )
            .expect("setting");
    }

    let journal = Journal::open(&path).expect("migrate");
    assert_eq!(journal.schema_version().expect("version"), 4);
    assert!(journal.integrity_check().expect("integrity").is_empty());

    let tasks = journal.list_tasks().expect("list");
    assert_eq!(tasks.len(), 3);
    for task in &tasks {
        assert_eq!(task.parent_id, None);
        assert_eq!(task.max_request_bytes, None);
        assert!(task.auth_sites.is_empty());
        assert!(task.request_headers.is_empty());
    }
    let paused = journal
        .load_task("paused-1")
        .expect("load")
        .expect("paused");
    assert_eq!(paused.status, TaskStatus::Paused);
    assert_eq!(paused.downloaded_bytes, 1024);
    let segments = journal.load_segments("paused-1").expect("segments");
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].status, SegmentStatus::Active);
    assert_eq!(
        journal
            .load_task("done-1")
            .expect("load")
            .expect("done")
            .status,
        TaskStatus::Completed
    );
    let media = journal.load_task("media-1").expect("load").expect("media");
    assert_eq!(media.kind, TaskKind::Media);
    assert_eq!(
        journal
            .load_media_job("media-1")
            .expect("job")
            .expect("job")
            .selector,
        "bv*+ba/b"
    );
    assert_eq!(
        journal.get_setting("theme").expect("setting").as_deref(),
        Some("dark")
    );

    // Bản sao lưu là journal v2 nguyên vẹn.
    let backup = PathBuf::from(format!("{}.bak", path.display()));
    let old = Connection::open(&backup).expect("backup");
    let version: i64 = old
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("backup version");
    assert_eq!(version, 2);
    let rows: i64 = old
        .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
        .expect("backup rows");
    assert_eq!(rows, 3);

    // Cột mới dùng được ngay: task con của task media cũ.
    let part = DownloadTask::new(
        "https://rr1.example.com/videoplayback",
        "Video.f399.webm",
        &dir,
        16,
    )
    .expect("part")
    .with_parent("media-1");
    journal.save_task(&part).expect("save child");
    assert_eq!(journal.list_children("media-1").expect("children").len(), 1);
    assert_eq!(journal.list_tasks().expect("list").len(), 3);
    drop(journal);

    // Mở lại: không di trú lần hai.
    let journal = Journal::open(&path).expect("reopen");
    assert_eq!(journal.schema_version().expect("version"), 4);
    assert_eq!(journal.list_children("media-1").expect("children").len(), 1);
}
