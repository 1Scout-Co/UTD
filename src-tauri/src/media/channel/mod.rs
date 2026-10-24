//! Tải cả kênh: liệt kê video của kênh TikTok, kênh/playlist YouTube bằng `yt-dlp --flat-playlist`
//! (yt-dlp tự lo phân trang và tham số của từng trang; UTD không tự gọi API web có chữ ký X-Bogus/a_bogus,
//! không gửi cookie). Danh sách đẩy dần sang giao diện qua `Channel` của Tauri; từng video sau đó là một
//! task media thường (`add_media_download`, kèm [`ChannelPick`] để nhớ id đã tải).
//!
//! Nghiên cứu và lý do chọn yt-dlp: `docs/specs/2026-10-23-utd-v1/notes/tiktok-channel.md`.

mod entries;
mod profile;
mod seen;

use std::collections::{HashMap, HashSet};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use utd_engine::Journal;

use entries::{ChannelEntry, ChannelHead};
use profile::ChannelProfile;
pub use seen::{ChannelPick, remember};

use crate::media::{auto_tools, interactive_tools, js_runtime_args, probe, tokio_command, tools};
use crate::state::AppState;

/// Kênh dài hơn thì chỉ lấy chừng này video mới nhất (TikTok/YouTube trả mới nhất trước).
pub const MAX_ENTRIES: usize = 5000;
/// Không có dòng mới trong chừng này thì coi như yt-dlp treo.
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// Gom video thành từng đợt gửi giao diện (yt-dlp in ~20 video/giây).
const BATCH_SIZE: usize = 30;
const BATCH_INTERVAL: Duration = Duration::from_millis(250);
const MAX_STDERR_BYTES: usize = 64 * 1024;
/// Một dòng JSON thật ~3 KB; dòng dài bất thường thì bỏ.
const MAX_LINE_BYTES: usize = 1024 * 1024;
const LIST_ATTEMPTS: u32 = 3;
const LIST_RETRY_DELAY: Duration = Duration::from_secs(3);
const TIKTOK_REFUSED: &str = "TikTok không trả danh sách của kênh này (kênh không tồn tại hoặc đang chặn). Thử lại sau ít phút.";

/// Lượt liệt kê đang chạy: mã lượt (giao diện đặt) ⇒ (số thứ tự, token huỷ).
static LISTINGS: LazyLock<Mutex<HashMap<String, (u64, CancellationToken)>>> =
    LazyLock::new(Default::default);
static SERIAL: AtomicU64 = AtomicU64::new(0);

fn listings() -> MutexGuard<'static, HashMap<String, (u64, CancellationToken)>> {
    LISTINGS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ListingEvent {
    /// Tên/khoá kênh, đọc từ dòng đầu.
    Head {
        head: ChannelHead,
    },
    /// Ảnh đại diện, người theo dõi: có thể tới trước hoặc sau vài đợt video.
    Profile {
        profile: ChannelProfile,
    },
    Entries {
        entries: Vec<ChannelEntry>,
    },
    /// Hết lượt. `complete`: đã đi hết danh sách; `error`: câu báo khi dừng giữa chừng.
    Done {
        total: usize,
        complete: bool,
        error: Option<String>,
    },
}

/// Nơi nhận sự kiện: `Channel` của Tauri, hoặc bộ gom trong test. `false` = bên nhận đã đi (hộp đóng).
trait Sink {
    fn deliver(&self, event: ListingEvent) -> bool;
}

impl Sink for Channel<ListingEvent> {
    fn deliver(&self, event: ListingEvent) -> bool {
        self.send(event).is_ok()
    }
}

/// Bắt đầu liệt kê và trả ngay; kết quả tới qua `on_event` (cuối cùng luôn là `done`).
#[tauri::command]
pub async fn channel_list(
    app: AppHandle,
    url: String,
    listing_id: String,
    on_event: Channel<ListingEvent>,
) -> Result<(), String> {
    let id_ok = |character: char| character.is_ascii_alphanumeric() || character == '-';
    if listing_id.is_empty() || listing_id.len() > 64 || !listing_id.chars().all(id_ok) {
        return Err("Mã lượt liệt kê không hợp lệ.".into());
    }
    let listing_url = crate::media_pages::channel_listing_url(&url)
        .ok_or_else(|| "Đây không phải link kênh TikTok hoặc kênh/playlist YouTube.".to_string())?;
    // Như Phân tích: lần đầu mở app thì chờ yt-dlp/deno đang tự tải.
    let tools = Arc::new(interactive_tools(auto_tools(&app).as_ref()).await?);
    let journal = app.state::<AppState>().journal.clone();
    let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
    let cancel = CancellationToken::new();
    if let Some((_, previous)) = listings().insert(listing_id.clone(), (serial, cancel.clone())) {
        previous.cancel();
    }
    let ready = Arc::new(Notify::new());
    spawn_profile(
        tools.clone(),
        listing_url.clone(),
        on_event.clone(),
        cancel.clone(),
        ready.clone(),
    );
    tauri::async_runtime::spawn(async move {
        let request = Request {
            tools: &tools,
            journal: &journal,
            url: &listing_url,
            playlist: listing_url.contains("/playlist?"),
            cancel: &cancel,
            ready: &ready,
        };
        let done = list_with_retry(&request, &on_event).await;
        ready.notify_one();
        let _ = on_event.deliver(done);
        let mut running = listings();
        if running
            .get(&listing_id)
            .is_some_and(|(current, _)| *current == serial)
        {
            running.remove(&listing_id);
        }
    });
    Ok(())
}

/// Dừng lượt liệt kê (nút Dừng, đóng hộp). Danh sách đã nhận vẫn dùng được.
#[tauri::command]
pub fn channel_list_cancel(listing_id: String) -> bool {
    listings()
        .remove(&listing_id)
        .map(|(_, token)| token.cancel())
        .is_some()
}

/// TikTok: chờ yt-dlp đọc xong đầu danh sách (hoặc hết lượt) rồi mới gọi trang nhúng. yt-dlp cũng cần
/// trang này để tìm secUid mà máy chủ hay trả 503 "overload-protect": gọi cùng lúc thì dễ hỏng cả hai.
fn spawn_profile(
    tools: Arc<tools::MediaTools>,
    listing_url: String,
    sink: Channel<ListingEvent>,
    cancel: CancellationToken,
    ready: Arc<Notify>,
) {
    tauri::async_runtime::spawn(async move {
        if listing_url.contains("tiktok.com/") {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = ready.notified() => {}
            }
        }
        tokio::select! {
            _ = cancel.cancelled() => {}
            profile = profile::fetch(&tools, &listing_url) => {
                if let Some(profile) = profile {
                    let _ = sink.deliver(ListingEvent::Profile { profile });
                }
            }
        }
    });
}

fn listing_args(url: &str) -> Vec<&str> {
    let mut args = vec![
        "--encoding",
        "utf-8",
        "--no-warnings",
        "--flat-playlist",
        // In từng video ngay khi đọc được trang, không đợi hết danh sách.
        "--lazy-playlist",
        "--dump-json",
        "--socket-timeout",
        "20",
    ];
    if url.contains("youtube.com/") {
        // Thẻ kênh YouTube không có ngày đăng; yt-dlp ước lượng từ "3 ngày trước".
        args.extend(["--extractor-args", "youtubetab:approximate_date"]);
    }
    args.push(url);
    args
}

/// TikTok đôi khi không trả cả trang hồ sơ lẫn trang nhúng (yt-dlp: "Unable to extract secondary user
/// ID"), lần sau lại được: lượt chưa có video nào thì thử lại, nghỉ lâu dần.
async fn list_with_retry(request: &Request<'_>, sink: &impl Sink) -> ListingEvent {
    let mut attempt = 1;
    loop {
        let done = list_entries(request, sink).await;
        if attempt >= LIST_ATTEMPTS || !worth_retrying(&done) {
            return done;
        }
        log::info!(
            "channel: TikTok chưa trả danh sách, thử lại lần {}",
            attempt + 1
        );
        tokio::select! {
            _ = request.cancel.cancelled() => return done,
            _ = tokio::time::sleep(LIST_RETRY_DELAY * attempt) => {}
        }
        attempt += 1;
    }
}

fn worth_retrying(done: &ListingEvent) -> bool {
    matches!(done, ListingEvent::Done { total: 0, error: Some(error), .. } if error == TIKTOK_REFUSED)
}

/// Một lượt liệt kê: phần mọi lần thử dùng chung.
struct Request<'a> {
    tools: &'a tools::MediaTools,
    journal: &'a Journal,
    url: &'a str,
    playlist: bool,
    cancel: &'a CancellationToken,
    /// Báo phần lấy ảnh đại diện: yt-dlp đã đọc được đầu danh sách.
    ready: &'a Notify,
}

async fn list_entries(request: &Request<'_>, sink: &impl Sink) -> ListingEvent {
    let mut command = tokio_command(&request.tools.yt_dlp);
    command
        .args(js_runtime_args(request.tools))
        .args(listing_args(request.url))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return failed(format!("Không chạy được yt-dlp: {error}")),
    };
    let stderr = child
        .stderr
        .take()
        .map(|stderr| tokio::spawn(read_stderr(stderr)));
    let Some(stdout) = child.stdout.take() else {
        return failed("Không đọc được kết quả của yt-dlp.".into());
    };
    let mut listing = Listing::new(sink, request.journal, request.playlist);
    listing.ready = Some(request.ready);
    let stop = listing.read(stdout, request.cancel).await;
    if stop != Stop::Ended {
        let _ = child.start_kill();
    }
    let success = child.wait().await.is_ok_and(|status| status.success());
    let stderr = match stderr {
        Some(task) => task.await.unwrap_or_default(),
        None => String::new(),
    };
    listing.finish(stop, success, &stderr)
}

fn failed(message: String) -> ListingEvent {
    ListingEvent::Done {
        total: 0,
        complete: false,
        error: Some(message),
    }
}

/// Đọc hết stderr (không đọc thì yt-dlp kẹt khi ống đầy), chỉ giữ phần cuối để phân loại lỗi.
async fn read_stderr(mut stderr: impl AsyncRead + Unpin) -> String {
    let mut kept = Vec::new();
    let mut buffer = [0u8; 8192];
    while let Ok(read) = stderr.read(&mut buffer).await {
        if read == 0 {
            break;
        }
        kept.extend_from_slice(&buffer[..read]);
        if kept.len() > MAX_STDERR_BYTES {
            kept.drain(..kept.len() - MAX_STDERR_BYTES);
        }
    }
    String::from_utf8_lossy(&kept).into_owned()
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Stop {
    /// yt-dlp đóng stdout (xong hoặc lỗi; xem mã thoát).
    Ended,
    Cancelled,
    Stalled,
    /// Bên nhận đã đi (hộp đóng): dừng cho đỡ tốn mạng.
    Gone,
    Full,
}

/// Trạng thái một lượt: đọc từng dòng, bỏ trùng, đánh dấu đã tải, gom đợt gửi giao diện.
struct Listing<'a, S: Sink> {
    sink: &'a S,
    journal: &'a Journal,
    playlist: bool,
    head_sent: bool,
    ready: Option<&'a Notify>,
    seen: seen::Downloaded,
    ids: HashSet<String>,
    batch: Vec<ChannelEntry>,
    flushed: Instant,
    total: usize,
}

impl<'a, S: Sink> Listing<'a, S> {
    fn new(sink: &'a S, journal: &'a Journal, playlist: bool) -> Self {
        Self {
            sink,
            journal,
            playlist,
            head_sent: false,
            ready: None,
            seen: seen::Downloaded::default(),
            ids: HashSet::new(),
            batch: Vec::new(),
            flushed: Instant::now(),
            total: 0,
        }
    }

    async fn read(&mut self, stdout: impl AsyncRead + Unpin, cancel: &CancellationToken) -> Stop {
        let mut reader = BufReader::new(stdout);
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = tokio::select! {
                // Huỷ trước: đã bấm Dừng thì không nhận thêm dòng đang chờ sẵn.
                biased;
                _ = cancel.cancelled() => return Stop::Cancelled,
                read = tokio::time::timeout(IDLE_TIMEOUT, reader.read_until(b'\n', &mut line)) => read,
            };
            match read {
                Err(_) => return Stop::Stalled,
                Ok(Err(_) | Ok(0)) => return Stop::Ended,
                Ok(Ok(_)) => {}
            }
            if line.len() <= MAX_LINE_BYTES && !self.line(&String::from_utf8_lossy(&line)) {
                return Stop::Gone;
            }
            if self.total >= MAX_ENTRIES {
                return Stop::Full;
            }
        }
    }

    /// `false` khi bên nhận đã đi.
    fn line(&mut self, line: &str) -> bool {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            return true;
        };
        if !self.head_sent
            && let Some(head) = entries::parse_head(&value, self.playlist)
        {
            if let Some(key) = &head.key {
                self.seen = seen::Downloaded::load(self.journal, key);
            }
            self.head_sent = true;
            if let Some(ready) = self.ready {
                ready.notify_one();
            }
            if !self.sink.deliver(ListingEvent::Head { head }) {
                return false;
            }
        }
        let Some(mut entry) = entries::parse_entry(&value) else {
            return true;
        };
        if !self.ids.insert(entry.id.clone()) {
            return true;
        }
        entry.downloaded = self.seen.contains(&entry.id);
        self.batch.push(entry);
        self.total += 1;
        if self.batch.len() >= BATCH_SIZE || self.flushed.elapsed() >= BATCH_INTERVAL {
            return self.flush();
        }
        true
    }

    fn flush(&mut self) -> bool {
        self.flushed = Instant::now();
        if self.batch.is_empty() {
            return true;
        }
        let entries = std::mem::take(&mut self.batch);
        self.sink.deliver(ListingEvent::Entries { entries })
    }

    fn finish(mut self, stop: Stop, success: bool, stderr: &str) -> ListingEvent {
        self.flush();
        let (complete, error) = match stop {
            Stop::Ended if success => (true, None),
            Stop::Ended => (false, Some(listing_error(stderr))),
            Stop::Cancelled | Stop::Gone => (false, None),
            Stop::Stalled => (
                false,
                Some("Nguồn không trả thêm video; danh sách có thể chưa đủ.".into()),
            ),
            Stop::Full => (
                false,
                Some(format!("Chỉ liệt kê {MAX_ENTRIES} video mới nhất.")),
            ),
        };
        ListingEvent::Done {
            total: self.total,
            complete,
            error,
        }
    }
}

/// Câu báo cho lỗi lúc liệt kê; lỗi chung dùng luật của `media_probe`.
fn listing_error(stderr: &str) -> String {
    let message = stderr.to_ascii_lowercase();
    if message.contains("account is private") || message.contains("likely either private") {
        return "Tài khoản riêng tư hoặc mọi video đều riêng tư. UTD chưa dùng cookie đăng nhập nên không xem được.".into();
    }
    if message.contains("does not have any videos") {
        return "Kênh chưa đăng video nào.".into();
    }
    if message.contains("secondary user id")
        || message.contains("unable to download user webpage")
        || message.contains("same page")
    {
        return TIKTOK_REFUSED.into();
    }
    if message.contains("does not exist") || message.contains("http error 404") {
        return "Không tìm thấy kênh.".into();
    }
    probe::classify_stderr(stderr)
}

#[cfg(test)]
mod tests;
