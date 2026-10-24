//! macOS như Finder: mở bằng app mặc định (`open`), mở Finder và chọn sẵn file (`open -R`), bỏ vào
//! Thùng rác (`NSFileManager.trashItemAtURL`, không bao giờ xoá hẳn). Không bao giờ panic.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSFileManager, NSString, NSURL};

use super::super::text;

/// Mã lỗi của Cocoa khi bỏ vào Thùng rác.
const NO_TRASH_ON_VOLUME: isize = 3328; // NSFeatureUnsupportedError: ổ mạng, ổ chỉ đọc…
const NO_PERMISSION: [isize; 2] = [513, 642]; // NSFileWriteNoPermissionError, …VolumeReadOnlyError
const NO_SUCH_FILE: [isize; 2] = [4, 260]; // NSFileNoSuchFileError, NSFileReadNoSuchFileError

/// macOS không có COM; chỉ cần autorelease pool cho các đối tượng Cocoa tạo trên luồng nền.
pub fn with_com<T>(work: impl FnOnce() -> T) -> T {
    objc2::rc::autoreleasepool(|_| work())
}

/// `/usr/bin/open` của hệ thống (đợi nó giao việc cho LaunchServices xong, vài chục ms).
fn open_with(arguments: &[&OsStr]) -> Result<(), String> {
    let status = Command::new("/usr/bin/open")
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => {
            log::warn!("open exited with {status}");
            Err(text::OPEN_FAILED.into())
        }
        Err(_) => Err(text::OPEN_FAILED.into()),
    }
}

/// Mở bằng app mặc định như nhấp đúp trong Finder (Guard đã chặn loại file chạy được).
pub fn open_file(path: &Path) -> Result<(), String> {
    open_with(&[path.as_os_str()])
}

/// Mở chính thư mục trong Finder. Thư mục dạng gói (`X.app`) mà `open` thì sẽ chạy nó: chỉ chọn sẵn.
pub fn open_folder(folder: &Path) -> Result<(), String> {
    let package = NSWorkspace::sharedWorkspace()
        .isFilePackageAtPath(&NSString::from_str(&folder.to_string_lossy()));
    if package {
        return select_in_folder(folder);
    }
    open_with(&[folder.as_os_str()])
}

/// Mở Finder ở thư mục chứa file và chọn sẵn file đó.
pub fn select_in_folder(file: &Path) -> Result<(), String> {
    open_with(&[OsStr::new("-R"), file.as_os_str()])
}

/// Chuyển từng file vào Thùng rác của ổ chứa nó. Ổ không có Thùng rác thì dừng, không xoá gì thêm.
pub fn recycle(paths: &[PathBuf]) -> Result<(), String> {
    let manager = NSFileManager::defaultManager();
    for path in paths {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        if let Err(error) = manager.trashItemAtURL_resultingItemURL_error(&url, None) {
            let code = error.code();
            if NO_SUCH_FILE.contains(&code) {
                continue;
            }
            log::warn!("Trash failed (NSError {code})");
            return Err(if code == NO_TRASH_ON_VOLUME {
                text::NO_RECYCLE_BIN
            } else if NO_PERMISSION.contains(&code) {
                text::DENIED
            } else {
                text::RECYCLE_FAILED
            }
            .into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trash_moves_files_out_and_skips_missing_ones() {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let root = std::env::temp_dir().join(format!("utd-trash-{id}"));
        std::fs::create_dir_all(&root).unwrap();
        let name = format!("utd thu {id}.txt");
        let file = root.join(&name);
        std::fs::write(&file, b"x").unwrap();
        with_com(|| recycle(&[file.clone(), root.join("khong-co.txt")])).unwrap();
        assert!(!file.exists());
        // Dọn bản thử khỏi Thùng rác (không đọc được ~/.Trash khi thiếu quyền thì thôi).
        if let Some(home) = std::env::var_os("HOME") {
            let _ = std::fs::remove_file(PathBuf::from(home).join(".Trash").join(&name));
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
