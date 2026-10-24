//! Phần đầu hộp Tải cả kênh: tên, ảnh đại diện, số người theo dõi. Chỉ để hiển thị nên lỗi thì bỏ qua.
//! - TikTok: trang nhúng công khai `www.tiktok.com/embed/@<user>` (chính yt-dlp cũng đọc trang này khi
//!   tìm secUid). Không gọi API web có chữ ký (X-Bogus, X-Gnarly, a_bogus), không giải thử thách chống
//!   bot của trang hồ sơ.
//! - YouTube: `yt-dlp --flat-playlist -J -I 0` (chỉ phần đầu danh sách, không có video).

use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use utd_engine::fetch::{FetchError, FetchPolicy, Url, fetch_text};

use crate::media::{js_runtime_args, tokio_command, tools};

const EMBED_HOST: &str = "www.tiktok.com";
/// Trang nhúng thật ~300 KB.
const EMBED_MAX_BYTES: u64 = 4 * 1024 * 1024;
const EMBED_ATTEMPTS: u32 = 3;
const EMBED_RETRY_DELAY: Duration = Duration::from_millis(1500);
const HEAD_TIMEOUT: Duration = Duration::from_secs(40);

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelProfile {
    pub name: Option<String>,
    pub avatar: Option<String>,
    pub followers: Option<u64>,
    /// Số video nếu trang cho biết (playlist YouTube); TikTok không công khai con số này ở trang nhúng.
    pub video_count: Option<u64>,
    /// Tài khoản riêng tư: chưa đăng nhập thì không liệt kê được.
    pub private: bool,
}

/// `listing_url` đã qua `channel_listing_url`.
pub async fn fetch(tools: &tools::MediaTools, listing_url: &str) -> Option<ChannelProfile> {
    let url = Url::parse(listing_url).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    if host == "tiktok.com" || host.ends_with(".tiktok.com") {
        let handle = url.path_segments()?.next()?.to_string();
        return tiktok(&handle).await;
    }
    if host == "youtube.com" || host.ends_with(".youtube.com") {
        return youtube(tools, listing_url).await;
    }
    None
}

async fn tiktok(handle: &str) -> Option<ChannelProfile> {
    let name = handle.strip_prefix('@')?;
    let plain = |character: char| character.is_ascii_alphanumeric() || "._-".contains(character);
    if name.is_empty() || name.len() > 64 || !name.chars().all(plain) {
        return None;
    }
    let mut policy = FetchPolicy::https_only(&[EMBED_HOST]);
    policy.connect_timeout = Duration::from_secs(10);
    policy.idle_timeout = Duration::from_secs(15);
    let url = format!("https://{EMBED_HOST}/embed/@{name}");
    for attempt in 1..=EMBED_ATTEMPTS {
        match fetch_text(&url, EMBED_MAX_BYTES, &policy).await {
            Ok(page) => return parse_tiktok_embed(&page.text),
            // Máy chủ trả 503 "overload-protect triggered" khá thường (~nửa số lần): nghỉ rồi thử lại.
            Err(FetchError::Status(503 | 429)) if attempt < EMBED_ATTEMPTS => {
                tokio::time::sleep(EMBED_RETRY_DELAY * attempt).await;
            }
            Err(error) => {
                log::info!("channel: không đọc được trang nhúng TikTok ({error})");
                return None;
            }
        }
    }
    None
}

/// `userInfo` trong JSON `__FRONTITY_CONNECT_STATE__` của trang nhúng.
pub fn parse_tiktok_embed(html: &str) -> Option<ChannelProfile> {
    let marker = html.find("__FRONTITY_CONNECT_STATE__")?;
    let body = marker + html[marker..].find('>')? + 1;
    let end = body + html[body..].find("</script>")?;
    let state: Value = serde_json::from_str(html[body..end].trim()).ok()?;
    let user = state
        .pointer("/source/data")?
        .as_object()?
        .values()
        .find_map(|page| page.get("userInfo"))?;
    let name = text(user, "nickname").or_else(|| text(user, "uniqueId"));
    Some(ChannelProfile {
        name,
        avatar: text(user, "avatarThumbUrl").filter(|url| image_url(url)),
        followers: user.get("followerCount").and_then(Value::as_u64),
        video_count: None,
        private: user.get("privateAccount").and_then(Value::as_bool) == Some(true),
    })
}

async fn youtube(tools: &tools::MediaTools, listing_url: &str) -> Option<ChannelProfile> {
    let mut command = tokio_command(&tools.yt_dlp);
    command
        .args(js_runtime_args(tools))
        .args([
            "--encoding",
            "utf-8",
            "--no-warnings",
            "--flat-playlist",
            "--playlist-items",
            "0",
            "--socket-timeout",
            "15",
            "-J",
            listing_url,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(HEAD_TIMEOUT, command.output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value: Value = serde_json::from_slice(&output.stdout).ok()?;
    Some(parse_youtube_head(&value))
}

/// Ảnh đại diện là ảnh `avatar_uncropped` hoặc ảnh vuông lớn nhất; ảnh ngang là banner.
pub fn parse_youtube_head(value: &Value) -> ChannelProfile {
    let thumbnails: Vec<&Value> = value
        .get("thumbnails")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| text(item, "url").is_some_and(|url| image_url(&url)))
                .collect()
        })
        .unwrap_or_default();
    let size = |item: &Value, key: &str| item.get(key).and_then(Value::as_u64);
    let avatar = thumbnails
        .iter()
        .find(|item| text(item, "id").as_deref() == Some("avatar_uncropped"))
        .or_else(|| {
            thumbnails
                .iter()
                .filter(|item| {
                    size(item, "width").is_some() && size(item, "width") == size(item, "height")
                })
                .max_by_key(|item| size(item, "width"))
        })
        .and_then(|item| text(item, "url"));
    ChannelProfile {
        name: ["channel", "uploader", "title"]
            .iter()
            .find_map(|key| text(value, key)),
        avatar,
        followers: value.get("channel_follower_count").and_then(Value::as_u64),
        video_count: value.get("playlist_count").and_then(Value::as_u64),
        private: false,
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn image_url(url: &str) -> bool {
    url.starts_with("https://") && utd_engine::validate_url(url).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Rút gọn từ trang nhúng thật (`/embed/@tiktok`, 2026-10-24), số liệu và link đã thay.
    const EMBED: &str = include_str!("fixtures/tiktok-embed.html");

    #[test]
    fn reads_the_tiktok_embed_user() {
        let profile = parse_tiktok_embed(EMBED).unwrap();
        assert_eq!(profile.name.as_deref(), Some("Sample Creator"));
        assert_eq!(
            profile.avatar.as_deref(),
            Some("https://p16-sign.tiktokcdn.com/avatar.jpeg?x-expires=1&x-signature=a")
        );
        assert_eq!(profile.followers, Some(96_400));
        assert!(!profile.private);
        assert_eq!(parse_tiktok_embed("<html>Access denied</html>"), None);
    }

    #[test]
    fn private_tiktok_accounts_are_flagged() {
        let html = EMBED.replace("\"privateAccount\":false", "\"privateAccount\":true");
        assert!(parse_tiktok_embed(&html).unwrap().private);
    }

    #[test]
    fn reads_the_youtube_avatar_not_the_banner() {
        let value = json!({
            "channel": "Sample Channel",
            "channel_follower_count": 46_500_000,
            "playlist_count": null,
            "thumbnails": [
                { "id": "0", "url": "https://yt3.example.com/banner=w1060", "width": 1060, "height": 175 },
                { "id": "7", "url": "https://yt3.example.com/avatar=s900", "width": 900, "height": 900 },
                { "id": "avatar_uncropped", "url": "https://yt3.example.com/avatar=s0" },
            ],
        });
        let profile = parse_youtube_head(&value);
        assert_eq!(profile.name.as_deref(), Some("Sample Channel"));
        assert_eq!(
            profile.avatar.as_deref(),
            Some("https://yt3.example.com/avatar=s0")
        );
        assert_eq!(profile.followers, Some(46_500_000));

        let square = json!({ "title": "Playlist", "playlist_count": 12, "thumbnails": [
            { "url": "https://yt3.example.com/a=s88", "width": 88, "height": 88 },
            { "url": "https://yt3.example.com/a=s176", "width": 176, "height": 176 },
        ]});
        let profile = parse_youtube_head(&square);
        assert_eq!(
            profile.avatar.as_deref(),
            Some("https://yt3.example.com/a=s176")
        );
        assert_eq!(profile.video_count, Some(12));
        assert_eq!(profile.name.as_deref(), Some("Playlist"));
    }
}
