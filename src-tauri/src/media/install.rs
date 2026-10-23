//! Tải hộ yt-dlp / ffmpeg từ GitHub khi người dùng bấm (bấm = đồng ý). Kiểm SHA-256 trước
//! khi dùng, giữ bản cũ của UTD thành `.bak` để quay lại, không đụng vào bản của app khác.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;
use utd_engine::fetch::{self, FetchError, FetchPolicy, Url};

use crate::events::{ToolProgress, TOOL_PROGRESS_EVENT};
use crate::media::{tokio_command, tools};

/// GitHub chuyển `releases/.../download` sang CDN đã ký; `objects.` là CDN cũ.
const HOSTS: &[&str] = &[
    "github.com",
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
];
const SUMS_MAX: u64 = 64 * 1024;
const MIB: u64 = 1024 * 1024;
const TMP_PREFIX: &str = "utd-tmp-";
const CHECK_TIMEOUT: Duration = Duration::from_secs(90);
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(600);
/// Bản `latest` của FFmpeg-Builds được dựng lại mỗi ngày: lệch hash/404 thì chờ rồi thử lại một lần.
const RETRY_DELAY: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    YtDlp,
    Ffmpeg,
}

struct Release {
    tool: &'static str,
    kind: Kind,
    asset: &'static str,
    url: &'static str,
    sums_url: &'static str,
    max_bytes: u64,
    need_free: u64,
}

const YT_DLP: Release = Release {
    tool: "yt-dlp",
    kind: Kind::YtDlp,
    asset: "yt-dlp.exe",
    url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe",
    sums_url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS",
    max_bytes: 64 * MIB,
    need_free: 200 * MIB,
};

const FFMPEG: Release = Release {
    tool: "ffmpeg",
    kind: Kind::Ffmpeg,
    asset: "ffmpeg-master-latest-win64-gpl.zip",
    url: "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
    sums_url: "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/checksums.sha256",
    max_bytes: 400 * MIB,
    need_free: 1200 * MIB,
};

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

fn begin(tool: &'static str) -> Result<(ActiveGuard, CancellationToken), String> {
    let mut active = ACTIVE
        .lock()
        .map_err(|_| "Không bắt đầu được việc tải công cụ.".to_string())?;
    if let Some((name, _)) = active.as_ref() {
        return Err(format!("Đang tải {name}. Chờ xong rồi thử lại."));
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

fn policy() -> FetchPolicy {
    FetchPolicy::https_only(HOSTS)
}

/// Gửi tiến độ ra UI, tối đa ~4 lần/giây (đổi bước thì gửi ngay).
struct Reporter {
    app: AppHandle,
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
        let _ = self.app.emit(
            TOOL_PROGRESS_EVENT,
            ToolProgress {
                tool: self.tool.to_string(),
                stage: self.stage,
                downloaded_bytes: downloaded,
                total_bytes: total,
            },
        );
    }
}

/// Thư mục làm việc tên ASCII trong `<data>\bin`; tự xoá khi xong hoặc lỗi.
struct WorkDir {
    path: PathBuf,
}

impl WorkDir {
    fn new(bin: &Path) -> Result<Self, String> {
        let path = bin.join(format!("{TMP_PREFIX}{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir(&path).map_err(|error| write_error(&error))?;
        Ok(Self { path })
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub async fn install(app: AppHandle, tool: &str) -> Result<tools::MediaToolsStatus, String> {
    let release = match tool {
        "yt-dlp" => &YT_DLP,
        "ffmpeg" => &FFMPEG,
        _ => return Err("Công cụ không hợp lệ.".into()),
    };
    let (_guard, cancel) = begin(release.tool)?;
    let bin = tools::bin_dir();
    std::fs::create_dir_all(&bin).map_err(|error| write_error(&error))?;
    clean_stale(&bin);
    check_space(&bin, release)?;
    let mut reporter = Reporter {
        app,
        tool: release.tool,
        stage: "checksum",
        last: None,
    };
    let result = match release.kind {
        Kind::YtDlp => install_yt_dlp(&bin, &cancel, &mut reporter).await,
        Kind::Ffmpeg => install_ffmpeg(&bin, &cancel, &mut reporter).await,
    };
    tools::forget_versions();
    if let Err(message) = &result {
        log::warn!("Installing {} failed: {message}", release.tool);
    }
    result?;
    tauri::async_runtime::spawn_blocking(tools::detect)
        .await
        .map_err(|error| error.to_string())
}

async fn install_yt_dlp(
    bin: &Path,
    cancel: &CancellationToken,
    reporter: &mut Reporter,
) -> Result<(), String> {
    let current = tauri::async_runtime::spawn_blocking(tools::detect)
        .await
        .ok()
        .and_then(|status| status.yt_dlp)
        .map(|info| info.version);
    let work = WorkDir::new(bin)?;
    let fresh = work.path.join("yt-dlp.exe");
    if !fetch_verified(&YT_DLP, &fresh, current.as_deref(), cancel, reporter).await? {
        return Ok(());
    }
    reporter.stage("check");
    let version = run_check(&fresh, "--version").await;
    if !version.as_deref().is_some_and(is_date_version) {
        return Err("Bản yt-dlp vừa tải không chạy được nên UTD giữ nguyên bản cũ.".into());
    }
    reporter.stage("install");
    swap_in(&[(fresh, bin.join("yt-dlp.exe"))])
}

async fn install_ffmpeg(
    bin: &Path,
    cancel: &CancellationToken,
    reporter: &mut Reporter,
) -> Result<(), String> {
    let work = WorkDir::new(bin)?;
    let archive = work.path.join("pkg.zip");
    fetch_verified(&FFMPEG, &archive, None, cancel, reporter).await?;
    reporter.stage("extract");
    let out = work.path.join("x");
    std::fs::create_dir(&out).map_err(|error| write_error(&error))?;
    extract(&work.path, cancel).await?;
    let _ = std::fs::remove_file(&archive);
    let files = locate_ffmpeg(&out)?;
    let mut checked = vec![&files.ffmpeg, &files.ffprobe];
    checked.extend(files.license.as_ref());
    if !checked.iter().all(|path| inside(&out, path)) {
        return Err("Gói ffmpeg có đường dẫn lạ nên UTD đã dừng.".into());
    }
    reporter.stage("check");
    let ffmpeg_ok = run_check(&files.ffmpeg, "-version").await;
    let ffprobe_ok = run_check(&files.ffprobe, "-version").await;
    if !ffmpeg_ok.is_some_and(|line| line.starts_with("ffmpeg version"))
        || !ffprobe_ok.is_some_and(|line| line.starts_with("ffprobe version"))
    {
        return Err("Bản ffmpeg vừa tải không chạy được nên UTD giữ nguyên bản cũ.".into());
    }
    reporter.stage("install");
    swap_in(&[
        (files.ffmpeg, bin.join("ffmpeg.exe")),
        (files.ffprobe, bin.join("ffprobe.exe")),
    ])?;
    // Giấy phép GPL đi kèm bản build: giữ cạnh binary.
    if let Some(license) = files.license {
        let _ = std::fs::copy(license, bin.join("ffmpeg-LICENSE.txt"));
    }
    Ok(())
}

/// Lấy SHA-256 rồi tải file vào `dest`. `false` = bản đang có đã mới nhất, không tải.
async fn fetch_verified(
    release: &Release,
    dest: &Path,
    current: Option<&str>,
    cancel: &CancellationToken,
    reporter: &mut Reporter,
) -> Result<bool, String> {
    for attempt in 0..2 {
        reporter.stage("checksum");
        let sums = match fetch::fetch_text(release.sums_url, SUMS_MAX, &policy()).await {
            Ok(sums) => sums,
            Err(FetchError::Status(404)) if attempt == 0 => {
                wait_retry(cancel, release.tool).await?;
                continue;
            }
            Err(error) => return Err(describe(error, release.tool)),
        };
        if let (Some(current), Some(tag)) = (current, release_tag(&sums.redirects))
            && is_date_version(current)
            && current >= tag.as_str()
        {
            return Ok(false);
        }
        let expected = checksum_for(&sums.text, release.asset).ok_or_else(|| {
            format!(
                "Danh sách SHA-256 không có {} nên UTD đã dừng.",
                release.asset
            )
        })?;
        reporter.stage("download");
        let result = fetch::download_verified(
            release.url,
            &expected,
            dest,
            release.max_bytes,
            &policy(),
            cancel,
            |done, total| reporter.bytes(done, total),
        )
        .await;
        match result {
            Ok(_) => return Ok(true),
            Err(FetchError::HashMismatch { .. } | FetchError::Status(404)) if attempt == 0 => {
                wait_retry(cancel, release.tool).await?;
            }
            Err(error) => return Err(describe(error, release.tool)),
        }
    }
    Err(format!(
        "Không tải được {}. Thử lại sau ít phút.",
        release.tool
    ))
}

async fn wait_retry(cancel: &CancellationToken, tool: &str) -> Result<(), String> {
    tokio::select! {
        _ = cancel.cancelled() => Err(describe(FetchError::Cancelled, tool)),
        _ = tokio::time::sleep(RETRY_DELAY) => Ok(()),
    }
}

fn describe(error: FetchError, tool: &str) -> String {
    log::warn!("Tool download error ({tool}): {error}");
    match error {
        FetchError::HostNotAllowed(host) | FetchError::InsecureUrl(host) => {
            format!("Nguồn tải chuyển tới máy chủ lạ ({host}) nên UTD đã dừng.")
        }
        FetchError::TooManyRedirects => "GitHub chuyển hướng quá nhiều lần nên UTD đã dừng.".into(),
        FetchError::Status(404) => {
            "GitHub chưa có bản phát hành (HTTP 404, có thể đang cập nhật). Thử lại sau ít phút."
                .into()
        }
        FetchError::Status(403 | 429) => {
            "GitHub đang giới hạn lượt tải. Thử lại sau ít phút.".into()
        }
        FetchError::Status(code) => format!("GitHub trả lỗi HTTP {code}. Thử lại sau."),
        FetchError::TooLarge(_) => "File tải về lớn bất thường nên UTD đã dừng.".into(),
        FetchError::Truncated { .. } => "Kết nối bị ngắt giữa chừng. Thử lại.".into(),
        FetchError::NotText | FetchError::InvalidUrl | FetchError::InvalidHash => {
            "Danh sách SHA-256 không đọc được nên UTD đã dừng.".into()
        }
        FetchError::HashMismatch { .. } => {
            "SHA-256 không khớp nên UTD đã xoá file vừa tải. Thử lại sau ít phút.".into()
        }
        FetchError::Timeout => "GitHub phản hồi quá lâu. Kiểm tra mạng rồi thử lại.".into(),
        FetchError::Cancelled => format!("Đã huỷ tải {tool}."),
        FetchError::Network(_) => "Không kết nối được GitHub. Kiểm tra mạng rồi thử lại.".into(),
        FetchError::Io(error) => write_error(&error),
    }
}

fn write_error(error: &std::io::Error) -> String {
    format!("Không ghi được vào thư mục công cụ: {error}")
}

fn check_space(bin: &Path, release: &Release) -> Result<(), String> {
    let Ok(free) = fs2::available_space(bin) else {
        return Ok(());
    };
    if free < release.need_free {
        return Err(format!(
            "Ổ đĩa còn ít chỗ: cần khoảng {} MB trống để tải {}.",
            release.need_free / MIB,
            release.tool
        ));
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

/// Dòng `<sha256>  <tên>` hoặc `<sha256> *<tên>`; tên phải khớp đúng (không khớp tiền tố).
/// Có hai dòng cùng tên mà khác hash ⇒ không tin.
fn checksum_for(text: &str, asset: &str) -> Option<String> {
    let mut found: Option<String> = None;
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        let Some((hash, name)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let name = name.trim_start();
        let name = name.strip_prefix('*').unwrap_or(name);
        if name != asset {
            continue;
        }
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let hash = hash.to_ascii_lowercase();
        if found.as_ref().is_some_and(|previous| *previous != hash) {
            return None;
        }
        found = Some(hash);
    }
    found
}

/// Tag thật của bản `latest`, đọc từ bước chuyển hướng
/// `github.com/<chủ>/<repo>/releases/download/<tag>/<tên>`.
fn release_tag(redirects: &[Url]) -> Option<String> {
    redirects.iter().find_map(|url| {
        if url.host_str()? != "github.com" {
            return None;
        }
        let segments: Vec<&str> = url.path_segments()?.collect();
        match segments.as_slice() {
            [_, _, "releases", "download", tag, _] if *tag != "latest" => Some(tag.to_string()),
            _ => None,
        }
    })
}

/// Phiên bản yt-dlp dạng `2026.10.21` (có thể thêm `.123456` ở bản nightly).
fn is_date_version(version: &str) -> bool {
    let bytes = version.as_bytes();
    bytes.len() >= 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'.'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'.'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
}

async fn run_check(path: &Path, flag: &'static str) -> Option<String> {
    let path = path.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || tools::run_version(&path, flag, CHECK_TIMEOUT))
        .await
        .ok()
        .flatten()
}

fn system_tar() -> Result<PathBuf, String> {
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let tar = root.join("System32").join("tar.exe");
    if tar.is_file() {
        Ok(tar)
    } else {
        Err(
            "Windows thiếu tar.exe (cần Windows 10 1803 trở lên) nên chưa giải nén được ffmpeg."
                .into(),
        )
    }
}

/// Giải nén `pkg.zip` vào `x` bằng tar.exe của Windows (bsdtar, từ chối đường dẫn `..`).
/// bsdtar đổi tham số sang bảng mã ANSI nên chỉ truyền tên ASCII tương đối; thư mục có
/// thể chứa chữ Unicode (tên người dùng) được đặt qua `current_dir`.
async fn extract(work: &Path, cancel: &CancellationToken) -> Result<(), String> {
    let tar = system_tar()?;
    let mut command = tokio_command(&tar);
    command
        .current_dir(work)
        .args(["-xf", "pkg.zip", "-C", "x"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = command
        .spawn()
        .map_err(|error| format!("Không chạy được tar.exe: {error}"))?;
    let output = tokio::select! {
        _ = cancel.cancelled() => return Err(describe(FetchError::Cancelled, "ffmpeg")),
        output = tokio::time::timeout(EXTRACT_TIMEOUT, child.wait_with_output()) => output,
    };
    let output = output
        .map_err(|_| "Giải nén ffmpeg quá lâu nên UTD đã dừng.".to_string())?
        .map_err(|error| format!("Không chạy được tar.exe: {error}"))?;
    if !output.status.success() {
        let detail: String = String::from_utf8_lossy(&output.stderr)
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(160)
            .collect();
        log::warn!("tar.exe failed: {detail}");
        return Err("Giải nén ffmpeg lỗi nên UTD đã dừng.".into());
    }
    Ok(())
}

struct FfmpegFiles {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
    license: Option<PathBuf>,
}

fn is_regular(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

/// Gói có dạng `<tên gói>/bin/ffmpeg.exe`; tìm một tầng thư mục, có giới hạn.
fn locate_ffmpeg(out: &Path) -> Result<FfmpegFiles, String> {
    let mut roots = vec![out.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(out) {
        roots.extend(
            entries
                .flatten()
                .take(16)
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| entry.path()),
        );
    }
    for root in roots {
        let ffmpeg = root.join("bin").join("ffmpeg.exe");
        let ffprobe = root.join("bin").join("ffprobe.exe");
        if is_regular(&ffmpeg) && is_regular(&ffprobe) {
            let license = Some(root.join("LICENSE.txt")).filter(|path| is_regular(path));
            return Ok(FfmpegFiles {
                ffmpeg,
                ffprobe,
                license,
            });
        }
    }
    Err("Gói ffmpeg không có bin\\ffmpeg.exe và bin\\ffprobe.exe nên UTD đã dừng.".into())
}

/// `path` (sau khi phân giải symlink/junction) nằm hẳn trong `root`.
fn inside(root: &Path, path: &Path) -> bool {
    match (std::fs::canonicalize(root), std::fs::canonicalize(path)) {
        (Ok(root), Ok(path)) => path != root && path.starts_with(&root),
        _ => false,
    }
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
                    "Không thay được {name}: {error}. Bản cũ được giữ nguyên."
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

    const A: &str = "33121d192d706e62f684a06f80918e3f3a8ba52ac3a65213b6951c39c7d9f555";
    const B: &str = "7ce38c77db13db50c64e656b1056a0448e6c38ec0f0ccf28e30dd5271c008850";

    fn scratch() -> PathBuf {
        let path = std::env::temp_dir().join(format!("utd-install-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("dir");
        path
    }

    #[test]
    fn checksum_matches_exact_names_only() {
        let sums =
            format!("{B}  yt-dlp_x86.exe\n{A}  yt-dlp.exe\n{B}  yt-dlp.exe.sig\n{B}  yt-dlp\n");
        assert_eq!(checksum_for(&sums, "yt-dlp.exe").as_deref(), Some(A));
        assert_eq!(checksum_for(&sums, "yt-dlp.ex"), None);

        let ffmpeg = format!(
            "{B}  ffmpeg-master-latest-win64-gpl-shared.zip\r\n{}  ffmpeg-master-latest-win64-gpl.zip\r\n",
            A.to_ascii_uppercase()
        );
        assert_eq!(
            checksum_for(&ffmpeg, "ffmpeg-master-latest-win64-gpl.zip").as_deref(),
            Some(A)
        );
    }

    #[test]
    fn checksum_accepts_binary_marker_and_rejects_bad_lines() {
        assert_eq!(
            checksum_for(&format!("\u{feff}{A} *yt-dlp.exe\n"), "yt-dlp.exe").as_deref(),
            Some(A)
        );
        assert_eq!(
            checksum_for(&format!("{A}\tyt-dlp.exe"), "yt-dlp.exe").as_deref(),
            Some(A)
        );
        assert_eq!(checksum_for("abc123  yt-dlp.exe\n", "yt-dlp.exe"), None);
        assert_eq!(
            checksum_for(&format!("{}  yt-dlp.exe", "z".repeat(64)), "yt-dlp.exe"),
            None
        );
        assert_eq!(checksum_for("", "yt-dlp.exe"), None);
        // Hai hash khác nhau cho cùng một tên: không tin.
        assert_eq!(
            checksum_for(&format!("{A}  yt-dlp.exe\n{B}  yt-dlp.exe\n"), "yt-dlp.exe"),
            None
        );
        assert_eq!(
            checksum_for(&format!("{A}  yt-dlp.exe\n{A}  yt-dlp.exe\n"), "yt-dlp.exe").as_deref(),
            Some(A)
        );
    }

    #[test]
    fn reads_release_tag_from_redirects() {
        let hops = [
            Url::parse(
                "https://github.com/yt-dlp/yt-dlp/releases/download/2026.10.21/SHA2-256SUMS",
            )
            .expect("url"),
            Url::parse(
                "https://release-assets.githubusercontent.com/github-production-release-asset/1/x",
            )
            .expect("url"),
        ];
        assert_eq!(release_tag(&hops).as_deref(), Some("2026.10.21"));
        let latest = [Url::parse(
            "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/checksums.sha256",
        )
        .expect("url")];
        assert_eq!(release_tag(&latest), None);
        assert_eq!(release_tag(&[]), None);
        assert!(is_date_version("2026.10.21"));
        assert!(is_date_version("2026.10.22.123456"));
        assert!(!is_date_version("không rõ"));
        assert!(!is_date_version("8.1.2"));
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
    fn locates_ffmpeg_pair_in_package_folder() {
        let root = scratch();
        let bin = root.join("ffmpeg-master-latest-win64-gpl").join("bin");
        std::fs::create_dir_all(&bin).expect("dirs");
        assert!(locate_ffmpeg(&root).is_err());
        std::fs::write(bin.join("ffmpeg.exe"), b"a").expect("file");
        assert!(locate_ffmpeg(&root).is_err(), "ffprobe is required too");
        std::fs::write(bin.join("ffprobe.exe"), b"b").expect("file");
        std::fs::write(bin.parent().expect("pkg").join("LICENSE.txt"), b"GPL").expect("file");
        let files = locate_ffmpeg(&root).expect("found");
        assert_eq!(files.ffmpeg, bin.join("ffmpeg.exe"));
        assert_eq!(files.ffprobe, bin.join("ffprobe.exe"));
        assert!(files.license.is_some());
        let _ = std::fs::remove_dir_all(&root);
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
        let (guard, token) = begin("yt-dlp").expect("first");
        assert_eq!(current().as_deref(), Some("yt-dlp"));
        assert!(begin("ffmpeg").is_err());
        assert!(cancel());
        assert!(token.is_cancelled());
        drop(guard);
        assert_eq!(current(), None);
        assert!(!cancel());
    }
}
