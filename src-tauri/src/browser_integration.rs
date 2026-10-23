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

#[tauri::command]
pub fn install_browser_integration() -> Result<BrowserIntegrationStatus, String> {
    let directory = crate::ipc::data_dir()?;
    refresh_extensions()?;
    let host_directory = directory.join("native-messaging");
    std::fs::create_dir_all(&host_directory).map_err(|error| error.to_string())?;
    let host_path = directory.join("utd-native-host.exe");
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    if current != host_path
        && let Err(first) = std::fs::copy(&current, &host_path)
    {
        let _ = std::fs::remove_file(&host_path);
        std::fs::copy(&current, &host_path).map_err(|second| {
            format!(
                "Không cài được native host ({first}; {second}). Đóng UTD đang chạy rồi thử lại."
            )
        })?;
    }

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

    build_status()
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
    let host_path = directory.join("utd-native-host.exe");
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
    for (relative, bytes) in EXTENSION_FILES {
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
        .map_err(|error| format!("Không ghi được đăng ký tích hợp: {error}"))?;
    subkey
        .set_value("", &manifest.to_string_lossy().to_string())
        .map_err(|error| format!("Không ghi được đăng ký tích hợp: {error}"))
}
