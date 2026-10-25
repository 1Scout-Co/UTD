use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::headers::RequestHeader;
use crate::model::{
    now_ms, validate_segments, DownloadTask, HashState, MediaJob, Segment, SegmentStatus,
    TaskError, TaskKind, TaskStatus,
};
use crate::{Error, Result};

const SCHEMA_VERSION: u32 = 4;

struct Migration {
    version: u32,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
    version: 1,
    sql: r#"
        CREATE TABLE schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        );

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

        CREATE TABLE settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );
    "#,
    },
    Migration {
        version: 2,
        sql: r#"
            CREATE TABLE media_jobs (
                task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
                url TEXT NOT NULL,
                page_url TEXT,
                selector TEXT NOT NULL,
                audio_format TEXT,
                title TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
        "#,
    },
    Migration {
        version: 3,
        // Task con ẩn (luồng media), trần byte mỗi yêu cầu, header riêng (JSON).
        sql: r#"
            ALTER TABLE tasks ADD COLUMN parent_id TEXT REFERENCES tasks(id) ON DELETE CASCADE;
            ALTER TABLE tasks ADD COLUMN max_request_bytes INTEGER
                CHECK (max_request_bytes IS NULL OR max_request_bytes > 0);
            ALTER TABLE tasks ADD COLUMN request_headers_json TEXT;
            CREATE INDEX idx_tasks_parent ON tasks(parent_id);
        "#,
    },
    Migration {
        version: 4,
        // Task cần đăng nhập ở trang nào (JSON danh sách tên miền). Chỉ là dấu, không bao giờ có cookie.
        sql: r#"
            ALTER TABLE tasks ADD COLUMN auth_sites_json TEXT;
        "#,
    },
];

const TASK_COLUMNS: &str = "id, url, final_url, file_name, dest_dir, temp_path, kind, status, \
     total_bytes, downloaded_bytes, etag, last_modified, expected_sha256, hash_state, connections, \
     error_json, created_at, updated_at, completed_at, parent_id, max_request_bytes, \
     request_headers_json, auth_sites_json";

/// Danh sách cho giao diện, đếm trạng thái, khôi phục: chỉ task gốc (task con đi theo cha).
const TOP_LEVEL: &str = "parent_id IS NULL";

#[derive(Debug)]
pub struct Journal {
    path: PathBuf,
    conn: Mutex<Connection>,
}

impl Journal {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let existed = path.exists();
        let mut conn = Connection::open(&path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        let version = read_schema_version(&conn)?;
        if version > SCHEMA_VERSION {
            return Err(Error::UnsupportedSchema {
                found: version,
                max: SCHEMA_VERSION,
            });
        }
        conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        if existed && version < SCHEMA_VERSION {
            conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
            backup_database(&path)?;
        }
        migrate(&mut conn)?;
        Ok(Self {
            path,
            conn: Mutex::new(conn),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn schema_version(&self) -> Result<u32> {
        let conn = self.conn()?;
        read_schema_version(&conn)
    }

    pub fn save_task(&self, task: &DownloadTask) -> Result<()> {
        task.validate()?;
        let error_json = task
            .error
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let total_bytes = task.total_bytes.map(to_i64).transpose()?;
        let downloaded_bytes = to_i64(task.downloaded_bytes)?;
        let final_url = task.final_url.as_deref();
        let etag = task.etag.as_deref();
        let last_modified = task.last_modified.as_deref();
        let expected_sha256 = task.expected_sha256.as_deref();
        let max_request_bytes = task.max_request_bytes.map(to_i64).transpose()?;
        let headers_json = if task.request_headers.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&task.request_headers)?)
        };
        let auth_sites_json = if task.auth_sites.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&task.auth_sites)?)
        };
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO tasks (
                id, url, final_url, file_name, dest_dir, temp_path, kind, status,
                total_bytes, downloaded_bytes, etag, last_modified, expected_sha256,
                hash_state, connections, error_json, created_at, updated_at, completed_at,
                parent_id, max_request_bytes, request_headers_json, auth_sites_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                ?18, ?19, ?20, ?21, ?22, ?23)
             ON CONFLICT(id) DO UPDATE SET
                url = excluded.url,
                final_url = excluded.final_url,
                file_name = excluded.file_name,
                dest_dir = excluded.dest_dir,
                temp_path = excluded.temp_path,
                kind = excluded.kind,
                status = excluded.status,
                total_bytes = excluded.total_bytes,
                downloaded_bytes = excluded.downloaded_bytes,
                etag = excluded.etag,
                last_modified = excluded.last_modified,
                expected_sha256 = excluded.expected_sha256,
                hash_state = excluded.hash_state,
                connections = excluded.connections,
                error_json = excluded.error_json,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at,
                completed_at = excluded.completed_at,
                parent_id = excluded.parent_id,
                max_request_bytes = excluded.max_request_bytes,
                request_headers_json = excluded.request_headers_json,
                auth_sites_json = excluded.auth_sites_json",
            params![
                &task.id,
                &task.url,
                final_url,
                &task.file_name,
                &task.dest_dir,
                &task.temp_path,
                task.kind.as_str(),
                task.status.as_str(),
                total_bytes,
                downloaded_bytes,
                etag,
                last_modified,
                expected_sha256,
                task.hash_state.as_str(),
                i64::from(task.connections),
                error_json,
                task.created_at,
                task.updated_at,
                task.completed_at,
                task.parent_id.as_deref(),
                max_request_bytes,
                headers_json,
                auth_sites_json,
            ],
        )?;
        Ok(())
    }

    pub fn load_task(&self, id: &str) -> Result<Option<DownloadTask>> {
        let conn = self.conn()?;
        let mut statement = conn.prepare(&format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"))?;
        let mut rows = statement.query([id])?;
        match rows.next()? {
            Some(row) => Ok(Some(task_from_row(row)?)),
            None => Ok(None),
        }
    }

    /// Task gốc cho danh sách (không gồm task con ẩn).
    pub fn list_tasks(&self) -> Result<Vec<DownloadTask>> {
        let conn = self.conn()?;
        collect_tasks(
            &conn,
            &format!("SELECT {{columns}} FROM tasks WHERE {TOP_LEVEL} ORDER BY created_at DESC, id ASC"),
            [],
        )
    }

    /// Task con của `parent_id`, theo thứ tự tạo.
    pub fn list_children(&self, parent_id: &str) -> Result<Vec<DownloadTask>> {
        let conn = self.conn()?;
        collect_tasks(
            &conn,
            "SELECT {columns} FROM tasks WHERE parent_id = ?1 ORDER BY created_at ASC, id ASC",
            [parent_id],
        )
    }

    pub fn recoverable_tasks(&self) -> Result<Vec<DownloadTask>> {
        let conn = self.conn()?;
        collect_tasks(
            &conn,
            &format!(
                "SELECT {{columns}} FROM tasks WHERE status IN ('queued', 'paused') AND {TOP_LEVEL}
                 ORDER BY created_at ASC"
            ),
            [],
        )
    }

    pub fn status_counts(&self) -> Result<Vec<(TaskStatus, u64)>> {
        let conn = self.conn()?;
        let mut statement = conn.prepare(&format!(
            "SELECT status, COUNT(*) FROM tasks WHERE {TOP_LEVEL} GROUP BY status"
        ))?;
        let mut counts: Vec<(TaskStatus, u64)> =
            TaskStatus::ALL.into_iter().map(|status| (status, 0)).collect();
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let status = TaskStatus::parse(&row.get::<_, String>(0)?)?;
            let count = from_i64(row.get::<_, i64>(1)?, "task count")?;
            if let Some(slot) = counts.iter_mut().find(|(candidate, _)| *candidate == status) {
                slot.1 = count;
            }
        }
        Ok(counts)
    }

    /// Xoá task; task con (và đoạn của chúng) xoá theo nhờ khoá ngoại.
    pub fn delete_task(&self, id: &str) -> Result<()> {
        let conn = self.conn()?;
        let changed = conn.execute("DELETE FROM tasks WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(Error::NotFound(format!("task {id}")));
        }
        Ok(())
    }

    /// Như `delete_task` nhưng trả về các task con vừa xoá để dọn file tạm của chúng.
    pub fn delete_task_with_children(&self, id: &str) -> Result<Vec<DownloadTask>> {
        let mut conn = self.conn()?;
        let transaction = conn.transaction()?;
        let children = collect_tasks(
            &transaction,
            "SELECT {columns} FROM tasks WHERE parent_id = ?1 ORDER BY created_at ASC, id ASC",
            [id],
        )?;
        let changed = transaction.execute("DELETE FROM tasks WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(Error::NotFound(format!("task {id}")));
        }
        transaction.commit()?;
        Ok(children)
    }

    pub fn set_task_status(&self, id: &str, status: TaskStatus) -> Result<()> {
        let conn = self.conn()?;
        let changed = conn.execute(
            "UPDATE tasks SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, status.as_str(), now_ms()],
        )?;
        if changed == 0 {
            return Err(Error::NotFound(format!("task {id}")));
        }
        Ok(())
    }

    pub fn mark_interrupted(&self) -> Result<usize> {
        let conn = self.conn()?;
        let changed = conn.execute(
            "UPDATE tasks SET status = 'paused', updated_at = ?1
             WHERE status IN ('resolving', 'downloading', 'verifying')",
            [now_ms()],
        )?;
        Ok(changed)
    }

    pub fn reset_task_progress(&self, task_id: &str) -> Result<()> {
        let mut conn = self.conn()?;
        let transaction = conn.transaction()?;
        transaction.execute("DELETE FROM segments WHERE task_id = ?1", [task_id])?;
        let changed = transaction.execute(
            "UPDATE tasks SET downloaded_bytes = 0, updated_at = ?2 WHERE id = ?1",
            params![task_id, now_ms()],
        )?;
        if changed == 0 {
            return Err(Error::NotFound(format!("task {task_id}")));
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn save_segments(&self, task_id: &str, segments: &[Segment]) -> Result<()> {
        let mut conn = self.conn()?;
        let transaction = conn.transaction()?;
        let total = read_task_total(&transaction, task_id)?;
        validate_segments(total, segments)?;
        let downloaded = segments
            .iter()
            .try_fold(0u64, |sum, segment| sum.checked_add(segment.downloaded))
            .ok_or_else(|| Error::InvalidInput("downloaded bytes overflow".into()))?;
        transaction.execute("DELETE FROM segments WHERE task_id = ?1", [task_id])?;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO segments (task_id, idx, start_byte, end_byte, downloaded, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for segment in segments {
                statement.execute(params![
                    task_id,
                    i64::from(segment.index),
                    to_i64(segment.start)?,
                    to_i64(segment.end)?,
                    to_i64(segment.downloaded)?,
                    segment.status.as_str(),
                ])?;
            }
        }
        let changed = transaction.execute(
            "UPDATE tasks SET downloaded_bytes = ?2, updated_at = ?3 WHERE id = ?1",
            params![task_id, to_i64(downloaded)?, now_ms()],
        )?;
        if changed == 0 {
            return Err(Error::NotFound(format!("task {task_id}")));
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn load_segments(&self, task_id: &str) -> Result<Vec<Segment>> {
        let conn = self.conn()?;
        let mut statement = conn.prepare(
            "SELECT idx, start_byte, end_byte, downloaded, status
             FROM segments WHERE task_id = ?1 ORDER BY idx ASC",
        )?;
        let mut rows = statement.query([task_id])?;
        let mut segments = Vec::new();
        while let Some(row) = rows.next()? {
            segments.push(segment_from_row(row)?);
        }
        Ok(segments)
    }

    pub fn checkpoint(&self, task_id: &str, segments: &[Segment], downloaded_bytes: u64) -> Result<()> {
        let mut conn = self.conn()?;
        let transaction = conn.transaction()?;
        let total = read_task_total(&transaction, task_id)?;
        if !segments.is_empty() {
            validate_segments(total, segments)?;
            if total.is_some() {
                let sum = segments
                    .iter()
                    .try_fold(0u64, |sum, segment| sum.checked_add(segment.downloaded))
                    .ok_or_else(|| Error::InvalidInput("downloaded bytes overflow".into()))?;
                if sum != downloaded_bytes {
                    return Err(Error::InvalidInput(format!(
                        "checkpoint claims {downloaded_bytes} bytes but segments hold {sum}"
                    )));
                }
            }
        }
        {
            let mut statement = transaction.prepare(
                "UPDATE segments SET downloaded = ?3, status = ?4
                 WHERE task_id = ?1 AND idx = ?2",
            )?;
            for segment in segments {
                let changed = statement.execute(params![
                    task_id,
                    i64::from(segment.index),
                    to_i64(segment.downloaded)?,
                    segment.status.as_str(),
                ])?;
                if changed == 0 {
                    return Err(Error::Corrupt(format!(
                        "segment {} is missing from task {task_id}",
                        segment.index
                    )));
                }
            }
        }
        let changed = transaction.execute(
            "UPDATE tasks SET downloaded_bytes = ?2, updated_at = ?3 WHERE id = ?1",
            params![task_id, to_i64(downloaded_bytes)?, now_ms()],
        )?;
        if changed == 0 {
            return Err(Error::NotFound(format!("task {task_id}")));
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        if key.is_empty() || key.len() > 64 {
            return Err(Error::InvalidInput("setting key must be 1-64 characters".into()));
        }
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value, now_ms()],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn()?;
        conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0))
            .optional()
            .map_err(Error::from)
    }

    /// Bỏ một khoá cài đặt (không có thì thôi).
    pub fn delete_setting(&self, key: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
        Ok(())
    }

    pub fn save_media_job(&self, job: &MediaJob) -> Result<()> {
        job.validate()?;
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO media_jobs (task_id, url, page_url, selector, audio_format, title, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(task_id) DO UPDATE SET
                url = excluded.url,
                page_url = excluded.page_url,
                selector = excluded.selector,
                audio_format = excluded.audio_format,
                title = excluded.title,
                created_at = excluded.created_at",
            params![
                &job.task_id,
                &job.url,
                job.page_url.as_deref(),
                &job.selector,
                job.audio_format.as_deref(),
                &job.title,
                job.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn load_media_job(&self, task_id: &str) -> Result<Option<MediaJob>> {
        let conn = self.conn()?;
        let job = conn
            .query_row(
                "SELECT task_id, url, page_url, selector, audio_format, title, created_at
                 FROM media_jobs WHERE task_id = ?1",
                [task_id],
                |row| {
                    Ok(MediaJob {
                        task_id: row.get(0)?,
                        url: row.get(1)?,
                        page_url: row.get(2)?,
                        selector: row.get(3)?,
                        audio_format: row.get(4)?,
                        title: row.get(5)?,
                        created_at: row.get(6)?,
                    })
                },
            )
            .optional()?;
        if let Some(job) = &job {
            job.validate()?;
        }
        Ok(job)
    }

    pub fn integrity_check(&self) -> Result<Vec<String>> {
        let conn = self.conn()?;
        let mut statement = conn.prepare("PRAGMA quick_check")?;
        let mut rows = statement.query([])?;
        let mut problems = Vec::new();
        while let Some(row) = rows.next()? {
            let line: String = row.get(0)?;
            if line != "ok" {
                problems.push(line);
            }
        }
        Ok(problems)
    }

    fn conn(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| Error::Corrupt("journal lock is poisoned".into()))
    }
}

fn collect_tasks(
    conn: &Connection,
    query: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<DownloadTask>> {
    let query = query.replace("{columns}", TASK_COLUMNS);
    let mut statement = conn.prepare(&query)?;
    let mut rows = statement.query(params)?;
    let mut tasks = Vec::new();
    while let Some(row) = rows.next()? {
        tasks.push(task_from_row(row)?);
    }
    Ok(tasks)
}

fn task_from_row(row: &Row<'_>) -> Result<DownloadTask> {
    let total_bytes = row
        .get::<_, Option<i64>>(8)?
        .map(|value| from_i64(value, "total_bytes"))
        .transpose()?;
    let downloaded_bytes = from_i64(row.get(9)?, "downloaded_bytes")?;
    let connections = u16::try_from(row.get::<_, i64>(14)?)
        .map_err(|_| Error::Corrupt("connections out of range".into()))?;
    let error = match row.get::<_, Option<String>>(15)? {
        Some(json) => Some(serde_json::from_str::<TaskError>(&json)?),
        None => None,
    };
    let max_request_bytes = row
        .get::<_, Option<i64>>(20)?
        .map(|value| from_i64(value, "max_request_bytes"))
        .transpose()?;
    let request_headers = match row.get::<_, Option<String>>(21)? {
        Some(json) => serde_json::from_str::<Vec<RequestHeader>>(&json)?,
        None => Vec::new(),
    };
    let auth_sites = match row.get::<_, Option<String>>(22)? {
        Some(json) => serde_json::from_str::<Vec<String>>(&json)?,
        None => Vec::new(),
    };
    let task = DownloadTask {
        id: row.get(0)?,
        url: row.get(1)?,
        final_url: row.get(2)?,
        file_name: row.get(3)?,
        dest_dir: row.get(4)?,
        temp_path: row.get(5)?,
        kind: TaskKind::parse(&row.get::<_, String>(6)?)?,
        status: TaskStatus::parse(&row.get::<_, String>(7)?)?,
        total_bytes,
        downloaded_bytes,
        etag: row.get(10)?,
        last_modified: row.get(11)?,
        expected_sha256: row.get(12)?,
        hash_state: HashState::parse(&row.get::<_, String>(13)?)?,
        connections,
        error,
        created_at: row.get(16)?,
        updated_at: row.get(17)?,
        completed_at: row.get(18)?,
        parent_id: row.get(19)?,
        max_request_bytes,
        request_headers,
        auth_sites,
    };
    task.validate()?;
    Ok(task)
}

fn segment_from_row(row: &Row<'_>) -> Result<Segment> {
    let index = u32::try_from(row.get::<_, i64>(0)?)
        .map_err(|_| Error::Corrupt("segment index out of range".into()))?;
    Ok(Segment {
        index,
        start: from_i64(row.get(1)?, "segment start")?,
        end: from_i64(row.get(2)?, "segment end")?,
        downloaded: from_i64(row.get(3)?, "segment downloaded")?,
        status: SegmentStatus::parse(&row.get::<_, String>(4)?)?,
    })
}

fn read_task_total(conn: &Connection, task_id: &str) -> Result<Option<u64>> {
    let total: Option<i64> = conn
        .query_row("SELECT total_bytes FROM tasks WHERE id = ?1", [task_id], |row| row.get(0))
        .optional()?
        .ok_or_else(|| Error::NotFound(format!("task {task_id}")))?;
    total.map(|value| from_i64(value, "total_bytes")).transpose()
}

fn read_schema_version(conn: &Connection) -> Result<u32> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    u32::try_from(version)
        .map_err(|_| Error::Corrupt(format!("invalid user_version: {version}")))
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let mut version = read_schema_version(conn)?;
    for migration in MIGRATIONS {
        if migration.version <= version {
            continue;
        }
        let transaction = conn.transaction()?;
        transaction.execute_batch(migration.sql)?;
        transaction.execute(
            "INSERT OR REPLACE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            params![migration.version, now_ms()],
        )?;
        transaction.pragma_update(None, "user_version", i64::from(migration.version))?;
        transaction.commit()?;
        version = migration.version;
    }
    Ok(())
}

/// Chép journal sang chỗ mới thành một file nhất quán (gồm cả phần còn trong WAL).
/// Không ghi đè: đích đã có thì trả lỗi. Nguồn chỉ được đọc.
pub fn copy_database(source: &Path, target: &Path) -> Result<()> {
    if target.exists() {
        return Err(Error::InvalidInput(format!(
            "{} already exists",
            target.display()
        )));
    }
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let mut partial = target.as_os_str().to_os_string();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    let _ = fs::remove_file(&partial);
    let conn = Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    let destination = partial
        .to_str()
        .ok_or_else(|| Error::InvalidInput("journal path is not valid UTF-8".into()))?;
    conn.execute("VACUUM INTO ?1", [destination])?;
    drop(conn);
    fs::rename(&partial, target)?;
    Ok(())
}

fn backup_database(path: &Path) -> Result<PathBuf> {
    let mut name = path.as_os_str().to_os_string();
    name.push(".bak");
    let backup = PathBuf::from(name);
    fs::copy(path, &backup)?;
    Ok(backup)
}

fn to_i64(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| Error::InvalidInput(format!("value {value} exceeds sqlite integer range")))
}

fn from_i64(value: i64, field: &str) -> Result<u64> {
    u64::try_from(value).map_err(|_| Error::Corrupt(format!("{field} is negative")))
}
