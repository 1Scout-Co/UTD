use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardAction {
    Allow,
    Quarantine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardReport {
    pub action: GuardAction,
    pub code: String,
    pub reason: String,
    pub detected: String,
}

pub fn inspect(path: &Path, claimed_name: &str) -> io::Result<GuardReport> {
    let mut prefix = [0u8; 512];
    let mut file = File::open(path)?;
    let read = file.read(&mut prefix)?;
    let prefix = &prefix[..read];
    let extension = Path::new(claimed_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if dangerous_extension(&extension) {
        return Ok(GuardReport {
            action: GuardAction::Quarantine,
            code: "guard.dangerous_type".into(),
            reason: "This is an executable or script file type and was quarantined.".into(),
            detected: extension,
        });
    }

    let detected = detect_magic(prefix);
    if is_executable_magic(detected) && !executable_extension(&extension) {
        return Ok(GuardReport {
            action: GuardAction::Quarantine,
            code: "guard.extension_mismatch".into(),
            reason: "The file content is executable but the file name suggests a safer type."
                .into(),
            detected: detected.unwrap_or("unknown").into(),
        });
    }

    Ok(GuardReport {
        action: GuardAction::Allow,
        code: String::new(),
        reason: String::new(),
        detected: detected.unwrap_or("unknown").into(),
    })
}

pub fn quarantine(
    path: &Path,
    claimed_name: &str,
    quarantine_dir: Option<&Path>,
) -> io::Result<PathBuf> {
    let directory = quarantine_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            path.parent()
                .unwrap_or_else(|| Path::new("."))
                .join(".utd-quarantine")
        });
    fs::create_dir_all(&directory)?;
    let suffix = &Uuid::new_v4().simple().to_string()[..8];
    let target = directory.join(format!("{claimed_name}.{suffix}.quarantined"));
    if fs::rename(path, &target).is_err() {
        fs::copy(path, &target)?;
        fs::remove_file(path)?;
    }
    if let Ok(metadata) = fs::metadata(&target) {
        let mut permissions = metadata.permissions();
        permissions.set_readonly(true);
        let _ = fs::set_permissions(&target, permissions);
    }
    Ok(target)
}

#[cfg(windows)]
pub fn mark_of_the_web(path: &Path) -> io::Result<()> {
    let mut stream = path.as_os_str().to_os_string();
    stream.push(":Zone.Identifier");
    fs::write(PathBuf::from(stream), "[ZoneTransfer]\r\nZoneId=3\r\n")
}

/// macOS: thuộc tính `com.apple.quarantine` như Safari/Chrome ghi. Gatekeeper kiểm chữ ký và hỏi
/// trước khi mở lần đầu thứ chạy được lấy ra từ file này (app trong .zip/.dmg cũng mang cờ theo).
#[cfg(target_os = "macos")]
pub fn mark_of_the_web(path: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    // Cờ 0x0081 = tải từ mạng; trường cuối là UUID sự kiện của LaunchServices, để trống được.
    let value = format!("0081;{seconds:08x};UTD;");
    let file = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: tên file và tên thuộc tính là chuỗi C kết thúc bằng NUL; `value` sống tới hết lệnh gọi.
    let done = unsafe {
        libc::setxattr(
            file.as_ptr(),
            c"com.apple.quarantine".as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    if done != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn mark_of_the_web(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Đuôi chạy được mã (hoặc mở thẳng thứ chạy mã) — luôn cách ly. `extension` viết thường, không dấu chấm.
/// Cố ý không có .iso/.img/.vhd: hay là file hợp lệ (bản cài Linux…) và Windows đã truyền MOTW vào trong.
pub fn dangerous_extension(extension: &str) -> bool {
    matches!(
        extension,
        "exe"
            | "chm"
            | "appref-ms"
            | "application"
            | "appx"
            | "appxbundle"
            | "msix"
            | "msixbundle"
            | "msc"
            | "url"
            | "scf"
            | "settingcontent-ms"
            | "xll"
            | "msi"
            | "msp"
            | "dll"
            | "sys"
            | "com"
            | "scr"
            | "pif"
            | "bat"
            | "cmd"
            | "vbs"
            | "vbe"
            | "js"
            | "jse"
            | "wsf"
            | "wsh"
            | "ps1"
            | "ps2"
            | "hta"
            | "lnk"
            | "reg"
            | "cpl"
            | "jar"
            | "apk"
            // Python: có py launcher thì bấm Mở là chạy, MOTW không hỏi gì.
            | "py"
            | "pyw"
            | "pyz"
            | "pyzw"
            | "pyc"
            // Còn lại trong danh sách rủi ro cao của Windows (Attachment Manager).
            | "ws"
            | "wsc"
            | "sct"
            | "vb"
            | "inf"
            | "psm1"
            | "library-ms"
            | "search-ms"
            | "diagcab"
    ) || (cfg!(target_os = "macos") && mac_dangerous_extension(extension))
}

/// Thêm cho macOS: app và bộ cài (.dmg/.pkg ngang .exe/.msi), script Terminal chạy khi nhấp đúp,
/// Automator/AppleScript, lối tắt mở thẳng file hay app khác, gói nạp vào hệ thống.
fn mac_dangerous_extension(extension: &str) -> bool {
    matches!(
        extension,
        "app"
            | "dmg"
            | "pkg"
            | "mpkg"
            | "command"
            | "tool"
            | "terminal"
            | "workflow"
            | "action"
            | "scpt"
            | "scptd"
            | "applescript"
            | "webloc"
            | "inetloc"
            | "fileloc"
            | "prefpane"
            | "kext"
            | "plugin"
            | "bundle"
            | "dylib"
            | "osax"
            | "qlgenerator"
            | "saver"
            | "appex"
            | "xpc"
            | "mobileconfig"
    )
}

fn executable_extension(extension: &str) -> bool {
    matches!(extension, "exe" | "msi" | "com" | "scr" | "dll" | "sys" | "msp")
}

fn detect_magic(prefix: &[u8]) -> Option<&'static str> {
    if prefix.starts_with(b"MZ") {
        return Some("windows-executable");
    }
    if prefix.starts_with(b"\x7fELF") {
        return Some("elf-executable");
    }
    if prefix.starts_with(&[0xCF, 0xFA, 0xED, 0xFE])
        || prefix.starts_with(&[0xCE, 0xFA, 0xED, 0xFE])
        || prefix.starts_with(&[0xFE, 0xED, 0xFA, 0xCE])
        || prefix.starts_with(&[0xFE, 0xED, 0xFA, 0xCF])
    {
        return Some("mach-o-executable");
    }
    // Mach-O "universal" (nhiều kiến trúc) dùng chung CA FE BA BE với file .class của Java: số kiến
    // trúc (u32 big-endian ngay sau) luôn nhỏ, còn .class ghi phiên bản Java ≥ 45 ở chỗ đó.
    if let Some(&[0xCA, 0xFE, 0xBA, 0xBE, a, b, c, d]) = prefix.get(..8)
        && (1..20).contains(&u32::from_be_bytes([a, b, c, d]))
    {
        return Some("mach-o-executable");
    }
    if prefix.starts_with(b"#!") {
        return Some("script");
    }
    if prefix.starts_with(b"%PDF") {
        return Some("pdf");
    }
    if prefix.starts_with(b"\x89PNG") {
        return Some("png");
    }
    if prefix.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpeg");
    }
    if prefix.get(4..8) == Some(b"ftyp") {
        return Some("video");
    }
    if prefix.starts_with(b"PK\x03\x04") {
        return Some("zip");
    }
    None
}

fn is_executable_magic(magic: Option<&str>) -> bool {
    matches!(
        magic,
        Some("windows-executable" | "elf-executable" | "mach-o-executable")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_file(name: &str, bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join(name);
        std::fs::write(&path, bytes).expect("write");
        (directory, path)
    }

    #[test]
    fn allows_pdf_and_video() {
        let (_directory, pdf) = temporary_file("report.pdf", b"%PDF-1.7\n...");
        assert_eq!(
            inspect(&pdf, "report.pdf").expect("inspect").action,
            GuardAction::Allow
        );

        let (_directory, video) = temporary_file("clip.mp4", b"\0\0\0\x18ftypisom....");
        assert_eq!(
            inspect(&video, "clip.mp4").expect("inspect").action,
            GuardAction::Allow
        );
    }

    #[test]
    fn quarantines_dangerous_types() {
        let (_directory, exe) = temporary_file("tool.exe", b"MZ....");
        let report = inspect(&exe, "tool.exe").expect("inspect");
        assert_eq!(report.action, GuardAction::Quarantine);
        assert_eq!(report.code, "guard.dangerous_type");

        for name in [
            "cong-cu.py",
            "an.PYW",
            "goi.pyz",
            "goi.pyzw",
            "ma.pyc",
            "x.wsc",
            "x.sct",
            "driver.inf",
            "mod.psm1",
            "thu-vien.library-ms",
            "tim.search-ms",
            "sua.diagcab",
        ] {
            let (_directory, file) = temporary_file(name, b"print('hi')\n");
            let report = inspect(&file, name).expect("inspect");
            assert_eq!(report.action, GuardAction::Quarantine, "{name}");
        }
    }

    #[test]
    fn detects_disguised_executables() {
        let (_directory, disguised) = temporary_file("invoice.pdf", b"MZ\x90\x00\x03....");
        let report = inspect(&disguised, "invoice.pdf").expect("inspect");
        assert_eq!(report.action, GuardAction::Quarantine);
        assert_eq!(report.code, "guard.extension_mismatch");
        assert_eq!(report.detected, "windows-executable");
    }

    #[test]
    fn moves_quarantined_files() {
        let directory = tempfile::tempdir().expect("tempdir");
        let source = directory.path().join("danger.exe");
        std::fs::write(&source, b"MZ....").expect("write");
        let target = quarantine(&source, "danger.exe", Some(&directory.path().join("q")))
            .expect("quarantine");
        assert!(!source.exists());
        assert!(target.exists());
        assert!(target.to_string_lossy().ends_with(".quarantined"));
    }

    #[test]
    fn detects_universal_mach_o_but_not_java_classes() {
        let (_directory, fat) = temporary_file("ffmpeg", b"\xCA\xFE\xBA\xBE\x00\x00\x00\x02....");
        let report = inspect(&fat, "ffmpeg").expect("inspect");
        assert_eq!(report.action, GuardAction::Quarantine);
        assert_eq!(report.detected, "mach-o-executable");
        let (_directory, class) = temporary_file("A.class", b"\xCA\xFE\xBA\xBE\x00\x00\x00\x41....");
        assert_eq!(inspect(&class, "A.class").expect("inspect").action, GuardAction::Allow);
    }

    #[test]
    fn mac_app_and_installer_types_follow_the_platform() {
        for name in [
            "UTD.dmg",
            "cai-dat.pkg",
            "chay.command",
            "x.tool",
            "tu-dong.workflow",
            "kich-ban.scpt",
            "mo.fileloc",
            "hoso.mobileconfig",
        ] {
            let (_directory, file) = temporary_file(name, b"data");
            let action = inspect(&file, name).expect("inspect").action;
            if cfg!(target_os = "macos") {
                assert_eq!(action, GuardAction::Quarantine, "{name}");
            } else {
                assert_eq!(action, GuardAction::Allow, "{name}: vô hại ngoài macOS");
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn writes_quarantine_attribute() {
        use std::os::unix::ffi::OsStrExt;
        let (_directory, file) = temporary_file("safe.pdf", b"%PDF-1.7");
        mark_of_the_web(&file).expect("quarantine");
        let path = std::ffi::CString::new(file.as_os_str().as_bytes()).expect("path");
        let mut value = [0u8; 128];
        // SAFETY: chuỗi C kết thúc bằng NUL, bộ đệm đủ `value.len()` byte.
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
        assert!(read > 0);
        let text = String::from_utf8_lossy(&value[..read as usize]).into_owned();
        assert!(text.starts_with("0081;") && text.ends_with(";UTD;"), "{text}");
    }

    #[cfg(windows)]
    #[test]
    fn writes_zone_identifier() {
        let (_directory, file) = temporary_file("safe.pdf", b"%PDF-1.7");
        mark_of_the_web(&file).expect("motw");
        let mut stream = file.as_os_str().to_os_string();
        stream.push(":Zone.Identifier");
        let contents = std::fs::read(PathBuf::from(stream)).expect("ads");
        assert!(String::from_utf8_lossy(&contents).contains("ZoneId=3"));
    }
}
