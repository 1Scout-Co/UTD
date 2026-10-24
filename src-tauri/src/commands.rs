use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use utd_engine::{
    DownloadControl, DownloadEngine, DownloadRequest, DownloadTask, Journal, TaskKind,
};

use crate::events::{emit_engine_event, BridgeError, ERROR_EVENT};
use crate::state::{AppState, RunningTasks};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddDownloadInput {
    pub url: String,
    pub dest_dir: Option<String>,
    pub file_name: Option<String>,
    pub connections: Option<u16>,
    pub expected_sha256: Option<String>,
}

/// Ngoài luồng giao diện: tạo/kiểm thư mục tải trên ổ mạng đang tắt có thể treo nhiều giây.
#[tauri::command]
pub async fn add_download(
    app: AppHandle,
    state: State<'_, AppState>,
    input: AddDownloadInput,
) -> Result<DownloadTask, String> {
    let engine = state.engine.clone();
    let running = state.running.clone();
    tauri::async_runtime::spawn_blocking(move || enqueue_download(app, engine, running, input))
        .await
        .map_err(|error| error.to_string())?
}

pub(crate) fn enqueue_download(
    app: AppHandle,
    engine: Arc<DownloadEngine>,
    running: Arc<RunningTasks>,
    input: AddDownloadInput,
) -> Result<DownloadTask, String> {
    // Trang video tải bằng HTTP chỉ được trang HTML: trả mã `media_page` để giao diện mở hộp Tải media.
    if crate::media_pages::is_media_page(&input.url) {
        return Err(crate::media_pages::media_page_error());
    }
    let dest_dir = match input.dest_dir.filter(|value| !value.trim().is_empty()) {
        Some(value) => checked_dest_dir(&value, &app_data_dir(&app)?)?,
        None => suggested_download_dir(&app)?,
    };
    let allow_rename = input.file_name.is_none();
    let mut request = DownloadRequest::new(input.url, dest_dir.to_string_lossy().into_owned());
    request.file_name = input.file_name.filter(|value| !value.trim().is_empty());
    // Không gửi số kết nối ⇒ tự động (engine tự dò tới mức tối đa); có gửi thì chỉ là trần.
    let max_connections = engine.options().max_connections;
    request.connections = input
        .connections
        .unwrap_or(max_connections)
        .clamp(1, max_connections);
    request.expected_sha256 = input
        .expected_sha256
        .filter(|value| !value.trim().is_empty());
    let task = engine.enqueue(request).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&task.dest_dir).map_err(|error| error.to_string())?;
    spawn_run(
        app,
        engine,
        running,
        task.id.clone(),
        false,
        allow_rename,
    )?;
    Ok(task)
}

#[tauri::command]
pub fn list_downloads(state: State<'_, AppState>) -> Result<Vec<DownloadTask>, String> {
    state
        .journal
        .list_tasks()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn pause_download(state: State<'_, AppState>, task_id: String) -> Result<(), String> {
    state.running.cancel(&task_id);
    Ok(())
}

#[tauri::command]
pub fn resume_download(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> Result<(), String> {
    if state.running.contains(&task_id) {
        return Err("download is already running".into());
    }
    let task = state
        .journal
        .load_task(&task_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "download not found".to_string())?;
    if task.kind == TaskKind::Media {
        return crate::media::spawn_media_run(
            app,
            state.journal.clone(),
            state.running.clone(),
            task_id,
        );
    }
    spawn_run(
        app,
        state.engine.clone(),
        state.running.clone(),
        task_id,
        true,
        false,
    )
}

#[tauri::command]
pub fn remove_download(
    state: State<'_, AppState>,
    task_id: String,
    delete_files: Option<bool>,
) -> Result<(), String> {
    if state.running.contains(&task_id) {
        return Err("pause the download before removing it".into());
    }
    remove_task(&state.journal, &task_id, delete_files.unwrap_or(false))
}

/// Bỏ task khỏi journal. Task media luôn dọn luồng tạm của nó (`.part` và luồng đã tải của task
/// con ẩn): file nội bộ, không hiện trong danh sách, bỏ lại thì không ai dọn.
fn remove_task(journal: &Journal, task_id: &str, delete_files: bool) -> Result<(), String> {
    let task = journal
        .load_task(task_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "download not found".to_string())?;
    if task.kind == TaskKind::Media {
        crate::media::parts::discard_parts(journal, task_id);
    }
    journal
        .delete_task(task_id)
        .map_err(|error| error.to_string())?;
    if delete_files {
        let target = Path::new(&task.dest_dir).join(&task.file_name);
        let _ = std::fs::remove_file(target);
        let _ = std::fs::remove_file(&task.temp_path);
    }
    Ok(())
}

/// Ngoài luồng giao diện: kiểm thư mục đã lưu chạm đĩa (ổ mạng tắt ⇒ chờ lâu, cửa sổ không treo).
#[tauri::command]
pub async fn default_download_dir(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        suggested_download_dir(&app).map(|path| path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Khoá trong bảng settings của journal; chuỗi rỗng = dùng mặc định.
const DOWNLOAD_DIR_KEY: &str = "download_dir";
/// Chừa chỗ cho tên file trong giới hạn 260 ký tự của Windows.
const MAX_DOWNLOAD_DIR_CHARS: usize = 200;
const INSIDE_DATA_DIR: &str = "Không dùng thư mục dữ liệu của UTD để tải.";
const SAVE_FAILED: &str = "Không lưu được cài đặt.";

/// Lưu thư mục tải mặc định; trả đường dẫn đã chuẩn hoá.
/// Chạy ngoài luồng giao diện: tạo thư mục / ghi thử trên ổ mạng có thể chậm.
#[tauri::command]
pub async fn set_default_download_dir(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<String, String> {
    let data = app_data_dir(&app)?;
    let journal = state.journal.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let folder = prepare_download_dir(&path, &data)?;
        let value = folder.to_string_lossy().into_owned();
        journal
            .set_setting(DOWNLOAD_DIR_KEY, &value)
            .map_err(|_| SAVE_FAILED.to_string())?;
        Ok(value)
    })
    .await
    .map_err(|_| SAVE_FAILED.to_string())?
}

/// Bỏ thư mục đã chọn, quay về `Downloads\UTD`; trả thư mục sẽ dùng.
#[tauri::command]
pub fn reset_default_download_dir(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    state
        .journal
        .set_setting(DOWNLOAD_DIR_KEY, "")
        .map_err(|_| SAVE_FAILED.to_string())?;
    fallback_download_dir(&app).map(|path| path.to_string_lossy().into_owned())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSpace {
    pub free_bytes: u64,
    pub total_bytes: u64,
    pub label: String,
}

/// Ngoài luồng giao diện, cùng lý do với `default_download_dir`.
#[tauri::command]
pub async fn disk_space(app: AppHandle, path: Option<String>) -> Result<DiskSpace, String> {
    tauri::async_runtime::spawn_blocking(move || measure_disk(&app, path))
        .await
        .map_err(|error| error.to_string())?
}

fn measure_disk(app: &AppHandle, path: Option<String>) -> Result<DiskSpace, String> {
    let path = match path.filter(|value| !value.trim().is_empty()) {
        Some(value) => PathBuf::from(value),
        None => suggested_download_dir(app)?,
    };
    std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
    let free_bytes = fs2::available_space(&path).map_err(|error| error.to_string())?;
    let total_bytes = fs2::total_space(&path).map_err(|error| error.to_string())?;
    Ok(DiskSpace {
        free_bytes,
        total_bytes,
        label: disk_label(&path),
    })
}

/// Tên ổ hiện cạnh dung lượng: `C:` trên Windows; trên macOS là tên Finder hiện ("Macintosh HD",
/// tên USB), không có thì `/Volumes/<tên>`.
fn disk_label(path: &Path) -> String {
    #[cfg(target_os = "macos")]
    if let Some(name) = mac_volume_name(path) {
        return name;
    }
    if cfg!(target_os = "macos")
        && let Some(name) = volume_root(path).strip_prefix("/Volumes").ok().and_then(Path::to_str)
        && !name.is_empty()
    {
        return name.to_string();
    }
    path.components()
        .next()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_else(|| "Ổ đĩa".into())
}

/// `NSURLVolumeLocalizedNameKey`: thư mục người dùng nằm ở ổ Data (firmlink) vẫn ra "Macintosh HD".
#[cfg(target_os = "macos")]
fn mac_volume_name(path: &Path) -> Option<String> {
    use objc2_foundation::{NSString, NSURL, NSURLVolumeLocalizedNameKey};
    objc2::rc::autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let mut value = None;
        // SAFETY: khoá là hằng của Foundation; `value` nhận đúng một đối tượng hoặc để None.
        unsafe { url.getResourceValue_forKey_error(&mut value, NSURLVolumeLocalizedNameKey) }
            .ok()?;
        let name = value?.downcast::<NSString>().ok()?.to_string();
        (!name.trim().is_empty()).then_some(name)
    })
}

/// Thư mục tải khi không chỉ định: thư mục đã lưu trong Cài đặt, nếu không thì `Downloads\UTD`.
/// Dùng chung cho add_download, add_media_download, link từ trình duyệt (IPC) và disk_space.
pub(crate) fn suggested_download_dir(app: &AppHandle) -> Result<PathBuf, String> {
    match saved_download_dir(app) {
        Some(folder) => Ok(folder),
        None => fallback_download_dir(app),
    }
}

fn fallback_download_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .download_dir()
        .or_else(|_| app.path().home_dir())
        .map_err(|error| error.to_string())?;
    Ok(base.join("UTD"))
}

/// None khi chưa đặt, journal chưa sẵn sàng (IPC chạy trước `manage`) hoặc giá trị hỏng.
fn saved_download_dir(app: &AppHandle) -> Option<PathBuf> {
    let state = app.try_state::<AppState>()?;
    let value = state.journal.get_setting(DOWNLOAD_DIR_KEY).ok().flatten()?;
    let data = app_data_dir(app).ok()?;
    usable_saved_dir(&value, &data)
}

/// Cùng chỗ với `data_dir` trong lib.rs.
pub(crate) fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    crate::ipc::data_dir().or_else(|_| {
        app.path()
            .app_local_data_dir()
            .map_err(|error| error.to_string())
    })
}

/// Giá trị đã lưu chỉ dùng khi vẫn hợp lệ và ổ đĩa còn đó (USB rút ra ⇒ về mặc định).
fn usable_saved_dir(value: &str, data_dir: &Path) -> Option<PathBuf> {
    if value.trim().is_empty() {
        return None;
    }
    let folder = parse_download_dir(value).ok()?;
    let root = volume_root(&folder);
    let usable = root.is_dir() && !folder.is_file() && !is_within(&folder, data_dir);
    usable.then_some(folder)
}

/// Gốc ổ chứa thư mục: `D:\` trên Windows; macOS gắn ổ ngoài ở `/Volumes/<tên>` (rút ra là mất).
fn volume_root(folder: &Path) -> PathBuf {
    if cfg!(target_os = "macos")
        && let Ok(rest) = folder.strip_prefix("/Volumes")
        && let Some(name) = rest.components().next()
    {
        return Path::new("/Volumes").join(name);
    }
    folder
        .components()
        .take_while(|component| matches!(component, Component::Prefix(_) | Component::RootDir))
        .collect()
}

/// Ký tự điều khiển hoặc đảo chiều chữ (bidi) làm tên thư mục hiển thị sai.
fn is_hidden_char(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Tên Windows không cho dùng: ký tự cấm, tên thiết bị (CON, NUL, COM1…), đuôi chấm/cách
/// (Windows tự cắt ⇒ thư mục thật khác chuỗi đã lưu).
fn is_bad_folder_name(name: &str) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    device || name.ends_with(['.', ' ']) || name.contains(['<', '>', ':', '"', '|', '?', '*'])
}

/// Chỉ kiểm chuỗi (chưa đụng đĩa); trả đường dẫn đã bỏ dấu `\` thừa.
fn parse_download_dir(raw: &str) -> Result<PathBuf, String> {
    let value = raw.trim();
    if value.is_empty() {
        return Err("Chưa chọn thư mục.".into());
    }
    if value.chars().count() > MAX_DOWNLOAD_DIR_CHARS {
        return Err("Đường dẫn quá dài.".into());
    }
    let path = Path::new(value);
    let bad_name = path.components().any(|component| match component {
        Component::Normal(name) => is_bad_folder_name(&name.to_string_lossy()),
        _ => false,
    });
    if value.chars().any(is_hidden_char) || bad_name {
        return Err("Đường dẫn có ký tự không hợp lệ.".into());
    }
    if !path.is_absolute() {
        return Err(if cfg!(windows) {
            "Cần đường dẫn đầy đủ, ví dụ D:\\Tải về.".into()
        } else {
            "Cần đường dẫn đầy đủ, ví dụ /Users/ban/Downloads/UTD.".into()
        });
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("Đường dẫn không được chứa \"..\".".into());
    }
    Ok(path.components().collect())
}

/// Kiểm đủ điều kiện, tạo thư mục nếu chưa có và thử ghi một tệp; trả đường dẫn sẽ lưu.
/// Dùng chung với "Chuyển tới" (files.rs): thư mục đích kiểm y như Cài đặt.
pub(crate) fn prepare_download_dir(raw: &str, data_dir: &Path) -> Result<PathBuf, String> {
    create_download_dir(raw, data_dir, true)
}

/// Thư mục hộp thoại Thêm gửi kèm: kiểm như Cài đặt nhưng không ghi thử (thêm download phải nhanh).
/// Chặn UTD-Data vì bộ gỡ cài có thể xoá thư mục đó (hooks.nsh, AC-24).
fn checked_dest_dir(raw: &str, data_dir: &Path) -> Result<PathBuf, String> {
    create_download_dir(raw, data_dir, false)
}

/// Kiểm chuỗi, chặn thư mục dữ liệu (cả đường dẫn thật), tạo thư mục; `probe` = ghi thử một tệp.
/// Hỏng ở bước nào thì gỡ các cấp vừa tạo.
fn create_download_dir(raw: &str, data_dir: &Path, probe: bool) -> Result<PathBuf, String> {
    let folder = parse_download_dir(raw)?;
    if folder.is_file() {
        return Err("Đây là tệp, hãy chọn một thư mục.".into());
    }
    if is_within(&folder, data_dir) {
        return Err(INSIDE_DATA_DIR.into());
    }
    // Các cấp thư mục sắp tạo mới, sâu nhất trước: hỏng thì gỡ lại cho sạch.
    let created: Vec<PathBuf> = folder
        .ancestors()
        .take_while(|level| !level.exists())
        .map(Path::to_path_buf)
        .collect();
    if let Err(error) = std::fs::create_dir_all(&folder) {
        remove_empty_dirs(&created);
        log::warn!("Download folder create failed: {}", error.kind());
        return Err("Không tạo được thư mục này.".into());
    }
    // Junction hoặc tên 8.3 vẫn có thể trỏ vào thư mục dữ liệu: so cả đường dẫn thật.
    let real_inside = match (
        std::fs::canonicalize(&folder),
        std::fs::canonicalize(data_dir),
    ) {
        (Ok(real), Ok(real_data)) => is_within(&real, &real_data),
        _ => false,
    };
    let checked = if real_inside {
        Err(INSIDE_DATA_DIR.to_string())
    } else if probe {
        probe_writable(&folder)
    } else {
        Ok(())
    };
    if let Err(message) = checked {
        remove_empty_dirs(&created);
        return Err(message);
    }
    Ok(folder)
}

/// `remove_dir` chỉ xoá thư mục rỗng: không bao giờ đụng dữ liệu có sẵn.
fn remove_empty_dirs(levels: &[PathBuf]) {
    for level in levels {
        let _ = std::fs::remove_dir(level);
    }
}

/// Tạo rồi xoá một tệp tên ngẫu nhiên: chứng minh ghi và xoá được trong thư mục.
fn probe_writable(folder: &Path) -> Result<(), String> {
    let failed = || "Không ghi được vào thư mục này.".to_string();
    let probe = folder.join(format!(".utd-probe-{}.tmp", uuid::Uuid::new_v4().simple()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|_| failed())?;
    let written = file.write_all(b"utd").and_then(|()| file.sync_all());
    drop(file);
    let removed = std::fs::remove_file(&probe);
    written.and(removed).map_err(|_| failed())
}

/// `path` nằm trong (hoặc chính là) `root`; Windows và ổ mặc định của macOS không phân biệt hoa thường.
fn is_within(path: &Path, root: &Path) -> bool {
    let parts = |value: &Path| -> Vec<String> {
        value
            .components()
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
    let path = parts(path);
    let root = parts(root);
    !root.is_empty() && path.len() >= root.len() && path[..root.len()] == root[..]
}

fn spawn_run(
    app: AppHandle,
    engine: Arc<DownloadEngine>,
    running: Arc<RunningTasks>,
    task_id: String,
    resume: bool,
    allow_rename: bool,
) -> Result<(), String> {
    let cancel = CancellationToken::new();
    if !running.insert(&task_id, cancel.clone()) {
        return Err("download is already running".into());
    }
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let event_app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = receiver.recv().await {
            emit_engine_event(&event_app, event);
        }
    });
    tauri::async_runtime::spawn(async move {
        let control = DownloadControl {
            cancel,
            events: Some(sender),
            initial_connections: None,
        };
        let result = if resume {
            engine.resume(&task_id, control).await
        } else {
            engine.run_new(&task_id, control, allow_rename).await
        };
        if let Err(error) = result {
            let _ = app.emit(
                ERROR_EVENT,
                BridgeError {
                    task_id: task_id.clone(),
                    message: error.to_string(),
                },
            );
        }
        running.remove(&task_id);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        std::env::temp_dir().join(format!("utd-download-dir-{}", uuid::Uuid::new_v4()))
    }

    fn text(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn rejects_bad_strings_before_touching_disk() {
        let root = scratch();
        let data = root.join("UTD-Data");
        assert!(prepare_download_dir("", &data).is_err());
        assert!(prepare_download_dir("   ", &data).is_err());
        assert!(prepare_download_dir("relative\\Tải về", &data).is_err());
        assert!(prepare_download_dir(&format!("{}\nx", text(&root)), &data).is_err());
        assert!(
            prepare_download_dir(&text(&root.join("phim\u{202E}4pm.exe")), &data).is_err(),
            "chặn ký tự đảo chiều"
        );
        assert!(prepare_download_dir(&text(&root.join("a").join("..").join("b")), &data).is_err());
        let long = root.join("x".repeat(MAX_DOWNLOAD_DIR_CHARS));
        assert_eq!(
            prepare_download_dir(&text(&long), &data).unwrap_err(),
            "Đường dẫn quá dài."
        );
        #[cfg(windows)]
        for name in ["a?b", "NUL", "com1.txt", "Phim.", "Phim "] {
            let bad = format!("{}\\x", text(&root.join(name)));
            assert!(prepare_download_dir(&bad, &data).is_err(), "{name}");
        }
        assert!(!root.exists(), "không tạo gì khi chuỗi sai");
    }

    #[test]
    fn rejects_files_and_the_app_data_dir() {
        let root = scratch();
        let data = root.join("UTD-Data");
        std::fs::create_dir_all(&data).unwrap();
        let file = root.join("note.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(prepare_download_dir(&text(&file), &data).is_err());
        assert_eq!(
            prepare_download_dir(&text(&data.join("tai")), &data).unwrap_err(),
            INSIDE_DATA_DIR
        );
        assert!(prepare_download_dir(&text(&data), &data).is_err());
        assert!(!data.join("tai").exists());
        #[cfg(windows)]
        assert!(
            prepare_download_dir(&text(&data).to_uppercase(), &data).is_err(),
            "Windows không phân biệt hoa thường"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn creates_missing_folder_and_leaves_no_probe() {
        let root = scratch();
        let data = root.join("UTD-Data");
        let target = root.join("Tải về").join("Phim");
        let raw = format!("  {}{}  ", text(&target), std::path::MAIN_SEPARATOR);
        let saved = prepare_download_dir(&raw, &data).unwrap();
        assert_eq!(saved, target, "bỏ khoảng trắng và dấu phân cách thừa");
        assert!(target.is_dir());
        assert_eq!(
            std::fs::read_dir(&target).unwrap().count(),
            0,
            "tệp thử đã xoá"
        );
        assert_eq!(prepare_download_dir(&text(&target), &data).unwrap(), target);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Viết kiểu `\\?\C:\…` qua mặt phép so chuỗi, nhưng đường dẫn thật vẫn bị chặn và dọn sạch.
    #[cfg(windows)]
    #[test]
    fn verbatim_path_into_data_dir_is_rejected_and_cleaned() {
        let root = scratch();
        let data = root.join("UTD-Data");
        std::fs::create_dir_all(&data).unwrap();
        let sneaky = format!(r"\\?\{}", text(&data.join("moi").join("sau")));
        assert_eq!(
            prepare_download_dir(&sneaky, &data).unwrap_err(),
            INSIDE_DATA_DIR
        );
        assert!(!data.join("moi").exists(), "gỡ các cấp vừa tạo");
        assert!(data.is_dir(), "không đụng thư mục có sẵn");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Thư mục chọn ở hộp thoại Thêm cũng không được vào UTD-Data (bộ gỡ cài xoá được thư mục đó).
    #[test]
    fn add_dialog_folder_cannot_point_into_data_dir() {
        let root = scratch();
        let data = root.join("UTD-Data");
        std::fs::create_dir_all(&data).unwrap();
        assert_eq!(
            checked_dest_dir(&text(&data.join("Phim")), &data).unwrap_err(),
            INSIDE_DATA_DIR
        );
        assert!(checked_dest_dir("Downloads\\UTD", &data).is_err(), "đường dẫn tương đối");
        #[cfg(windows)]
        {
            let upper = text(&data.join("Phim")).to_uppercase();
            assert_eq!(checked_dest_dir(&upper, &data).unwrap_err(), INSIDE_DATA_DIR);
            let verbatim = format!(r"\\?\{}", text(&data.join("moi")));
            assert_eq!(checked_dest_dir(&verbatim, &data).unwrap_err(), INSIDE_DATA_DIR);
            assert!(!data.join("moi").exists(), "gỡ thư mục vừa tạo");
        }
        let target = root.join("Tải về");
        assert_eq!(checked_dest_dir(&text(&target), &data).unwrap(), target);
        assert!(target.is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn saved_value_is_used_only_when_valid() {
        let root = scratch();
        let data = root.join("UTD-Data");
        let folder = root.join("Tải về");
        assert_eq!(
            usable_saved_dir(&text(&folder), &data),
            Some(folder.clone())
        );
        assert_eq!(usable_saved_dir("", &data), None);
        assert_eq!(usable_saved_dir("relative", &data), None);
        assert_eq!(usable_saved_dir(&text(&data.join("x")), &data), None);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("file.bin");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(usable_saved_dir(&text(&file), &data), None);
        #[cfg(windows)]
        {
            let missing_drive = (b'D'..=b'Z')
                .map(|letter| format!("{}:\\", letter as char))
                .find(|drive| !Path::new(drive).exists());
            if let Some(drive) = missing_drive {
                let gone = format!("{drive}Phim");
                assert_eq!(
                    usable_saved_dir(&gone, &data),
                    None,
                    "ổ đã rút ⇒ về mặc định"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn removing_a_paused_media_task_cleans_its_hidden_streams() {
        let root = scratch();
        let folder = root.join("Tải về");
        std::fs::create_dir_all(&folder).unwrap();
        let journal = Journal::open(root.join("data").join("utd.db")).unwrap();
        let dir = text(&folder);
        let media = DownloadTask::new("https://www.youtube.com/watch?v=x", "Clip.mp4", &*dir, 1)
            .unwrap()
            .with_kind(TaskKind::Media);
        journal.save_task(&media).unwrap();
        let child = |name: &str| {
            DownloadTask::new("https://rr1.googlevideo.com/videoplayback", name, &*dir, 16)
                .unwrap()
                .with_parent(media.id.clone())
        };
        let video = child("Clip.f399.mp4");
        let mut audio = child("Clip.f251.webm");
        audio.status = utd_engine::TaskStatus::Completed;
        audio.hash_state = utd_engine::HashState::Unavailable;
        audio.total_bytes = Some(4);
        audio.downloaded_bytes = 4;
        journal.save_task(&video).unwrap();
        journal.save_task(&audio).unwrap();
        std::fs::write(&video.temp_path, b"part").unwrap();
        std::fs::write(folder.join("Clip.f251.webm"), b"done").unwrap();

        remove_task(&journal, &media.id, false).unwrap();
        assert_eq!(
            std::fs::read_dir(&folder).unwrap().count(),
            0,
            "luồng tạm đã dọn"
        );
        assert!(journal.load_task(&media.id).unwrap().is_none());
        assert!(journal.list_children(&media.id).unwrap().is_empty());
        drop(journal);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn within_compares_whole_components() {
        let root = scratch();
        let data = root.join("UTD-Data");
        assert!(is_within(&data, &data));
        assert!(is_within(&data.join("bin"), &data));
        assert!(!is_within(&root.join("UTD-Data2"), &data));
        assert!(!is_within(&root, &data));
        #[cfg(target_os = "macos")]
        assert!(
            is_within(&root.join("utd-data").join("bin"), &data),
            "APFS mặc định không phân biệt hoa thường"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unplugged_mac_volume_falls_back_to_default() {
        let data = scratch().join("UTD-Data");
        let gone = format!("/Volumes/utd-khong-co-{}/Phim", uuid::Uuid::new_v4().simple());
        assert_eq!(usable_saved_dir(&gone, &data), None, "ổ ngoài đã rút ⇒ về mặc định");
        assert_eq!(volume_root(Path::new("/Users/ban/Downloads")), PathBuf::from("/"));
        assert_eq!(
            volume_root(Path::new("/Volumes/USB/Phim")),
            PathBuf::from("/Volumes/USB")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_disk_label_is_the_volume_name() {
        let boot = disk_label(Path::new("/"));
        assert!(!boot.is_empty() && boot != "/", "tên ổ khởi động, không phải \"/\": {boot}");
        assert_eq!(disk_label(&std::env::temp_dir()), boot, "ổ Data vẫn mang tên ổ khởi động");
        let usb = format!("utd-khong-co-{}", uuid::Uuid::new_v4().simple());
        assert_eq!(disk_label(&Path::new("/Volumes").join(&usb).join("Phim")), usb);
    }

    #[cfg(windows)]
    #[test]
    fn windows_disk_label_is_the_drive() {
        assert_eq!(disk_label(Path::new(r"C:\Users\ban\Downloads")), "C:");
    }
}
