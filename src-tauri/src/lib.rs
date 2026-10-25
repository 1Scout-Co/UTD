#[cfg(target_os = "macos")]
mod app_nap;
mod batch;
mod browser_integration;
mod coded;
mod commands;
mod detail;
mod detail_log;
mod events;
mod files;
mod ipc;
mod language;
mod legacy;
mod media;
mod media_pages;
pub mod native_host;
mod preflight;
mod queue;
mod state;
mod titlebar;
mod updater;

#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::Manager;
use utd_engine::{DownloadEngine, EngineOptions, Journal};

use crate::queue::DownloadQueue;
use crate::state::{AppState, RunningTasks};

fn data_dir(app: &tauri::AppHandle) -> PathBuf {
    ipc::data_dir().unwrap_or_else(|_| {
        app.path()
            .app_local_data_dir()
            .unwrap_or_else(|_| PathBuf::from(".").join("utd-data"))
    })
}

/// Sau khi tự cập nhật hay cài đè, `utd-native-host.exe` trong UTD-Data vẫn là bản chép exe cũ nên trình
/// duyệt chạy bản cũ. Tích hợp đã bật mà exe khác (cỡ hoặc giờ sửa; CopyFile giữ giờ sửa) thì chép lại.
#[cfg(windows)]
fn refresh_native_host(directory: &Path) {
    // `tauri dev` không được tự thay host của bản đã cài bằng exe debug.
    if cfg!(debug_assertions) {
        return;
    }
    if let Ok(current) = std::env::current_exe()
        && let Err(error) = sync_native_host(&current, directory)
    {
        log::warn!("Native host refresh failed: {error}");
    }
}

#[cfg(windows)]
fn sync_native_host(current: &Path, directory: &Path) -> std::io::Result<()> {
    let host = directory.join("utd-native-host.exe");
    let manifest = directory.join("native-messaging").join("chromium.json");
    if !manifest.is_file() || !host.is_file() || current == host {
        return Ok(());
    }
    let stamp = |path: &Path| {
        std::fs::metadata(path)
            .ok()
            .map(|meta| (meta.len(), meta.modified().ok()))
    };
    if stamp(current) == stamp(&host) {
        return Ok(());
    }
    // Chép ra tệp tạm rồi thay: hỏng giữa chừng không để lại exe cụt. Host đang chạy thì thay lỗi, để lần sau.
    let fresh = directory.join("utd-native-host.exe.new");
    let replaced = std::fs::copy(current, &fresh).and_then(|_| std::fs::rename(&fresh, &host));
    if replaced.is_err() {
        let _ = std::fs::remove_file(&fresh);
    }
    replaced
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn native_host_copy_follows_the_running_exe() {
        let root = std::env::temp_dir().join(format!("utd-host-{}", uuid::Uuid::new_v4()));
        let data = root.join("UTD-Data");
        std::fs::create_dir_all(data.join("native-messaging")).unwrap();
        let current = root.join("UTD.exe");
        std::fs::write(&current, b"ban moi 0.2.0").unwrap();
        let host = data.join("utd-native-host.exe");
        std::fs::write(&host, b"ban cu").unwrap();

        sync_native_host(&current, &data).unwrap();
        assert_eq!(std::fs::read(&host).unwrap(), b"ban cu", "chưa bật tích hợp thì không đụng");

        std::fs::write(data.join("native-messaging").join("chromium.json"), b"{}").unwrap();
        sync_native_host(&current, &data).unwrap();
        assert_eq!(std::fs::read(&host).unwrap(), b"ban moi 0.2.0");
        assert!(!data.join("utd-native-host.exe.new").exists());

        std::fs::remove_file(&current).unwrap();
        std::fs::write(&current, b"khac").unwrap();
        std::fs::remove_file(&host).unwrap();
        sync_native_host(&current, &data).unwrap();
        assert!(!host.exists(), "chưa có host (gỡ tích hợp) thì không tạo mới");
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let isolated = ipc::isolated_data_dir();
    if let Some(directory) = &isolated
        && std::env::var_os("WEBVIEW2_USER_DATA_FOLDER").is_none()
    {
        // Bản chạy thử cần WebView2 riêng: dùng chung thư mục với bản thật (khác tham số) thì không mở được.
        // SAFETY: đầu `run`, chưa có luồng nào khác đọc biến môi trường.
        unsafe { std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", directory.join("webview")) };
    }
    let mut builder = tauri::Builder::default();
    // Bản chạy thử (`UTD_DATA_DIR`) chạy song song với bản thật nên không giữ một cửa sổ duy nhất.
    if isolated.is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _arguments, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }));
    }
    builder
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            // Trước pipe server: link từ trình duyệt tới khi giao diện chưa nghe thì giữ ở đây.
            app.manage(media_pages::PendingMedia::default());
            app.manage(batch::PendingBatch::default());
            // Màn Chi tiết đang mở task nào (detail.rs): engine đẩy bản đồ đoạn cho đúng task đó.
            app.manage(detail::DetailWatch::default());
            let directory = data_dir(app.handle());
            std::fs::create_dir_all(&directory)?;
            let isolated = ipc::isolated_data_dir().is_some();
            // Bản chạy thử không chép dữ liệu cũ của người dùng, không thay host của bản đã cài.
            let migration = if isolated {
                None
            } else {
                legacy::migrate(&directory).unwrap_or_else(|error| {
                    log::warn!("Legacy data migration failed: {error}");
                    None
                })
            };
            let journal = Arc::new(Journal::open(directory.join("utd.db"))?);
            // Task bị ngắt (app tắt ngang) và task dừng để cài bản mới chạy lại trước, ở đầu hàng đợi.
            let mut front = updater::take_resume_ids(&journal);
            front.extend(queue::interrupted(&journal));
            journal.mark_interrupted()?;
            let queue = DownloadQueue::new(journal.clone());
            if let Err(error) = queue.restore(&front) {
                log::warn!("Queue restore failed: {error}");
            }
            let quarantine_dir = directory.join("quarantine");
            std::fs::create_dir_all(&quarantine_dir)?;
            let engine = Arc::new(DownloadEngine::new(
                journal.clone(),
                EngineOptions {
                    quarantine_dir: Some(quarantine_dir.to_string_lossy().into_owned()),
                    ..EngineOptions::default()
                },
            )?);
            let running = Arc::new(RunningTasks::default());
            let ipc_config = ipc::create_config();
            ipc::write_config(&directory, &ipc_config).map_err(std::io::Error::other)?;
            // Manifest native host cũ trỏ vào thư mục dữ liệu cũ: đăng ký lại ở chỗ mới.
            if migration.is_some_and(|migration| migration.browser_integration)
                && let Err(error) = browser_integration::install_browser_integration()
            {
                log::warn!("Browser integration refresh failed: {error}");
            }
            #[cfg(windows)]
            if !isolated {
                let directory = directory.clone();
                std::thread::spawn(move || refresh_native_host(&directory));
            }
            // Extension đã cài nạp từ UTD-Data\extension: sau khi cập nhật UTD thì ghi bản mới vào đó
            // (`tauri dev` không ghi đè extension của bản đã cài).
            if isolated || !cfg!(debug_assertions) {
                let extension = directory.join("extension");
                std::thread::spawn(move || {
                    match browser_integration::sync_extensions(&extension) {
                        Ok(0) => {}
                        Ok(count) => log::info!("Browser extension refreshed: {count} files"),
                        Err(error) => log::warn!("Browser extension refresh failed: {error}"),
                    }
                });
            }
            // App bị kéo sang chỗ khác sau khi bật tích hợp: script host phải trỏ theo.
            #[cfg(target_os = "macos")]
            if !isolated
                && !cfg!(debug_assertions)
                && let Err(error) = browser_integration::mac::refresh_host_script(&directory)
            {
                log::warn!("Native host refresh failed: {error}");
            }
            ipc::spawn_server(
                app.handle().clone(),
                engine.clone(),
                queue.clone(),
                ipc_config,
                directory.clone(),
            );
            // Công cụ media tự tải lần đầu / tự cập nhật (cần journal để nhớ công tắc và lần kiểm).
            let auto = media::start_auto_tools(app.handle(), journal.clone());
            #[cfg(target_os = "macos")]
            {
                let (running, auto) = (running.clone(), auto.clone());
                app_nap::watch(move || !running.is_empty() || auto.installs_missing());
            }
            app.manage(auto);
            app.manage(AppState {
                engine,
                journal,
                running,
                queue: queue.clone(),
            });
            queue.start(Arc::new(queue::AppRuntime(app.handle().clone())));
            // Task media chờ công cụ tự tải nằm trong hàng mà không giữ chỗ: công cụ xong thì chạy.
            media::watch_tools_for_queue(app.handle());
            log::info!("UTD engine ready at {}", directory.display());
            // Sau AppState: cần journal (cài đặt, download chạy tiếp sau khi cập nhật).
            updater::init(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::add_download,
            commands::list_downloads,
            commands::pause_download,
            commands::resume_download,
            commands::remove_download,
            commands::default_download_dir,
            commands::set_default_download_dir,
            commands::reset_default_download_dir,
            commands::disk_space,
            files::open_download,
            files::reveal_download,
            files::rename_download,
            files::move_download,
            files::delete_download_file,
            files::file_icons,
            browser_integration::browser_integration_status,
            browser_integration::install_browser_integration,
            browser_integration::open_extension_folder,
            media::media_tools_status,
            media::install_media_tool,
            media::cancel_media_tool_install,
            media::media_tools_auto,
            media::set_media_tools_auto,
            media::retry_media_tools,
            media::media_probe,
            media::add_media_download,
            media::channel::channel_list,
            media::channel::channel_list_cancel,
            media_pages::take_media_request,
            batch::take_batch_request,
            batch::add_batch_downloads,
            queue::queue_settings,
            queue::set_queue_limit,
            commands::inspect_link,
            // Màn Chi tiết (S-15..S-21) và cảnh báo trước khi tải (S-22).
            detail::download_detail,
            detail::watch_download,
            detail::download_log,
            preflight::check_links,
            updater::check_for_update,
            updater::install_update,
            updater::get_update_check,
            updater::set_update_check,
            language::get_ui_language,
            language::set_ui_language,
            language::apply_ui_language,
            titlebar::set_caption_buttons,
            titlebar::show_window_menu,
            titlebar::title_bar_double_click,
            titlebar::is_window_active,
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
