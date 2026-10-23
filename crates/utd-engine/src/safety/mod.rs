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

#[cfg(not(windows))]
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
