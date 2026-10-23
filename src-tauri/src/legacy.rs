//! Bản trước lưu dữ liệu ở `%LOCALAPPDATA%\UTD`, trùng thư mục cài của bộ cài NSIS (currentUser).
//! Lần đầu mở bản dùng `UTD-Data`: chép journal, chuyển công cụ media và khu cách ly sang chỗ mới.
//! Thư mục cũ giữ nguyên để quay lại được.

use std::path::{Path, PathBuf};

pub struct Migration {
    pub browser_integration: bool,
}

pub fn migrate(data_dir: &Path) -> Result<Option<Migration>, String> {
    match legacy_dir() {
        Some(legacy) => migrate_from(&legacy, data_dir),
        None => Ok(None),
    }
}

fn legacy_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(|local| PathBuf::from(local).join("UTD"))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Có file này trong UTD-Data = đã chuyển xong (hoặc đã quyết định giữ journal mới): không chạy lại.
const MARKER: &str = "legacy-migrated";

fn migrate_from(legacy: &Path, data_dir: &Path) -> Result<Option<Migration>, String> {
    let legacy_db = legacy.join("utd.db");
    let target_db = data_dir.join("utd.db");
    let marker = data_dir.join(MARKER);
    if legacy == data_dir || !legacy_db.is_file() || marker.exists() {
        return Ok(None);
    }
    std::fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
    // Lần trước chép hỏng thì app đã mở journal rỗng ở chỗ mới: chép lại đè lên nó.
    // Journal mới đã có download thì giữ nguyên, không trộn hai lịch sử.
    if target_db.exists() && has_tasks(&target_db)? {
        log::warn!(
            "Kept old UTD journal in {}: {} already has downloads",
            legacy.display(),
            data_dir.display()
        );
        write_marker(&marker);
        return Ok(None);
    }
    copy_journal(&legacy_db, &target_db)?;
    for name in ["bin", "quarantine"] {
        move_entries(&legacy.join(name), &data_dir.join(name));
    }
    write_marker(&marker);
    log::info!(
        "Moved UTD data from {} to {}",
        legacy.display(),
        data_dir.display()
    );
    Ok(Some(Migration {
        browser_integration: legacy.join("native-messaging").is_dir(),
    }))
}

fn has_tasks(journal: &Path) -> Result<bool, String> {
    let journal = utd_engine::Journal::open(journal).map_err(|error| error.to_string())?;
    let tasks = journal.list_tasks().map_err(|error| error.to_string())?;
    Ok(!tasks.is_empty())
}

/// Chép ra tệp tạm trước: chép hỏng thì journal đang có (dù rỗng) vẫn nguyên.
fn copy_journal(source: &Path, target: &Path) -> Result<(), String> {
    let staged = target.with_file_name("utd.db.migrating");
    let _ = std::fs::remove_file(&staged);
    utd_engine::db::copy_database(source, &staged).map_err(|error| error.to_string())?;
    let mut old = vec![target.to_path_buf()];
    for suffix in ["-wal", "-shm"] {
        let mut name = target.as_os_str().to_os_string();
        name.push(suffix);
        old.push(PathBuf::from(name));
    }
    let cleared = old.iter().try_for_each(|path| match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    });
    let placed = cleared.and_then(|()| std::fs::rename(&staged, target));
    if let Err(error) = placed {
        let _ = std::fs::remove_file(&staged);
        return Err(error.to_string());
    }
    Ok(())
}

fn write_marker(marker: &Path) {
    if let Err(error) = std::fs::write(marker, b"1") {
        log::warn!("Could not write {}: {error}", marker.display());
    }
}

/// Cùng ổ nên `rename` là tức thì (file cách ly có thể nhiều GB). Không ghi đè file đã có ở đích.
fn move_entries(source: &Path, target: &Path) {
    let Ok(entries) = std::fs::read_dir(source) else {
        return;
    };
    if std::fs::create_dir_all(target).is_err() {
        return;
    }
    for entry in entries.flatten() {
        let destination = target.join(entry.file_name());
        if destination.exists() {
            continue;
        }
        if let Err(error) = std::fs::rename(entry.path(), &destination) {
            log::warn!("Could not move {}: {error}", entry.path().display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use utd_engine::{DownloadTask, Journal};

    fn scratch() -> PathBuf {
        std::env::temp_dir().join(format!("utd-legacy-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn moves_journal_tools_and_quarantine_once() {
        let root = scratch();
        let legacy = root.join("UTD");
        let data_dir = root.join("UTD-Data");
        let journal = Journal::open(legacy.join("utd.db")).unwrap();
        let task = DownloadTask::new(
            "https://example.com/a.bin",
            "a.bin",
            root.join("Tải về").to_string_lossy().into_owned(),
            4,
        )
        .unwrap();
        journal.save_task(&task).unwrap();
        std::fs::create_dir_all(legacy.join("bin")).unwrap();
        std::fs::write(legacy.join("bin").join("yt-dlp.exe"), b"tool").unwrap();
        std::fs::create_dir_all(legacy.join("quarantine")).unwrap();
        std::fs::write(legacy.join("quarantine").join("x.exe.1.quarantined"), b"held").unwrap();
        std::fs::create_dir_all(legacy.join("native-messaging")).unwrap();

        let migration = migrate_from(&legacy, &data_dir).unwrap().expect("migrated");
        assert!(migration.browser_integration);
        drop(journal);
        let copied = Journal::open(data_dir.join("utd.db")).unwrap();
        assert_eq!(copied.load_task(&task.id).unwrap(), Some(task));
        assert!(data_dir.join("bin").join("yt-dlp.exe").is_file());
        assert!(data_dir.join("quarantine").join("x.exe.1.quarantined").is_file());
        assert!(legacy.join("utd.db").is_file(), "bản cũ được giữ");

        assert!(migrate_from(&legacy, &data_dir).unwrap().is_none(), "chỉ chạy một lần");
        drop(copied);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn saved_task(journal: &Journal, root: &Path, name: &str) -> DownloadTask {
        let task = DownloadTask::new(
            format!("https://example.com/{name}"),
            name,
            root.join("Tải về").to_string_lossy().into_owned(),
            4,
        )
        .unwrap();
        journal.save_task(&task).unwrap();
        task
    }

    /// Lần đầu chép hỏng ⇒ app mở journal rỗng ở UTD-Data; lần mở sau phải chép lại, không bỏ rơi lịch sử cũ.
    #[test]
    fn retries_after_a_failed_copy() {
        let root = scratch();
        let legacy = root.join("UTD");
        let data_dir = root.join("UTD-Data");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("utd.db"), b"khong phai sqlite").unwrap();
        assert!(migrate_from(&legacy, &data_dir).is_err());
        assert!(!data_dir.join(MARKER).exists());
        drop(Journal::open(data_dir.join("utd.db")).unwrap());

        std::fs::remove_file(legacy.join("utd.db")).unwrap();
        let old = Journal::open(legacy.join("utd.db")).unwrap();
        let task = saved_task(&old, &root, "a.bin");
        drop(old);
        assert!(migrate_from(&legacy, &data_dir).unwrap().is_some(), "chép lại");
        let copied = Journal::open(data_dir.join("utd.db")).unwrap();
        assert_eq!(copied.load_task(&task.id).unwrap(), Some(task));
        drop(copied);
        assert!(data_dir.join(MARKER).exists());
        assert!(!data_dir.join("utd.db.migrating").exists());
        assert!(migrate_from(&legacy, &data_dir).unwrap().is_none(), "xong thì thôi");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn keeps_a_new_journal_that_already_has_downloads() {
        let root = scratch();
        let legacy = root.join("UTD");
        let data_dir = root.join("UTD-Data");
        let old = Journal::open(legacy.join("utd.db")).unwrap();
        saved_task(&old, &root, "cu.bin");
        drop(old);
        let fresh = Journal::open(data_dir.join("utd.db")).unwrap();
        let mine = saved_task(&fresh, &root, "moi.bin");
        drop(fresh);
        assert!(migrate_from(&legacy, &data_dir).unwrap().is_none());
        assert!(data_dir.join(MARKER).exists(), "không hỏi lại mỗi lần mở");
        let kept = Journal::open(data_dir.join("utd.db")).unwrap();
        assert_eq!(kept.list_tasks().unwrap(), vec![mine]);
        drop(kept);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn skips_when_there_is_no_old_journal() {
        let root = scratch();
        std::fs::create_dir_all(root.join("UTD")).unwrap();
        assert!(migrate_from(&root.join("UTD"), &root.join("UTD-Data"))
            .unwrap()
            .is_none());
        assert!(!root.join("UTD-Data").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
