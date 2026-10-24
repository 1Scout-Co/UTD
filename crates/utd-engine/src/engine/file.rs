use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(crate) struct PartFile {
    file: File,
}

impl PartFile {
    pub fn open(path: &Path, total: Option<u64>) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        if let Some(total) = total {
            file.set_len(total)?;
        }
        Ok(Self { file })
    }

    pub fn reset(&self, total: Option<u64>) -> io::Result<()> {
        self.file.set_len(0)?;
        if let Some(total) = total {
            self.file.set_len(total)?;
        }
        Ok(())
    }

    pub fn write_at(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        let mut written = 0usize;
        while written < data.len() {
            let count = write_at(&self.file, &data[written..], offset + written as u64)?;
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "positional write returned zero bytes",
                ));
            }
            written += count;
        }
        Ok(())
    }
}

pub(crate) fn target_path(dest_dir: &str, file_name: &str) -> PathBuf {
    Path::new(dest_dir).join(file_name)
}

/// Đưa file tạm vào chỗ mà không ghi đè; tên đã có ⇒ lỗi `AlreadyExists`, file tạm giữ nguyên.
pub(crate) async fn move_no_clobber(temp: &Path, target: &Path) -> io::Result<()> {
    let temp = temp.to_path_buf();
    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || move_blocking(&temp, &target))
        .await
        .map_err(io::Error::other)?
}

/// Hard link rồi xoá file tạm: tạo link báo lỗi nếu tên đã có, kể cả file vừa xuất hiện (rename
/// trên Windows thì ghi đè). Ổ không có hard link (FAT32/exFAT, vài ổ mạng): đổi tên không thay thế.
fn move_blocking(temp: &Path, target: &Path) -> io::Result<()> {
    match std::fs::hard_link(temp, target) {
        Ok(()) => {
            if let Err(error) = std::fs::remove_file(temp) {
                let _ = std::fs::remove_file(target);
                return Err(error);
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(error),
        Err(_) => rename_no_replace(temp, target),
    }
}

/// `MoveFileExW` không có `MOVEFILE_REPLACE_EXISTING`: hệ điều hành kiểm tên ngay trong lệnh đổi
/// tên (chạy cả trên FAT32/exFAT, ổ mạng), tên đã có ⇒ `ERROR_ALREADY_EXISTS` (`AlreadyExists`).
#[cfg(windows)]
fn rename_no_replace(temp: &Path, target: &Path) -> io::Result<()> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let (from, to) = (wide_path(temp)?, wide_path(target)?);
    // SAFETY: hai chuỗi UTF-16 kết thúc bằng NUL, sống tới hết lệnh gọi; flags = 0.
    let moved = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) };
    if moved == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Đường dẫn cho API `W`: tuyệt đối (đã chuẩn hoá `/`, `..`) kèm `\\?\` để qua giới hạn 260 ký
/// tự, như `std::fs` tự làm (tên video dài).
#[cfg(windows)]
fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    let full: Vec<u16> = std::path::absolute(path)?
        .as_os_str()
        .encode_wide()
        .collect();
    let starts = |prefix: &str| full.starts_with(&prefix.encode_utf16().collect::<Vec<_>>());
    let mut wide: Vec<u16> = if starts(r"\\?\") || starts(r"\\.\") {
        full
    } else if starts(r"\\") {
        // `\\máy\thư mục` ⇒ `\\?\UNC\máy\thư mục`.
        r"\\?\UNC"
            .encode_utf16()
            .chain(full[1..].iter().copied())
            .collect()
    } else {
        r"\\?\".encode_utf16().chain(full).collect()
    };
    if wide.contains(&0) {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    wide.push(0);
    Ok(wide)
}

/// Unix không có lệnh chung để đổi tên không thay thế: kiểm rồi đổi tên (còn một khe rất nhỏ).
#[cfg(unix)]
fn rename_no_replace(temp: &Path, target: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(target).is_ok() {
        return Err(io::Error::from(io::ErrorKind::AlreadyExists));
    }
    std::fs::rename(temp, target)
}

#[cfg(windows)]
fn write_at(file: &File, data: &[u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_write(data, offset)
}

#[cfg(unix)]
fn write_at(file: &File, data: &[u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.write_at(data, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_target_that_appeared_after_the_download() {
        let directory = tempfile::tempdir().expect("tempdir");
        let temp = directory.path().join("video.mp4.part");
        let target = directory.path().join("video.mp4");
        std::fs::write(&temp, b"downloaded").expect("part");
        std::fs::write(&target, b"user file").expect("target");

        let error = move_blocking(&temp, &target).expect_err("overwrote the target");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&target).expect("target"), b"user file");
        assert_eq!(std::fs::read(&temp).expect("part"), b"downloaded");
    }

    #[test]
    fn moves_into_a_free_name() {
        let directory = tempfile::tempdir().expect("tempdir");
        let temp = directory.path().join("video.mp4.part");
        let target = directory.path().join("video.mp4");
        std::fs::write(&temp, b"downloaded").expect("part");

        move_blocking(&temp, &target).expect("move");
        assert_eq!(std::fs::read(&target).expect("target"), b"downloaded");
        assert!(!temp.exists(), "the part file was left behind");
    }

    /// Nhánh dự phòng (ổ không có hard link): tên vừa xuất hiện sau lúc kiểm vẫn không bị thay.
    #[test]
    fn the_rename_fallback_keeps_an_existing_target() {
        let directory = tempfile::tempdir().expect("tempdir");
        let temp = directory.path().join("video.mp4.part");
        let target = directory.path().join("video.mp4");
        std::fs::write(&temp, b"downloaded").expect("part");
        std::fs::write(&target, b"user file").expect("target");

        let error = rename_no_replace(&temp, &target).expect_err("overwrote the target");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&target).expect("target"), b"user file");
        assert_eq!(std::fs::read(&temp).expect("part"), b"downloaded");
    }

    /// Đường dẫn quá 260 ký tự (tiêu đề video dài) và viết bằng `/` vẫn đổi tên được.
    #[test]
    fn the_rename_fallback_moves_long_and_slash_paths() {
        let directory = tempfile::tempdir().expect("tempdir");
        let folder = directory.path().join("d".repeat(120)).join("e".repeat(120));
        std::fs::create_dir_all(&folder).expect("folder");
        let temp = folder.join(format!("{}.mp4.part", "v".repeat(100)));
        let target = folder.join(format!("{}.mp4", "v".repeat(100)));
        std::fs::write(&temp, b"downloaded").expect("part");

        rename_no_replace(&temp, &target).expect("long move");
        assert_eq!(std::fs::read(&target).expect("target"), b"downloaded");
        assert!(!temp.exists(), "the part file was left behind");

        let slashed = PathBuf::from(format!("{}/video.mp4", directory.path().display()));
        rename_no_replace(&target, &slashed).expect("slash move");
        let moved = directory.path().join("video.mp4");
        assert_eq!(std::fs::read(moved).expect("moved"), b"downloaded");
    }
}
