mod auto;
/// Tải cả kênh: `channel::channel_list`, `channel::channel_list_cancel`.
pub(crate) mod channel;
mod fast;
mod finish;
mod install;
#[cfg(test)]
mod install_tests;
mod mux;
mod output;
/// Luồng tiện ích đọc thẳng từ trang (Tin Facebook…): `page_streams::checked`, `page_streams::forget`.
pub(crate) mod page_streams;
/// `parts::discard_parts`: dọn luồng con trước khi xoá task media khỏi danh sách.
pub(crate) mod parts;
mod preset;
pub(crate) mod probe;
mod probe_cache;
mod resolve;
mod rollup;
mod runner;
mod sources;
mod tools;
mod unpack;
mod verify;

pub use auto::{start as start_auto_tools, Auto, AutoState};

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;
use utd_engine::{
    now_ms, validate_file_name, validate_url, DownloadTask, HashState, MediaJob, TaskKind,
    TaskStatus,
};

use utd_engine::Journal;
use crate::events::{BridgeError, ERROR_EVENT};
use crate::queue::{Entry, Place};
use crate::state::{AppState, RunningTasks};


/// `add_media_download`. Hai cách chọn định dạng:
/// - `selector` (+ `audioFormat`) lấy từ `media_probe` (hộp Tải media);
/// - `quality` (`best` · `1080` · `720` · `audio-m4a` · `audio-mp3`) cho lô link: không cần Phân tích trước,
///   tới lượt chạy UTD tự Phân tích, chọn định dạng gần nhất và đặt tên theo tiêu đề (`title` có thể rỗng).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddMediaInput {
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub selector: String,
    pub quality: Option<String>,
    pub audio_format: Option<String>,
    pub page_url: Option<String>,
    /// Thư mục tải; không gửi ⇒ thư mục mặc định trong Cài đặt.
    pub dest_dir: Option<String>,
    /// `true` ⇒ xếp cuối hàng đợi (lô "Thêm vào hàng đợi", tải cả kênh); mặc định chen đầu hàng.
    #[serde(default)]
    pub later: bool,
    /// Tải cả kênh: thư mục con (tên kênh) trong thư mục tải.
    pub subfolder: Option<String>,
    /// Tải cả kênh: video này của kênh nào, để lần sau "Tải video mới" bỏ qua.
    pub channel: Option<channel::ChannelPick>,
    /// Lần cho phép đăng nhập (src/auth) đi kèm hộp Tải media / lô link: yt-dlp và engine dùng cookie đó.
    #[serde(default)]
    pub auth_grant: Option<String>,
}

impl AddMediaInput {
    /// Link trang video trong lô: chất lượng chung, tên theo tiêu đề lúc tới lượt chạy.
    pub(crate) fn with_quality(url: String, quality: String) -> Self {
        Self {
            url,
            title: String::new(),
            selector: String::new(),
            quality: Some(quality),
            audio_format: None,
            page_url: None,
            dest_dir: None,
            later: false,
            subfolder: None,
            channel: None,
            auth_grant: None,
        }
    }
}

/// Định dạng đã kiểm của một yêu cầu media.
struct Choice {
    selector: String,
    audio_format: Option<String>,
}

fn checked_choice(input: &AddMediaInput) -> Result<Choice, String> {
    if let Some(raw) = &input.quality {
        let quality = preset::Quality::parse(raw).ok_or("[media.bad_quality] Chất lượng không hợp lệ.")?;
        return Ok(Choice {
            selector: quality.selector(),
            audio_format: quality.audio_format().map(str::to_string),
        });
    }
    let selector = input.selector.trim();
    if selector.is_empty() || selector.len() > 512 || selector.chars().any(char::is_control) {
        return Err("[media.bad_selector] Bộ chọn định dạng không hợp lệ.".into());
    }
    if let Some(format) = &input.audio_format
        && !matches!(format.as_str(), "m4a" | "mp3")
    {
        return Err("[media.bad_audio] Định dạng âm thanh không hợp lệ.".into());
    }
    Ok(Choice {
        selector: selector.to_string(),
        audio_format: input.audio_format.clone(),
    })
}

/// Dò công cụ có chạy `--version` nên làm ngoài luồng giao diện.
#[tauri::command]
pub async fn media_tools_status() -> Result<tools::MediaToolsStatus, String> {
    let mut status = tauri::async_runtime::spawn_blocking(tools::detect)
        .await
        .map_err(|error| error.to_string())?;
    status.installing = install::current();
    Ok(status)
}

/// Tải yt-dlp, deno hoặc ffmpeg chính chủ vào `<data>\bin` (người dùng đã bấm nút).
#[tauri::command]
pub async fn install_media_tool(
    app: AppHandle,
    tool: String,
) -> Result<tools::MediaToolsStatus, String> {
    install::install(app, &tool).await
}

#[tauri::command]
pub fn cancel_media_tool_install() -> bool {
    install::cancel()
}

/// Trạng thái tự tải/cập nhật công cụ (mục Công cụ media, thẻ tiến độ ở thanh bên).
#[tauri::command]
pub fn media_tools_auto(auto: tauri::State<'_, Auto>) -> AutoState {
    auto.state()
}

/// Công tắc "Tự tải và cập nhật" trong Cài đặt.
#[tauri::command]
pub fn set_media_tools_auto(
    auto: tauri::State<'_, Auto>,
    enabled: bool,
) -> Result<AutoState, String> {
    auto.set_enabled(enabled)
}

/// Nút "Thử lại" khi tự tải công cụ lỗi (vd vừa có mạng lại): thử ngay, bỏ thời gian chờ.
#[tauri::command]
pub fn retry_media_tools(auto: tauri::State<'_, Auto>) -> AutoState {
    auto.kick();
    auto.state()
}

/// Công cụ cho lệnh yt-dlp mà người dùng đang đợi (Phân tích, liệt kê kênh): lúc công cụ đang tự tải
/// (lần đầu mở app) thì chờ yt-dlp và deno một lượt; vẫn thiếu yt-dlp thì câu lỗi nói rõ UTD tự thử lại.
pub(crate) async fn interactive_tools(auto: Option<&Auto>) -> Result<tools::MediaTools, String> {
    if let Some(auto) = auto {
        let (essential, optional) = ([sources::Tool::YtDlp], [sources::Tool::Deno]);
        let never = CancellationToken::new();
        let _ = auto.wait_for(&essential, &optional, 1, &never).await;
    }
    tauri::async_runtime::spawn_blocking(tools::resolve)
        .await
        .map_err(|error| error.to_string())?
        .map_err(|missing| match auto {
            Some(auto) => auto.missing_message(sources::Tool::YtDlp, missing),
            None => missing,
        })
}

/// Phân tích một link (hộp Tải media).
#[tauri::command]
pub async fn media_probe(
    app: AppHandle,
    url: String,
    auth_grant: Option<String>,
) -> Result<probe::MediaProbeResult, String> {
    let url = validate_url(&url).map_err(|error| error.to_string())?;
    let grant = crate::commands::signed_in(&app, auth_grant.as_deref(), url.as_str());
    let result = analyze(&app, url.as_str(), grant.as_deref()).await;
    // S-14: popup tiện ích đếm hộp này vào việc chờ đăng nhập của trang.
    if let Err(message) = &result
        && needs_sign_in(message)
        && let Some(site) = crate::auth::site::of_url(url.as_str())
    {
        app.state::<AppState>().auth.wait_dialog(&site, url.as_str());
    }
    result
}

/// Câu lỗi Phân tích nghĩa là nguồn đòi đăng nhập (cho phép trang đó là đọc được).
pub(crate) fn needs_sign_in(message: &str) -> bool {
    message.starts_with("[media.login]") || message.starts_with("[auth.")
}

/// Phân tích một link (hộp Tải media, nút trên trang). yt-dlp lỗi kiểu extractor ⇒ kiểm bản mới, có thì cài rồi
/// phân tích lại một lần.
pub(crate) async fn analyze(
    app: &AppHandle,
    url: &str,
    grant: Option<&crate::auth::Grant>,
) -> Result<probe::MediaProbeResult, String> {
    let auto = auto_tools(app);
    let mut retried = false;
    loop {
        let tools = interactive_tools(auto.as_ref()).await?;
        let ffmpeg = tools.ffmpeg.is_some()
            || auto
                .as_ref()
                .is_some_and(|auto| auto.will_install(sources::Tool::Ffmpeg));
        let failure = match probe::probe(&tools, url, ffmpeg, grant).await {
            Ok(result) => return Ok(result),
            Err(failure) => failure,
        };
        let refreshed = match &auto {
            Some(auto) if !retried && failure.outdated => {
                auto.refresh_after_failure(&tools.yt_dlp_version).await
            }
            _ => false,
        };
        if !refreshed {
            return Err(failure.message);
        }
        retried = true;
    }
}

/// Ngoài luồng giao diện: kiểm/tạo thư mục tải và đọc thư mục để tránh trùng tên đều chạm đĩa.
#[tauri::command]
pub async fn add_media_download(
    app: AppHandle,
    input: AddMediaInput,
) -> Result<DownloadTask, String> {
    let started = std::time::Instant::now();
    let task = tauri::async_runtime::spawn_blocking(move || enqueue_media(app, input))
        .await
        .map_err(|error| error.to_string())??;
    // Mốc đo: thời gian lệnh "Tải" (job đã chạy song song, log `media.timing … start`).
    let elapsed = started.elapsed().as_millis();
    log::info!("media.timing task={} queued ms={elapsed}", task.id);
    Ok(task)
}

pub(crate) fn enqueue_media(app: AppHandle, mut input: AddMediaInput) -> Result<DownloadTask, String> {
    // Video Douyin trong danh sách hồ sơ (channel/douyin.rs): tải thẳng link MP4 đã có, yt-dlp không đọc được Douyin.
    let page = channel::douyin::streams_for(&input).map(|(title, streams)| {
        if input.title.trim().is_empty() {
            input.title = title;
        }
        input.selector = page_streams::SELECTOR.into();
        input.quality = None;
        streams
    });
    enqueue_with(app, input, page)
}

/// Video đọc thẳng từ trang (`page_streams`, nút "Tải video này"): như `enqueue_media`, không cookie (link luồng đã
/// có chữ ký của trang).
pub(crate) fn enqueue_page(
    app: AppHandle,
    input: AddMediaInput,
    streams: &[page_streams::PageStream],
) -> Result<DownloadTask, String> {
    enqueue_with(app, input, Some(streams.to_vec()))
}

/// Tạo task rồi xếp hàng. `page`: luồng đọc từ trang, lưu trước khi xếp hàng để runner thấy ngay.
fn enqueue_with(
    app: AppHandle,
    mut input: AddMediaInput,
    page: Option<Vec<page_streams::PageStream>>,
) -> Result<DownloadTask, String> {
    let (journal, queue) = {
        let state = app.state::<AppState>();
        (state.journal.clone(), state.queue.clone())
    };
    let folder = crate::commands::download_dir_for(&app, input.dest_dir.as_deref())?;
    let destination = with_subfolder(folder, input.subfolder.as_deref())?;
    let place = if input.later { Place::Back } else { Place::Front };
    let pick = input.channel.take();
    let grant = match page {
        Some(_) => None,
        None => crate::commands::signed_in(&app, input.auth_grant.as_deref(), &input.url),
    };
    let task = create_media(&journal, &destination, input, grant.as_deref())?;
    if let Some(streams) = &page
        && let Err(error) = page_streams::save(&journal, &task.id, streams)
    {
        let _ = journal.delete_task(&task.id);
        return Err(error);
    }
    if let Some(grant) = &grant {
        app.state::<AppState>().auth.bind(&task.id, &grant.id);
    }
    if let Err(error) = queue.submit(vec![Entry::new(task.id.clone())], place) {
        page_streams::forget(&journal, &task.id);
        let _ = journal.delete_task(&task.id);
        return Err(error);
    }
    if let Some(pick) = &pick
        && let Err(error) = channel::remember(&journal, pick, &task.id)
    {
        log::warn!("channel: không nhớ được video đã tải ({error})");
    }
    Ok(task)
}

/// Tạo task media + job trong journal, chưa xếp hàng (`destination` đã kiểm). Lô link dùng chung.
pub(crate) fn create_media(
    journal: &Journal,
    destination: &Path,
    input: AddMediaInput,
    grant: Option<&crate::auth::Grant>,
) -> Result<DownloadTask, String> {
    let url = validate_url(&input.url).map_err(|error| error.to_string())?;
    let choice = checked_choice(&input)?;
    if let Some(page_url) = &input.page_url {
        validate_url(page_url).map_err(|error| error.to_string())?;
    }
    let title = input.title.trim();
    let title = if !title.is_empty() {
        title.to_string()
    } else if input.quality.is_some() {
        preset::placeholder_title(&url)
    } else {
        "Media".to_string()
    };
    let base = unique_base(destination, &safe_base(&title));
    let extension = choice.audio_format.as_deref().unwrap_or("mp4");
    let file_name = format!("{base}.{extension}");
    validate_file_name(&file_name).map_err(|error| error.to_string())?;

    let mut task = DownloadTask::new(
        url.to_string(),
        file_name,
        destination.to_string_lossy().into_owned(),
        1,
    )
    .map_err(|error| error.to_string())?;
    task.kind = TaskKind::Media;
    task.status = TaskStatus::Queued;
    task.hash_state = HashState::Unavailable;
    if let Some(grant) = grant {
        task.auth_sites = vec![grant.site.clone()];
    }
    task.validate().map_err(|error| error.to_string())?;
    journal.save_task(&task).map_err(|error| error.to_string())?;

    let job = MediaJob {
        task_id: task.id.clone(),
        url: url.to_string(),
        page_url: input.page_url,
        selector: choice.selector,
        audio_format: choice.audio_format,
        title: title.chars().take(300).collect(),
        created_at: now_ms(),
    };
    if let Err(error) = journal.save_media_job(&job) {
        let _ = journal.delete_task(&task.id);
        return Err(error.to_string());
    }
    Ok(task)
}

/// Thư mục tải, cộng thư mục con (tên kênh) khi tải cả kênh; tên con được làm sạch thành một tầng.
fn with_subfolder(
    folder: std::path::PathBuf,
    subfolder: Option<&str>,
) -> Result<std::path::PathBuf, String> {
    let Some(name) = subfolder.map(str::trim).filter(|name| !name.is_empty()) else {
        return Ok(folder);
    };
    let destination = folder.join(safe_base(name));
    std::fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    Ok(destination)
}

fn auto_tools(app: &AppHandle) -> Option<Auto> {
    app.try_state::<Auto>().map(|state| state.inner().clone())
}

/// Hàng đợi hỏi trước khi chạy task media (`queue::Runtime::ready`): công cụ task cần còn đang tự tải
/// (`Auto::holds_media`) ⇒ chưa chạy, task vẫn "Đang chờ lượt" mà không giữ chỗ.
pub(crate) fn waits_for_tools(app: &AppHandle, task_id: &str) -> bool {
    let Some(auto) = auto_tools(app) else {
        return false;
    };
    if auto.holds_media(false) {
        return true;
    }
    // Chỉ còn ffmpeg (bắt buộc khi tách âm thanh) đang chờ thử lại: tuỳ task.
    auto.holds_media(true)
        && app.try_state::<AppState>().is_some_and(|state| {
            state
                .journal
                .load_media_job(task_id)
                .ok()
                .flatten()
                .is_some_and(|job| job.audio_format.is_some())
        })
}

/// Task media vừa vào hàng: còn thiếu công cụ thì tự tải thử lại ngay, khỏi đợi hết thời gian chờ.
pub(crate) fn want_tools(app: &AppHandle) {
    if let Some(auto) = auto_tools(app)
        && !auto.state().pending.is_empty()
    {
        auto.kick();
    }
}

/// Công cụ tự tải đổi trạng thái (xong một lượt, lỗi, tắt, Huỷ) ⇒ hàng đợi xem lại task media đang chờ.
pub(crate) fn watch_tools_for_queue(app: &AppHandle) {
    let Some(auto) = auto_tools(app) else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut receiver = auto.subscribe();
        let mut last = None;
        loop {
            let holds = Some((auto.holds_media(false), auto.holds_media(true)));
            if holds != last
                && let Some(state) = app.try_state::<AppState>()
            {
                last = holds;
                state.queue.pump();
            }
            if receiver.changed().await.is_err() {
                return;
            }
        }
    });
}

pub(crate) fn spawn_media_run(
    app: AppHandle,
    journal: Arc<Journal>,
    running: Arc<RunningTasks>,
    task_id: String,
) -> Result<(), String> {
    let cancel = CancellationToken::new();
    if !running.insert(&task_id, cancel.clone()) {
        return Err("download is already running".into());
    }
    tauri::async_runtime::spawn(async move {
        let failure = runner::run(app.clone(), journal.clone(), task_id.clone(), cancel)
            .await
            .err();
        if let Some(message) = &failure {
            let _ = app.emit(
                ERROR_EVENT,
                BridgeError {
                    task_id: task_id.clone(),
                    message: message.clone(),
                },
            );
        }
        // Xong rồi thì không giữ cookie cho task này nữa (notes/auth-plan.md §6).
        if let Ok(Some(task)) = journal.load_task(&task_id)
            && task.status == TaskStatus::Completed
        {
            app.state::<AppState>().auth.release(&task_id);
        }
        running.remove(&task_id);
        crate::recover::after_run(&app, &task_id, failure.as_deref());
        crate::queue::finished(&app, &task_id);
    });
    Ok(())
}

pub(crate) fn std_command(program: &Path) -> Command {
    let mut command = Command::new(program);
    // Windows mặc định in theo bảng mã ANSI (cp1252…), làm hỏng tên file tiếng Việt.
    command.env("PYTHONIOENCODING", "utf-8").env("PYTHONUTF8", "1");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

pub(crate) fn tokio_command(program: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(program);
    command.env("PYTHONIOENCODING", "utf-8").env("PYTHONUTF8", "1");
    #[cfg(windows)]
    {
        command.creation_flags(0x0800_0000);
    }
    // Nhóm tiến trình riêng (pgid = pid của nó) để `kill_tree` giết được cả tiến trình con.
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    command
}

/// Dừng tiến trình cùng các tiến trình con. yt-dlp.exe và yt-dlp_macos (PyInstaller một file) chạy lại
/// chính nó thành tiến trình con: chỉ giết tiến trình ngoài thì tiến trình con vẫn liệt kê/tải tiếp và
/// giữ ống stdout/stderr, bên đọc chờ tới khi nó tự xong (nút Dừng của Tải cả kênh treo cả phút).
/// Windows: `taskkill /T` giết cả cây. macOS: `killpg` cả nhóm mà `tokio_command` tạo. Phải gọi lúc
/// tiến trình ngoài còn sống (chưa `wait`).
pub(crate) async fn kill_tree(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(raw) = child.id()
        && let Ok(pid) = libc::pid_t::try_from(raw)
    {
        // SAFETY: chỉ gửi tín hiệu tới nhóm của tiến trình con này (chưa được thu dọn nên pid chưa bị dùng lại).
        unsafe { libc::killpg(pid, libc::SIGKILL) };
        // Tiến trình ngoài đang fork đúng lúc đó (máy bận: cả trăm ms) thì tiến trình con vào nhóm sau lần
        // trên, chạy tiếp và giữ ống stdout. Tiến trình ngoài chỉ chết khi fork đã xong, tức con đã ở trong
        // nhóm: chờ nó chết mà không thu dọn (WNOWAIT ⇒ pgid chưa thể bị dùng lại) rồi gửi lần nữa.
        let exited = tokio::task::spawn_blocking(move || loop {
            // SAFETY: siginfo_t toàn số, toàn 0 hợp lệ; chỉ chờ tiến trình con của mình, để nguyên cho tokio.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let result = unsafe {
                libc::waitid(libc::P_PID, raw, &mut info, libc::WEXITED | libc::WNOWAIT)
            };
            if result == 0 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                break;
            }
        });
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), exited).await;
        // SAFETY: như trên.
        unsafe { libc::killpg(pid, libc::SIGKILL) };
    }
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        let taskkill = std::env::var_os("SystemRoot")
            .map(|root| std::path::PathBuf::from(root).join(r"System32\taskkill.exe"))
            .unwrap_or_else(|| "taskkill.exe".into());
        let _ = tokio_command(&taskkill)
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await;
    }
    let _ = child.start_kill();
}

pub(crate) fn ffmpeg_location(tools: &tools::MediaTools) -> Option<String> {
    tools::ffmpeg_directory(tools.ffmpeg.as_deref())
}

/// Mọi lệnh yt-dlp đi qua đây: cờ cô lập + runtime JavaScript, rồi nơi gọi thêm cờ riêng.
pub(crate) fn yt_dlp_command(tools: &tools::MediaTools) -> tokio::process::Command {
    let mut command = tokio_command(&tools.yt_dlp);
    command.args(isolation_args(tools)).args(js_runtime_args(tools));
    command
}

/// Lệnh yt-dlp kèm đăng nhập nếu có lần cho phép: cookie qua named pipe (`auth::pipe`, không ghi đĩa) và
/// User-Agent của trình duyệt (có trang gắn phiên với User-Agent). Người gọi chạy bằng `run_signed` (báo PID
/// cho pipe) hoặc tự `allow(pid)` ngay sau khi sinh.
pub(crate) fn yt_dlp_signed(
    tools: &tools::MediaTools,
    grant: Option<&crate::auth::Grant>,
) -> Result<(tokio::process::Command, Option<crate::auth::pipe::CookiePipe>), utd_engine::TaskError> {
    let mut command = yt_dlp_command(tools);
    let Some(grant) = grant else {
        return Ok((command, None));
    };
    let pipe = crate::auth::pipe::open(&grant.jar).map_err(|error| {
        log::warn!("cookie pipe for yt-dlp failed: {error}");
        sign_in_pipe_error()
    })?;
    command.arg("--cookies").arg(pipe.path());
    if let Some(agent) = &grant.user_agent {
        command.arg("--user-agent").arg(agent);
    }
    Ok((command, Some(pipe)))
}

/// yt-dlp không nhận được cookie (pipe lỗi, hoặc yt-dlp bỏ qua không báo): không bao giờ tải tiếp như chưa
/// đăng nhập (notes/auth-plan.md §5.1).
pub(crate) fn sign_in_pipe_error() -> utd_engine::TaskError {
    utd_engine::TaskError {
        class: utd_engine::ErrorClass::User,
        code: "auth.pipe".into(),
        message: "UTD không đưa được đăng nhập cho yt-dlp.".into(),
    }
}

/// Như `Command::output`, nhưng báo PID cho pipe cookie ngay khi sinh. `Ok((output, false))` = có pipe mà
/// yt-dlp không đọc cookie: kết quả không đáng tin.
pub(crate) async fn run_signed(
    mut command: tokio::process::Command,
    pipe: Option<&crate::auth::pipe::CookiePipe>,
) -> std::io::Result<(std::process::Output, bool)> {
    let child = command.spawn()?;
    if let (Some(pipe), Some(pid)) = (pipe, child.id()) {
        pipe.allow(pid);
    }
    let output = child.wait_with_output().await?;
    let Some(pipe) = pipe else {
        return Ok((output, true));
    };
    // Cờ bật ở tác vụ phục vụ pipe ngay khi yt-dlp đọc xong; chờ chút cho lượt rất nhanh.
    for _ in 0..15 {
        if pipe.loaded() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    Ok((output, pipe.loaded()))
}

/// Không nạp file cấu hình (của người dùng, cạnh exe, của bản cài qua WinGet/Scoop) và plugin ngoài: một
/// dòng `--print-traffic` trong `yt-dlp.conf` in lộ cả header `Cookie`, `--cookies-from-browser` tự gửi
/// cookie người dùng chưa cho phép (thử thật: notes/auth-plan.md §5.2). `--no-plugin-dirs` có từ
/// 2025.03.21; bản cũ hơn báo "no such option" nên không truyền.
fn isolation_args(tools: &tools::MediaTools) -> Vec<&'static str> {
    const FIRST_NO_PLUGIN_DIRS: &str = "2025.03.21";
    let mut args = vec!["--ignore-config"];
    if verify::is_date_version(&tools.yt_dlp_version)
        && tools.yt_dlp_version.as_str() >= FIRST_NO_PLUGIN_DIRS
    {
        args.push("--no-plugin-dirs");
    }
    args
}

/// yt-dlp từ 2025.11.12 giải thử thách của YouTube bằng runtime JavaScript (deno). Chỉ rõ deno đã
/// dò được (của UTD hoặc trên máy); bản yt-dlp cũ hơn không biết cờ này nên không truyền.
fn js_runtime_args(tools: &tools::MediaTools) -> Vec<OsString> {
    const FIRST_EJS: &str = "2025.11.12";
    let supported = verify::is_date_version(&tools.yt_dlp_version)
        && tools.yt_dlp_version.as_str() >= FIRST_EJS;
    match &tools.deno {
        Some(deno) if supported => {
            let mut runtime = OsString::from("deno:");
            runtime.push(deno);
            vec!["--js-runtimes".into(), runtime]
        }
        _ => Vec::new(),
    }
}

fn safe_base(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                ' '
            } else {
                character
            }
        })
        .collect();
    let mut base = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    if base.chars().count() > 100 {
        base = base.chars().take(100).collect::<String>();
        base = base.trim_end_matches(['.', ' ']).to_string();
    }
    if base.is_empty() {
        base = "media".into();
    }
    if validate_file_name(&format!("{base}.mp4")).is_err() {
        base = format!("_{base}");
    }
    if validate_file_name(&format!("{base}.mp4")).is_err() {
        base = "media".into();
    }
    base
}

fn unique_base(destination: &Path, base: &str) -> String {
    for counter in 0..1000 {
        let candidate = if counter == 0 {
            base.to_string()
        } else {
            format!("{base} ({counter})")
        };
        if !conflicts(destination, &candidate) {
            return candidate;
        }
    }
    format!("{base} {}", now_ms())
}

fn conflicts(destination: &Path, base: &str) -> bool {
    let prefix = format!("{base}.");
    let variant_prefix = format!("{base} (");
    let Ok(entries) = std::fs::read_dir(destination) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        name == base || name.starts_with(&prefix) || name.starts_with(&variant_prefix)
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn tools_with(version: &str, deno: Option<&str>) -> tools::MediaTools {
        tools::MediaTools {
            yt_dlp: PathBuf::from("yt-dlp.exe"),
            yt_dlp_version: version.into(),
            ffmpeg: None,
            deno: deno.map(PathBuf::from),
        }
    }

    #[test]
    fn points_new_yt_dlp_at_deno_only() {
        let deno = r"C:\UTD-Data\bin\deno.exe";
        assert_eq!(
            js_runtime_args(&tools_with("2026.10.21", Some(deno))),
            [OsString::from("--js-runtimes"), OsString::from(format!("deno:{deno}"))]
        );
        assert!(js_runtime_args(&tools_with("2026.10.21", None)).is_empty());
        // Bản trước EJS không biết cờ này: truyền vào là lỗi "no such option".
        assert!(js_runtime_args(&tools_with("2025.10.22", Some(deno))).is_empty());
        assert!(js_runtime_args(&tools_with("không rõ", Some(deno))).is_empty());
    }

    /// Mọi lệnh yt-dlp bỏ file cấu hình; bản trước 2025.03.21 (đã thử 2025.02.19) không biết
    /// `--no-plugin-dirs` nên chỉ còn `--ignore-config`.
    #[test]
    fn every_yt_dlp_command_ignores_user_config_and_plugins() {
        let deno = r"C:\UTD-Data\bin\deno.exe";
        let args = |version: &str| -> Vec<String> {
            yt_dlp_command(&tools_with(version, Some(deno)))
                .as_std()
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            args("2026.10.21"),
            ["--ignore-config", "--no-plugin-dirs", "--js-runtimes", &format!("deno:{deno}")]
        );
        assert_eq!(args("2025.03.21")[..2], ["--ignore-config", "--no-plugin-dirs"]);
        assert_eq!(args("2025.02.19"), ["--ignore-config"]);
        assert_eq!(args("không rõ"), ["--ignore-config"]);
    }

    /// Như yt-dlp.exe / yt-dlp_macos: `cmd` chạy `ping` (macOS: `sh` chạy `sleep`) làm tiến trình con,
    /// tiến trình con giữ ống stdout. Giết cả cây thì đọc stdout tới EOF ngay; chỉ giết tiến trình ngoài
    /// thì phải chờ tiến trình con tự xong (~30 s). `; :` để `sh` không `exec` thẳng `sleep`.
    #[test]
    fn kill_tree_also_stops_children_holding_the_pipe() {
        use tokio::io::AsyncReadExt;
        let (program, args) = if cfg!(windows) {
            ("cmd.exe", ["/C", "ping -n 30 127.0.0.1"])
        } else {
            ("/bin/sh", ["-c", "echo x; sleep 30; :"])
        };
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let mut child = tokio_command(Path::new(program))
                .args(args)
                .stdout(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let mut stdout = child.stdout.take().unwrap();
            let mut first = [0u8; 1];
            stdout.read_exact(&mut first).await.unwrap();
            kill_tree(&mut child).await;
            let mut rest = Vec::new();
            let drained = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                stdout.read_to_end(&mut rest),
            )
            .await;
            assert!(drained.is_ok(), "tiến trình con vẫn giữ ống stdout");
            // `kill_tree` chỉ chờ, không thu dọn: `wait` của tokio vẫn lấy được trạng thái thật.
            let status = child.wait().await.expect("tokio vẫn thu dọn được tiến trình ngoài");
            assert!(!status.success());
        });
    }
}
