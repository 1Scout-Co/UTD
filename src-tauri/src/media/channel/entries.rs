//! Đọc từng dòng của `yt-dlp --flat-playlist -j` (một video một dòng JSON) thành mục gọn cho hộp Tải cả
//! kênh. Dòng nào thiếu id/link thì bỏ, không làm hỏng cả danh sách.

use serde::Serialize;
use serde_json::Value;

/// Tiêu đề dài hơn thì cắt (mô tả TikTok có thể vài nghìn chữ).
const MAX_TITLE_CHARS: usize = 300;
const MAX_ID_CHARS: usize = 128;

/// Một video trong kênh: đủ để hiện danh sách và thêm task media.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelEntry {
    pub id: String,
    pub url: String,
    pub title: String,
    /// Giây Unix (UTC) lúc đăng; YouTube chỉ có ngày ước lượng (`approximate_date`).
    pub timestamp: Option<i64>,
    pub duration_seconds: Option<u64>,
    pub view_count: Option<u64>,
    pub thumbnail: Option<String>,
    /// Lý do không tải được (riêng tư, hội viên, đang phát trực tiếp); `None` = tải được.
    pub blocked: Option<String>,
    /// Đã tải (hoặc đang tải) ở lần trước, theo id video.
    pub downloaded: bool,
}

/// Kênh của danh sách, đọc từ các trường `playlist_*` mà yt-dlp gắn vào mỗi dòng.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelHead {
    /// `<trang>:<id danh sách>` (vd `tiktok.com:MS4w…`, `youtube.com:UC…`): khoá nhớ video đã tải.
    pub key: Option<String>,
    /// Tên để hiện và đặt thư mục con: tên kênh, hoặc tên playlist khi là playlist.
    pub name: String,
    /// `@tên` của kênh nếu có.
    pub handle: Option<String>,
}

pub fn parse_entry(value: &Value) -> Option<ChannelEntry> {
    // Trang chủ kênh YouTube trả các thẻ (Video, Shorts…) là danh sách lồng: không phải video.
    if matches!(text(value, "_type"), Some("playlist" | "multi_video")) {
        return None;
    }
    let id = text(value, "id")
        .filter(|id| id.chars().count() <= MAX_ID_CHARS && !id.chars().any(char::is_control))?
        .to_string();
    let url = text(value, "url")
        .or_else(|| text(value, "webpage_url"))
        .filter(|url| utd_engine::validate_url(url).is_ok())?
        .to_string();
    Some(ChannelEntry {
        title: title_of(value).unwrap_or_else(|| id.clone()),
        timestamp: timestamp_of(value),
        duration_seconds: number(value, "duration").map(|seconds| seconds.round() as u64),
        view_count: number(value, "view_count").map(|views| views as u64),
        thumbnail: thumbnail_of(value),
        blocked: blocked_reason(value),
        downloaded: false,
        id,
        url,
    })
}

/// `playlist`: link là playlist YouTube ⇒ tên là tên playlist, không phải tên người đăng.
pub fn parse_head(value: &Value, playlist: bool) -> Option<ChannelHead> {
    let named = |keys: &[&str]| keys.iter().find_map(|key| clean(text(value, key)?));
    let name = if playlist {
        named(&["playlist_title", "playlist", "playlist_uploader"])
    } else {
        named(&[
            "playlist_channel",
            "playlist_uploader",
            "channel",
            "playlist_title",
            "playlist",
        ])
    }?;
    let handle = named(&["playlist_uploader_id", "uploader"])
        .map(|handle| match handle.starts_with('@') {
            true => handle,
            false => format!("@{handle}"),
        })
        .filter(|handle| !playlist && handle.len() > 1 && !handle.contains(char::is_whitespace));
    Some(ChannelHead {
        key: key_of(value),
        name,
        handle,
    })
}

fn key_of(value: &Value) -> Option<String> {
    let id = text(value, "playlist_id").filter(|id| {
        !id.is_empty()
            && id.len() <= MAX_ID_CHARS
            && id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_.@".contains(character))
    })?;
    let page = text(value, "playlist_webpage_url").or_else(|| text(value, "webpage_url"))?;
    let host = utd_engine::validate_url(page)
        .ok()?
        .host_str()?
        .to_ascii_lowercase();
    let labels: Vec<&str> = host.trim_end_matches('.').rsplit('.').take(2).collect();
    let site = labels.into_iter().rev().collect::<Vec<_>>().join(".");
    Some(format!("{site}:{id}"))
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn number(value: &Value, key: &str) -> Option<f64> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite() && *number >= 0.0)
}

/// Gộp khoảng trắng/xuống dòng, cắt dài; rỗng ⇒ `None`.
fn clean(raw: &str) -> Option<String> {
    let joined = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = joined.chars().take(MAX_TITLE_CHARS).collect();
    (!cut.is_empty()).then_some(cut)
}

fn title_of(value: &Value) -> Option<String> {
    clean(text(value, "title").unwrap_or("")).or_else(|| {
        let description = text(value, "description")?;
        clean(description.lines().find(|line| !line.trim().is_empty())?)
    })
}

fn timestamp_of(value: &Value) -> Option<i64> {
    ["timestamp", "release_timestamp"]
        .iter()
        .find_map(|key| number(value, key))
        .map(|seconds| seconds as i64)
        .or_else(|| text(value, "upload_date").and_then(date_seconds))
}

/// `YYYYMMDD` ⇒ giây Unix lúc 00:00 UTC ngày đó.
fn date_seconds(date: &str) -> Option<i64> {
    if date.len() != 8 || !date.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let year: i64 = date[..4].parse().ok()?;
    let month: i64 = date[4..6].parse().ok()?;
    let day: i64 = date[6..].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Đếm ngày từ 1970-01-01 theo lịch Gregory (thuật toán days_from_civil).
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some((era * 146_097 + day_of_era - 719_468) * 86_400)
}

/// Ảnh nhỏ cho danh sách: `thumbnail` nếu có; không thì ảnh hẹp nhất còn rộng ≥ 160 px, ảnh `cover`
/// (TikTok), hoặc ảnh đầu tiên.
fn thumbnail_of(value: &Value) -> Option<String> {
    let usable = |url: &str| url.starts_with("https://") && utd_engine::validate_url(url).is_ok();
    if let Some(url) = text(value, "thumbnail").filter(|url| usable(url)) {
        return Some(url.to_string());
    }
    let thumbnails: Vec<&Value> = value
        .get("thumbnails")
        .and_then(Value::as_array)?
        .iter()
        .filter(|item| text(item, "url").is_some_and(usable))
        .collect();
    let width = |item: &Value| item.get("width").and_then(Value::as_u64);
    let picked = thumbnails
        .iter()
        .filter(|item| width(item).is_some_and(|width| width >= 160))
        .min_by_key(|item| width(item))
        .or_else(|| {
            thumbnails
                .iter()
                .find(|item| text(item, "id") == Some("cover"))
        })
        .or_else(|| thumbnails.first())?;
    text(picked, "url").map(str::to_string)
}

/// Lý do không tải được, dạng mã ổn định (`private`, `premium`, `members`, `login`, `live`, `upcoming`):
/// giao diện hiện nhãn theo ngôn ngữ (`channel.blocked.<mã>`).
fn blocked_reason(value: &Value) -> Option<String> {
    let reason = match text(value, "availability") {
        Some("private") => Some("private"),
        Some("premium_only") => Some("premium"),
        Some("subscriber_only") => Some("members"),
        Some("needs_auth") => Some("login"),
        _ => None,
    };
    let live = match text(value, "live_status") {
        Some("is_live") => Some("live"),
        Some("is_upcoming") => Some("upcoming"),
        _ => None,
    };
    reason.or(live).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Rút gọn từ `yt-dlp 2026.10.21 --flat-playlist -j` thật (@tiktok, kênh YouTube); id, chữ và link
    /// đã thay bằng giá trị giả, bỏ trường không dùng.
    const TIKTOK: &str = include_str!("fixtures/tiktok-flat.jsonl");
    const YOUTUBE: &str = include_str!("fixtures/youtube-flat.jsonl");

    fn lines(fixture: &str) -> Vec<Value> {
        fixture
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn reads_tiktok_entries_with_date_views_and_cover() {
        let values = lines(TIKTOK);
        let entries: Vec<ChannelEntry> = values.iter().filter_map(parse_entry).collect();
        assert_eq!(entries.len(), 3, "dòng thiếu link bị bỏ");
        let first = &entries[0];
        assert_eq!(first.id, "7700000000000000001");
        assert_eq!(
            first.url,
            "https://www.tiktok.com/@sample.creator/video/7700000000000000001"
        );
        assert_eq!(first.title, "Một ngày ở Sài Gòn cùng đội làm phim...");
        assert_eq!(first.timestamp, Some(1_792_798_142));
        assert_eq!(first.duration_seconds, Some(107));
        assert_eq!(first.view_count, Some(44_100));
        assert_eq!(
            first.thumbnail.as_deref(),
            Some("https://p16-sign.tiktokcdn.com/cover-1.image?x-expires=1&x-signature=a")
        );
        assert_eq!(first.blocked, None);
        assert!(!first.downloaded);
        assert_eq!(
            entries[1].title, "Dòng đầu mô tả",
            "thiếu tiêu đề ⇒ dòng đầu mô tả"
        );
        assert_eq!(entries[1].duration_seconds, None);
        assert_eq!(
            entries[2].title, "7700000000000000003",
            "không có chữ nào ⇒ id"
        );

        let head = parse_head(&values[0], false).unwrap();
        assert_eq!(
            head.key.as_deref(),
            Some("tiktok.com:MS4wLjABAAAAsample-secUid_0123")
        );
        assert_eq!(head.name, "Sample Creator");
        assert_eq!(head.handle.as_deref(), Some("@sample.creator"));
    }

    #[test]
    fn reads_youtube_tab_entries_and_skips_nested_tabs() {
        let values = lines(YOUTUBE);
        let entries: Vec<ChannelEntry> = values.iter().filter_map(parse_entry).collect();
        assert_eq!(
            entries.len(),
            3,
            "thẻ lồng (_type playlist) không phải video"
        );
        assert_eq!(
            entries[0].url,
            "https://www.youtube.com/watch?v=abcDEF12345"
        );
        assert_eq!(entries[0].timestamp, Some(1_792_454_400));
        assert_eq!(entries[0].view_count, None);
        assert_eq!(
            entries[0].thumbnail.as_deref(),
            Some("https://i.ytimg.com/vi/abcDEF12345/hq720.jpg?w=360"),
            "ảnh hẹp nhất còn ≥ 160 px"
        );
        assert_eq!(entries[1].blocked.as_deref(), Some("members"));
        assert_eq!(entries[2].blocked.as_deref(), Some("upcoming"));
        assert_eq!(
            entries[2].timestamp,
            Some(1_791_590_400),
            "upload_date khi thiếu timestamp"
        );

        let head = parse_head(&values[0], false).unwrap();
        assert_eq!(
            head.key.as_deref(),
            Some("youtube.com:UCsampleChannel0123456789")
        );
        assert_eq!(head.name, "Sample Channel");
        assert_eq!(head.handle.as_deref(), Some("@SampleChannel"));
        let playlist = parse_head(&values[0], true).unwrap();
        assert_eq!(playlist.name, "Sample Channel - Videos");
        assert_eq!(playlist.handle, None);
    }

    #[test]
    fn rejects_entries_without_a_safe_id_or_link() {
        let base = json!({ "id": "x1", "url": "https://www.tiktok.com/@a/video/1" });
        assert!(parse_entry(&base).is_some());
        let mut bad = base.clone();
        bad["url"] = json!("javascript:alert(1)");
        assert!(parse_entry(&bad).is_none());
        let mut bad = base.clone();
        bad["id"] = json!("a\nb");
        assert!(parse_entry(&bad).is_none());
        let mut bad = base;
        bad["id"] = json!("x".repeat(MAX_ID_CHARS + 1));
        assert!(parse_entry(&bad).is_none());
    }

    #[test]
    fn head_key_needs_a_plain_playlist_id() {
        let value = json!({
            "playlist_id": "../../etc",
            "playlist_webpage_url": "https://www.tiktok.com/@a",
            "playlist_title": "a",
        });
        let head = parse_head(&value, false).unwrap();
        assert_eq!(head.key, None);
        assert_eq!(head.name, "a");
    }

    #[test]
    fn converts_upload_dates() {
        assert_eq!(date_seconds("19700101"), Some(0));
        assert_eq!(date_seconds("20000301"), Some(951_868_800));
        assert_eq!(date_seconds("20261023"), Some(1_792_713_600));
        assert_eq!(date_seconds("2026102"), None);
        assert_eq!(date_seconds("20261332"), None);
    }

    #[test]
    fn long_titles_are_collapsed_and_cut() {
        let value = json!({
            "id": "1",
            "url": "https://youtu.be/1",
            "title": format!("a\n\n  b {}", "c".repeat(500)),
        });
        let title = parse_entry(&value).unwrap().title;
        assert!(title.starts_with("a b c"));
        assert_eq!(title.chars().count(), MAX_TITLE_CHARS);
    }
}
