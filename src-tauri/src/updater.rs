//! Tự cập nhật (SPEC §2, AC-20, P-03). Chỉ đọc `latest.json` tĩnh trên GitHub Releases của dự án:
//! không gửi phiên bản, máy hay download nào. Gói NSIS được plugin kiểm chữ ký minisign (khoá công
//! khai trong tauri.conf.json) trước khi chạy: sai chữ ký ⇒ từ chối, giữ nguyên bản đang dùng.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Error as UpdaterError, Update, UpdaterExt};
use utd_engine::{Journal, TaskStatus};

use crate::events::{UpdateProgress, UPDATE_AVAILABLE_EVENT, UPDATE_PROGRESS_EVENT};
use crate::state::{AppState, RunningTasks};

/// Khoá trong bảng settings của journal: "off" = tắt tự kiểm tra; không có/khác = bật.
const CHECK_KEY: &str = "update_check";
/// Download bị dừng để cài bản mới, mỗi dòng một id: lần mở sau tự chạy tiếp.
const RESUME_KEY: &str = "update_resume";
const STARTUP_DELAY: Duration = Duration::from_secs(15);
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// Bộ cài ~10 MB: đủ cho mạng chậm mà không treo mãi.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const PAUSE_WAIT: Duration = Duration::from_secs(10);
const REPORT_EVERY: Duration = Duration::from_millis(250);
const BACKUPS_KEPT: usize = 3;
const NOTES_MAX_CHARS: usize = 1500;

const DISABLED: &str = "Bản này chưa bật tự cập nhật.";
const LATEST: &str = "Bạn đang dùng bản mới nhất.";
const SAVE_FAILED: &str = "Không lưu được cài đặt.";
const BAD_SIGNATURE: &str = "Gói cập nhật sai chữ ký nên đã bị từ chối. Bạn vẫn dùng bản hiện tại.";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
    /// Ngày phát hành dạng dd/mm/yyyy.
    pub date: Option<String>,
}

/// Trạng thái cho mục Cập nhật trong Cài đặt.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    /// false: cấu hình updater thiếu/hỏng, app vẫn chạy bình thường.
    pub ready: bool,
    pub enabled: bool,
    /// Bản mới tìm thấy lần trước (lúc mở app hoặc khi bấm Kiểm tra).
    pub available: Option<UpdateInfo>,
    pub installing: bool,
}

pub struct Updates {
    ready: bool,
    pending: Mutex<Option<Update>>,
    installing: AtomicBool,
}

/// Gọi trong `setup` sau `manage(AppState)`. Plugin lỗi cấu hình chỉ tắt tự cập nhật, app vẫn mở.
pub fn init(app: &tauri::App) {
    let ready = match app
        .handle()
        .plugin(tauri_plugin_updater::Builder::new().build())
    {
        Ok(()) => true,
        Err(error) => {
            log::warn!("Updater disabled: {error}");
            false
        }
    };
    app.manage(Updates {
        ready,
        pending: Mutex::new(None),
        installing: AtomicBool::new(false),
    });
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        resume_after_update(&handle);
        if ready {
            startup_check(handle).await;
        }
    });
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let update = find_update(&app).await?;
    let info = update.as_ref().map(info_of);
    remember(&app, update);
    Ok(info)
}

#[tauri::command]
pub fn get_update_check(app: AppHandle) -> UpdateCheck {
    let updates = app.try_state::<Updates>();
    let available = updates
        .as_ref()
        .and_then(|updates| updates.pending.lock().ok()?.as_ref().map(info_of));
    UpdateCheck {
        ready: updates.as_ref().is_some_and(|updates| updates.ready),
        enabled: check_enabled(&app),
        available,
        installing: updates.is_some_and(|updates| updates.installing.load(Ordering::Acquire)),
    }
}

#[tauri::command]
pub fn set_update_check(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| SAVE_FAILED.to_string())?;
    state
        .journal
        .set_setting(CHECK_KEY, if enabled { "on" } else { "off" })
        .map_err(|_| SAVE_FAILED.to_string())?;
    Ok(enabled)
}

/// Tải gói (kiểm chữ ký) → dừng download đang chạy → sao lưu journal → chạy bộ cài.
/// Windows: plugin chạy bộ cài (`/P /UPDATE /R`) rồi thoát app; bộ cài mở lại UTD bản mới.
/// macOS: plugin giải nén `UTD.app.tar.gz` (đã kiểm chữ ký) đè lên `UTD.app` đang chạy, rồi `restart`.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let updates = app
        .try_state::<Updates>()
        .ok_or_else(|| DISABLED.to_string())?;
    let _guard = Installing::begin(&updates.installing)?;
    let tool_busy = crate::media::media_tools_status()
        .await
        .is_ok_and(|status| status.installing.is_some());
    if tool_busy {
        return Err("Đang tải công cụ media. Chờ xong rồi cập nhật.".into());
    }
    // Giữ bản mới trong slot (không `take`): mở lại Cài đặt giữa chừng vẫn thấy thẻ bản mới và tiến độ;
    // cài lỗi thì vẫn bấm Cập nhật lại được.
    let pending = updates.pending.lock().ok().and_then(|slot| slot.clone());
    let mut update = match pending {
        Some(update) => update,
        None => {
            let found = find_update(&app).await?.ok_or_else(|| LATEST.to_string())?;
            remember(&app, Some(found.clone()));
            found
        }
    };
    // Tải xong mới dừng download của người dùng: gói hỏng/sai chữ ký thì không ảnh hưởng gì.
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    let mut reporter = Reporter::new(app.clone());
    reporter.send();
    let bytes = update
        .download(|chunk, total| reporter.chunk(chunk, total), || {})
        .await
        .map_err(|error| friendly(&error))?;
    reporter.stage("install");
    let paused = pause_running(&app).await;
    let journal = app
        .try_state::<AppState>()
        .map(|state| state.journal.clone());
    if let Some(journal) = &journal {
        backup_journal(&app, &update.current_version).await;
        let _ = journal.set_setting(RESUME_KEY, &paused.join("\n"));
    }
    // `install` kiểm chữ ký lần nữa; Windows chỉ quay lại đây khi không chạy được bộ cài.
    if let Err(error) = update.install(&bytes) {
        if let Some(journal) = &journal {
            let _ = journal.set_setting(RESUME_KEY, "");
        }
        resume_tasks(&app, &paused);
        return Err(friendly(&error));
    }
    app.restart()
}

/// Một lần cài mỗi lúc; tự nhả khi xong hoặc lỗi.
struct Installing<'a>(&'a AtomicBool);

impl<'a> Installing<'a> {
    fn begin(flag: &'a AtomicBool) -> Result<Self, String> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "Đang cập nhật. Chờ một chút.".to_string())?;
        Ok(Self(flag))
    }
}

impl Drop for Installing<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Gửi tiến độ ra UI tối đa ~4 lần/giây (đổi bước thì gửi ngay).
struct Reporter {
    app: AppHandle,
    stage: &'static str,
    done: u64,
    total: Option<u64>,
    last: Option<Instant>,
}

impl Reporter {
    fn new(app: AppHandle) -> Self {
        Self {
            app,
            stage: "download",
            done: 0,
            total: None,
            last: None,
        }
    }

    fn chunk(&mut self, len: usize, total: Option<u64>) {
        self.done = self.done.saturating_add(len as u64);
        self.total = total;
        if self.last.is_none_or(|last| last.elapsed() >= REPORT_EVERY) {
            self.send();
        }
    }

    fn stage(&mut self, stage: &'static str) {
        self.stage = stage;
        self.send();
    }

    fn send(&mut self) {
        self.last = Some(Instant::now());
        let _ = self.app.emit(
            UPDATE_PROGRESS_EVENT,
            UpdateProgress {
                stage: self.stage,
                downloaded_bytes: self.done,
                total_bytes: self.total,
            },
        );
    }
}

async fn find_update(app: &AppHandle) -> Result<Option<Update>, String> {
    let ready = app
        .try_state::<Updates>()
        .is_some_and(|updates| updates.ready);
    if !ready {
        return Err(DISABLED.into());
    }
    let updater = app
        .updater_builder()
        .timeout(CHECK_TIMEOUT)
        .build()
        .map_err(|error| friendly(&error))?;
    updater.check().await.map_err(|error| friendly(&error))
}

fn remember(app: &AppHandle, update: Option<Update>) {
    if let Some(updates) = app.try_state::<Updates>()
        && let Ok(mut slot) = updates.pending.lock()
    {
        *slot = update;
    }
}

fn check_enabled(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .and_then(|state| state.journal.get_setting(CHECK_KEY).ok().flatten())
        .is_none_or(|value| value != "off")
}

/// Kiểm im lặng sau khi mở app; lỗi mạng lúc này không làm phiền người dùng.
async fn startup_check(app: AppHandle) {
    tokio::time::sleep(STARTUP_DELAY).await;
    if !check_enabled(&app) {
        return;
    }
    if let Ok(Some(update)) = find_update(&app).await {
        let info = info_of(&update);
        remember(&app, Some(update));
        let _ = app.emit(UPDATE_AVAILABLE_EVENT, info);
    }
}

fn info_of(update: &Update) -> UpdateInfo {
    let notes = update
        .body
        .as_deref()
        .map(str::trim)
        .filter(|notes| !notes.is_empty())
        .map(|notes| notes.chars().take(NOTES_MAX_CHARS).collect());
    let date = update
        .raw_json
        .get("pub_date")
        .and_then(|value| value.as_str())
        .and_then(day_of);
    UpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes,
        date,
    }
}

/// "2026-10-23T08:00:00Z" → "23/10/2026".
fn day_of(stamp: &str) -> Option<String> {
    let day = stamp.get(..10)?;
    let mut parts = day.split('-');
    let (year, month, date) = (parts.next()?, parts.next()?, parts.next()?);
    let digits =
        |part: &str, len: usize| part.len() == len && part.bytes().all(|b| b.is_ascii_digit());
    (digits(year, 4) && digits(month, 2) && digits(date, 2))
        .then(|| format!("{date}/{month}/{year}"))
}

/// Câu ngắn cho người dùng; chi tiết chỉ vào log (không ghi URL: link CDN có chữ ký tạm).
fn friendly(error: &UpdaterError) -> String {
    match error {
        UpdaterError::Reqwest(inner) => log::warn!(
            "Updater network error (timeout: {}, connect: {}, status: {:?})",
            inner.is_timeout(),
            inner.is_connect(),
            inner.status()
        ),
        other => log::warn!("Updater error: {other}"),
    }
    let message = match error {
        UpdaterError::ReleaseNotFound => "Chưa có bản phát hành nào trên GitHub. Thử lại sau.",
        UpdaterError::Reqwest(inner) if inner.is_timeout() => {
            "Máy chủ cập nhật không trả lời. Thử lại sau."
        }
        UpdaterError::Reqwest(_) => "Không kết nối được GitHub. Kiểm tra mạng rồi thử lại.",
        UpdaterError::Network(_) => "Không tải được gói cập nhật. Thử lại sau.",
        UpdaterError::Serialization(_) | UpdaterError::Semver(_) | UpdaterError::UrlParse(_) => {
            "Thông tin bản mới bị lỗi. Thử lại sau."
        }
        UpdaterError::TargetNotFound(_) | UpdaterError::TargetsNotFound(_) => {
            if cfg!(target_os = "macos") {
                "Bản mới chưa có gói cho macOS."
            } else {
                "Bản mới chưa có gói cho Windows 64-bit."
            }
        }
        UpdaterError::Minisign(_)
        | UpdaterError::Base64(_)
        | UpdaterError::SignatureUtf8(_)
        | UpdaterError::SignedVersionMismatch { .. }
        | UpdaterError::MissingSignedVersion => BAD_SIGNATURE,
        UpdaterError::EmptyEndpoints | UpdaterError::InsecureTransportProtocol => DISABLED,
        UpdaterError::Io(_)
        | UpdaterError::InvalidUpdaterFormat
        | UpdaterError::TempDirNotFound => {
            "Không chạy được bộ cài bản mới. Bạn vẫn dùng bản hiện tại."
        }
        _ => "Không cập nhật được. Thử lại sau.",
    };
    message.to_string()
}

/// Dừng mọi download đang chạy (journal giữ tiến độ); trả id để chạy tiếp sau khi mở lại.
async fn pause_running(app: &AppHandle) -> Vec<String> {
    let Some((journal, running)) = app
        .try_state::<AppState>()
        .map(|state| (state.journal.clone(), state.running.clone()))
    else {
        return Vec::new();
    };
    let ids = running_ids(&journal, &running);
    for id in &ids {
        running.cancel(id);
    }
    let deadline = Instant::now() + PAUSE_WAIT;
    while ids.iter().any(|id| running.contains(id)) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    ids
}

fn running_ids(journal: &Journal, running: &RunningTasks) -> Vec<String> {
    journal
        .list_tasks()
        .unwrap_or_default()
        .into_iter()
        .map(|task| task.id)
        .filter(|id| running.contains(id))
        .collect()
}

/// Chạy lại các download đã dừng để cài bản mới (chỉ task còn ở trạng thái tạm dừng).
fn resume_after_update(app: &AppHandle) {
    let Some(journal) = app
        .try_state::<AppState>()
        .map(|state| state.journal.clone())
    else {
        return;
    };
    let saved = journal
        .get_setting(RESUME_KEY)
        .ok()
        .flatten()
        .unwrap_or_default();
    if saved.trim().is_empty() {
        return;
    }
    let _ = journal.set_setting(RESUME_KEY, "");
    let ids: Vec<String> = saved
        .lines()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect();
    resume_tasks(app, &ids);
}

fn resume_tasks(app: &AppHandle, ids: &[String]) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    for id in ids {
        let paused = matches!(
            state.journal.load_task(id),
            Ok(Some(task)) if task.status == TaskStatus::Paused
        );
        if paused
            && let Err(error) =
                crate::commands::resume_download(app.clone(), app.state(), id.clone())
        {
            log::warn!("Resume after update failed: {error}");
        }
    }
}

/// Bản mới có thể đổi cấu trúc DB: chép journal sang `<data>\backup\utd-<bản cũ>.db` để quay lại.
async fn backup_journal(app: &AppHandle, version: &str) {
    let data = crate::data_dir(app);
    let safe: String = version
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
        .collect();
    let folder = data.join("backup");
    let target = folder.join(format!("utd-{safe}.db"));
    let source = data.join("utd.db");
    let keep_in = folder.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        // Bản sao cùng phiên bản từ lần thử trước: chép lại cho mới.
        let _ = std::fs::remove_file(&target);
        let copied = utd_engine::db::copy_database(&source, &target);
        prune_backups(&keep_in, BACKUPS_KEPT);
        copied
    })
    .await;
    if !matches!(result, Ok(Ok(()))) {
        log::warn!(
            "Journal backup before update failed in {}",
            folder.display()
        );
    }
}

/// Giữ `keep` bản sao mới nhất `utd-*.db`.
fn prune_backups(folder: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let mut backups: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("utd-") && name.ends_with(".db")
        })
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    backups.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, path) in backups.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_of_formats_rfc3339_dates() {
        assert_eq!(
            day_of("2026-10-23T08:00:00Z").as_deref(),
            Some("23/10/2026")
        );
        assert_eq!(day_of("2026-10-23").as_deref(), Some("23/10/2026"));
        assert_eq!(day_of("23/10/2026"), None);
        assert_eq!(day_of("2026-1-2"), None);
        assert_eq!(day_of(""), None);
    }

    #[test]
    fn signature_errors_are_rejections() {
        assert_eq!(friendly(&UpdaterError::MissingSignedVersion), BAD_SIGNATURE);
        let mismatch = UpdaterError::SignedVersionMismatch {
            signed: "0.1.0".into(),
            announced: "9.9.9".into(),
        };
        assert_eq!(friendly(&mismatch), BAD_SIGNATURE);
        assert!(friendly(&UpdaterError::ReleaseNotFound).contains("Chưa có bản phát hành"));
    }

    #[test]
    fn install_guard_allows_one_at_a_time() {
        let flag = AtomicBool::new(false);
        let first = Installing::begin(&flag).expect("lần đầu được");
        assert!(Installing::begin(&flag).is_err(), "lần hai bị chặn");
        drop(first);
        assert!(Installing::begin(&flag).is_ok(), "xong thì nhả");
    }

    #[test]
    fn prune_keeps_newest_backups_only() {
        let folder = std::env::temp_dir().join(format!("utd-backup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        for index in 0..5 {
            let path = folder.join(format!("utd-0.{index}.0.db"));
            std::fs::write(&path, b"x").unwrap();
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            let stamp = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000 + index);
            file.set_modified(stamp).unwrap();
        }
        std::fs::write(folder.join("other.txt"), b"x").unwrap();
        prune_backups(&folder, 3);
        let mut left: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            ["other.txt", "utd-0.2.0.db", "utd-0.3.0.db", "utd-0.4.0.db"]
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Cấu hình đóng gói vào app (tauri.conf.json → plugins.updater).
    fn shipped_config() -> serde_json::Value {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        config["plugins"]["updater"].clone()
    }

    /// Plugin đọc cấu hình đúng bằng `serde_json::from_value::<Config>`: lỗi ở đây là lỗi
    /// `app.handle().plugin(...)` trong `init` (trả Err, app vẫn mở, updater tắt).
    #[test]
    fn broken_updater_config_is_an_error_not_a_panic() {
        let broken = [
            serde_json::Value::Null,
            serde_json::json!({ "pubkey": 5 }),
            serde_json::json!({ "pubkey": "x", "endpoints": ["không phải URL"] }),
            serde_json::json!({ "pubkey": "x", "windows": { "installMode": "silent" } }),
        ];
        for config in broken {
            let parsed = serde_json::from_value::<tauri_plugin_updater::Config>(config.clone());
            assert!(parsed.is_err(), "{config}");
        }
    }

    #[test]
    fn shipped_config_checks_github_with_signed_versions() {
        let raw = shipped_config();
        let config = serde_json::from_value::<tauri_plugin_updater::Config>(raw.clone())
            .expect("cấu hình thật hợp lệ");
        let endpoints: Vec<&str> = config.endpoints.iter().map(|url| url.as_str()).collect();
        assert_eq!(
            endpoints,
            ["https://github.com/1Scout-Co/UTD/releases/latest/download/latest.json"]
        );
        assert!(
            config.require_signed_version,
            "chặn hạ cấp bằng bản cũ có chữ ký"
        );
        assert!(!config.allow_downgrades);
        assert!(!config.dangerous_insecure_transport_protocol);
        assert!(!config.dangerous_accept_invalid_certs);
        assert_eq!(raw["windows"]["installMode"], "passive");
        let key = config.pubkey.as_str();
        assert!(
            !key.is_empty() && key.trim() == key,
            "pubkey là một dòng base64"
        );
    }
}
