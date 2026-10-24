use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;

use crate::media::sources::Tool;
use crate::media::verify;

/// Nơi tìm thấy công cụ: bản riêng của UTD hoặc bản đã có sẵn trên máy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolSource {
    Utd,
    Path,
    Winget,
    Scoop,
    Choco,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub path: String,
    pub version: String,
    pub source: ToolSource,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaToolsStatus {
    pub yt_dlp: Option<ToolInfo>,
    pub ffmpeg: Option<ToolInfo>,
    /// Runtime JavaScript yt-dlp dùng để giải thử thách của YouTube.
    pub deno: Option<ToolInfo>,
    pub bin_dir: String,
    /// Công cụ đang được tải/cài (nếu có).
    pub installing: Option<String>,
}

impl MediaToolsStatus {
    pub fn info(&self, tool: Tool) -> Option<ToolInfo> {
        match tool {
            Tool::YtDlp => self.yt_dlp.clone(),
            Tool::Deno => self.deno.clone(),
            Tool::Ffmpeg => self.ffmpeg.clone(),
        }
    }
}

pub struct MediaTools {
    pub yt_dlp: PathBuf,
    /// `--version` của yt-dlp đang dùng: quyết định có truyền `--js-runtimes` không, và để biết
    /// bản vừa lỗi đã được thay chưa.
    pub yt_dlp_version: String,
    pub ffmpeg: Option<PathBuf>,
    pub deno: Option<PathBuf>,
}

const YT_DLP: &[&str] = &["yt-dlp.exe", "yt-dlp"];
const FFMPEG: &[&str] = &["ffmpeg.exe", "ffmpeg"];
const FFPROBE: &[&str] = &["ffprobe.exe", "ffprobe"];
const DENO: &[&str] = &["deno.exe", "deno"];
/// Tiền tố thư mục gói WinGet (so không phân biệt hoa thường).
const YT_DLP_PACKAGES: &[&str] = &["yt-dlp.yt-dlp"];
const FFMPEG_PACKAGES: &[&str] = &["gyan.ffmpeg", "btbn.ffmpeg", "yt-dlp.ffmpeg"];
const DENO_PACKAGES: &[&str] = &["denoland.deno"];
/// yt-dlp đòi deno 2 trở lên (bản 1.x bị bỏ qua kèm cảnh báo).
const DENO_MIN_MAJOR: u64 = 2;
/// Chế độ chạy thử (`UTD_DATA_DIR`) + biến này = "1": chỉ dùng công cụ trong `<data>\bin`, bỏ qua bản
/// có sẵn trên máy, để thử luồng tự tải lần đầu trên máy đã cài công cụ.
const OWN_ONLY_ENV: &str = "UTD_TOOLS_OWN_ONLY";
/// Giới hạn quét để không bao giờ chậm: số mục đọc mỗi thư mục, số bản đem chạy `--version`.
const MAX_ENTRIES: usize = 256;
const MAX_VERSION_CHECKS: usize = 6;
const VERSION_TIMEOUT: Duration = Duration::from_secs(20);
const FAILED_RETRY: Duration = Duration::from_secs(60);
const UNKNOWN: &str = "không rõ";

#[derive(Debug, Clone)]
struct Candidate {
    path: PathBuf,
    source: ToolSource,
}

/// Các chỗ tìm công cụ, theo thứ tự ưu tiên.
struct Places {
    bin_dir: PathBuf,
    /// PATH, rồi chỗ cài quen thuộc không nằm trong PATH của app (`~/.deno/bin`, Homebrew trên macOS).
    path_dirs: Vec<PathBuf>,
    /// Gốc của trình quản lý gói: WinGet, Scoop, Chocolatey.
    managers: Vec<(ToolSource, PathBuf)>,
}

/// Một lượt dò mỗi lúc: nhiều download media cùng chạy lúc cache nguội (vd. sau khi cập nhật) thì chỉ
/// lượt đầu chạy `--version`, các lượt sau chờ rồi dùng kết quả đã lưu.
fn lookup_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOOKUP: Mutex<()> = Mutex::new(());
    LOOKUP.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn detect() -> MediaToolsStatus {
    let _lookup = lookup_lock();
    let places = Places::current();
    MediaToolsStatus {
        yt_dlp: pick_yt_dlp(&places).map(|(candidate, version)| info(candidate, version)),
        ffmpeg: pick_ffmpeg(&places).map(|(candidate, version)| info(candidate, version)),
        deno: pick_deno(&places).map(|(candidate, version)| info(candidate, version)),
        bin_dir: places.bin_dir.display().to_string(),
        installing: None,
    }
}

pub fn resolve() -> Result<MediaTools, String> {
    let _lookup = lookup_lock();
    let places = Places::current();
    let (yt_dlp, yt_dlp_version) = pick_yt_dlp(&places)
        .ok_or_else(|| "Chưa có yt-dlp. Mở Cài đặt → Công cụ media để tải yt-dlp.".to_string())?;
    let ffmpeg = pick_ffmpeg(&places).map(|(candidate, _)| candidate.path);
    let deno = pick_deno(&places).map(|(candidate, _)| candidate.path);
    Ok(MediaTools {
        yt_dlp: yt_dlp.path,
        yt_dlp_version,
        ffmpeg,
        deno,
    })
}

pub fn ffmpeg_directory(ffmpeg: Option<&Path>) -> Option<String> {
    ffmpeg
        .and_then(Path::parent)
        .map(|parent| parent.display().to_string())
}

pub fn bin_dir() -> PathBuf {
    crate::ipc::data_dir()
        .map(|directory| directory.join("bin"))
        .unwrap_or_else(|_| PathBuf::from("bin"))
}

fn info(candidate: Candidate, version: String) -> ToolInfo {
    ToolInfo {
        path: candidate.path.display().to_string(),
        version,
        source: candidate.source,
    }
}

/// Bản trong `<data>\bin` luôn thắng; nếu không có thì lấy bản `--version` mới nhất
/// (phiên bản yt-dlp dạng ngày `2026.08.19` nên so chuỗi là đủ).
fn pick_yt_dlp(places: &Places) -> Option<(Candidate, String)> {
    let candidates = candidates(YT_DLP, YT_DLP_PACKAGES, places);
    if let Some(own) = candidates
        .iter()
        .find(|item| item.source == ToolSource::Utd)
        && let Some(version) = version_of(&own.path, "--version")
    {
        return Some((own.clone(), version));
    }
    let mut best: Option<(Candidate, String)> = None;
    for candidate in candidates.iter().take(MAX_VERSION_CHECKS) {
        let Some(version) = version_of(&candidate.path, "--version") else {
            continue;
        };
        if best.as_ref().is_none_or(|(_, current)| version > *current) {
            best = Some((candidate.clone(), version));
        }
    }
    best.or_else(|| {
        candidates
            .into_iter()
            .next()
            .map(|candidate| (candidate, UNKNOWN.to_string()))
    })
}

/// ffmpeg: lấy bản đầu tiên chạy được, ưu tiên thư mục có cả ffprobe (yt-dlp cần cả hai).
fn pick_ffmpeg(places: &Places) -> Option<(Candidate, String)> {
    let candidates = candidates(FFMPEG, FFMPEG_PACKAGES, places);
    let mut fallback: Option<(Candidate, String)> = None;
    for candidate in candidates.iter().take(MAX_VERSION_CHECKS) {
        let Some(line) = version_of(&candidate.path, "-version") else {
            continue;
        };
        let version = short_ffmpeg_version(&line);
        if has_sibling(&candidate.path, FFPROBE) {
            return Some((candidate.clone(), version));
        }
        if fallback.is_none() {
            fallback = Some((candidate.clone(), version));
        }
    }
    fallback.or_else(|| {
        candidates
            .into_iter()
            .next()
            .map(|candidate| (candidate, UNKNOWN.to_string()))
    })
}

/// deno: bản của UTD thắng nếu chạy được (đứng đầu danh sách), không thì bản đầu tiên đủ mới
/// (deno 2+). Bản không chạy được hoặc quá cũ coi như không có: UTD tự tải bản riêng.
fn pick_deno(places: &Places) -> Option<(Candidate, String)> {
    let candidates = candidates(DENO, DENO_PACKAGES, places);
    candidates.iter().take(MAX_VERSION_CHECKS).find_map(|candidate| {
        let line = version_of(&candidate.path, "--version")?;
        let (major, minor, patch) = verify::semver(&line)?;
        (major >= DENO_MIN_MAJOR).then(|| (candidate.clone(), format!("{major}.{minor}.{patch}")))
    })
}

fn has_sibling(path: &Path, names: &[&str]) -> bool {
    path.parent()
        .is_some_and(|parent| names.iter().any(|name| parent.join(name).is_file()))
}

/// "ffmpeg version 8.1.2-full_build-www.gyan.dev Copyright…" → "8.1.2-full_build-www.gyan.dev".
fn short_ffmpeg_version(line: &str) -> String {
    line.strip_prefix("ffmpeg version ")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or(line)
        .chars()
        .take(60)
        .collect()
}

impl Places {
    fn current() -> Self {
        if own_only() {
            return Self {
                bin_dir: bin_dir(),
                path_dirs: Vec::new(),
                managers: Vec::new(),
            };
        }
        let mut path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|value| std::env::split_paths(&value).collect())
            .unwrap_or_default();
        // PATH của tiến trình có thể cũ (cài công cụ sau khi mở app): đọc thêm PATH đã lưu.
        path_dirs.extend(registry_path_dirs());
        path_dirs.extend(usual_dirs());
        Self {
            bin_dir: bin_dir(),
            path_dirs,
            managers: manager_roots(),
        }
    }
}

fn own_only() -> bool {
    crate::ipc::isolated_data_dir().is_some()
        && std::env::var_os(OWN_ONLY_ENV).is_some_and(|value| value == "1")
}

/// Chỗ cài quen thuộc mà PATH của app có thể thiếu: bộ cài deno (`~/.deno/bin`); app macOS mở từ
/// Finder chỉ có PATH tối thiểu nên thêm Homebrew/MacPorts.
fn usual_dirs() -> Vec<PathBuf> {
    let home = env_dir("USERPROFILE").or_else(|| env_dir("HOME"));
    let mut dirs: Vec<PathBuf> = home
        .map(|home| home.join(".deno").join("bin"))
        .into_iter()
        .collect();
    if cfg!(target_os = "macos") {
        dirs.extend(
            ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"]
                .into_iter()
                .map(PathBuf::from),
        );
    }
    dirs
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn manager_roots() -> Vec<(ToolSource, PathBuf)> {
    let mut roots = Vec::new();
    if let Some(local) = env_dir("LOCALAPPDATA") {
        roots.push((ToolSource::Winget, local.join("Microsoft").join("WinGet")));
    }
    if let Some(program_files) = env_dir("ProgramFiles") {
        roots.push((ToolSource::Winget, program_files.join("WinGet")));
    }
    let program_data = env_dir("ProgramData");
    let scoop = env_dir("SCOOP").or_else(|| env_dir("USERPROFILE").map(|home| home.join("scoop")));
    let scoop_global =
        env_dir("SCOOP_GLOBAL").or_else(|| program_data.as_ref().map(|data| data.join("scoop")));
    let choco = env_dir("ChocolateyInstall")
        .or_else(|| program_data.as_ref().map(|data| data.join("chocolatey")));
    for root in [scoop, scoop_global].into_iter().flatten() {
        roots.push((ToolSource::Scoop, root));
    }
    if let Some(root) = choco {
        roots.push((ToolSource::Choco, root));
    }
    roots
}

/// Thứ tự: `<data>\bin`, PATH, rồi chỗ cài quen thuộc của WinGet/Scoop/Chocolatey.
fn candidates(names: &[&str], packages: &[&str], places: &Places) -> Vec<Candidate> {
    let mut found: Vec<Candidate> = Vec::new();
    let push = |path: PathBuf, found: &mut Vec<Candidate>| {
        if !path.is_absolute() || !path.is_file() {
            return;
        }
        let key = lowercase(&path);
        if found.iter().any(|item| lowercase(&item.path) == key) {
            return;
        }
        let source = classify(&path, places);
        found.push(Candidate { path, source });
    };
    for name in names {
        push(places.bin_dir.join(name), &mut found);
    }
    for directory in places.path_dirs.iter().filter(|dir| dir.is_absolute()) {
        for name in names {
            push(directory.join(name), &mut found);
        }
    }
    for (source, root) in &places.managers {
        for path in manager_files(*source, root, names, packages) {
            push(path, &mut found);
        }
    }
    found
}

fn manager_files(
    source: ToolSource,
    root: &Path,
    names: &[&str],
    packages: &[&str],
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let add_dir = |directory: &Path, paths: &mut Vec<PathBuf>| {
        paths.extend(names.iter().map(|name| directory.join(name)));
    };
    match source {
        ToolSource::Winget => {
            add_dir(&root.join("Links"), &mut paths);
            for package in package_dirs(&root.join("Packages"), packages) {
                add_dir(&package, &mut paths);
                add_dir(&package.join("bin"), &mut paths);
                for child in subdirs(&package) {
                    add_dir(&child.join("bin"), &mut paths);
                    add_dir(&child, &mut paths);
                }
            }
        }
        ToolSource::Scoop => add_dir(&root.join("shims"), &mut paths),
        ToolSource::Choco => add_dir(&root.join("bin"), &mut paths),
        ToolSource::Utd | ToolSource::Path => {}
    }
    paths
}

fn subdirs(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .take(MAX_ENTRIES)
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    dirs.sort();
    // Bản mới thường có tên lớn hơn (ffmpeg-8.1.2 sau ffmpeg-8.0).
    dirs.reverse();
    dirs
}

fn package_dirs(packages_root: &Path, prefixes: &[&str]) -> Vec<PathBuf> {
    subdirs(packages_root)
        .into_iter()
        .filter(|path| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            prefixes.iter().any(|prefix| name.starts_with(prefix))
        })
        .collect()
}

fn lowercase(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().to_lowercase())
}

fn classify(path: &Path, places: &Places) -> ToolSource {
    let path = lowercase(path);
    if path.starts_with(lowercase(&places.bin_dir)) {
        return ToolSource::Utd;
    }
    places
        .managers
        .iter()
        .find(|(_, root)| path.starts_with(lowercase(root)))
        .map_or(ToolSource::Path, |(source, _)| *source)
}

#[cfg(windows)]
fn registry_path_dirs() -> Vec<PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let keys = [
        (HKEY_CURRENT_USER, "Environment"),
        (
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ),
    ];
    let mut dirs = Vec::new();
    for (hive, name) in keys {
        let Ok(key) = RegKey::predef(hive).open_subkey(name) else {
            continue;
        };
        let Ok(value) = key.get_value::<String, _>("Path") else {
            continue;
        };
        dirs.extend(std::env::split_paths(&expand_env(&value)));
    }
    dirs
}

#[cfg(not(windows))]
fn registry_path_dirs() -> Vec<PathBuf> {
    Vec::new()
}

/// Mở `%TÊN%` trong PATH kiểu REG_EXPAND_SZ; biến không có thì giữ nguyên.
fn expand_env(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        output.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let name = &after[..end];
        match std::env::var(name) {
            Ok(value) if !name.is_empty() => output.push_str(&value),
            _ => {
                output.push('%');
                output.push_str(name);
                output.push('%');
            }
        }
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    output
}

#[derive(Clone, PartialEq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

struct Cached {
    stamp: Stamp,
    version: Option<String>,
    checked: Instant,
}

/// Kết quả `--version` theo (đường dẫn, kích thước, giờ sửa): file đổi thì chạy lại.
fn cache() -> &'static Mutex<HashMap<PathBuf, Cached>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Cached>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Quên kết quả cũ (sau khi cài/cập nhật công cụ).
pub fn forget_versions() {
    if let Ok(mut map) = cache().lock() {
        map.clear();
    }
}

fn version_of(path: &Path, flag: &str) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    let stamp = Stamp {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    };
    if let Ok(map) = cache().lock()
        && let Some(cached) = map.get(path)
        && cached.stamp == stamp
        && (cached.version.is_some() || cached.checked.elapsed() < FAILED_RETRY)
    {
        return cached.version.clone();
    }
    let version = run_version(path, flag, VERSION_TIMEOUT);
    if let Ok(mut map) = cache().lock() {
        if map.len() > 64 {
            map.clear();
        }
        map.insert(
            path.to_path_buf(),
            Cached {
                stamp,
                version: version.clone(),
                checked: Instant::now(),
            },
        );
    }
    version
}

/// Chạy `<tool> <flag>` có hạn giờ; trả dòng đầu stdout khi thoát thành công.
pub(crate) fn run_version(path: &Path, flag: &str, limit: Duration) -> Option<String> {
    let mut child = crate::media::std_command(path)
        .arg(flag)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut stdout).take(64 * 1024).read_to_end(&mut bytes);
        let _ = sender.send(bytes);
    });
    let deadline = Instant::now() + limit;
    let bytes = receiver.recv_timeout(limit).ok();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) if bytes.is_some() => break Some(status),
            Ok(None) if bytes.is_some() && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    if !status?.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes?).into_owned();
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(120).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        std::env::temp_dir().join(format!("utd-tools-{}", uuid::Uuid::new_v4()))
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, b"").expect("file");
    }

    #[test]
    fn finds_tools_in_package_manager_dirs_and_labels_source() {
        let root = scratch();
        let winget = root.join("WinGet");
        let gyan = winget
            .join("Packages")
            .join("Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe")
            .join("ffmpeg-8.1.2-full_build")
            .join("bin")
            .join("ffmpeg.exe");
        let ytdlp = winget
            .join("Packages")
            .join("yt-dlp.yt-dlp_Microsoft.Winget.Source_8wekyb3d8bbwe")
            .join("yt-dlp.exe");
        let shim = root.join("scoop").join("shims").join("yt-dlp.exe");
        let choco = root.join("chocolatey").join("bin").join("ffmpeg.exe");
        let unrelated = winget
            .join("Packages")
            .join("Other.Tool")
            .join("ffmpeg.exe");
        for path in [&gyan, &ytdlp, &shim, &choco, &unrelated] {
            touch(path);
        }
        let places = Places {
            bin_dir: root.join("UTD-Data").join("bin"),
            path_dirs: vec![PathBuf::from("relative-dir")],
            managers: vec![
                (ToolSource::Winget, winget.clone()),
                (ToolSource::Scoop, root.join("scoop")),
                (ToolSource::Choco, root.join("chocolatey")),
            ],
        };

        let ffmpeg = candidates(FFMPEG, FFMPEG_PACKAGES, &places);
        let paths: Vec<_> = ffmpeg
            .iter()
            .map(|item| (item.path.clone(), item.source))
            .collect();
        assert_eq!(
            paths,
            vec![
                (gyan.clone(), ToolSource::Winget),
                (choco, ToolSource::Choco)
            ]
        );

        let yt = candidates(YT_DLP, YT_DLP_PACKAGES, &places);
        let sources: Vec<_> = yt.iter().map(|item| item.source).collect();
        assert_eq!(sources, vec![ToolSource::Winget, ToolSource::Scoop]);

        // Bản của UTD đứng đầu và được gắn nhãn "utd"; PATH trùng không bị đếm hai lần.
        let own = places.bin_dir.join("yt-dlp.exe");
        touch(&own);
        let places = Places {
            path_dirs: vec![places.bin_dir.clone()],
            ..places
        };
        let yt = candidates(YT_DLP, YT_DLP_PACKAGES, &places);
        assert_eq!(yt[0].path, own);
        assert_eq!(yt[0].source, ToolSource::Utd);
        assert_eq!(yt.len(), 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn expands_registry_path_variables() {
        let path = std::env::var("PATH").expect("PATH");
        assert_eq!(expand_env("%PATH%"), path);
        assert_eq!(
            expand_env(r"%UTD_NO_SUCH_VAR%\bin;C:\x;50%"),
            r"%UTD_NO_SUCH_VAR%\bin;C:\x;50%"
        );
        assert_eq!(expand_env("%%"), "%%");
    }

    #[test]
    fn shortens_ffmpeg_version_line() {
        assert_eq!(
            short_ffmpeg_version("ffmpeg version 8.1.2-full_build-www.gyan.dev Copyright (c) 2000"),
            "8.1.2-full_build-www.gyan.dev"
        );
        assert_eq!(short_ffmpeg_version("N-127943"), "N-127943");
    }
}
