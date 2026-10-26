//! Tải cả hồ sơ Douyin (notes/douyin.md). yt-dlp không liệt kê được hồ sơ Douyin, và từ 2026-09 Douyin chặn mọi
//! lời gọi API không có chữ ký của chính trang (UTD không tự làm a_bogus/X-Bogus/`x-secsdk-web-signature`). Nên
//! danh sách lấy ngay trong tab: người dùng chuột phải trên trang hồ sơ › Tải bằng UTD, tiện ích
//! (`extensions/*/douyin-channel.js`) lật từng trang `aweme/post` bằng `fetch` của trang rồi đẩy dần sang đây (tin
//! `channel-entries`); hộp Tải cả kênh (`channel_list`) đọc từ chỗ này thay cho yt-dlp. Mỗi video mang sẵn link MP4
//! không watermark có tiếng: lúc thêm task, [`streams_for`] đổi sang luồng đọc từ trang (`page_streams`), không chạy
//! yt-dlp, không cookie. Mọi thứ chỉ ở RAM.

use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use utd_engine::{validate_url, Journal};

use super::entries::{ChannelEntry, ChannelHead};
use super::{seen, ListingEvent, Sink};
use crate::media::page_streams::{Kind, PageStream};
use crate::media::AddMediaInput;

/// Lượt lấy danh sách cũ hơn chừng này thì bỏ (hộp mở sau đó phải bấm lại trên trang).
const KEEP: Duration = Duration::from_secs(30 * 60);
/// Không có trang danh sách mới trong chừng này thì báo danh sách có thể chưa đủ.
const IDLE: Duration = Duration::from_secs(120);
const MAX_FEEDS: usize = 16;
/// Nhớ link MP4 của chừng này video gần nhất.
const MAX_STREAMS: usize = 6000;
const MAX_TITLE_CHARS: usize = 300;

const BROWSER: &str = "[channel.douyin_browser] Mở trang hồ sơ Douyin trong trình duyệt rồi bấm chuột phải › Tải bằng UTD: danh sách được lấy ngay trong trang.";
const BLOCKED: &str = "[channel.douyin_blocked] Douyin không trả thêm danh sách (có thể cần xác minh trên trang). Mở trang hồ sơ, xác minh nếu được hỏi rồi bấm lại.";
const LOGIN: &str = "[channel.douyin_login] Douyin đòi đăng nhập để xem danh sách này. Đăng nhập trên trang Douyin rồi bấm lại.";

/// Một phần danh sách tiện ích gửi (`url` của hồ sơ đi riêng).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Part {
    #[serde(default)]
    pub entries: Vec<PartEntry>,
    #[serde(default)]
    pub author: Option<Author>,
    #[serde(default)]
    pub done: bool,
    /// `login` | `blocked`.
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Author {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PartEntry {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub timestamp: Option<i64>,
    #[serde(default)]
    pub duration_seconds: Option<u64>,
    #[serde(default)]
    pub thumbnail: Option<String>,
    #[serde(default)]
    pub variants: Vec<Variant>,
}

/// Một bản MP4 có tiếng: `height` là cạnh ngắn (1080 = 1080p).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Variant {
    pub url: String,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub h265: bool,
}

struct Feed {
    name: Option<String>,
    entries: Vec<ChannelEntry>,
    done: bool,
    error: Option<String>,
    updated: Instant,
}

impl Feed {
    fn new() -> Self {
        Self {
            name: None,
            entries: Vec::new(),
            done: false,
            error: None,
            updated: Instant::now(),
        }
    }
}

/// Link hồ sơ (dạng chuẩn của `channel_listing_url`) ⇒ lượt lấy danh sách.
static FEEDS: LazyLock<Mutex<HashMap<String, Feed>>> = LazyLock::new(Default::default);
static CHANGED: LazyLock<Notify> = LazyLock::new(Notify::new);
/// (mã video, tiêu đề, các bản MP4).
type Known = (String, String, Vec<Variant>);

/// Video có link MP4 từ danh sách hồ sơ, cũ nhất ở đầu.
static STREAMS: LazyLock<Mutex<VecDeque<Known>>> = LazyLock::new(Default::default);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Link hồ sơ Douyin dạng chuẩn, hoặc `None`.
pub(crate) fn profile_url(url: &str) -> Option<String> {
    crate::media_pages::channel_listing_url(url).filter(|url| url.starts_with("https://www.douyin.com/user/"))
}

fn video_id(url: &str) -> Option<String> {
    let url = validate_url(url.trim()).ok()?;
    let host = url.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    if host != "douyin.com" && !host.ends_with(".douyin.com") {
        return None;
    }
    let mut parts = url.path_segments()?.filter(|part| !part.is_empty());
    let kind = parts.next()?;
    let id = parts.next()?;
    let digits = (8..=25).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_digit());
    (matches!(kind, "video" | "note") && digits && parts.next().is_none()).then(|| id.to_string())
}

/// Nhận một phần danh sách. Phần đầu (rỗng, chưa xong) mở lượt mới cho hồ sơ đó.
pub(crate) fn push(url: &str, part: Part) -> Result<(), String> {
    let key = profile_url(url).ok_or("[channel.not_channel] Không phải link hồ sơ Douyin.")?;
    let start = part.entries.is_empty() && !part.done && part.error.is_none() && part.author.is_none();
    let mut kept = Vec::new();
    {
        let mut feeds = lock(&FEEDS);
        feeds.retain(|_, feed| feed.updated.elapsed() < KEEP);
        if feeds.len() >= MAX_FEEDS && !feeds.contains_key(&key) {
            let oldest = feeds.iter().min_by_key(|(_, feed)| feed.updated).map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                feeds.remove(&oldest);
            }
        }
        if start {
            feeds.insert(key.clone(), Feed::new());
        }
        let feed = feeds.entry(key).or_insert_with(Feed::new);
        feed.updated = Instant::now();
        if let Some(name) = part.author.and_then(|author| author.name).map(|name| clean(&name)).filter(|name| !name.is_empty()) {
            feed.name = Some(name);
        }
        for entry in part.entries {
            let Some(id) = video_id(&entry.url).filter(|id| *id == entry.id) else {
                continue;
            };
            if feed.entries.iter().any(|known| known.id == id) {
                continue;
            }
            let title = entry.title.as_deref().map(clean).filter(|title| !title.is_empty()).unwrap_or_else(|| id.clone());
            let variants: Vec<Variant> = entry
                .variants
                .into_iter()
                .filter(|variant| variant.url.starts_with("https://") && validate_url(&variant.url).is_ok())
                .take(4)
                .collect();
            feed.entries.push(ChannelEntry {
                id: id.clone(),
                url: format!("https://www.douyin.com/{}/{id}", if entry.url.contains("/note/") { "note" } else { "video" }),
                title: title.clone(),
                timestamp: entry.timestamp.filter(|seconds| *seconds > 0),
                duration_seconds: entry.duration_seconds,
                view_count: None,
                thumbnail: entry.thumbnail.filter(|url| validate_url(url).is_ok()),
                // Bài ảnh hoặc không có bản MP4 nào: UTD không tải được.
                blocked: variants.is_empty().then(|| "photo".to_string()),
                downloaded: false,
            });
            if !variants.is_empty() {
                kept.push((id, title, variants));
            }
        }
        if part.done {
            feed.done = true;
            feed.error = part.error.map(|error| if error == "login" { LOGIN } else { BLOCKED }.to_string());
        }
    }
    if !kept.is_empty() {
        let mut streams = lock(&STREAMS);
        for (id, title, variants) in kept {
            streams.retain(|(known, _, _)| *known != id);
            if streams.len() >= MAX_STREAMS {
                streams.pop_front();
            }
            streams.push_back((id, title, variants));
        }
    }
    CHANGED.notify_waiters();
    Ok(())
}

fn clean(text: &str) -> String {
    text.chars()
        .map(|character| if character.is_control() { ' ' } else { character })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_TITLE_CHARS)
        .collect()
}

/// Hộp Tải cả kênh của hồ sơ `url`: phát danh sách tiện ích đã/đang gửi tới khi xong. Trả sự kiện cuối (`done`).
pub(super) async fn list(journal: &Journal, url: &str, sink: &impl Sink, cancel: &CancellationToken) -> ListingEvent {
    let Some(key) = profile_url(url) else {
        return super::failed(BROWSER.into());
    };
    let seen_key = format!("douyin.com:{}", key.rsplit('/').next().unwrap_or_default());
    let downloaded = seen::Downloaded::load(journal, &seen_key);
    let mut sent = 0;
    let mut head_sent = false;
    loop {
        let changed = CHANGED.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        let snapshot = lock(&FEEDS).get(&key).map(|feed| {
            (feed.name.clone(), feed.entries[sent.min(feed.entries.len())..].to_vec(), feed.done, feed.error.clone())
        });
        let Some((name, mut fresh, done, error)) = snapshot else {
            return super::failed(BROWSER.into());
        };
        if !head_sent && (!fresh.is_empty() || done) {
            head_sent = true;
            let head = ChannelHead {
                key: Some(seen_key.clone()),
                name: name.unwrap_or_else(|| "Douyin".into()),
                handle: None,
            };
            if !sink.deliver(ListingEvent::Head { head }) {
                return stopped(sent);
            }
        }
        if !fresh.is_empty() {
            sent += fresh.len();
            for entry in &mut fresh {
                entry.downloaded = downloaded.contains(&entry.id);
            }
            if !sink.deliver(ListingEvent::Entries { entries: fresh }) {
                return stopped(sent);
            }
        }
        if done {
            return ListingEvent::Done {
                total: sent,
                complete: error.is_none(),
                error,
            };
        }
        tokio::select! {
            _ = cancel.cancelled() => return stopped(sent),
            _ = &mut changed => {}
            _ = tokio::time::sleep(IDLE) => {
                return ListingEvent::Done {
                    total: sent,
                    complete: false,
                    error: Some("[channel.partial] Nguồn không trả thêm video; danh sách có thể chưa đủ.".into()),
                };
            }
        }
    }
}

fn stopped(total: usize) -> ListingEvent {
    ListingEvent::Done {
        total,
        complete: false,
        error: None,
    }
}

/// Video Douyin có link MP4 trong danh sách hồ sơ ⇒ (tiêu đề, luồng) theo chất lượng đã chọn: `height<=720` (hoặc
/// chất lượng `720` của lô link) ⇒ bản cao nhất không quá mức đó; H.264 ⇒ bỏ bản H.265 nếu có bản H.264; còn lại bản
/// cao nhất (cùng mức thì H.264 trước — tiện ích đã xếp). Không có ⇒ `None` (đường yt-dlp như cũ).
pub(crate) fn streams_for(input: &AddMediaInput) -> Option<(String, Vec<PageStream>)> {
    let id = video_id(&input.url)?;
    let (title, variants) = lock(&STREAMS)
        .iter()
        .find(|(known, _, _)| *known == id)
        .map(|(_, title, variants)| (title.clone(), variants.clone()))?;
    let selector = input.selector.as_str();
    let limit = input
        .quality
        .as_deref()
        .and_then(|quality| quality.parse::<u32>().ok())
        .or_else(|| {
            let at = selector.find("height<=")? + "height<=".len();
            selector[at..].chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()
        });
    let mut fitting: Vec<&Variant> = variants.iter().filter(|variant| limit.is_none_or(|limit| variant.height <= limit)).collect();
    if fitting.is_empty() {
        fitting = variants.iter().rev().take(1).collect();
    }
    if selector.contains("vcodec^=avc") && fitting.iter().any(|variant| !variant.h265) {
        fitting.retain(|variant| !variant.h265);
    }
    let picked = fitting.first()?;
    let stream = PageStream {
        url: picked.url.clone(),
        kind: Kind::Both,
        codecs: None,
        size_bytes: picked.size_bytes,
    };
    Some((title, vec![stream]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE: &str = "https://www.douyin.com/user/MS4wLjABAAAAdouyinTest01?from_tab_name=main";

    fn entry(id: &str, variants: Vec<Variant>) -> PartEntry {
        PartEntry {
            id: id.into(),
            url: format!("https://www.douyin.com/video/{id}"),
            title: Some(format!("Video {id}\n#tag")),
            timestamp: Some(1_760_000_000),
            duration_seconds: Some(12),
            thumbnail: None,
            variants,
        }
    }

    fn variant(height: u32, h265: bool) -> Variant {
        Variant {
            url: format!("https://v3-web.douyinvod.com/{height}{}/video.mp4", if h265 { "h265" } else { "" }),
            height,
            size_bytes: Some(u64::from(height) * 1000),
            h265,
        }
    }

    #[derive(Default)]
    struct Collect(Mutex<Vec<ListingEvent>>);

    impl Sink for Collect {
        fn deliver(&self, event: ListingEvent) -> bool {
            self.0.lock().unwrap().push(event);
            true
        }
    }

    #[test]
    fn picks_the_variant_for_the_chosen_quality() {
        let ladder = vec![variant(1080, true), variant(720, false), variant(540, false)];
        push(
            PROFILE,
            Part {
                entries: vec![entry("7400000000000000101", ladder)],
                ..Part::default()
            },
        )
        .unwrap();
        let pick = |selector: &str, quality: Option<&str>| {
            let mut input = AddMediaInput::with_quality("https://www.douyin.com/video/7400000000000000101".into(), "best".into());
            input.quality = quality.map(str::to_string);
            input.selector = selector.into();
            streams_for(&input).map(|(title, streams)| (title, streams[0].url.clone()))
        };
        let (title, best) = pick("bv*+ba/b", None).unwrap();
        assert_eq!(title, "Video 7400000000000000101 #tag", "tiêu đề một dòng");
        assert!(best.contains("1080h265"), "cao nhất");
        assert!(pick("bv*[height<=720]+ba/b[height<=720]", None).unwrap().1.contains("/720/"));
        assert!(pick("bv*[vcodec^=avc]+ba[ext=m4a]/b", None).unwrap().1.contains("/720/"), "H.264 bỏ bản H.265");
        assert!(pick("", Some("360")).unwrap().1.contains("/540/"), "không có bản nào đủ nhỏ ⇒ bản nhỏ nhất");
        assert!(streams_for(&AddMediaInput::with_quality("https://www.douyin.com/video/7499999999999999999".into(), "best".into())).is_none());
        assert!(streams_for(&AddMediaInput::with_quality("https://www.youtube.com/watch?v=7400000000000000101".into(), "best".into())).is_none());
    }

    #[tokio::test]
    async fn streams_the_profile_list_as_the_extension_sends_it() {
        let root = std::env::temp_dir().join(format!("utd-douyin-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let journal = Journal::open(root.join("utd.db")).unwrap();
        let profile = "https://www.douyin.com/user/MS4wLjABAAAAdouyinTest02";
        let collect = Collect::default();
        assert!(matches!(
            list(&journal, "https://www.douyin.com/user/MS4wLjABAAAAnobody00", &collect, &CancellationToken::new()).await,
            ListingEvent::Done { error: Some(error), .. } if error.starts_with("[channel.douyin_browser]")
        ));

        push(profile, Part::default()).unwrap();
        let sender = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let first = Part {
                entries: vec![entry("7400000000000000201", vec![variant(720, false)]), entry("7400000000000000202", vec![])],
                author: Some(Author { name: Some("Người đăng".into()) }),
                ..Part::default()
            };
            push(profile, first).unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            let last = Part {
                entries: vec![entry("7400000000000000201", vec![variant(720, false)]), entry("7400000000000000203", vec![variant(540, false)])],
                done: true,
                error: Some("blocked".into()),
                ..Part::default()
            };
            push(profile, last).unwrap();
        });
        let done = list(&journal, profile, &collect, &CancellationToken::new()).await;
        sender.await.unwrap();
        let events = collect.0.lock().unwrap();
        let ListingEvent::Head { head } = &events[0] else { panic!("{events:?}") };
        assert_eq!(head.name, "Người đăng");
        assert_eq!(head.key.as_deref(), Some("douyin.com:MS4wLjABAAAAdouyinTest02"));
        let ids: Vec<String> = events
            .iter()
            .filter_map(|event| match event {
                ListingEvent::Entries { entries } => Some(entries.iter().map(|entry| entry.id.clone()).collect::<Vec<_>>()),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(ids, ["7400000000000000201", "7400000000000000202", "7400000000000000203"], "không trùng");
        let ListingEvent::Entries { entries } = &events[1] else { panic!() };
        assert!(entries[1].blocked.is_some(), "không có MP4 ⇒ không tải được");
        assert!(matches!(done, ListingEvent::Done { total: 3, complete: false, error: Some(error) } if error.starts_with("[channel.douyin_blocked]")));
        drop(events);
        drop(journal);
        let _ = std::fs::remove_dir_all(root);
    }
}
