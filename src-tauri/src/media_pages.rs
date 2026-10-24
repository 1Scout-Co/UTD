//! Nhận ra link trang video (YouTube, TikTok, Facebook…). Link kiểu này là trang HTML: tải bằng HTTP chỉ
//! được tệp "watch" rỗng (AC-06), nên phải mở luồng media (yt-dlp) để chọn chất lượng.
//!
//! Hai lối vào dùng chung luật này:
//! - `add_download` (hộp Thêm): trả lỗi bắt đầu bằng [`MEDIA_PAGE_ERROR`] (`media_page: …`); giao diện thấy mã
//!   này thì đóng hộp Thêm và mở hộp Tải media với link đó.
//! - Named pipe (chuột phải / popup của extension): không tạo task, phát [`crate::events::MEDIA_REQUEST_EVENT`]
//!   cho giao diện, hiện + focus cửa sổ chính. Giao diện chưa kịp nghe (UTD vừa mở) thì giữ link trong
//!   [`PendingMedia`], giao diện lấy bằng `take_media_request` khi sẵn sàng.
//!
//! Link kênh/playlist (TikTok `@user`, kênh và playlist YouTube) đi cùng hai lối vào đó nhưng mở hộp Tải cả
//! kênh: `add_download` trả [`CHANNEL_PAGE_ERROR`] kèm link, pipe phát cùng sự kiện và giao diện tự nhận ra.
//!
//! Bản TS cùng luật: `src/features/home/model/media-pages.ts`. Sửa một bên thì sửa cả bên kia (cùng bộ ca thử).

use std::sync::{Mutex, MutexGuard};

use tauri::{AppHandle, Emitter, Manager, State};

use crate::events::{MediaRequest, MEDIA_REQUEST_EVENT};

/// Mã đầu chuỗi lỗi của `add_download` khi URL là trang video.
pub const MEDIA_PAGE_ERROR: &str = "media_page";

/// Câu trả cho extension khi link được chuyển sang hộp Tải media.
pub const MEDIA_PAGE_NOTE: &str = "Đã mở UTD để chọn chất lượng";

const MAX_URL_BYTES: usize = 8192;

pub fn media_page_error() -> String {
    format!("{MEDIA_PAGE_ERROR}: Đây là link video, dùng Tải video để chọn chất lượng.")
}

/// Mã đầu chuỗi lỗi khi URL là trang kênh/playlist: `channel_page: <link>`, giao diện mở hộp Tải cả kênh
/// với link đó.
pub const CHANNEL_PAGE_ERROR: &str = "channel_page";

/// Câu trả cho extension khi link kênh được chuyển sang hộp Tải cả kênh.
pub const CHANNEL_PAGE_NOTE: &str = "Đã mở UTD để tải cả kênh";

pub fn channel_page_error(url: &str) -> String {
    format!("{CHANNEL_PAGE_ERROR}: {}", url.trim())
}

/// Link kênh lẫn trong lô (hộp Thêm hàng loạt, chất lượng chung): lô không liệt kê kênh, chỉ báo cách làm.
pub const CHANNEL_IN_BATCH: &str = "[batch.channel_link] Đây là link kênh: dán vào ô Thêm URL để chọn video của kênh.";

/// Câu lỗi là mã `channel_page:` (Phân tích thấy link rút gọn trỏ tới kênh).
pub fn is_channel_page_error(message: &str) -> bool {
    message.starts_with(CHANNEL_PAGE_ERROR)
}

/// Thẻ của kênh YouTube liệt kê được video (`/@kênh/videos`); `featured` là trang chủ kênh.
const YOUTUBE_TABS: &[&str] = &["videos", "shorts", "streams", "featured"];

/// Đuôi tệp tải thẳng: `…/x.mp4` là tệp, không phải trang video.
const FILE_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "webm", "mkv", "mov", "avi", "flv", "ts", "m3u8", "mpd", "mp3", "m4a", "aac", "opus",
    "ogg", "wav", "flac", "jpg", "jpeg", "png", "gif", "webp", "zip", "rar", "7z", "exe", "msi", "iso",
    "pdf",
];

/// Trang của SoundCloud không phải người đăng (`soundcloud.com/discover/…`).
const SOUNDCLOUD_PAGES: &[&str] = &[
    "discover", "stream", "search", "upload", "you", "charts", "settings", "messages", "notifications",
    "pages", "people", "terms-of-use", "mobile", "jobs", "imprint", "pro", "tags", "stations", "feed",
    "signin", "login", "logout", "connect", "artists", "creators", "stats",
];

/// Tên miền của trang bài SoundCloud (như yt-dlp); `help.`, `developers.`… là trang tài liệu.
const SOUNDCLOUD_HOSTS: &[&str] = &["soundcloud.com", "www.soundcloud.com", "m.soundcloud.com"];

/// Thẻ trong trang người đăng (`soundcloud.com/<user>/likes`), không phải bài.
const SOUNDCLOUD_TABS: &[&str] = &[
    "sets", "likes", "tracks", "albums", "reposts", "followers", "following", "popular-tracks", "comments",
    "spotlight", "toptracks",
];

struct Page<'a> {
    host: &'a str,
    path: Vec<&'a str>,
    /// Có `?v=<id>` (YouTube, Facebook).
    video_param: bool,
    /// Có `?list=<id>` (playlist YouTube).
    list_param: bool,
}

impl Page<'_> {
    fn at(&self, index: usize) -> &str {
        self.path.get(index).copied().unwrap_or("")
    }

    fn has(&self, index: usize) -> bool {
        self.path.len() > index
    }
}

/// Luật đường dẫn của một trang: đúng khi đường dẫn là trang xem một video/bài.
type Rule = fn(&Page<'_>) -> bool;

/// (tên miền, luật đường dẫn). Tên miền cụ thể (vm.tiktok.com) đứng trước tên miền chung (tiktok.com).
/// Tên miền khớp cả tên miền con: `youtube.com` gồm `www.`, `m.`, `music.`.
const SITES: &[(&str, Rule)] = &[
    ("youtu.be", short_link),
    ("youtube.com", youtube),
    ("youtube-nocookie.com", youtube),
    ("vm.tiktok.com", short_link),
    ("vt.tiktok.com", short_link),
    ("tiktok.com", tiktok),
    ("fb.watch", short_link),
    ("facebook.com", facebook),
    ("instagram.com", instagram),
    ("x.com", twitter),
    ("twitter.com", twitter),
    ("player.vimeo.com", vimeo_player),
    ("vimeo.com", vimeo),
    ("dai.ly", short_link),
    ("dailymotion.com", dailymotion),
    ("clips.twitch.tv", short_link),
    ("twitch.tv", twitch),
    ("on.soundcloud.com", short_link),
    ("soundcloud.com", soundcloud),
    ("b23.tv", short_link),
    ("bilibili.com", bilibili),
];

/// Trang kênh/playlist tải được cả loạt. Link rút gọn (`vm.tiktok.com`) chưa biết là video hay kênh: đi
/// đường trang video, `media_probe` thấy là kênh thì trả [`CHANNEL_PAGE_ERROR`].
const CHANNEL_SITES: &[(&str, Rule)] = &[("tiktok.com", tiktok_user), ("youtube.com", youtube_channel)];

/// Link là trang xem video/nghe nhạc của một trang media lớn (không phải trang chủ, kênh hay tệp trực tiếp).
pub fn is_media_page(raw: &str) -> bool {
    matches_site(raw, SITES)
}

/// Link là trang kênh TikTok (`tiktok.com/@user`) hoặc kênh/playlist YouTube.
pub fn is_channel_page(raw: &str) -> bool {
    matches_site(raw, CHANNEL_SITES)
}

fn matches_site(raw: &str, sites: &[(&str, Rule)]) -> bool {
    let Ok(url) = utd_engine::validate_url(raw.trim()) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let path: Vec<&str> = url
        .path_segments()
        .map(|parts| parts.filter(|part| !part.is_empty()).collect())
        .unwrap_or_default();
    if path.last().is_some_and(|last| is_file_name(last)) {
        return false;
    }
    let param = |name: &str| {
        url.query_pairs()
            .any(|(key, value)| key == name && !value.is_empty())
    };
    let page = Page {
        host: &host,
        path,
        video_param: param("v"),
        list_param: param("list"),
    };
    sites
        .iter()
        .find(|(domain, _)| on_domain(&host, domain))
        .is_some_and(|(_, rule)| rule(&page))
}

/// Link để yt-dlp liệt kê: bỏ tham số thừa (`?lang=…`), trang chủ kênh YouTube ⇒ thẻ Video (trang chủ
/// chỉ là danh sách các thẻ). `None` khi không phải trang kênh.
pub fn channel_listing_url(raw: &str) -> Option<String> {
    if !is_channel_page(raw) {
        return None;
    }
    let mut url = utd_engine::validate_url(raw.trim()).ok()?;
    let host = url.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    let list = url
        .query_pairs()
        .find(|(key, _)| key == "list")
        .map(|(_, value)| value.into_owned());
    url.set_query(None);
    url.set_fragment(None);
    let path: Vec<String> = url
        .path_segments()
        .map(|parts| parts.filter(|part| !part.is_empty()).map(str::to_string).collect())
        .unwrap_or_default();
    if on_domain(&host, "youtube.com") {
        if path.first().is_some_and(|first| first == "playlist") {
            url.query_pairs_mut().append_pair("list", list.as_deref()?);
            return Some(url.to_string());
        }
        let owner = if path.first().is_some_and(|first| first.starts_with('@')) { 1 } else { 2 };
        let mut parts = path[..owner].to_vec();
        let tab = path.get(owner).map(String::as_str).unwrap_or("featured");
        parts.push(if tab == "featured" { "videos" } else { tab }.to_string());
        url.set_path(&parts.join("/"));
        return Some(url.to_string());
    }
    url.set_path(&path.join("/"));
    Some(url.to_string())
}

/// `host` là `domain` hoặc tên miền con của nó (`notyoutube.com` không tính).
fn on_domain(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|rest| rest.ends_with('.'))
}

fn is_file_name(part: &str) -> bool {
    part.rsplit_once('.').is_some_and(|(stem, extension)| {
        !stem.is_empty() && FILE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
    })
}

fn is_digits(part: &str) -> bool {
    !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
}

fn short_link(page: &Page<'_>) -> bool {
    page.has(0)
}

fn youtube(page: &Page<'_>) -> bool {
    (page.at(0) == "watch" && page.video_param)
        || (matches!(page.at(0), "shorts" | "live" | "embed" | "v" | "clip") && page.has(1))
}

fn tiktok(page: &Page<'_>) -> bool {
    (page.at(0).starts_with('@') && page.at(1) == "video" && page.has(2))
        || (page.at(0) == "t" && page.has(1))
}

/// `@user` đứng một mình (không phải `/video/…`, `/live`).
fn tiktok_user(page: &Page<'_>) -> bool {
    page.path.len() == 1 && is_handle(page.at(0))
}

/// `@kênh`, `channel/<id>`, `c/<tên>`, `user/<tên>` (kèm thẻ Video/Shorts/Trực tiếp) hoặc `playlist?list=`.
fn youtube_channel(page: &Page<'_>) -> bool {
    if page.at(0) == "playlist" {
        return page.list_param && page.path.len() == 1;
    }
    let owner = if is_handle(page.at(0)) {
        1
    } else if matches!(page.at(0), "channel" | "c" | "user") && page.has(1) {
        2
    } else {
        return false;
    };
    match page.path.len() - owner {
        0 => true,
        1 => YOUTUBE_TABS.contains(&page.at(owner)),
        _ => false,
    }
}

fn is_handle(part: &str) -> bool {
    part.len() > 1 && part.starts_with('@')
}

fn facebook(page: &Page<'_>) -> bool {
    (matches!(page.at(0), "watch" | "video.php") && page.video_param)
        || (page.at(0) == "reel" && page.has(1))
        || (page.at(0) == "share" && matches!(page.at(1), "v" | "r") && page.has(2))
        || page.path.windows(2).any(|pair| pair[0] == "videos")
}

fn instagram(page: &Page<'_>) -> bool {
    let kind = |index: usize| matches!(page.at(index), "reel" | "reels" | "p" | "tv");
    (kind(0) && page.has(1)) || (kind(1) && page.has(2))
}

fn twitter(page: &Page<'_>) -> bool {
    page.at(1) == "status" && is_digits(page.at(2))
}

fn vimeo_player(page: &Page<'_>) -> bool {
    page.at(0) == "video" && is_digits(page.at(1))
}

fn vimeo(page: &Page<'_>) -> bool {
    let last = page.path.last().copied().unwrap_or("");
    is_digits(page.at(0)) || (matches!(page.at(0), "channels" | "groups") && is_digits(last))
}

fn dailymotion(page: &Page<'_>) -> bool {
    page.at(0) == "video" && page.has(1)
}

fn twitch(page: &Page<'_>) -> bool {
    (page.at(0) == "videos" && page.has(1)) || (page.at(1) == "clip" && page.has(2))
}

fn soundcloud(page: &Page<'_>) -> bool {
    // `/<user>/<track>` hoặc link riêng tư `/<user>/<track>/s-<mã>`.
    let private = page.path.len() == 3 && page.at(2).starts_with("s-");
    SOUNDCLOUD_HOSTS.contains(&page.host)
        && (page.path.len() == 2 || private)
        && !SOUNDCLOUD_PAGES.contains(&page.at(0))
        && !SOUNDCLOUD_TABS.contains(&page.at(1))
}

fn bilibili(page: &Page<'_>) -> bool {
    page.at(0) == "video" && page.has(1)
}

/// `pageUrl` từ trình duyệt chỉ để tham khảo: sai hoặc quá dài thì bỏ, không từ chối cả yêu cầu.
pub fn clean_page_url(page_url: Option<String>) -> Option<String> {
    page_url
        .map(|value| value.trim().to_string())
        .filter(|value| value.len() <= MAX_URL_BYTES && utd_engine::validate_url(value).is_ok())
}

/// Link từ trình duyệt tới lúc giao diện chưa nghe `utd://media-request` (WebView2 còn đang mở, sự kiện
/// không được giữ lại): giữ link mới nhất tới khi giao diện hỏi. Sau lần hỏi đầu, giao diện đã nghe ⇒
/// phát thẳng.
#[derive(Default)]
pub struct PendingMedia(Mutex<Pending>);

#[derive(Default)]
struct Pending {
    ready: bool,
    request: Option<MediaRequest>,
}

impl PendingMedia {
    /// Giao diện chưa sẵn sàng ⇒ giữ lại (thay link cũ) và trả `true`.
    fn hold(&self, request: &MediaRequest) -> bool {
        let mut pending = self.lock();
        if !pending.ready {
            pending.request = Some(request.clone());
        }
        !pending.ready
    }

    /// Giao diện đã nghe sự kiện: lấy link đang giữ (nếu có); từ nay phát thẳng.
    fn take(&self) -> Option<MediaRequest> {
        let mut pending = self.lock();
        pending.ready = true;
        pending.request.take()
    }

    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Giao diện vừa nghe `utd://media-request`: link trình duyệt gửi tới trước đó (nếu có).
#[tauri::command]
pub fn take_media_request(pending: State<'_, PendingMedia>) -> Option<MediaRequest> {
    pending.take()
}

/// Đưa link trang video sang giao diện: hiện + focus cửa sổ chính rồi phát `utd://media-request`.
pub fn open_media_flow(app: &AppHandle, request: MediaRequest) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    if app
        .try_state::<PendingMedia>()
        .is_some_and(|pending| pending.hold(&request))
    {
        return;
    }
    if let Err(error) = app.emit(MEDIA_REQUEST_EVENT, request) {
        log::warn!("Media request event failed: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cùng bộ ca với `e2e/media-routing.spec.ts` (bản TS).
    const MEDIA_PAGES: &[&str] = &[
        "https://www.youtube.com/watch?v=09OGSRptb0M",
        "https://youtube.com/watch?v=09OGSRptb0M&t=30s",
        "https://m.youtube.com/watch?v=09OGSRptb0M",
        "https://music.youtube.com/watch?v=09OGSRptb0M&list=RDAMVM09OGSRptb0M",
        "https://www.youtube.com/shorts/abcDEF12345",
        "https://www.youtube.com/live/abcDEF12345?feature=share",
        "https://www.youtube.com/embed/abcDEF12345",
        "https://www.youtube.com/clip/UgkxAbc123",
        "https://WWW.YOUTUBE.COM./watch?v=09OGSRptb0M",
        "  https://www.youtube.com/watch?v=09OGSRptb0M  ",
        "https://youtu.be/09OGSRptb0M",
        "https://youtu.be/09OGSRptb0M?si=abc",
        "https://www.tiktok.com/@user.name/video/7234567890123456789",
        "https://vm.tiktok.com/ZMabc123/",
        "https://vt.tiktok.com/ZSabc123/",
        "https://www.facebook.com/somepage/videos/1234567890/",
        "https://www.facebook.com/watch/?v=1234567890",
        "https://m.facebook.com/watch?v=1234567890",
        "https://www.facebook.com/reel/1234567890",
        "https://fb.watch/abcDEF/",
        "https://www.instagram.com/reel/Cabc123/",
        "https://www.instagram.com/reels/Cabc123/",
        "https://www.instagram.com/p/Cabc123/",
        "https://www.instagram.com/tv/Cabc123/",
        "https://x.com/someone/status/1234567890123456789",
        "https://twitter.com/someone/status/1234567890123456789?s=20",
        "https://mobile.twitter.com/someone/status/123",
        "https://vimeo.com/123456789",
        "https://player.vimeo.com/video/123456789",
        "https://www.dailymotion.com/video/x8abcd1",
        "https://dai.ly/x8abcd1",
        "https://www.twitch.tv/videos/1234567890",
        "https://clips.twitch.tv/FunnyClipSlug",
        "https://www.twitch.tv/streamer/clip/FunnyClipSlug",
        "https://soundcloud.com/artist-name/track-name",
        "https://m.soundcloud.com/artist-name/track-name",
        "https://www.bilibili.com/video/BV1xx411c7mD",
    ];

    const OTHER_URLS: &[&str] = &[
        "https://www.youtube.com/",
        "https://www.youtube.com/@SomeChannel",
        "https://www.youtube.com/@SomeChannel/videos",
        "https://www.youtube.com/channel/UC1234567890",
        "https://www.youtube.com/watch",
        "https://www.youtube.com/watch?v=",
        "https://www.youtube.com/playlist?list=PL123",
        "https://youtu.be/",
        "https://www.tiktok.com/",
        "https://www.tiktok.com/@user.name",
        "https://www.facebook.com/",
        "https://www.facebook.com/somepage",
        "https://www.facebook.com/watch/",
        "https://www.facebook.com/somepage/videos/",
        "https://www.facebook.com/somepage/videos/clip.mp4",
        "https://www.instagram.com/someone/",
        "https://www.instagram.com/reels/",
        "https://x.com/someone",
        "https://x.com/home",
        "https://vimeo.com/",
        "https://vimeo.com/someuser",
        "https://www.dailymotion.com/someuser",
        "https://www.twitch.tv/streamer",
        "https://soundcloud.com/artist-name",
        "https://soundcloud.com/artist-name/likes",
        "https://soundcloud.com/artist-name/sets/my-playlist",
        "https://soundcloud.com/discover/sets",
        "https://soundcloud.com/stats/overview",
        "https://help.soundcloud.com/hc/en-us",
        "https://developers.soundcloud.com/docs/api",
        "https://www.bilibili.com/",
        "https://example.com/videos/clip.mp4",
        "https://example.com/releases/app-1.2.3.zip",
        "https://cdn.example.com/watch?v=abc",
        "https://rr3---sn-8qj-i5oed.googlevideo.com/videoplayback?expire=1&itag=399",
        "https://notyoutube.com/watch?v=abc",
        "https://youtube.com.evil.example/watch?v=abc",
        "https://user:pass@www.youtube.com/watch?v=abc",
        "ftp://www.youtube.com/watch?v=abc",
        "www.youtube.com/watch?v=abc",
        "not a url",
        "",
    ];

    /// Cùng bộ ca với `e2e/channel-download.spec.ts` (bản TS).
    const CHANNEL_PAGES: &[&str] = &[
        "https://www.tiktok.com/@tiktok",
        "https://www.tiktok.com/@user.name?lang=en&is_from_webapp=1",
        "https://m.tiktok.com/@user.name/",
        "https://www.youtube.com/@SomeChannel",
        "https://www.youtube.com/@SomeChannel/videos",
        "https://www.youtube.com/@SomeChannel/shorts",
        "https://www.youtube.com/@SomeChannel/streams",
        "https://m.youtube.com/@SomeChannel/featured",
        "https://www.youtube.com/channel/UC1234567890",
        "https://www.youtube.com/channel/UC1234567890/videos",
        "https://www.youtube.com/c/SomeName",
        "https://www.youtube.com/user/SomeName/videos",
        "https://www.youtube.com/playlist?list=PL123",
        "https://music.youtube.com/playlist?list=OLAK5uy_abc",
    ];

    const NOT_CHANNEL_PAGES: &[&str] = &[
        "https://www.tiktok.com/",
        "https://www.tiktok.com/@",
        "https://www.tiktok.com/@user.name/video/7234567890123456789",
        "https://www.tiktok.com/@user.name/photo/7234567890123456789",
        "https://www.tiktok.com/@user.name/live",
        "https://www.tiktok.com/explore",
        "https://vm.tiktok.com/ZMabc123/",
        "https://www.youtube.com/",
        "https://www.youtube.com/watch?v=09OGSRptb0M&list=PL123",
        "https://www.youtube.com/playlist",
        "https://www.youtube.com/playlist?list=",
        "https://www.youtube.com/@SomeChannel/community",
        "https://www.youtube.com/@SomeChannel/playlists",
        "https://www.youtube.com/@SomeChannel/videos/extra",
        "https://www.youtube.com/@clip.mp4",
        "https://www.youtube.com/channel",
        "https://www.youtube.com/feed/subscriptions",
        "https://notyoutube.com/@SomeChannel",
        "https://tiktok.com.evil.example/@user",
        "https://user:pass@www.tiktok.com/@user",
        "ftp://www.tiktok.com/@user",
        "not a url",
        "",
    ];

    #[test]
    fn channel_pages_open_the_channel_flow() {
        for url in CHANNEL_PAGES {
            assert!(is_channel_page(url), "{url}");
            assert!(!is_media_page(url), "{url}");
        }
        for url in NOT_CHANNEL_PAGES {
            assert!(!is_channel_page(url), "{url}");
        }
        for url in MEDIA_PAGES {
            assert!(!is_channel_page(url), "{url}");
        }
    }

    #[test]
    fn listing_url_drops_extras_and_picks_the_videos_tab() {
        let cases = [
            (
                "https://www.tiktok.com/@user.name?lang=en&is_from_webapp=1#top",
                "https://www.tiktok.com/@user.name",
            ),
            ("https://m.tiktok.com/@user.name/", "https://m.tiktok.com/@user.name"),
            ("https://www.youtube.com/@Some", "https://www.youtube.com/@Some/videos"),
            ("https://www.youtube.com/@Some/featured", "https://www.youtube.com/@Some/videos"),
            ("https://www.youtube.com/@Some/shorts?x=1", "https://www.youtube.com/@Some/shorts"),
            (
                "https://www.youtube.com/channel/UC123",
                "https://www.youtube.com/channel/UC123/videos",
            ),
            (
                "https://www.youtube.com/playlist?si=abc&list=PL123",
                "https://www.youtube.com/playlist?list=PL123",
            ),
        ];
        for (raw, expected) in cases {
            assert_eq!(channel_listing_url(raw).as_deref(), Some(expected), "{raw}");
        }
        assert_eq!(channel_listing_url("https://www.youtube.com/watch?v=abc"), None);
        assert_eq!(channel_listing_url("https://vm.tiktok.com/ZMabc123/"), None);
    }

    #[test]
    fn channel_error_carries_the_link() {
        assert_eq!(
            channel_page_error(" https://www.tiktok.com/@a "),
            "channel_page: https://www.tiktok.com/@a"
        );
    }

    #[test]
    fn video_pages_go_to_the_media_flow() {
        for url in MEDIA_PAGES {
            assert!(is_media_page(url), "{url}");
        }
    }

    #[test]
    fn homepages_channels_and_files_stay_http() {
        for url in OTHER_URLS {
            assert!(!is_media_page(url), "{url}");
        }
    }

    #[test]
    fn domain_match_needs_a_dot_boundary() {
        assert!(on_domain("youtube.com", "youtube.com"));
        assert!(on_domain("music.youtube.com", "youtube.com"));
        assert!(!on_domain("notyoutube.com", "youtube.com"));
        assert!(!on_domain("box.com", "x.com"));
        assert!(on_domain("mobile.x.com", "x.com"));
    }

    #[test]
    fn error_carries_the_code_the_ui_checks() {
        assert!(media_page_error().starts_with("media_page:"));
    }

    #[test]
    fn early_browser_requests_wait_for_the_ui() {
        let pending = PendingMedia::default();
        let request = |url: &str| MediaRequest {
            url: url.into(),
            page_url: None,
        };
        assert!(pending.hold(&request("https://youtu.be/a")));
        assert!(pending.hold(&request("https://youtu.be/b")));
        assert_eq!(pending.take(), Some(request("https://youtu.be/b")), "giữ link mới nhất");
        assert_eq!(pending.take(), None);
        assert!(
            !pending.hold(&request("https://youtu.be/c")),
            "giao diện đã nghe ⇒ phát thẳng"
        );
        assert_eq!(pending.take(), None);
    }

    #[test]
    fn page_url_is_kept_only_when_valid() {
        assert_eq!(
            clean_page_url(Some(" https://www.youtube.com/ ".into())),
            Some("https://www.youtube.com/".into())
        );
        assert_eq!(clean_page_url(None), None);
        assert_eq!(clean_page_url(Some("javascript:alert(1)".into())), None);
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL_BYTES));
        assert_eq!(clean_page_url(Some(long)), None);
    }
}
