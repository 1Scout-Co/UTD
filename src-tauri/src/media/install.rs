//! Tải công cụ media chính chủ (yt-dlp, deno, ffmpeg) vào `<data>\bin`: lấy SHA-256 trước, tải và
//! kiểm hash, giải nén, chạy thử, rồi mới thay bản cũ (bản cũ của UTD giữ thành `.bak` để quay lại).
//! Không đụng vào bản của app khác. Người dùng bấm nút trong Cài đặt (bấm = đồng ý) hoặc app tự tải
//! lần đầu / tự cập nhật (auto.rs, có công tắc trong Cài đặt). Nguồn theo nền tảng: sources.rs.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;
use utd_engine::fetch::{self, FetchError, FetchPolicy};

use crate::events::{TOOL_PROGRESS_EVENT, ToolProgress};
use crate::media::sources::{self, Asset, Pack, Platform, Release, Sums, Tool};
use crate::media::{tools, unpack, verify};

const SUMS_MAX: u64 = 64 * 1024;
const MIB: u64 = 1024 * 1024;
const TMP_PREFIX: &str = "utd-tmp-";
const CHECK_TIMEOUT: Duration = Duration::from_secs(90);
/// Bản `latest` của FFmpeg-Builds được dựng lại mỗi ngày: lệch hash/404 thì chờ rồi thử lại một lần.
pub(crate) const RETRY_DELAY: Duration = Duration::from_secs(20);

/// Kết quả một lần cài.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Installed,
    /// Bản đang dùng đã mới nhất: không tải gì.
    Current,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InstallError {
    /// Đang có lần cài khác (người dùng bấm, hoặc app tự tải).
    Busy(String),
    Cancelled(String),
    Failed(String),
}

impl InstallError {
    pub(crate) fn into_message(self) -> String {
        match self {
            Self::Busy(message) | Self::Cancelled(message) | Self::Failed(message) => message,
        }
    }
}

/// Mỗi lúc chỉ một lần cài; giữ token để nút Huỷ dừng được.
static ACTIVE: Mutex<Option<(&'static str, CancellationToken)>> = Mutex::new(None);

struct ActiveGuard;

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = ACTIVE.lock() {
            *active = None;
        }
    }
}

fn begin(tool: &'static str) -> Result<(ActiveGuard, CancellationToken), InstallError> {
    let mut active = ACTIVE
        .lock()
        .map_err(|_| InstallError::Failed("[tool.start_failed] Không bắt đầu được việc tải công cụ.".into()))?;
    if let Some((name, _)) = active.as_ref() {
        return Err(InstallError::Busy(format!(
            "[tool.busy] Đang tải {name}. Chờ xong rồi thử lại."
        )));
    }
    let token = CancellationToken::new();
    *active = Some((tool, token.clone()));
    Ok((ActiveGuard, token))
}

/// Công cụ đang được tải/cài, nếu có.
pub fn current() -> Option<String> {
    ACTIVE
        .lock()
        .ok()
        .and_then(|active| active.as_ref().map(|(name, _)| name.to_string()))
}

/// Huỷ lần cài đang chạy; `false` khi không có gì để huỷ.
pub fn cancel() -> bool {
    ACTIVE
        .lock()
        .ok()
        .and_then(|active| active.as_ref().map(|(_, token)| token.cancel()))
        .is_some()
}

/// Nơi nhận tiến độ: giao diện (sự kiện) hoặc bài thử.
pub(crate) type Emit = Arc<dyn Fn(ToolProgress) + Send + Sync>;

pub(crate) fn emitter(app: &AppHandle) -> Emit {
    let app = app.clone();
    Arc::new(move |progress| {
        let _ = app.emit(TOOL_PROGRESS_EVENT, progress);
    })
}

/// Gửi tiến độ, tối đa ~4 lần/giây (đổi bước thì gửi ngay).
struct Reporter {
    emit: Emit,
    tool: &'static str,
    stage: &'static str,
    last: Option<Instant>,
}

impl Reporter {
    fn stage(&mut self, stage: &'static str) {
        self.stage = stage;
        self.last = None;
        self.bytes(0, None);
    }

    fn bytes(&mut self, downloaded: u64, total: Option<u64>) {
        let finished = total.is_some_and(|total| downloaded >= total);
        if !finished
            && self
                .last
                .is_some_and(|last| last.elapsed() < Duration::from_millis(250))
        {
            return;
        }
        self.last = Some(Instant::now());
        (self.emit)(ToolProgress {
            tool: self.tool.to_string(),
            stage: self.stage,
            downloaded_bytes: downloaded,
            total_bytes: total,
        });
    }
}

/// Thư mục làm việc tên ASCII trong `<data>\bin`; tự xoá khi xong hoặc lỗi.
struct WorkDir {
    path: PathBuf,
}

impl WorkDir {
    fn new(bin: &Path) -> Result<Self, InstallError> {
        let path = bin.join(format!("{TMP_PREFIX}{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(path.join("x")).map_err(|error| write_error(&error))?;
        Ok(Self { path })
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Một lần cài: gói nào, vào đâu, luật mạng, bản đang dùng (để bỏ qua khi đã mới nhất).
pub(crate) struct Job<'a> {
    pub release: &'a Release,
    pub bin: &'a Path,
    pub policy: FetchPolicy,
    /// `None` = luôn tải (ffmpeg, hoặc chưa có bản nào).
    pub current: Option<String>,
    pub emit: Emit,
    pub retry_delay: Duration,
}

/// Nút "Tải/Cập nhật" trong Cài đặt.
pub async fn install(app: AppHandle, tool: &str) -> Result<tools::MediaToolsStatus, String> {
    let tool = Tool::parse(tool).ok_or_else(|| "[tool.invalid] Công cụ không hợp lệ.".to_string())?;
    let platform = Platform::current()
        .ok_or_else(|| "[tool.unsupported_os] Chưa tự tải được công cụ trên hệ điều hành này.".to_string())?;
    let release = sources::release(tool, platform);
    let current = if tool == Tool::Ffmpeg {
        None
    } else {
        tauri::async_runtime::spawn_blocking(tools::detect)
            .await
            .ok()
            .and_then(|status| status.info(tool).map(|info| info.version))
    };
    let bin = tools::bin_dir();
    let job = Job {
        release: &release,
        bin: &bin,
        policy: release.policy(),
        current,
        emit: emitter(&app),
        retry_delay: RETRY_DELAY,
    };
    install_job(&job)
        .await
        .map_err(InstallError::into_message)?;
    tauri::async_runtime::spawn_blocking(tools::detect)
        .await
        .map_err(|error| error.to_string())
}

/// Cài theo `job` (một lần cài mỗi lúc trong cả app).
pub(crate) async fn install_job(job: &Job<'_>) -> Result<Outcome, InstallError> {
    let tool = job.release.tool.name();
    let (_guard, cancel) = begin(tool)?;
    std::fs::create_dir_all(job.bin).map_err(|error| write_error(&error))?;
    clean_stale(job.bin);
    check_space(job.bin, job.release)?;
    let mut reporter = Reporter {
        emit: job.emit.clone(),
        tool,
        stage: "checksum",
        last: None,
    };
    let result = run_job(job, &cancel, &mut reporter).await;
    tools::forget_versions();
    match &result {
        Ok(outcome) => log::info!("Tool {tool}: {outcome:?}"),
        Err(error) => log::warn!("Installing {tool} failed: {error:?}"),
    }
    result
}

async fn run_job(
    job: &Job<'_>,
    cancel: &CancellationToken,
    reporter: &mut Reporter,
) -> Result<Outcome, InstallError> {
    let work = WorkDir::new(job.bin)?;
    let out = work.path.join("x");
    for (index, asset) in job.release.assets.iter().enumerate() {
        let archive = format!("a{index}.zip");
        let dest = match asset.pack {
            Pack::Binary => out.join(&job.release.binaries[0].name),
            Pack::Zip => work.path.join(&archive),
        };
        // Chỉ gói đầu so phiên bản: các gói sau thuộc cùng một bản.
        let current = job.current.as_deref().filter(|_| index == 0);
        if !fetch_asset(job, asset, &dest, current, cancel, reporter).await? {
            return Ok(Outcome::Current);
        }
        if asset.pack == Pack::Zip {
            reporter.stage("extract");
            unpack::extract(&work.path, &archive, cancel)
                .await
                .map_err(|message| stopped(cancel, job.release.tool, message))?;
            let _ = std::fs::remove_file(&dest);
        }
    }
    let pairs = check_binaries(job, &out, reporter).await?;
    reporter.stage("install");
    swap_in(&pairs).map_err(InstallError::Failed)?;
    // Giấy phép đi kèm bản build (GPL của ffmpeg): giữ cạnh binary.
    if let (Some(name), Some((first, _))) = (job.release.license, pairs.first())
        && let Some(license) = unpack::locate_license(&out, first, name)
    {
        let target = job
            .bin
            .join(format!("{}-LICENSE.txt", job.release.tool.name()));
        let _ = std::fs::copy(license, target);
    }
    Ok(Outcome::Installed)
}

fn stopped(cancel: &CancellationToken, tool: Tool, message: String) -> InstallError {
    if cancel.is_cancelled() {
        describe(FetchError::Cancelled, tool.name())
    } else {
        InstallError::Failed(message)
    }
}

/// Tìm từng file chạy được trong gói, cấp quyền chạy, chạy thử; trả cặp (file mới, chỗ đặt).
async fn check_binaries(
    job: &Job<'_>,
    out: &Path,
    reporter: &mut Reporter,
) -> Result<Vec<(PathBuf, PathBuf)>, InstallError> {
    reporter.stage("check");
    let tool = job.release.tool.name();
    let mut pairs = Vec::new();
    for binary in &job.release.binaries {
        let found = unpack::locate(out, &binary.name).ok_or_else(|| {
            InstallError::Failed(format!(
                "[tool.package_incomplete] Gói {tool} không có {} nên UTD đã dừng.",
                binary.name
            ))
        })?;
        unpack::make_executable(&found).map_err(|error| write_error(&error))?;
        let line = run_check(&found, binary.flag).await;
        if !line.as_deref().is_some_and(binary.accept) {
            return Err(InstallError::Failed(format!(
                "[tool.broken_new] Bản {tool} vừa tải không chạy được nên UTD giữ nguyên bản cũ."
            )));
        }
        pairs.push((found, job.bin.join(&binary.name)));
    }
    Ok(pairs)
}

/// Link tải thật và SHA-256 của nó.
struct Located {
    url: String,
    name: String,
    hash: Option<String>,
    /// Tag thật của bản mới nhất (GitHub), để so với bản đang dùng.
    tag: Option<String>,
}

async fn locate_asset(asset: &Asset, policy: &FetchPolicy) -> Result<Located, FetchError> {
    let (url, name, sidecar) = match &asset.sums {
        Sums::List { url, name } => (url, name, false),
        Sums::Sidecar { url, name } => (url, name, true),
        Sums::Beside => {
            let target = fetch::resolve_redirect(&asset.url, policy).await?;
            let name = target
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .unwrap_or_default()
                .to_string();
            let sums = fetch::fetch_text(&format!("{target}.sha256"), SUMS_MAX, policy).await?;
            return Ok(Located {
                url: target.to_string(),
                hash: verify::checksum_for(&sums.text, &name),
                name,
                tag: None,
            });
        }
    };
    let sums = fetch::fetch_text(url, SUMS_MAX, policy).await?;
    let tag = verify::release_tag(&sums.redirects);
    let hash = if sidecar {
        verify::sidecar_hash(&sums.text, name)
    } else {
        verify::checksum_for(&sums.text, name)
    };
    let url = match (&asset.pinned, &tag) {
        (Some(pinned), Some(tag)) => pinned.replace("{tag}", tag),
        _ => asset.url.clone(),
    };
    Ok(Located {
        url,
        name: name.clone(),
        hash,
        tag,
    })
}

/// Lấy SHA-256 rồi tải file vào `dest`. `false` = bản đang có đã mới nhất, không tải.
async fn fetch_asset(
    job: &Job<'_>,
    asset: &Asset,
    dest: &Path,
    current: Option<&str>,
    cancel: &CancellationToken,
    reporter: &mut Reporter,
) -> Result<bool, InstallError> {
    let tool = job.release.tool;
    for attempt in 0..2 {
        reporter.stage("checksum");
        let located = match locate_asset(asset, &job.policy).await {
            Ok(located) => located,
            Err(FetchError::Status(404)) if attempt == 0 => {
                wait_retry(job, cancel).await?;
                continue;
            }
            Err(error) => return Err(describe(error, tool.name())),
        };
        if let (Some(current), Some(tag)) = (current, located.tag.as_deref())
            && verify::is_current(tool, current, tag)
        {
            return Ok(false);
        }
        let expected = located.hash.ok_or_else(|| {
            InstallError::Failed(format!(
                "[tool.checksum_missing] Danh sách SHA-256 không có {} nên UTD đã dừng.",
                located.name
            ))
        })?;
        reporter.stage("download");
        let result = fetch::download_verified(
            &located.url,
            &expected,
            dest,
            job.release.max_bytes,
            &job.policy,
            cancel,
            |done, total| reporter.bytes(done, total),
        )
        .await;
        match result {
            Ok(_) => return Ok(true),
            Err(FetchError::HashMismatch { .. } | FetchError::Status(404)) if attempt == 0 => {
                wait_retry(job, cancel).await?;
            }
            Err(error) => return Err(describe(error, tool.name())),
        }
    }
    Err(InstallError::Failed(format!(
        "[tool.download_failed] Không tải được {}. Thử lại sau ít phút.",
        tool.name()
    )))
}

async fn wait_retry(job: &Job<'_>, cancel: &CancellationToken) -> Result<(), InstallError> {
    tokio::select! {
        _ = cancel.cancelled() => Err(describe(FetchError::Cancelled, job.release.tool.name())),
        _ = tokio::time::sleep(job.retry_delay) => Ok(()),
    }
}

fn describe(error: FetchError, tool: &str) -> InstallError {
    log::warn!("Tool download error ({tool}): {error}");
    let message = match error {
        FetchError::Cancelled => return InstallError::Cancelled(format!("[tool.cancelled] Đã huỷ tải {tool}.")),
        FetchError::HostNotAllowed(host) | FetchError::InsecureUrl(host) => {
            format!("[tool.redirect_host] Nguồn tải chuyển tới máy chủ lạ ({host}) nên UTD đã dừng.")
        }
        FetchError::TooManyRedirects => {
            "[tool.redirect_loop] Nguồn tải chuyển hướng quá nhiều lần nên UTD đã dừng.".into()
        }
        FetchError::Status(404) => {
            "[tool.not_released] Nguồn tải chưa có bản phát hành (HTTP 404, có thể đang cập nhật). Thử lại sau ít phút."
                .into()
        }
        FetchError::Status(403 | 429) => {
            "[tool.rate_limited] Nguồn tải đang giới hạn lượt tải. Thử lại sau ít phút.".into()
        }
        FetchError::Status(code) => format!("[tool.http] Nguồn tải trả lỗi HTTP {code}. Thử lại sau."),
        FetchError::TooLarge(_) => "[tool.too_large] File tải về lớn bất thường nên UTD đã dừng.".into(),
        FetchError::Truncated { .. } => "[tool.interrupted] Kết nối bị ngắt giữa chừng. Thử lại.".into(),
        FetchError::NotText | FetchError::InvalidUrl | FetchError::InvalidHash => {
            "[tool.checksum_unreadable] Danh sách SHA-256 không đọc được nên UTD đã dừng.".into()
        }
        FetchError::HashMismatch { .. } => {
            "[tool.checksum_mismatch] SHA-256 không khớp nên UTD đã xoá file vừa tải. Thử lại sau ít phút.".into()
        }
        FetchError::Timeout => "[tool.timeout] Nguồn tải phản hồi quá lâu. Kiểm tra mạng rồi thử lại.".into(),
        FetchError::Network(_) => "[tool.connect] Không kết nối được nguồn tải. Kiểm tra mạng rồi thử lại.".into(),
        FetchError::Io(error) => return write_error(&error),
    };
    InstallError::Failed(message)
}

fn write_error(error: &std::io::Error) -> InstallError {
    InstallError::Failed(format!("[tool.write_failed] Không ghi được vào thư mục công cụ: {error}"))
}

fn check_space(bin: &Path, release: &Release) -> Result<(), InstallError> {
    let Ok(free) = fs2::available_space(bin) else {
        return Ok(());
    };
    if free < release.need_free {
        return Err(InstallError::Failed(format!(
            "[tool.disk_full] Ổ đĩa còn ít chỗ: cần khoảng {} MB trống để tải {}.",
            release.need_free / MIB,
            release.tool.name()
        )));
    }
    Ok(())
}

/// Dọn thư mục tạm còn sót khi app bị tắt giữa lúc cài.
fn clean_stale(bin: &Path) {
    let Ok(entries) = std::fs::read_dir(bin) else {
        return;
    };
    for entry in entries.flatten().take(256) {
        let stale = entry.file_name().to_string_lossy().starts_with(TMP_PREFIX)
            && entry.file_type().is_ok_and(|kind| kind.is_dir());
        if stale {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

async fn run_check(path: &Path, flag: &'static str) -> Option<String> {
    let path = path.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || tools::run_version(&path, flag, CHECK_TIMEOUT))
        .await
        .ok()
        .flatten()
}

struct Placed {
    target: PathBuf,
    backup: Option<PathBuf>,
}

/// Đưa từng file mới vào chỗ; bản cũ đổi tên thành `.bak` (đổi tên được cả khi exe đang
/// chạy nên download khác không hỏng). Lỗi giữa chừng ⇒ trả lại mọi bản cũ.
fn swap_in(pairs: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    let mut placed: Vec<Placed> = Vec::new();
    for (source, target) in pairs {
        match place(source, target) {
            Ok(done) => placed.push(done),
            Err(error) => {
                for done in placed.iter().rev() {
                    undo(done);
                }
                let name = target.file_name().unwrap_or_default().to_string_lossy();
                return Err(format!(
                    "[tool.replace_failed] Không thay được {name}: {error}. Bản cũ được giữ nguyên."
                ));
            }
        }
    }
    Ok(())
}

fn place(source: &Path, target: &Path) -> std::io::Result<Placed> {
    let backup = if target.exists() {
        let backup = backup_path(target);
        std::fs::rename(target, &backup)?;
        Some(backup)
    } else {
        None
    };
    if let Err(error) = std::fs::rename(source, target) {
        if let Some(backup) = &backup {
            let _ = std::fs::rename(backup, target);
        }
        return Err(error);
    }
    Ok(Placed {
        target: target.to_path_buf(),
        backup,
    })
}

fn backup_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let backup = target.with_file_name(format!("{name}.bak"));
    if backup.exists() && std::fs::remove_file(&backup).is_err() {
        // Bản .bak cũ còn đang chạy: giữ nó, dùng tên khác.
        return target.with_file_name(format!("{name}.{}.bak", utd_engine::now_ms()));
    }
    backup
}

fn undo(placed: &Placed) {
    let _ = std::fs::remove_file(&placed.target);
    if let Some(backup) = &placed.backup {
        let _ = std::fs::rename(backup, &placed.target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let path = std::env::temp_dir().join(format!("utd-install-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("dir");
        path
    }

    #[test]
    fn swap_keeps_backup_and_rolls_back_on_failure() {
        let root = scratch();
        let target = root.join("yt-dlp.exe");
        std::fs::write(&target, b"old").expect("old");
        let fresh = root.join("new.exe");
        std::fs::write(&fresh, b"new").expect("new");

        swap_in(&[(fresh, target.clone())]).expect("swap");
        assert_eq!(std::fs::read(&target).expect("target"), b"new");
        assert_eq!(
            std::fs::read(root.join("yt-dlp.exe.bak")).expect("bak"),
            b"old"
        );

        // ffmpeg mới vào được nhưng ffprobe thiếu nguồn ⇒ cả hai trở về như cũ.
        let ffmpeg = root.join("ffmpeg.exe");
        let ffprobe = root.join("ffprobe.exe");
        std::fs::write(&ffmpeg, b"old-ffmpeg").expect("ffmpeg");
        std::fs::write(&ffprobe, b"old-ffprobe").expect("ffprobe");
        let fresh_ffmpeg = root.join("fresh-ffmpeg.exe");
        std::fs::write(&fresh_ffmpeg, b"new-ffmpeg").expect("fresh");
        let result = swap_in(&[
            (fresh_ffmpeg, ffmpeg.clone()),
            (root.join("missing.exe"), ffprobe.clone()),
        ]);
        assert!(result.is_err());
        assert_eq!(std::fs::read(&ffmpeg).expect("ffmpeg"), b"old-ffmpeg");
        assert_eq!(std::fs::read(&ffprobe).expect("ffprobe"), b"old-ffprobe");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_one_install_at_a_time() {
        let _serial = crate::media::install_tests::serial_blocking();
        let (guard, token) = begin("yt-dlp").expect("first");
        assert_eq!(current().as_deref(), Some("yt-dlp"));
        assert!(matches!(begin("ffmpeg"), Err(InstallError::Busy(_))));
        assert!(cancel());
        assert!(token.is_cancelled());
        drop(guard);
        assert_eq!(current(), None);
        assert!(!cancel());
    }
}
