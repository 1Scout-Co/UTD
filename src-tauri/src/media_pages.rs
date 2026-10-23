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

/// Link là trang xem video/nghe nhạc của một trang media lớn (không phải trang chủ, kênh hay tệp trực tiếp).
pub fn is_media_page(raw: &str) -> bool {
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
    let page = Page {
        host: &host,
        path,
        video_param: url
            .query_pairs()
            .any(|(key, value)| key == "v" && !value.is_empty()),
    };
    SITES
        .iter()
        .find(|(domain, _)| on_domain(&host, domain))
        .is_some_and(|(_, rule)| rule(&page))
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
