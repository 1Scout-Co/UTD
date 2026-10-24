//! macOS: native host là script nhỏ gọi `UTD.app/Contents/MacOS/UTD` ở chế độ host; manifest chép vào
//! `~/Library/Application Support/<trình duyệt>/NativeMessagingHosts/` của từng trình duyệt đã cài.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::{HOST_FILE, HOST_NAME};

/// Thư mục của trình duyệt trong `~/Library/Application Support` (chỉ ghi khi thư mục đó có sẵn).
const CHROMIUM_BROWSERS: [&str; 7] = [
    "Google/Chrome",
    "Google/Chrome Beta",
    "Google/Chrome Canary",
    "Chromium",
    "Microsoft Edge",
    "BraveSoftware/Brave-Browser",
    "Vivaldi",
];
const FIREFOX_BROWSERS: [&str; 1] = ["Mozilla"];

const MOVE_TO_APPLICATIONS: &str = "[browser.move_to_apps] Kéo UTD vào thư mục Applications, mở lại rồi bật tích hợp.";

/// Đường dẫn app đang chạy, chỉ khi nó nằm yên một chỗ. Mở thẳng từ file .dmg, hay từ Downloads khi
/// còn cờ quarantine (macOS chạy bản sao tạm chỉ-đọc ở `AppTranslocation`) thì chỗ đó sẽ biến mất.
pub(super) fn installed_exe() -> Result<PathBuf, String> {
    let current = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|error| error.to_string())?;
    if current.to_string_lossy().contains("/AppTranslocation/") || read_only(&current) {
        return Err(MOVE_TO_APPLICATIONS.into());
    }
    Ok(current)
}

fn read_only(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(text) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: chuỗi C kết thúc bằng NUL; `stats` chỉ đọc khi lệnh gọi trả 0.
    let done = unsafe { libc::statvfs(text.as_ptr(), stats.as_mut_ptr()) };
    done == 0 && unsafe { stats.assume_init() }.f_flag & libc::ST_RDONLY != 0
}

/// Script trình duyệt chạy: đặt `UTD_NATIVE_HOST=1` rồi `exec` app (Chrome/Firefox thêm đối số riêng).
/// Bản chạy thử (`UTD_DATA_DIR`) ghi luôn thư mục dữ liệu để host nói chuyện đúng bản đó.
pub(super) fn host_script(app: &Path, data_dir: Option<&Path>) -> String {
    let mut env = String::from("UTD_NATIVE_HOST=1");
    if let Some(directory) = data_dir {
        env.push_str(&format!(" UTD_DATA_DIR={}", quote(directory)));
    }
    format!(
        "#!/bin/sh\n# UTD: trình duyệt chạy file này để gửi link sang UTD. Bật lại tích hợp thì tạo lại.\n{env} exec {} \"$@\"\n",
        quote(app)
    )
}

/// Bọc trong nháy đơn cho `sh`; nháy đơn bên trong thành `'\''`.
fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

/// Ghi ra tệp tạm rồi đổi tên: trình duyệt không bao giờ chạy phải script viết dở.
pub(super) fn write_host_script(
    host: &Path,
    app: &Path,
    data_dir: Option<&Path>,
) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let fresh = host.with_extension("new");
    let _ = std::fs::remove_file(&fresh);
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o755)
        .open(&fresh)
        .and_then(|mut file| file.write_all(host_script(app, data_dir).as_bytes()))
        .and_then(|()| std::fs::rename(&fresh, host));
    if written.is_err() {
        let _ = std::fs::remove_file(&fresh);
    }
    written
}

/// Đã bật tích hợp mà app đổi chỗ (kéo sang thư mục khác): viết lại script trỏ vào chỗ mới.
pub fn refresh_host_script(directory: &Path) -> io::Result<()> {
    let host = directory.join(HOST_FILE);
    let manifest = directory.join("native-messaging").join("chromium.json");
    if !manifest.is_file() || !host.is_file() {
        return Ok(());
    }
    let Ok(app) = installed_exe() else {
        return Ok(());
    };
    if std::fs::read_to_string(&host).ok() == Some(host_script(&app, None)) {
        return Ok(());
    }
    write_host_script(&host, &app, None)
}

pub(super) fn register(chromium_manifest: &Path, firefox_manifest: &Path) -> Result<(), String> {
    let home = std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .ok_or_else(|| "[browser.no_home] Không tìm thấy thư mục người dùng.".to_string())?;
    let support = PathBuf::from(home)
        .join("Library")
        .join("Application Support");
    register_in(&support, chromium_manifest, firefox_manifest).map(drop)
}

/// Chép manifest cho mọi trình duyệt có thư mục dữ liệu trong `support`; trả số trình duyệt đã ghi.
fn register_in(support: &Path, chromium: &Path, firefox: &Path) -> Result<usize, String> {
    let mut written = 0;
    let groups = [
        (&CHROMIUM_BROWSERS[..], chromium),
        (&FIREFOX_BROWSERS[..], firefox),
    ];
    for (browsers, manifest) in groups {
        for browser in browsers {
            let root = support.join(browser);
            if !root.is_dir() {
                continue;
            }
            let hosts = root.join("NativeMessagingHosts");
            std::fs::create_dir_all(&hosts)
                .and_then(|()| std::fs::copy(manifest, hosts.join(format!("{HOST_NAME}.json"))))
                .map_err(|error| format!("[browser.register_failed] Không ghi được đăng ký tích hợp: {error}"))?;
            written += 1;
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_quotes_paths_and_passes_browser_arguments() {
        let app = Path::new("/Applications/UTD 'beta'.app/Contents/MacOS/UTD");
        let script = host_script(app, None);
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.ends_with(
            "UTD_NATIVE_HOST=1 exec '/Applications/UTD '\\''beta'\\''.app/Contents/MacOS/UTD' \"$@\"\n"
        ));
        let isolated = host_script(app, Some(Path::new("/tmp/utd thu")));
        assert!(isolated.contains("UTD_NATIVE_HOST=1 UTD_DATA_DIR='/tmp/utd thu' exec "));
    }

    #[test]
    fn script_is_executable_and_runs() {
        let root = std::env::temp_dir().join(format!("utd-host-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let host = root.join(HOST_FILE);
        // `/usr/bin/env` in biến môi trường: thấy cờ host là script đã chạy đúng.
        write_host_script(&host, Path::new("/usr/bin/env"), None).unwrap();
        let output = std::process::Command::new(&host).output().unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).contains("UTD_NATIVE_HOST=1"));
        assert!(!root.join("utd-native-host.new").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn manifests_go_only_to_installed_browsers() {
        let root = std::env::temp_dir().join(format!("utd-nm-{}", uuid::Uuid::new_v4()));
        let support = root.join("Application Support");
        std::fs::create_dir_all(support.join("Google").join("Chrome")).unwrap();
        std::fs::create_dir_all(support.join("Mozilla")).unwrap();
        let (chromium, firefox) = (root.join("chromium.json"), root.join("firefox.json"));
        std::fs::write(&chromium, b"{\"c\":1}").unwrap();
        std::fs::write(&firefox, b"{\"f\":1}").unwrap();

        assert_eq!(register_in(&support, &chromium, &firefox).unwrap(), 2);
        let chrome = support.join("Google/Chrome/NativeMessagingHosts/app.utd.desktop.json");
        assert_eq!(std::fs::read(chrome).unwrap(), b"{\"c\":1}");
        let mozilla = support.join("Mozilla/NativeMessagingHosts/app.utd.desktop.json");
        assert_eq!(std::fs::read(mozilla).unwrap(), b"{\"f\":1}");
        assert!(
            !support.join("Microsoft Edge").exists(),
            "không tạo thư mục trình duyệt chưa cài"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
