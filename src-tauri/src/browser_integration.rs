#[cfg(target_os = "macos")]
pub mod mac;

use std::path::{Path, PathBuf};

use serde::Serialize;

pub const HOST_NAME: &str = "app.utd.desktop";
pub const CHROMIUM_EXTENSION_ID: &str = "aicoifnjolbpaollfoigidpecmpebeon";
pub const FIREFOX_EXTENSION_ID: &str = "utd@utd.app";

const EXTENSION_FILES: &[(&str, &[u8])] = &[
    (
        "chromium/manifest.json",
        include_bytes!("../../extensions/chromium/manifest.json"),
    ),
    (
        "chromium/background.js",
        include_bytes!("../../extensions/chromium/background.js"),
    ),
    (
        "chromium/popup.html",
        include_bytes!("../../extensions/chromium/popup.html"),
    ),
    (
        "chromium/popup.js",
        include_bytes!("../../extensions/chromium/popup.js"),
    ),
    (
        "chromium/icons/32.png",
        include_bytes!("../../extensions/chromium/icons/32.png"),
    ),
    (
        "chromium/icons/128.png",
        include_bytes!("../../extensions/chromium/icons/128.png"),
    ),
    (
        "firefox/manifest.json",
        include_bytes!("../../extensions/firefox/manifest.json"),
    ),
    (
        "firefox/background.js",
        include_bytes!("../../extensions/firefox/background.js"),
    ),
    (
        "firefox/popup.html",
        include_bytes!("../../extensions/firefox/popup.html"),
    ),
    (
        "firefox/popup.js",
        include_bytes!("../../extensions/firefox/popup.js"),
    ),
    (
        "firefox/icons/32.png",
        include_bytes!("../../extensions/firefox/icons/32.png"),
    ),
    (
        "firefox/icons/128.png",
        include_bytes!("../../extensions/firefox/icons/128.png"),
    ),
];

/// `_locales/<mã>/messages.json` của tiện ích (chrome.i18n) cho cả Chromium và Firefox. Thêm ngôn ngữ ở
/// src/shared/i18n/languages.ts thì thêm mã vào đây (scripts/i18n-check.mjs kiểm đủ file).
macro_rules! extension_locales {
    ($($code:literal),* $(,)?) => {
        &[$(
            (
                concat!("chromium/_locales/", $code, "/messages.json"),
                include_bytes!(concat!("../../extensions/chromium/_locales/", $code, "/messages.json")),
            ),
            (
                concat!("firefox/_locales/", $code, "/messages.json"),
                include_bytes!(concat!("../../extensions/firefox/_locales/", $code, "/messages.json")),
            ),
        )*]
    };
}

const EXTENSION_LOCALES: &[(&str, &[u8])] = extension_locales![
    "vi", "en", "zh_CN", "zh_TW", "ja", "ko", "th", "id", "hi", "es", "pt_BR", "fr", "de", "ru", "tr", "ar",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserIntegrationStatus {
    pub installed: bool,
    pub host_path: String,
    pub manifest_path: String,
    pub chromium_extension_id: String,
    pub firefox_extension_id: String,
    pub extension_dir: String,
}

#[tauri::command]
pub fn browser_integration_status() -> Result<BrowserIntegrationStatus, String> {
    build_status()
}

/// Native host trong thư mục dữ liệu: Windows là bản chép `UTD.exe`, macOS là script gọi app trong
/// `UTD.app` (chép binary ra ngoài bundle thì mất chữ ký của bundle, cập nhật xong vẫn chạy bản cũ).
#[cfg(windows)]
pub const HOST_FILE: &str = "utd-native-host.exe";
#[cfg(not(windows))]
pub const HOST_FILE: &str = "utd-native-host";

/// Thử chính tính năng cài tích hợp ở bản chạy thử, trên máy không có bản UTD thật: đặt thêm biến này = 1.
const TEST_INTEGRATION_ENV: &str = "UTD_TEST_BROWSER_INTEGRATION";

/// Đăng ký native host là của chung cả máy (HKCU, `~/Library/.../NativeMessagingHosts`): bản chạy thử
/// (`UTD_DATA_DIR`) ghi đè thì trình duyệt thật mất host khi dọn thư mục thử (Windows) hoặc gửi link sang
/// bản thử (macOS). Nên bản thử không cài, trừ khi bật `UTD_TEST_BROWSER_INTEGRATION=1`.
fn check_isolated_install(isolated: bool, allowed: bool) -> Result<(), String> {
    if isolated && !allowed {
        return Err(
            "[browser.test_build] Bản chạy thử (UTD_DATA_DIR) không cài tích hợp trình duyệt, để khỏi ghi đè bản UTD đã cài."
                .into(),
        );
    }
    Ok(())
}

#[tauri::command]
pub fn install_browser_integration() -> Result<BrowserIntegrationStatus, String> {
    check_isolated_install(
        crate::ipc::isolated_data_dir().is_some(),
        std::env::var_os(TEST_INTEGRATION_ENV).is_some_and(|value| value == "1"),
    )?;
    let directory = crate::ipc::data_dir()?;
    refresh_extensions()?;
    let host_directory = directory.join("native-messaging");
    std::fs::create_dir_all(&host_directory).map_err(|error| error.to_string())?;
    let host_path = directory.join(HOST_FILE);
    install_host(&host_path)?;

    let chromium_manifest = host_directory.join("chromium.json");
    let firefox_manifest = host_directory.join("firefox.json");
    write_manifest(
        &chromium_manifest,
        &serde_json::json!({
            "name": HOST_NAME,
            "description": "UTD download manager native host",
            "path": host_path.to_string_lossy(),
            "type": "stdio",
            "allowed_origins": [format!("chrome-extension://{CHROMIUM_EXTENSION_ID}/")],
        }),
    )?;
    write_manifest(
        &firefox_manifest,
        &serde_json::json!({
            "name": HOST_NAME,
            "description": "UTD download manager native host",
            "path": host_path.to_string_lossy(),
            "type": "stdio",
            "allowed_extensions": [FIREFOX_EXTENSION_ID],
        }),
    )?;

    #[cfg(windows)]
    register_windows(&chromium_manifest, &firefox_manifest)?;
    #[cfg(target_os = "macos")]
    mac::register(&chromium_manifest, &firefox_manifest)?;

    build_status()
}

#[cfg(windows)]
fn install_host(host_path: &Path) -> Result<(), String> {
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    if current != host_path
        && let Err(first) = std::fs::copy(&current, host_path)
    {
        let _ = std::fs::remove_file(host_path);
        std::fs::copy(&current, host_path).map_err(|second| {
            format!(
                "[browser.host_locked] Không cài được native host ({first}; {second}). Đóng UTD đang chạy rồi thử lại."
            )
        })?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_host(host_path: &Path) -> Result<(), String> {
    let app = mac::installed_exe()?;
    mac::write_host_script(host_path, &app, crate::ipc::isolated_data_dir().as_deref())
        .map_err(|error| format!("[browser.host_failed] Không cài được native host ({error})."))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn install_host(_: &Path) -> Result<(), String> {
    Err("[browser.unsupported_os] Tích hợp trình duyệt mới có trên Windows và macOS.".into())
}

#[tauri::command]
pub fn open_extension_folder() -> Result<String, String> {
    let directory = extension_root()?;
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&directory)
            .spawn()
            .map_err(|error| error.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&directory)
            .spawn()
            .map_err(|error| error.to_string())?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&directory)
            .spawn()
            .map_err(|error| error.to_string())?;
    }
    Ok(directory.display().to_string())
}

fn build_status() -> Result<BrowserIntegrationStatus, String> {
    let directory = crate::ipc::data_dir()?;
    let host_path = directory.join(HOST_FILE);
    let manifest_path = directory.join("native-messaging").join("chromium.json");
    let extension_root = extension_root()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    Ok(BrowserIntegrationStatus {
        installed: host_path.exists() && manifest_path.exists() && !extension_root.is_empty(),
        host_path: host_path.display().to_string(),
        manifest_path: manifest_path.display().to_string(),
        chromium_extension_id: CHROMIUM_EXTENSION_ID.into(),
        firefox_extension_id: FIREFOX_EXTENSION_ID.into(),
        extension_dir: extension_root,
    })
}

fn extension_root() -> Result<PathBuf, String> {
    let directory = crate::ipc::data_dir()?.join("extension");
    let manifest = directory.join("chromium").join("manifest.json");
    if !manifest.exists() {
        write_extensions(&directory)?;
    }
    Ok(directory)
}

/// Mở app: tích hợp đã cài (thư mục extension có manifest) thì ghi lại file nào khác bản đi kèm app, để
/// code extension mới (menu, quyền mới) tới được người đã cài sau khi cập nhật UTD; trình duyệt nạp khi
/// khởi động lại hoặc bấm Reload. Không xoá thư mục (trình duyệt đang nạp từ đó). Trả số file đã ghi.
pub fn sync_extensions(directory: &Path) -> Result<usize, String> {
    if !directory.join("chromium").join("manifest.json").is_file() {
        return Ok(0);
    }
    let mut written = 0;
    for (relative, bytes) in EXTENSION_FILES.iter().chain(EXTENSION_LOCALES) {
        let path = directory.join(relative);
        if std::fs::read(&path).is_ok_and(|current| current == *bytes) {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
        written += 1;
    }
    Ok(written)
}

fn refresh_extensions() -> Result<PathBuf, String> {
    let directory = crate::ipc::data_dir()?.join("extension");
    if directory.exists() {
        std::fs::remove_dir_all(&directory).map_err(|error| error.to_string())?;
    }
    write_extensions(&directory)?;
    Ok(directory)
}

fn write_extensions(directory: &Path) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    for (relative, bytes) in EXTENSION_FILES.iter().chain(EXTENSION_LOCALES) {
        let path = directory.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn write_manifest(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let payload = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    std::fs::write(path, payload).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn register_windows(chromium_manifest: &Path, firefox_manifest: &Path) -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    for key in [
        r"Software\Google\Chrome\NativeMessagingHosts\app.utd.desktop",
        r"Software\Microsoft\Edge\NativeMessagingHosts\app.utd.desktop",
        r"Software\Chromium\NativeMessagingHosts\app.utd.desktop",
        r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\app.utd.desktop",
    ] {
        write_registry(&current_user, key, chromium_manifest)?;
    }
    write_registry(
        &current_user,
        r"Software\Mozilla\NativeMessagingHosts\app.utd.desktop",
        firefox_manifest,
    )
}

#[cfg(windows)]
fn write_registry(root: &winreg::RegKey, key: &str, manifest: &Path) -> Result<(), String> {
    let (subkey, _) = root
        .create_subkey(key)
        .map_err(|error| format!("[browser.register_failed] Không ghi được đăng ký tích hợp: {error}"))?;
    subkey
        .set_value("", &manifest.to_string_lossy().to_string())
        .map_err(|error| format!("[browser.register_failed] Không ghi được đăng ký tích hợp: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        std::env::temp_dir().join(format!("utd-extension-{}", uuid::Uuid::new_v4()))
    }

    /// Cập nhật UTD (extension có menu "Tải tất cả link", quyền `scripting`): mở app thì file trong thư
    /// mục extension đã cài được ghi lại; chưa cài tích hợp thì không tạo gì.
    #[test]
    fn startup_refreshes_an_installed_extension_only() {
        let root = scratch();
        let directory = root.join("extension");
        assert_eq!(sync_extensions(&directory), Ok(0));
        assert!(!directory.exists(), "chưa cài tích hợp: không tạo thư mục");

        write_extensions(&directory).unwrap();
        assert_eq!(sync_extensions(&directory), Ok(0), "đã khớp: không ghi lại");
        let background = directory.join("chromium").join("background.js");
        std::fs::write(&background, b"// ban cu, chua co MENU_ALL").unwrap();
        std::fs::remove_file(directory.join("firefox").join("popup.js")).unwrap();
        assert_eq!(sync_extensions(&directory), Ok(2));
        let fresh = std::fs::read(&background).unwrap();
        assert_eq!(fresh, include_bytes!("../../extensions/chromium/background.js"));
        assert!(directory.join("firefox").join("popup.js").is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_builds_do_not_register_with_the_real_browser() {
        assert!(check_isolated_install(false, false).is_ok());
        let refused = check_isolated_install(true, false).unwrap_err();
        assert!(refused.contains("Bản chạy thử"), "{refused}");
        assert!(check_isolated_install(true, true).is_ok(), "thử tính năng trên máy không có bản thật");
    }

    fn message_keys(bytes: &[u8]) -> Vec<String> {
        let value: serde_json::Value = serde_json::from_slice(bytes).expect("messages.json là JSON");
        let mut keys: Vec<String> = value.as_object().expect("đối tượng JSON").keys().cloned().collect();
        keys.sort();
        keys
    }

    /// Thiếu khoá thì chrome.i18n trả chuỗi rỗng: menu chuột phải hay nút trong popup mất chữ.
    #[test]
    fn every_extension_locale_has_the_english_messages() {
        let english = EXTENSION_LOCALES
            .iter()
            .find(|(path, _)| *path == "chromium/_locales/en/messages.json")
            .expect("có tiếng Anh (default_locale)");
        for (path, bytes) in EXTENSION_LOCALES {
            assert_eq!(message_keys(bytes), message_keys(english.1), "{path}");
        }
    }
}
