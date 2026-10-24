//! Đổi tên / chuyển file trên đĩa: không bao giờ ghi đè; hỏng giữa chừng thì file gốc còn nguyên.

use std::io;
use std::path::{Path, PathBuf};

use super::text;

/// Mã lỗi Windows hay gặp khi đổi tên/chép file.
#[cfg(windows)]
mod code {
    pub const NOT_FOUND: [i32; 2] = [2, 3];
    pub const ACCESS_DENIED: i32 = 5;
    pub const IN_USE: [i32; 2] = [32, 33];
    pub const NOT_SAME_DEVICE: i32 = 17;
    pub const ALREADY_EXISTS: [i32; 2] = [80, 183];
    pub const DISK_FULL: [i32; 2] = [39, 112];
    pub const NAME_TOO_LONG: i32 = 206;
}

/// Cùng ý nghĩa, theo errno của macOS/Unix (số khác Windows: 17 ở đây là "đã có").
#[cfg(unix)]
mod code {
    pub const NOT_FOUND: [i32; 2] = [libc::ENOENT, libc::ENOTDIR];
    pub const ACCESS_DENIED: i32 = libc::EACCES;
    pub const IN_USE: [i32; 2] = [libc::EBUSY, libc::ETXTBSY];
    pub const NOT_SAME_DEVICE: i32 = libc::EXDEV;
    pub const ALREADY_EXISTS: [i32; 2] = [libc::EEXIST, libc::ENOTEMPTY];
    pub const DISK_FULL: [i32; 2] = [libc::ENOSPC, libc::EDQUOT];
    pub const NAME_TOO_LONG: i32 = libc::ENAMETOOLONG;
}

use code::{
    ACCESS_DENIED, ALREADY_EXISTS, DISK_FULL, IN_USE, NAME_TOO_LONG, NOT_FOUND, NOT_SAME_DEVICE,
};

/// Câu ngắn cho người dùng; chi tiết chỉ ghi mã lỗi vào log (không ghi đường dẫn).
pub(super) fn describe(error: &io::Error) -> String {
    let code = error.raw_os_error().unwrap_or(-1);
    let message = if NOT_FOUND.contains(&code) || error.kind() == io::ErrorKind::NotFound {
        text::FILE_GONE
    } else if IN_USE.contains(&code) {
        text::IN_USE
    } else if code == ACCESS_DENIED || error.kind() == io::ErrorKind::PermissionDenied {
        text::DENIED
    } else if ALREADY_EXISTS.contains(&code) || error.kind() == io::ErrorKind::AlreadyExists {
        text::EXISTS
    } else if DISK_FULL.contains(&code) {
        text::DISK_FULL
    } else if code == NAME_TOO_LONG {
        text::TOO_LONG
    } else {
        log::warn!("File operation failed: {:?} (os {code})", error.kind());
        text::FAILED
    };
    message.into()
}

/// Có file/thư mục/liên kết ở `path` (kể cả liên kết hỏng).
pub(super) fn occupied(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// Đổi tên trong cùng ổ, không ghi đè. Trên Windows đổi hoa/thường của chính file đó vẫn được.
#[cfg(windows)]
pub(super) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use windows::Win32::Storage::FileSystem::{MOVE_FILE_FLAGS, MoveFileExW};
    use windows::core::PCWSTR;

    let (source, target) = (verbatim_wide(from), verbatim_wide(to));
    // Không có MOVEFILE_REPLACE_EXISTING ⇒ đích đã có thì lỗi; không có COPY_ALLOWED ⇒ khác ổ thì lỗi 17.
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(target.as_ptr()),
            MOVE_FILE_FLAGS(0),
        )
    }
    .map_err(|error| {
        let code = error.code().0 as u32;
        if code & 0xFFFF_0000 == 0x8007_0000 {
            io::Error::from_raw_os_error((code & 0xFFFF) as i32)
        } else {
            io::Error::other(error)
        }
    })
}

/// Đường dẫn dạng `\\?\` (UTF-16, kết thúc bằng 0): thư mục 200 ký tự + tên 180 ký tự vượt
/// giới hạn 260 ký tự của API cũ. Hàm `std::fs` tự làm việc này, `MoveFileExW` thì không.
#[cfg(windows)]
fn verbatim_wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let full = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let text: Vec<u16> = full.as_os_str().encode_wide().collect();
    let (slash, colon) = (u16::from(b'\\'), u16::from(b':'));
    let disk = text.len() > 2 && text[1] == colon && text[2] == slash;
    let unc = text.len() > 2
        && text[0] == slash
        && text[1] == slash
        && !matches!(char::from_u32(u32::from(text[2])), Some('?' | '.'));
    let mut out: Vec<u16> = Vec::with_capacity(text.len() + 9);
    if disk {
        out.extend(r"\\?\".encode_utf16());
        out.extend(&text);
    } else if unc {
        // `\\máy\chia-sẻ\…` → `\\?\UNC\máy\chia-sẻ\…`
        out.extend(r"\\?\UNC".encode_utf16());
        out.extend(&text[1..]);
    } else {
        out.extend(&text);
    }
    out.push(0);
    out
}

/// macOS: `renamex_np(RENAME_EXCL)` đổi tên một bước, đích đã có thì lỗi `EEXIST`. Ổ mặc định không
/// phân biệt hoa/thường nên đổi hoa/thường của chính file đó cũng báo "đã có": nhận ra rồi đổi thẳng.
#[cfg(target_os = "macos")]
pub(super) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = |path: &Path| {
        std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
    };
    let (source, target) = (c_path(from)?, c_path(to)?);
    // SAFETY: hai chuỗi C kết thúc bằng NUL, sống tới hết lệnh gọi.
    if unsafe { libc::renamex_np(source.as_ptr(), target.as_ptr(), libc::RENAME_EXCL) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::EEXIST) if same_file(from, to) => std::fs::rename(from, to),
        // Ổ không hỗ trợ RENAME_EXCL (exFAT, FAT32, ổ mạng cũ).
        Some(libc::ENOTSUP | libc::EINVAL) => link_then_unlink(from, to),
        _ => Err(error),
    }
}

#[cfg(target_os = "macos")]
fn same_file(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::symlink_metadata(left), std::fs::symlink_metadata(right)) {
        (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
        _ => false,
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    link_then_unlink(from, to)
}

/// Liên kết cứng (đích đã có thì lỗi) rồi gỡ tên cũ; ổ không có liên kết cứng thì kiểm rồi đổi tên.
#[cfg(unix)]
fn link_then_unlink(from: &Path, to: &Path) -> io::Result<()> {
    match std::fs::hard_link(from, to) {
        Ok(()) => std::fs::remove_file(from),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists || occupied(to) => Err(error),
        Err(error) if error.raw_os_error() == Some(NOT_SAME_DEVICE) => Err(error),
        Err(_) => std::fs::rename(from, to),
    }
}

/// Chuyển `from` → `to` (đích chưa có). Cùng ổ: đổi tên. Khác ổ: chép sang tên tạm cạnh đích,
/// so kích thước, đổi thành tên thật, gắn lại Mark of the Web (`mark_web`) rồi mới xoá file gốc.
/// Không xoá được file gốc (đang mở) ⇒ gỡ bản chép, báo lỗi: file gốc còn nguyên.
pub(super) fn move_file(from: &Path, to: &Path, mark_web: bool) -> Result<(), String> {
    if occupied(to) {
        return Err(text::EXISTS.into());
    }
    match rename_no_replace(from, to) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(NOT_SAME_DEVICE) => {
            copy_across(from, to, mark_web)
        }
        Err(error) => Err(describe(&error)),
    }
}

fn copy_across(from: &Path, to: &Path, mark_web: bool) -> Result<(), String> {
    let staging = staging_path(to);
    if occupied(&staging) {
        return Err(text::FAILED.into());
    }
    let cleanup = |message: String| {
        let _ = std::fs::remove_file(&staging);
        Err(message)
    };
    let expected = std::fs::metadata(from)
        .map_err(|error| describe(&error))?
        .len();
    if let Err(error) = std::fs::copy(from, &staging) {
        return cleanup(describe(&error));
    }
    let synced = std::fs::OpenOptions::new()
        .write(true)
        .open(&staging)
        .and_then(|file| file.sync_all());
    let copied = std::fs::metadata(&staging).map(|metadata| metadata.len());
    if synced.is_err() || copied.ok() != Some(expected) {
        return cleanup(text::COPY_SHORT.into());
    }
    if let Err(error) = rename_no_replace(&staging, to) {
        return cleanup(describe(&error));
    }
    // Chép sang ổ FAT/exFAT hoặc ổ mạng làm rơi luồng Zone.Identifier.
    if mark_web {
        let _ = utd_engine::safety::mark_of_the_web(to);
    }
    if let Err(error) = std::fs::remove_file(from) {
        let _ = std::fs::remove_file(to);
        return Err(describe(&error));
    }
    Ok(())
}

/// Tên tạm ẩn cạnh đích, không trùng ai: `.<tên>.<8 hex>.utd-move`.
fn staging_path(to: &Path) -> PathBuf {
    let name = to
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    to.with_file_name(format!(".{name}.{}.utd-move", &suffix[..8]))
}

/// Trả file về chỗ cũ khi journal không lưu được (cố hết sức, chỉ ghi log nếu hỏng).
pub(super) fn move_back(now: &Path, was: &Path, mark_web: bool) {
    if let Err(message) = move_file(now, was, mark_web) {
        log::warn!("Could not move a file back after a journal error: {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let root = std::env::temp_dir().join(format!("utd-disk-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn rename_never_overwrites() {
        let root = scratch();
        let (part, other) = (root.join("a.zip.1234abcd.part"), root.join("b.zip.part"));
        std::fs::write(&part, b"dang tai").unwrap();
        std::fs::write(&other, b"cua nguoi khac").unwrap();
        assert!(rename_no_replace(&part, &other).is_err());
        assert_eq!(std::fs::read(&other).unwrap(), b"cua nguoi khac");
        assert_eq!(move_file(&part, &other, false).unwrap_err(), text::EXISTS);
        assert_eq!(
            std::fs::read(&part).unwrap(),
            b"dang tai",
            "file gốc còn nguyên"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn move_into_another_folder() {
        let root = scratch();
        let target_dir = root.join("Phim");
        std::fs::create_dir_all(&target_dir).unwrap();
        let from = root.join("a.mp4");
        std::fs::write(&from, b"video").unwrap();
        let to = target_dir.join("a.mp4");
        move_file(&from, &to, true).unwrap();
        assert!(!from.exists());
        assert_eq!(std::fs::read(&to).unwrap(), b"video");
        assert_eq!(
            move_file(&root.join("mat.mp4"), &root.join("x.mp4"), false).unwrap_err(),
            text::FILE_GONE
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copy_across_cleans_up_and_keeps_the_source() {
        let root = scratch();
        let from = root.join("game.iso");
        std::fs::write(&from, b"iso data").unwrap();
        let to = root.join("copy").join("game.iso");
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        copy_across(&from, &to, true).unwrap();
        assert!(!from.exists());
        assert_eq!(std::fs::read(&to).unwrap(), b"iso data");
        assert_eq!(
            std::fs::read_dir(to.parent().unwrap()).unwrap().count(),
            1,
            "không còn file tạm"
        );
        // Đích bị chiếm giữa chừng ⇒ lỗi, nguồn còn nguyên, không sót file tạm.
        let again = root.join("again.iso");
        std::fs::write(&again, b"second").unwrap();
        assert_eq!(copy_across(&again, &to, false).unwrap_err(), text::EXISTS);
        assert_eq!(std::fs::read(&again).unwrap(), b"second");
        assert_eq!(std::fs::read(&to).unwrap(), b"iso data");
        assert_eq!(std::fs::read_dir(to.parent().unwrap()).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_paths_for_long_names() {
        let text = |path: &str| {
            String::from_utf16_lossy(&verbatim_wide(Path::new(path)))
                .trim_end_matches('\0')
                .to_string()
        };
        assert_eq!(text(r"C:\a\b.zip"), r"\\?\C:\a\b.zip");
        assert_eq!(text("C:/a/b.zip"), r"\\?\C:\a\b.zip");
        assert_eq!(text(r"\\nas\share\b.zip"), r"\\?\UNC\nas\share\b.zip");
        assert_eq!(text(r"\\?\C:\a"), r"\\?\C:\a");
    }

    #[cfg(windows)]
    #[test]
    fn rename_handles_paths_over_260_chars() {
        let root = scratch();
        let deep = root.join("x".repeat(120)).join("y".repeat(120));
        std::fs::create_dir_all(&deep).unwrap();
        let from = deep.join(format!("{}.part", "a".repeat(100)));
        let to = deep.join(format!("{}.part", "b".repeat(100)));
        assert!(to.as_os_str().len() > 260);
        std::fs::write(&from, b"part").unwrap();
        move_file(&from, &to, false).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"part");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn case_only_rename_works_on_windows_and_mac() {
        let root = scratch();
        let (lower, upper) = (root.join("phim.mp4"), root.join("Phim.MP4"));
        std::fs::write(&lower, b"x").unwrap();
        rename_no_replace(&lower, &upper).unwrap();
        let names: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["Phim.MP4".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn copy_across_reapplies_mark_of_the_web() {
        let root = scratch();
        let from = root.join("tai-lieu.pdf");
        std::fs::write(&from, b"%PDF-1.7").unwrap();
        let to = root.join("dich").join("tai-lieu.pdf");
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        copy_across(&from, &to, true).unwrap();
        let mut stream = to.as_os_str().to_os_string();
        stream.push(":Zone.Identifier");
        let zone = std::fs::read_to_string(PathBuf::from(stream)).unwrap();
        assert!(zone.contains("ZoneId=3"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// macOS: "Mark of the Web" là thuộc tính mở rộng `com.apple.quarantine` (Gatekeeper đọc nó).
    #[cfg(target_os = "macos")]
    #[test]
    fn copy_across_reapplies_the_quarantine_attribute() {
        use std::os::unix::ffi::OsStrExt;
        let root = scratch();
        let from = root.join("tai-lieu.pdf");
        std::fs::write(&from, b"%PDF-1.7").unwrap();
        let to = root.join("dich").join("tai-lieu.pdf");
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        copy_across(&from, &to, true).unwrap();
        let path = std::ffi::CString::new(to.as_os_str().as_bytes()).unwrap();
        let mut value = [0u8; 128];
        // SAFETY: chuỗi C kết thúc bằng NUL, bộ đệm đủ chỗ cho `value.len()` byte.
        let read = unsafe {
            libc::getxattr(
                path.as_ptr(),
                c"com.apple.quarantine".as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        assert!(read > 0, "có cờ quarantine");
        let text = String::from_utf8_lossy(&value[..read as usize]).into_owned();
        assert!(text.starts_with("0081;") && text.contains(";UTD;"), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
