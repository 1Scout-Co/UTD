//! Kiểm tên mới và chứng minh một đường dẫn đúng là file của task (không đụng đĩa ngoài đọc).

use std::ffi::OsStr;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use utd_engine::safety::{self, GuardAction};
use utd_engine::{DownloadTask, validate_file_name};

use super::text;

/// Đuôi hay bị dùng làm mồi: `hoa-don.pdf.exe` trông như PDF khi Windows ẩn đuôi.
const DECOYS: [&str; 34] = [
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "csv", "odt", "jpg", "jpeg",
    "png", "gif", "bmp", "webp", "svg", "heic", "mp3", "mp4", "m4a", "mkv", "avi", "mov", "wmv",
    "wav", "flac", "zip", "rar", "7z", "iso", "html", "htm",
];

/// Cùng một danh sách với Download Guard, để đổi tên không lách được cách ly.
pub(super) fn runs_code(extension: &str) -> bool {
    safety::dangerous_extension(&extension.to_ascii_lowercase())
}

/// Đuôi file viết thường; rỗng khi không có đuôi.
pub(super) fn extension_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_lowercase()
}

/// Ký tự vô hình hoặc đảo chiều chữ (bidi): `phim\u{202E}4pm.exe` hiện thành `phimexe.mp4`.
fn is_hidden_char(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
    )
}

/// Tên mới hợp lệ cho Windows và không giả dạng loại file khác. Trả tên đã bỏ khoảng trắng hai đầu.
pub(super) fn check_new_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(text::EMPTY_NAME.into());
    }
    if name.chars().any(is_hidden_char) {
        return Err(text::HIDDEN_CHARS.into());
    }
    validate_file_name(name).map_err(|_| text::BAD_NAME.to_string())?;
    let parts: Vec<&str> = name.split('.').collect();
    if let [.., last] = parts.as_slice()
        && parts.len() >= 2
        && runs_code(last)
    {
        let decoy = match parts.as_slice() {
            [_, .., inner, _] => DECOYS.contains(&inner.trim().to_lowercase().as_str()),
            _ => false,
        };
        // Nhiều khoảng trắng liền nhau đẩy đuôi thật ra khỏi chỗ nhìn thấy.
        if decoy || name.contains("   ") {
            return Err(text::SPOOFED.into());
        }
    }
    Ok(name.to_string())
}

/// Hai tên chỉ khác hoa/thường: Windows và ổ mặc định của macOS (APFS) coi là cùng một file.
pub(super) fn same_name(left: &str, right: &str) -> bool {
    cfg!(any(windows, target_os = "macos")) && left.to_lowercase() == right.to_lowercase()
}

/// Đổi tên file đã xong không được biến nó thành thứ Guard sẽ cách ly (đổi `.txt` thành `.cmd`…).
pub(super) fn guard_allows(file: &Path, old_name: &str, new_name: &str) -> Result<(), String> {
    let quarantined = |name: &str| {
        safety::inspect(file, name)
            .map(|report| report.action == GuardAction::Quarantine)
            .map_err(|_| text::READ_FAILED.to_string())
    };
    if quarantined(new_name)? && !quarantined(old_name)? {
        return Err(text::RUNNABLE.into());
    }
    Ok(())
}

/// File `name` nằm ngay trong `dir`: None khi không có; lỗi khi là thư mục, liên kết,
/// hoặc đường dẫn thật trỏ ra ngoài. Trả đường dẫn thật (canonical, dạng `\\?\`).
pub(super) fn owned_file(dir: &Path, name: &OsStr) -> Result<Option<PathBuf>, String> {
    if !is_plain_name(name) {
        return Err(text::OUTSIDE.into());
    }
    let path = dir.join(name);
    match std::fs::symlink_metadata(&path) {
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(None);
        }
        Err(_) => return Err(text::DENIED.into()),
        Ok(metadata) if !metadata.is_file() => return Err(text::OUTSIDE.into()),
        Ok(_) => {}
    }
    let real = std::fs::canonicalize(&path).map_err(|_| text::DENIED.to_string())?;
    let real_dir = std::fs::canonicalize(dir).map_err(|_| text::DENIED.to_string())?;
    if real.parent() != Some(real_dir.as_path()) {
        return Err(text::OUTSIDE.into());
    }
    Ok(Some(real))
}

/// Một thành phần tên thường (không `..`, không dấu phân cách).
fn is_plain_name(name: &OsStr) -> bool {
    let mut parts = Path::new(name).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    )
}

/// File đích của task (`dest_dir\file_name`), chỉ khi nó còn đó.
pub(super) fn final_file(task: &DownloadTask) -> Result<Option<PathBuf>, String> {
    owned_file(Path::new(&task.dest_dir), OsStr::new(&task.file_name))
}

/// Tên file `.part` của task; phải nằm ngay trong `dest_dir` (engine luôn tạo như vậy).
pub(super) fn part_name(task: &DownloadTask) -> Result<&OsStr, String> {
    let temp = Path::new(&task.temp_path);
    let inside = temp
        .parent()
        .is_some_and(|parent| same_dir_text(parent, Path::new(&task.dest_dir)));
    let is_part = temp
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("part"));
    match temp.file_name() {
        Some(name) if inside && is_part => Ok(name),
        _ => Err(text::OUTSIDE.into()),
    }
}

/// File `.part` của task, chỉ khi nó còn đó.
pub(super) fn part_file(task: &DownloadTask) -> Result<Option<PathBuf>, String> {
    owned_file(Path::new(&task.dest_dir), part_name(task)?)
}

/// So hai thư mục theo chữ (Windows không phân biệt hoa/thường), bỏ `\` thừa.
fn same_dir_text(left: &Path, right: &Path) -> bool {
    let parts = |path: &Path| -> Vec<String> {
        path.components()
            .filter(|component| !matches!(component, Component::CurDir))
            .map(|component| {
                let text = component.as_os_str().to_string_lossy();
                if cfg!(any(windows, target_os = "macos")) {
                    text.to_lowercase()
                } else {
                    text.into_owned()
                }
            })
            .collect()
    };
    parts(left) == parts(right)
}

/// Cùng một thư mục trên đĩa (đi qua junction, tên 8.3, `\\?\`); không đọc được thì so chữ.
pub(super) fn same_dir(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => same_dir_text(left, right),
    }
}

/// Bản cách ly duy nhất của task trong `quarantine\` (`<tên>.<8 hex>.quarantined`).
/// Có hai bản trùng tên thì không đoán: trả None.
pub(super) fn quarantined_copy(quarantine: &Path, file_name: &str) -> Option<PathBuf> {
    let prefix = format!("{file_name}.");
    let mut matches = std::fs::read_dir(quarantine)
        .ok()?
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                return false;
            };
            let Some(rest) = name.strip_prefix(&prefix) else {
                return false;
            };
            let Some(suffix) = rest.strip_suffix(".quarantined") else {
                return false;
            };
            suffix.len() == 8
                && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
                && entry.file_type().is_ok_and(|kind| kind.is_file())
        });
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    owned_file(quarantine, &first.file_name()).ok().flatten()
}

/// Đường dẫn cho API shell của Windows: bỏ tiền tố `\\?\` mà `canonicalize` thêm vào.
#[cfg(windows)]
pub(super) fn shell_path(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let root = std::env::temp_dir().join(format!("utd-files-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn new_names_are_checked_like_guard() {
        assert_eq!(check_new_name("  Phim hay.mp4 ").unwrap(), "Phim hay.mp4");
        assert_eq!(check_new_name("setup.v1.2.exe").unwrap(), "setup.v1.2.exe");
        assert_eq!(check_new_name("báo cáo.pdf").unwrap(), "báo cáo.pdf");
        for bad in [
            "", "   ", "a/b.txt", "a\\b.txt", "x:y.txt", "NUL.txt", "..", "ten.",
        ] {
            assert!(check_new_name(bad).is_err(), "{bad:?}");
        }
        assert_eq!(
            check_new_name("hoa-don.pdf.exe").unwrap_err(),
            text::SPOOFED
        );
        assert_eq!(check_new_name("anh.JPG .scr").unwrap_err(), text::SPOOFED);
        assert_eq!(
            check_new_name("tai lieu      .exe").unwrap_err(),
            text::SPOOFED
        );
        assert_eq!(
            check_new_name("phim\u{202E}4pm.exe").unwrap_err(),
            text::HIDDEN_CHARS
        );
        assert_eq!(
            check_new_name("a\u{200B}b.zip").unwrap_err(),
            text::HIDDEN_CHARS
        );
        assert!(
            check_new_name("ghi chu.pdf.txt").is_ok(),
            "đuôi cuối vô hại"
        );
    }

    #[test]
    fn extension_helpers() {
        assert_eq!(extension_of("Game.EXE"), "exe");
        assert_eq!(extension_of("README"), "");
        assert!(runs_code("Ps1"));
        assert!(!runs_code("zip"));
    }

    #[test]
    fn owned_file_stays_inside_the_folder() {
        let root = scratch();
        let inner = root.join("Tải về");
        std::fs::create_dir_all(inner.join("sub")).unwrap();
        std::fs::write(inner.join("a.zip"), b"x").unwrap();
        std::fs::write(root.join("ngoai.zip"), b"x").unwrap();
        let real = owned_file(&inner, OsStr::new("a.zip")).unwrap().unwrap();
        assert!(real.ends_with("a.zip"));
        assert_eq!(owned_file(&inner, OsStr::new("mat.zip")).unwrap(), None);
        assert!(owned_file(&inner, OsStr::new("..")).is_err());
        assert!(owned_file(&inner, OsStr::new("../ngoai.zip")).is_err());
        assert!(
            owned_file(&inner, OsStr::new("sub")).is_err(),
            "thư mục không phải file"
        );
        assert_eq!(
            owned_file(&root.join("khong-co"), OsStr::new("a.zip")).unwrap(),
            None
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn part_must_sit_next_to_the_target() {
        let root = scratch();
        let dir = root.to_string_lossy().into_owned();
        let mut task = DownloadTask::new("https://example.com/a.zip", "a.zip", &dir, 4).unwrap();
        assert!(
            part_name(&task)
                .unwrap()
                .to_string_lossy()
                .ends_with(".part")
        );
        assert_eq!(part_file(&task).unwrap(), None);
        std::fs::write(&task.temp_path, b"part").unwrap();
        assert!(part_file(&task).unwrap().is_some());
        task.temp_path = root
            .join("sub")
            .join("a.zip.part")
            .to_string_lossy()
            .into_owned();
        assert!(part_name(&task).is_err(), "không chạm file ở thư mục con");
        task.temp_path = root.join("a.zip").to_string_lossy().into_owned();
        assert!(part_name(&task).is_err(), "chỉ nhận file .part");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn quarantine_copy_must_be_unique() {
        let root = scratch();
        assert_eq!(quarantined_copy(&root, "x.exe"), None);
        std::fs::write(root.join("x.exe.0123abcd.quarantined"), b"MZ").unwrap();
        std::fs::write(root.join("x.exe.zz.quarantined"), b"MZ").unwrap();
        std::fs::write(root.join("xx.exe.0123abcd.quarantined"), b"MZ").unwrap();
        assert!(quarantined_copy(&root, "x.exe").is_some());
        std::fs::write(root.join("x.exe.89abcdef.quarantined"), b"MZ").unwrap();
        assert_eq!(
            quarantined_copy(&root, "x.exe"),
            None,
            "hai bản thì không đoán"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn shell_paths_drop_the_verbatim_prefix() {
        assert_eq!(
            shell_path(Path::new(r"\\?\C:\a\b.zip")),
            PathBuf::from(r"C:\a\b.zip")
        );
        assert_eq!(
            shell_path(Path::new(r"\\?\UNC\nas\share\b.zip")),
            PathBuf::from(r"\\nas\share\b.zip")
        );
        assert_eq!(shell_path(Path::new(r"D:\x")), PathBuf::from(r"D:\x"));
    }

    #[cfg(windows)]
    #[test]
    fn same_dir_ignores_case_and_trailing_separator() {
        let root = scratch();
        let upper = PathBuf::from(format!("{}\\", root.to_string_lossy().to_uppercase()));
        assert!(same_dir(&root, &upper));
        assert!(!same_dir(&root, &root.join("khac")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn guard_blocks_turning_a_file_runnable() {
        let root = scratch();
        let file = root.join("ghi-chu.txt");
        std::fs::write(&file, b"echo hi").unwrap();
        assert_eq!(
            guard_allows(&file, "ghi-chu.txt", "ghi-chu.cmd").unwrap_err(),
            text::RUNNABLE
        );
        assert!(guard_allows(&file, "ghi-chu.txt", "ghi chu moi.txt").is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }
}
