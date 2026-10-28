//! Kiểm một đợt `social-list` tiện ích gửi (dữ liệu lấy trong trang, không tin hẳn): chỉ giữ link https của đúng
//! mạng xã hội / CDN của nó, id gọn, chữ cắt ngắn. Mục hỏng thì bỏ, không làm hỏng cả đợt.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Một đợt tối đa chừng này mục (tiện ích gửi ≤ 200).
pub const MAX_PART_ENTRIES: usize = 500;
const MAX_ID_CHARS: usize = 128;
const MAX_TITLE_CHARS: usize = 300;
const MAX_NAME_CHARS: usize = 200;
const MAX_ALBUM_CHARS: usize = 100;
const MAX_LINK_BYTES: usize = 4096;
/// 2004-01-01: trước đó không có Facebook.
const MIN_TIMESTAMP: i64 = 1_072_915_200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Facebook,
    Instagram,
    Threads,
}

impl Platform {
    /// Trang của link hồ sơ / nhóm (`url` của tin nhắn).
    pub fn of_page(url: &str) -> Option<Self> {
        let host = host_of(url)?;
        if within(&host, "facebook.com") {
            Some(Self::Facebook)
        } else if within(&host, "instagram.com") {
            Some(Self::Instagram)
        } else if within(&host, "threads.net") || within(&host, "threads.com") {
            Some(Self::Threads)
        } else {
            None
        }
    }

    /// Tiền tố khoá nhớ mục đã tải (`media::channel::seen`).
    pub fn site(self) -> &'static str {
        match self {
            Self::Facebook => "facebook.com",
            Self::Instagram => "instagram.com",
            Self::Threads => "threads.com",
        }
    }

    /// Trang bài viết (link mở lại / yt-dlp tải video khi không có link file).
    fn owns_page(self, host: &str) -> bool {
        match self {
            Self::Facebook => within(host, "facebook.com"),
            Self::Instagram => within(host, "instagram.com"),
            Self::Threads => within(host, "threads.net") || within(host, "threads.com"),
        }
    }
}

/// Nhóm media trên trang (chip lọc trong hộp).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Posts,
    Photos,
    Videos,
    Reels,
    Stories,
    Highlights,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Photo,
    Video,
}

/// Một ảnh/video. Hình dạng giống `ChannelEntry` (hộp Tải cả kênh dùng lại) cộng các trường riêng.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialEntry {
    pub id: String,
    /// Trang bài viết nếu có, không thì link file.
    pub url: String,
    pub title: String,
    pub timestamp: Option<i64>,
    pub duration_seconds: Option<u64>,
    pub view_count: Option<u64>,
    pub thumbnail: Option<String>,
    pub blocked: Option<String>,
    pub downloaded: bool,
    pub source: Source,
    pub kind: Kind,
    /// Link file trên CDN (đã có chữ ký của trang): tải thẳng, không cookie.
    pub file: Option<String>,
    /// Trang bài viết; video không có `file` thì yt-dlp tải từ đây.
    pub page: Option<String>,
    /// Tên tin nổi bật / album.
    pub album: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialHead {
    pub key: Option<String>,
    pub name: String,
    pub handle: Option<String>,
}

/// Cùng hình dạng `ChannelProfile` của hộp Tải cả kênh.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialProfile {
    pub name: Option<String>,
    pub avatar: Option<String>,
    pub followers: Option<u64>,
    pub video_count: Option<u64>,
    pub private: bool,
}

/// Đợt cuối: `complete` = đã đi hết; `error` = câu có mã khi dừng giữa chừng (bị chặn tạm, cần đăng nhập…).
#[derive(Debug, Clone, PartialEq)]
pub struct Finish {
    pub complete: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub listing_id: String,
    pub part: u32,
    pub platform: Platform,
    pub url: String,
    pub head: Option<SocialHead>,
    pub profile: Option<SocialProfile>,
    pub entries: Vec<SocialEntry>,
    pub finish: Option<Finish>,
}

/// Lỗi tiện ích được phép báo (mã cố định ⇒ câu của app, không hiện chữ trang gửi).
const FINISH_ERRORS: &[(&str, &str)] = &[
    (
        "social.rate_limited",
        "[social.rate_limited] Trang tạm chặn vì gọi nhiều. Đợi vài phút rồi thử lại; danh sách đã có vẫn tải được.",
    ),
    (
        "social.login",
        "[social.login] Cần đăng nhập trên trình duyệt để xem mục này. Đăng nhập rồi bấm lại trên trang.",
    ),
    (
        "social.private",
        "[social.private] Tài khoản riêng tư hoặc bạn chưa được xem. Danh sách có thể thiếu.",
    ),
    (
        "social.changed",
        "[social.changed] Trang đã đổi cách trả dữ liệu; danh sách có thể thiếu. Cập nhật UTD rồi thử lại.",
    ),
    (
        "social.not_found",
        "[social.not_found] Không tìm thấy trang này hoặc trang không có ảnh/video.",
    ),
    (
        "social.partial",
        "[social.partial] Trang không trả thêm mục; danh sách có thể chưa đủ.",
    ),
    (
        "social.limited",
        "[social.limited] Chỉ liệt kê 20.000 mục mới nhất.",
    ),
];

pub fn finish_error(code: &str) -> String {
    FINISH_ERRORS
        .iter()
        .find(|(known, _)| *known == code)
        .map(|(_, message)| (*message).to_string())
        .unwrap_or_else(|| FINISH_ERRORS[5].1.to_string())
}

/// `url`: trang đang mở (tin nhắn); `value`: phần `social`.
pub fn parse_message(url: &str, value: &Value, now: i64) -> Result<Message, String> {
    let invalid = || "[social.invalid] Danh sách từ trình duyệt không hợp lệ.".to_string();
    let platform = Platform::of_page(url).ok_or_else(invalid)?;
    let listing_id = text(value, "listingId")
        .filter(|id| {
            (8..=64).contains(&id.len())
                && id.chars().all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
        .ok_or_else(invalid)?
        .to_string();
    let part = value
        .get("part")
        .and_then(Value::as_u64)
        .and_then(|part| u32::try_from(part).ok())
        .ok_or_else(invalid)?;
    let raw = value.get("entries").and_then(Value::as_array);
    if raw.is_some_and(|list| list.len() > MAX_PART_ENTRIES) {
        return Err(invalid());
    }
    let entries = raw
        .map(|list| list.iter().filter_map(|entry| parse_entry(platform, entry, now)).collect())
        .unwrap_or_default();
    let head = value.get("head").and_then(|head| parse_head(platform, head));
    let profile = value.get("head").map(parse_profile);
    let finish = value.get("done").filter(|done| done.is_object()).map(|done| Finish {
        complete: done.get("complete").and_then(Value::as_bool).unwrap_or(false),
        error: text(done, "error").map(finish_error),
    });
    Ok(Message {
        listing_id,
        part,
        platform,
        url: url.to_string(),
        head,
        profile,
        entries,
        finish,
    })
}

fn parse_head(platform: Platform, value: &Value) -> Option<SocialHead> {
    let name = clean(text(value, "name")?, MAX_NAME_CHARS)?;
    let key = text(value, "id")
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 100
                && id
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
        })
        .map(|id| format!("{}:{id}", platform.site()));
    let handle = text(value, "handle")
        .and_then(|handle| clean(handle, 100))
        .map(|handle| match handle.starts_with('@') {
            true => handle,
            false => format!("@{handle}"),
        })
        .filter(|handle| handle.len() > 1 && !handle.contains(char::is_whitespace));
    Some(SocialHead { key, name, handle })
}

fn parse_profile(value: &Value) -> SocialProfile {
    SocialProfile {
        name: text(value, "name").and_then(|name| clean(name, MAX_NAME_CHARS)),
        avatar: text(value, "avatar").and_then(cdn_link),
        followers: value.get("followers").and_then(Value::as_u64),
        video_count: None,
        private: value.get("private").and_then(Value::as_bool).unwrap_or(false),
    }
}

pub fn parse_entry(platform: Platform, value: &Value, now: i64) -> Option<SocialEntry> {
    let id = text(value, "id")
        .filter(|id| {
            !id.is_empty()
                && id.chars().count() <= MAX_ID_CHARS
                && id
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || "_:.-".contains(character))
        })?
        .to_string();
    let source: Source = serde_json::from_value(value.get("source")?.clone()).ok()?;
    let kind: Kind = serde_json::from_value(value.get("kind")?.clone()).ok()?;
    let file = text(value, "url").and_then(cdn_link);
    let page = text(value, "page").and_then(|page| page_link(platform, page));
    // Ảnh phải có file; video không có file thì yt-dlp tải từ trang bài viết.
    if file.is_none() && (kind == Kind::Photo || page.is_none()) {
        return None;
    }
    let timestamp = value
        .get("timestamp")
        .and_then(Value::as_i64)
        .filter(|seconds| (MIN_TIMESTAMP..=now + 86_400).contains(seconds));
    let number = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_f64)
            .filter(|number| number.is_finite() && *number >= 0.0)
    };
    Some(SocialEntry {
        url: page.clone().or_else(|| file.clone())?,
        title: text(value, "title")
            .and_then(|title| clean(title, MAX_TITLE_CHARS))
            .unwrap_or_default(),
        timestamp,
        duration_seconds: number("durationSeconds").map(|seconds| seconds.round() as u64),
        view_count: None,
        thumbnail: text(value, "thumb").and_then(cdn_link),
        blocked: None,
        downloaded: false,
        album: text(value, "album").and_then(|album| clean(album, MAX_ALBUM_CHARS)),
        id,
        source,
        kind,
        file,
        page,
    })
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// Gộp khoảng trắng/xuống dòng, cắt dài; rỗng ⇒ `None`.
fn clean(raw: &str, max: usize) -> Option<String> {
    let joined = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|character| !character.is_control())
        .take(max)
        .collect::<String>();
    (!joined.is_empty()).then_some(joined)
}

fn host_of(url: &str) -> Option<String> {
    let url = utd_engine::validate_url(url).ok()?;
    Some(url.host_str()?.trim_end_matches('.').to_ascii_lowercase())
}

fn within(host: &str, site: &str) -> bool {
    host == site || host.ends_with(&format!(".{site}"))
}

fn https_host(link: &str) -> Option<String> {
    if link.len() > MAX_LINK_BYTES || !link.starts_with("https://") {
        return None;
    }
    host_of(link)
}

/// Link file trên CDN của Meta (ảnh, video, ảnh đại diện): chỉ chừng này host, để trang lạ không mượn hộp này
/// cho UTD tải file bất kỳ.
pub fn cdn_link(link: &str) -> Option<String> {
    let host = https_host(link)?;
    ["fbcdn.net", "cdninstagram.com", "fbsbx.com"]
        .iter()
        .any(|site| within(&host, site))
        .then(|| link.to_string())
}

fn page_link(platform: Platform, link: &str) -> Option<String> {
    let host = https_host(link)?;
    platform.owns_page(&host).then(|| link.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: i64 = 1_790_000_000;

    #[test]
    fn platforms_come_from_the_page_host() {
        assert_eq!(Platform::of_page("https://www.facebook.com/groups/1"), Some(Platform::Facebook));
        assert_eq!(Platform::of_page("https://web.facebook.com/x"), Some(Platform::Facebook));
        assert_eq!(Platform::of_page("https://www.instagram.com/nasa/"), Some(Platform::Instagram));
        assert_eq!(Platform::of_page("https://www.threads.net/@zuck"), Some(Platform::Threads));
        assert_eq!(Platform::of_page("https://www.threads.com/@zuck"), Some(Platform::Threads));
        assert_eq!(Platform::of_page("https://notfacebook.com/x"), None);
        assert_eq!(Platform::of_page("https://facebook.com.evil.test/x"), None);
        assert_eq!(Platform::of_page("file:///c:/x"), None);
    }

    #[test]
    fn entries_keep_only_meta_links_and_tidy_text() {
        let photo = json!({
            "id": "1789:2", "source": "posts", "kind": "photo",
            "url": "https://scontent-hkg1-1.cdninstagram.com/v/t51/1_n.jpg?stp=dst-jpg&oh=1",
            "page": "https://www.instagram.com/p/Cabc/", "thumb": "https://evil.test/t.jpg",
            "timestamp": 1_700_000_000, "title": "  Hoa\nđào \u{0007}nở  ", "album": "Đà Lạt",
        });
        let entry = parse_entry(Platform::Instagram, &photo, NOW).unwrap();
        assert_eq!(entry.title, "Hoa đào nở");
        assert_eq!(entry.url, "https://www.instagram.com/p/Cabc/");
        assert_eq!(entry.thumbnail, None, "ảnh nhỏ ngoài CDN bị bỏ");
        assert_eq!(entry.album.as_deref(), Some("Đà Lạt"));
        assert_eq!(entry.timestamp, Some(1_700_000_000));

        let elsewhere = json!({ "id": "1", "source": "photos", "kind": "photo", "url": "https://evil.test/a.jpg" });
        assert_eq!(parse_entry(Platform::Facebook, &elsewhere, NOW), None);
        let http = json!({ "id": "1", "source": "photos", "kind": "photo", "url": "http://scontent.fbcdn.net/a.jpg" });
        assert_eq!(parse_entry(Platform::Facebook, &http, NOW), None);
        let bad_id = json!({ "id": "../x", "source": "photos", "kind": "photo", "url": "https://scontent.fbcdn.net/a.jpg" });
        assert_eq!(parse_entry(Platform::Facebook, &bad_id, NOW), None);
        let bad_source = json!({ "id": "1", "source": "exe", "kind": "photo", "url": "https://scontent.fbcdn.net/a.jpg" });
        assert_eq!(parse_entry(Platform::Facebook, &bad_source, NOW), None);
        // Trang bài viết phải đúng mạng của tin nhắn.
        let wrong_page = json!({ "id": "1", "source": "videos", "kind": "video", "page": "https://www.instagram.com/p/x/" });
        assert_eq!(parse_entry(Platform::Facebook, &wrong_page, NOW), None);
    }

    #[test]
    fn videos_without_a_file_fall_back_to_their_page_but_photos_do_not() {
        let video = json!({ "id": "9", "source": "videos", "kind": "video", "page": "https://www.facebook.com/watch/?v=9", "timestamp": 99 });
        let entry = parse_entry(Platform::Facebook, &video, NOW).unwrap();
        assert_eq!(entry.file, None);
        assert_eq!(entry.url, "https://www.facebook.com/watch/?v=9");
        assert_eq!(entry.timestamp, None, "ngày lạ bị bỏ");
        let photo = json!({ "id": "9", "source": "photos", "kind": "photo", "page": "https://www.facebook.com/photo/?fbid=9" });
        assert_eq!(parse_entry(Platform::Facebook, &photo, NOW), None);
    }

    #[test]
    fn messages_check_the_listing_and_map_errors_to_fixed_text() {
        let value = json!({
            "listingId": "sb-abc12345", "part": 0,
            "head": { "id": "100044", "name": "Trang Thử", "handle": "trangthu", "avatar": "https://scontent.xx.fbcdn.net/a.jpg", "followers": 12 },
            "entries": [{ "id": "1", "source": "photos", "kind": "photo", "url": "https://scontent.xx.fbcdn.net/1.jpg" }, { "id": "" }],
            "done": { "complete": false, "error": "<b>lừa</b>" },
        });
        let message = parse_message("https://www.facebook.com/trangthu", &value, NOW).unwrap();
        assert_eq!(message.entries.len(), 1);
        let head = message.head.unwrap();
        assert_eq!(head.key.as_deref(), Some("facebook.com:100044"));
        assert_eq!(head.handle.as_deref(), Some("@trangthu"));
        assert_eq!(message.profile.unwrap().followers, Some(12));
        let finish = message.finish.unwrap();
        assert!(finish.error.unwrap().starts_with("[social.partial]"), "mã lạ ⇒ câu chung, không hiện chữ của trang");

        let limited = json!({ "listingId": "sb-abc12345", "part": 3, "done": { "error": "social.rate_limited" } });
        let message = parse_message("https://www.instagram.com/nasa/", &limited, NOW).unwrap();
        assert!(message.finish.unwrap().error.unwrap().starts_with("[social.rate_limited]"));

        assert!(parse_message("https://example.com/", &limited, NOW).is_err());
        let short = json!({ "listingId": "x", "part": 0 });
        assert!(parse_message("https://www.instagram.com/nasa/", &short, NOW).is_err());
        let flood: Vec<Value> = (0..=MAX_PART_ENTRIES).map(|_| json!({})).collect();
        let flood = json!({ "listingId": "sb-abc12345", "part": 1, "entries": flood });
        assert!(parse_message("https://www.instagram.com/nasa/", &flood, NOW).is_err());
    }
}
