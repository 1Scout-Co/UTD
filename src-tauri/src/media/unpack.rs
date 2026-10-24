//! Giải nén gói công cụ bằng tar có sẵn của hệ điều hành (bsdtar: từ chối đường dẫn `..`) và tìm
//! file chạy được trong đó, chỉ nhận file nằm hẳn trong thư mục giải nén.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::media::tokio_command;

const EXTRACT_TIMEOUT: Duration = Duration::from_secs(600);

fn system_tar() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let tar = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join("tar.exe");
    #[cfg(not(windows))]
    let tar = PathBuf::from("/usr/bin/tar");
    if tar.is_file() {
        Ok(tar)
    } else if cfg!(windows) {
        Err("[tool.no_tar_windows] Windows thiếu tar.exe (cần Windows 10 1803 trở lên) nên chưa giải nén được.".into())
    } else {
        Err("[tool.no_tar_mac] Máy thiếu /usr/bin/tar nên chưa giải nén được.".into())
    }
}

/// Giải nén `<work>/<archive>` vào `<work>/x`. bsdtar trên Windows đổi tham số sang bảng mã ANSI
/// nên chỉ truyền tên ASCII tương đối; thư mục có thể chứa chữ Unicode (tên người dùng) được đặt
/// qua `current_dir`.
pub(crate) async fn extract(
    work: &Path,
    archive: &str,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let tar = system_tar()?;
    let mut command = tokio_command(&tar);
    command
        .current_dir(work)
        .args(["-xf", archive, "-C", "x"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = command
        .spawn()
        .map_err(|error| format!("[tool.tar_failed] Không chạy được tar: {error}"))?;
    let output = tokio::select! {
        _ = cancel.cancelled() => return Err("cancelled".into()),
        output = tokio::time::timeout(EXTRACT_TIMEOUT, child.wait_with_output()) => output,
    };
    let output = output
        .map_err(|_| "[tool.unpack_timeout] Giải nén quá lâu nên UTD đã dừng.".to_string())?
        .map_err(|error| format!("[tool.tar_failed] Không chạy được tar: {error}"))?;
    if !output.status.success() {
        let detail: String = String::from_utf8_lossy(&output.stderr)
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(160)
            .collect();
        log::warn!("tar failed: {detail}");
        return Err("[tool.unpack_failed] Giải nén gói công cụ lỗi nên UTD đã dừng.".into());
    }
    Ok(())
}

fn is_regular(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

/// Tìm `name` ở gốc, trong `bin`, hoặc một tầng thư mục gói (`<gói>/bin/ffmpeg.exe`), có giới hạn.
pub(crate) fn locate(out: &Path, name: &str) -> Option<PathBuf> {
    let mut roots = vec![out.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(out) {
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .take(16)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .collect();
        dirs.sort();
        roots.extend(dirs);
    }
    roots
        .iter()
        .flat_map(|root| [root.join(name), root.join("bin").join(name)])
        .find(|path| is_regular(path) && inside(out, path))
}

/// File giấy phép ở gốc gói (cạnh `bin`), nếu có.
pub(crate) fn locate_license(out: &Path, binary: &Path, name: &str) -> Option<PathBuf> {
    let parent = binary.parent()?;
    let root = if parent.file_name().is_some_and(|dir| dir == "bin") {
        parent.parent()?
    } else {
        parent
    };
    Some(root.join(name)).filter(|path| is_regular(path) && inside(out, path))
}

/// `path` (sau khi phân giải symlink/junction) nằm hẳn trong `root`.
pub(crate) fn inside(root: &Path, path: &Path) -> bool {
    match (std::fs::canonicalize(root), std::fs::canonicalize(path)) {
        (Ok(root), Ok(path)) => path != root && path.starts_with(&root),
        _ => false,
    }
}

/// macOS: file tải bằng HTTP không có quyền chạy.
pub(crate) fn make_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let path = std::env::temp_dir().join(format!("utd-unpack-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("dir");
        path
    }

    #[test]
    fn archive_paths_must_stay_inside_the_extract_dir() {
        let root = scratch();
        let out = root.join("x");
        std::fs::create_dir_all(out.join("pkg").join("bin")).expect("dirs");
        let good = out.join("pkg").join("bin").join("ffmpeg.exe");
        std::fs::write(&good, b"exe").expect("file");
        let outside = root.join("evil.exe");
        std::fs::write(&outside, b"exe").expect("file");

        assert!(inside(&out, &good));
        assert!(!inside(&out, &out.join("..").join("evil.exe")));
        assert!(!inside(&out, &outside));
        assert!(!inside(&out, &out));
        assert!(!inside(&out, &out.join("missing.exe")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn locates_binaries_at_root_or_in_package_bin() {
        let root = scratch();
        let bin = root.join("ffmpeg-master-latest-win64-gpl").join("bin");
        std::fs::create_dir_all(&bin).expect("dirs");
        assert_eq!(locate(&root, "ffmpeg.exe"), None);
        std::fs::write(bin.join("ffmpeg.exe"), b"a").expect("file");
        std::fs::write(bin.parent().expect("pkg").join("LICENSE.txt"), b"GPL").expect("file");
        let ffmpeg = locate(&root, "ffmpeg.exe").expect("found");
        assert_eq!(ffmpeg, bin.join("ffmpeg.exe"));
        assert_eq!(locate(&root, "ffprobe.exe"), None);
        assert_eq!(
            locate_license(&root, &ffmpeg, "LICENSE.txt"),
            Some(bin.parent().expect("pkg").join("LICENSE.txt"))
        );
        // deno: file nằm ngay gốc gói.
        std::fs::write(root.join("deno.exe"), b"d").expect("file");
        assert_eq!(locate(&root, "deno.exe"), Some(root.join("deno.exe")));
        assert_eq!(
            locate_license(&root, &root.join("deno.exe"), "LICENSE.txt"),
            None
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Đóng gói `files` (tên → nội dung) thành `<work>/<zip>` bằng chính tar của hệ điều hành.
    pub(crate) fn make_zip(work: &Path, zip: &str, files: &[(&str, &[u8])]) {
        let source = work.join(format!("{zip}-src"));
        std::fs::create_dir_all(&source).expect("src");
        for (name, bytes) in files {
            let path = source.join(name);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
            std::fs::write(path, bytes).expect("file");
        }
        let names: Vec<&str> = files
            .iter()
            .map(|(name, _)| name.split('/').next().expect("name"))
            .collect();
        let status = crate::media::std_command(&system_tar().expect("tar"))
            .current_dir(&source)
            .args(["--format", "zip", "-cf"])
            .arg(format!("../{zip}"))
            .args(names)
            .status()
            .expect("tar");
        assert!(status.success());
        std::fs::remove_dir_all(&source).expect("clean");
    }

    #[tokio::test]
    async fn extracts_a_real_zip_with_system_tar() {
        let work = scratch();
        std::fs::create_dir_all(work.join("x")).expect("x");
        make_zip(&work, "a0.zip", &[("deno.exe", b"hi")]);
        extract(&work, "a0.zip", &CancellationToken::new())
            .await
            .expect("extract");
        let found = locate(&work.join("x"), "deno.exe").expect("deno");
        assert_eq!(std::fs::read(found).expect("read"), b"hi");
        std::fs::write(work.join("bad.zip"), b"not a zip").expect("bad");
        assert!(
            extract(&work, "bad.zip", &CancellationToken::new())
                .await
                .is_err()
        );
        let _ = std::fs::remove_dir_all(&work);
    }
}
