//! Tải hàng loạt Facebook / Instagram / Threads (`docs/specs/2026-10-23-utd-v1/notes/social-bulk.md`).
//!
//! Tiện ích liệt kê ảnh/video ngay trong trang người dùng đã đăng nhập (không xuất cookie) và gửi từng đợt qua
//! native host (`social-list`). App giữ danh sách trong RAM, mở hộp Tải cả kênh ở chế độ hàng loạt và đẩy mục
//! sang đó; mỗi đợt app trả lời tiện ích nên đi tiếp hay dừng (nút Dừng, đóng hộp) và loại nào còn cần lấy, để
//! khỏi gọi trang thừa (giảm nguy cơ bị chặn tạm).

pub(crate) mod download;
mod entries;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager};
use utd_engine::Journal;

use entries::{Message, Platform, SocialEntry, SocialHead, SocialProfile, Source};

use crate::media::channel::Downloaded;
use crate::state::AppState;

/// Một hồ sơ tối đa chừng này mục (tiện ích cũng dừng ở đây).
const MAX_ENTRIES: usize = 20_000;
/// Giữ tối đa chừng này danh sách; danh sách không đụng tới quá lâu thì bỏ.
const MAX_LISTINGS: usize = 8;
const IDLE_EXPIRY: Duration = Duration::from_secs(30 * 60);
/// Gửi lại danh sách cho hộp vừa mở theo từng đợt.
const REPLAY_CHUNK: usize = 250;
pub const SOCIAL_REQUEST_EVENT: &str = "utd://social-request";
/// Câu trả cho tiện ích ở đợt đầu.
const SOCIAL_NOTE: &str = "Đã mở UTD để chọn ảnh/video";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SocialEvent {
    Head { head: SocialHead },
    Profile { profile: SocialProfile },
    Entries { entries: Vec<SocialEntry> },
    Done { total: usize, complete: bool, error: Option<String> },
}

/// Giao diện mở hộp với danh sách này.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialRequest {
    pub listing_id: String,
    pub url: String,
    pub platform: Platform,
}

/// Điều khiển từ hộp, trả cho tiện ích mỗi đợt.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Control {
    go: bool,
    /// Loại còn cần lấy; `None` = mọi loại.
    sources: Option<Vec<Source>>,
    /// Mục cũ hơn (giây Unix) thì thôi phân trang tiếp.
    since: Option<i64>,
}

impl Control {
    const STOP: Self = Self {
        go: false,
        sources: None,
        since: None,
    };
}

type Sink = Arc<dyn Fn(SocialEvent) -> bool + Send + Sync>;

struct Listing {
    request: SocialRequest,
    head: Option<SocialHead>,
    profile: Option<SocialProfile>,
    entries: Vec<SocialEntry>,
    ids: HashSet<String>,
    seen: Downloaded,
    control: Control,
    sink: Option<Sink>,
    finish: Option<entries::Finish>,
    touched: Instant,
}

impl Listing {
    fn deliver(&mut self, event: SocialEvent) {
        if let Some(sink) = &self.sink
            && !sink(event)
        {
            // Hộp đã đóng mà chưa kịp báo: thôi lấy tiếp.
            self.sink = None;
            self.control = Control::STOP;
        }
    }

    fn done_event(&self) -> Option<SocialEvent> {
        self.finish.as_ref().map(|finish| SocialEvent::Done {
            total: self.entries.len(),
            complete: finish.complete,
            error: finish.error.clone(),
        })
    }
}

#[derive(Default)]
struct Registry {
    listings: HashMap<String, Listing>,
    /// Đợt đầu tới lúc giao diện chưa nghe sự kiện (UTD vừa mở): giữ lại, `take_social_request` đọc.
    pending: Option<SocialRequest>,
    ui_ready: bool,
}

/// Kết quả nhận một đợt.
struct Received {
    reply: Control,
    /// Đợt đầu của danh sách mới: mở hộp.
    opened: Option<SocialRequest>,
    /// Đợt cuối của tiện ích: đưa UTD lên trước (lúc còn thu thì không, để tab trên trình duyệt không bị che).
    finished: bool,
}

impl Registry {
    fn receive(&mut self, message: Message, journal: &Journal, now: Instant) -> Received {
        self.listings
            .retain(|_, listing| now.duration_since(listing.touched) < IDLE_EXPIRY);
        let mut opened = None;
        if !self.listings.contains_key(&message.listing_id) {
            // Đợt sau của danh sách đã bỏ (hộp đóng, hết hạn): bảo tiện ích dừng.
            if message.part > 0 {
                return Received {
                    reply: Control::STOP,
                    opened: None,
                    finished: false,
                };
            }
            if self.listings.len() >= MAX_LISTINGS
                && let Some(oldest) = self
                    .listings
                    .iter()
                    .min_by_key(|(_, listing)| listing.touched)
                    .map(|(id, _)| id.clone())
            {
                self.listings.remove(&oldest);
            }
            let request = SocialRequest {
                listing_id: message.listing_id.clone(),
                url: message.url.clone(),
                platform: message.platform,
            };
            opened = Some(request.clone());
            self.listings.insert(
                message.listing_id.clone(),
                Listing {
                    request,
                    head: None,
                    profile: None,
                    entries: Vec::new(),
                    ids: HashSet::new(),
                    seen: Downloaded::default(),
                    control: Control {
                        go: true,
                        sources: None,
                        since: None,
                    },
                    sink: None,
                    finish: None,
                    touched: now,
                },
            );
        }
        let Some(listing) = self.listings.get_mut(&message.listing_id) else {
            return Received {
                reply: Control::STOP,
                opened: None,
                finished: false,
            };
        };
        listing.touched = now;
        if listing.request.platform != message.platform {
            return Received {
                reply: Control::STOP,
                opened,
                finished: false,
            };
        }
        if listing.head.is_none()
            && let Some(head) = message.head
        {
            if let Some(key) = &head.key {
                listing.seen = Downloaded::load(journal, key);
            }
            listing.head = Some(head.clone());
            listing.deliver(SocialEvent::Head { head });
        }
        if let Some(profile) = message.profile.filter(|_| listing.profile.is_none()) {
            listing.profile = Some(profile.clone());
            listing.deliver(SocialEvent::Profile { profile });
        }
        let mut fresh = Vec::new();
        for mut entry in message.entries {
            if listing.entries.len() >= MAX_ENTRIES {
                break;
            }
            // Cùng ảnh/video có thể hiện ở hai nhóm (reels cũng nằm trong bài viết): giữ lần đầu.
            if !listing.ids.insert(entry.id.clone()) {
                continue;
            }
            entry.downloaded = listing.seen.contains(&entry.id);
            listing.entries.push(entry.clone());
            fresh.push(entry);
        }
        if !fresh.is_empty() {
            listing.deliver(SocialEvent::Entries { entries: fresh });
        }
        let full = listing.entries.len() >= MAX_ENTRIES;
        let finished = message.finish.is_some();
        let finish = match message.finish {
            Some(finish) => Some(finish),
            None if full => Some(entries::Finish {
                complete: false,
                error: Some(entries::finish_error("social.limited")),
            }),
            None => None,
        };
        if let Some(finish) = finish.filter(|_| listing.finish.is_none()) {
            listing.finish = Some(finish);
            if let Some(done) = listing.done_event() {
                listing.deliver(done);
            }
        }
        let reply = if listing.finish.is_some() {
            Control::STOP
        } else {
            listing.control.clone()
        };
        Received {
            reply,
            opened,
            finished,
        }
    }

    /// Hộp mở: gửi lại phần đã có rồi nhận tiếp trực tiếp.
    fn watch(&mut self, listing_id: &str, sink: Sink) -> Result<(), String> {
        let listing = self
            .listings
            .get_mut(listing_id)
            .ok_or_else(|| "[social.gone] Danh sách đã hết hạn. Bấm lại trên trang.".to_string())?;
        listing.touched = Instant::now();
        let mut events = Vec::new();
        if let Some(head) = &listing.head {
            events.push(SocialEvent::Head { head: head.clone() });
        }
        if let Some(profile) = &listing.profile {
            events.push(SocialEvent::Profile {
                profile: profile.clone(),
            });
        }
        for chunk in listing.entries.chunks(REPLAY_CHUNK) {
            events.push(SocialEvent::Entries {
                entries: chunk.to_vec(),
            });
        }
        events.extend(listing.done_event());
        listing.sink = Some(sink);
        for event in events {
            listing.deliver(event);
        }
        Ok(())
    }

    fn control(&mut self, listing_id: &str, control: Control) -> bool {
        let Some(listing) = self.listings.get_mut(listing_id) else {
            return false;
        };
        listing.touched = Instant::now();
        if !listing.control.go {
            return true;
        }
        listing.control = control;
        if !listing.control.go && listing.finish.is_none() {
            // Dừng: hộp hiện "đã dừng" ngay, không chờ đợt sau của tiện ích.
            listing.finish = Some(entries::Finish {
                complete: false,
                error: None,
            });
            if let Some(done) = listing.done_event() {
                listing.deliver(done);
            }
        }
        true
    }

    fn entries(&self, listing_id: &str) -> Option<(Option<String>, Vec<SocialEntry>)> {
        let listing = self.listings.get(listing_id)?;
        let key = listing.head.as_ref().and_then(|head| head.key.clone());
        Some((key, listing.entries.clone()))
    }
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(Default::default);

fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn now_seconds() -> i64 {
    utd_engine::now_ms() / 1000
}

/// Một đợt từ tiện ích (pipe `social-list`). Trả lời: `{go, sources, since}` + câu báo ở đợt đầu.
pub fn receive(app: &AppHandle, url: &str, social: &Value) -> Result<(Value, Option<&'static str>), String> {
    let message = entries::parse_message(url, social, now_seconds())?;
    let journal = app.state::<AppState>().journal.clone();
    let (received, emit) = {
        let mut registry = registry();
        let received = registry.receive(message, &journal, Instant::now());
        let mut emit = None;
        if let Some(request) = &received.opened {
            if registry.ui_ready {
                emit = Some(request.clone());
            } else {
                registry.pending = Some(request.clone());
            }
        }
        (received, emit)
    };
    let note = received.opened.as_ref().map(|_| SOCIAL_NOTE);
    if received.finished
        && let Some(window) = app.get_webview_window("main")
    {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    if let Some(request) = emit
        && let Err(error) = app.emit(SOCIAL_REQUEST_EVENT, request)
    {
        log::warn!("social request event failed: {error}");
    }
    let reply = serde_json::to_value(&received.reply).map_err(|error| error.to_string())?;
    Ok((reply, note))
}

/// Giao diện vừa nghe `utd://social-request`: danh sách tới trước đó (nếu có).
#[tauri::command]
pub fn take_social_request() -> Option<SocialRequest> {
    let mut registry = registry();
    registry.ui_ready = true;
    registry.pending.take()
}

/// Hộp mở: nhận danh sách (phần đã có gửi lại ngay, phần sau tới dần). Sự kiện cuối là `done`.
#[tauri::command]
pub fn social_watch(listing_id: String, on_event: Channel<SocialEvent>) -> Result<(), String> {
    let sink: Sink = Arc::new(move |event| on_event.send(event).is_ok());
    registry().watch(&listing_id, sink)
}

/// Hộp đổi lựa chọn: `go = false` là Dừng (giữ phần đã có); `sources`/`since` để tiện ích bỏ phần không cần.
#[tauri::command]
pub fn social_control(
    listing_id: String,
    go: bool,
    sources: Option<Vec<Source>>,
    since: Option<i64>,
) -> bool {
    registry().control(&listing_id, Control { go, sources, since })
}

/// Hộp đóng: bỏ danh sách, tiện ích dừng ở đợt sau.
#[tauri::command]
pub fn social_close(listing_id: String) -> bool {
    registry().listings.remove(&listing_id).is_some()
}

#[cfg(test)]
mod tests;
