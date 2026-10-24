use std::process::Stdio;

use serde::Serialize;
use serde_json::Value;
use tokio::time::{timeout, Duration};

use crate::media::{auto, js_runtime_args, probe_cache, resolve, tokio_command, tools};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaProbeResult {
    pub url: String,
    pub webpage_url: String,
    pub title: String,
    pub uploader: Option<String>,
    pub duration_seconds: Option<u64>,
    pub thumbnail: Option<String>,
    pub formats: Vec<MediaFormatOption>,
    pub ffmpeg_available: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFormatOption {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub size_bytes: Option<u64>,
    pub bitrate_kbps: Option<u64>,
    pub selector: String,
    pub audio_format: Option<String>,
    pub badge: Option<String>,
    pub recommended: bool,
}

struct Format {
    /// Mã ghi vào selector khi `plain` (`399`, `140-23[language=en-US]`); `None` = mã chỉ là số thứ
    /// tự yt-dlp tự đánh, không ghim ổn định được.
    pin: Option<String>,
    ext: String,
    height: Option<u64>,
    has_video: bool,
    has_audio: bool,
    /// http(s), không nghi DRM, mã ghi được vào selector: engine UTD tải thẳng được.
    plain: bool,
    /// `preference` của extractor, thiếu là -1 như yt-dlp; YouTube gán -10 cho format hỏng.
    preference: i64,
    size: Option<u64>,
    tbr: Option<f64>,
    abr: Option<f64>,
}

/// Một lựa chọn: selector cho yt-dlp, kích thước và bitrate hiển thị.
struct Choice {
    selector: String,
    size: Option<u64>,
    bitrate: Option<f64>,
}

/// Phân tích lỗi: câu cho người dùng, và `outdated` = lỗi kiểu bản yt-dlp mới có thể sửa được.
pub struct ProbeFailure {
    pub message: String,
    pub outdated: bool,
}

impl From<String> for ProbeFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            outdated: false,
        }
    }
}

/// `ffmpeg_available`: có ffmpeg, hoặc app đang tự tải (lúc bấm Tải thì task chờ ffmpeg xong).
pub async fn probe(
    tools: &tools::MediaTools,
    url: &str,
    ffmpeg_available: bool,
) -> Result<MediaProbeResult, ProbeFailure> {
    let mut command = tokio_command(&tools.yt_dlp);
    command
        .args(js_runtime_args(tools))
        .args([
            "--encoding",
            "utf-8",
            "--no-playlist",
            // Link là kênh (kể cả link rút gọn `vm.tiktok.com` trỏ tới kênh): chỉ đọc đầu danh sách để
            // chuyển sang hộp Tải cả kênh, không phân tích từng video.
            "--flat-playlist",
            "--playlist-items",
            "0",
            "--no-warnings",
            "--skip-download",
            "--socket-timeout",
            "15",
            "-J",
            url,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = timeout(Duration::from_secs(45), command.output())
        .await
        .map_err(|_| "Nguồn media phản hồi quá lâu.".to_string())?
        .map_err(|error| format!("Không chạy được yt-dlp: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ProbeFailure {
            message: classify_stderr(&stderr),
            outdated: auto::outdated(&stderr),
        });
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "yt-dlp trả về dữ liệu không đọc được.".to_string())?;
    let result = build_result(url, &value, ffmpeg_available)?;
    // Lượt tải ngay sau đó dùng lại các luồng này, khỏi chạy yt-dlp lần hai.
    probe_cache::remember(&value, url, resolve::now_secs());
    Ok(result)
}

fn build_result(
    original_url: &str,
    value: &Value,
    ffmpeg_available: bool,
) -> Result<MediaProbeResult, String> {
    if value.get("_has_drm").and_then(Value::as_bool) == Some(true) {
        return Err("Nội dung có DRM; UTD không vượt qua bảo vệ bản quyền.".into());
    }
    if value.get("_type").and_then(Value::as_str) == Some("playlist") {
        let link = value
            .get("webpage_url")
            .and_then(Value::as_str)
            .filter(|link| utd_engine::validate_url(link).is_ok())
            .unwrap_or(original_url);
        return Err(crate::media_pages::channel_page_error(link));
    }
    if value.get("is_live").and_then(Value::as_bool) == Some(true)
        || value
            .get("live_status")
            .and_then(Value::as_str)
            .is_some_and(|status| status == "is_live" || status == "is_upcoming")
    {
        return Err("Chưa hỗ trợ tải livestream.".into());
    }
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or("Media")
        .to_string();
    let webpage_url = value
        .get("webpage_url")
        .and_then(Value::as_str)
        .unwrap_or(original_url)
        .to_string();
    let uploader = value
        .get("uploader")
        .or_else(|| value.get("channel"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let duration_seconds = value
        .get("duration")
        .and_then(Value::as_f64)
        .map(|duration| duration.max(0.0) as u64);
    let thumbnail = value
        .get("thumbnail")
        .and_then(Value::as_str)
        .map(str::to_string);
    let formats = parse_formats(value)?;
    Ok(MediaProbeResult {
        url: original_url.to_string(),
        webpage_url,
        title,
        uploader,
        duration_seconds,
        thumbnail,
        formats: format_options(&formats, ffmpeg_available),
        ffmpeg_available,
    })
}

/// Giữ đúng thứ tự của yt-dlp: kém trước, tốt sau.
fn parse_formats(value: &Value) -> Result<Vec<Format>, String> {
    let items = value
        .get("formats")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let formats: Vec<Format> = items
        .iter()
        .filter(|item| item.get("has_drm").and_then(Value::as_bool) != Some(true))
        .filter_map(|item| parse_format(item, items))
        .collect();
    if formats
        .iter()
        .all(|format| !format.has_video && !format.has_audio)
    {
        return Err("Không tìm thấy định dạng media có thể tải.".into());
    }
    Ok(formats)
}

fn format_options(formats: &[Format], ffmpeg_available: bool) -> Vec<MediaFormatOption> {
    let heights = video_heights(formats);
    let mut options = vec![best_option(formats, heights.first().copied())];
    for height in listed_heights(&heights) {
        options.push(height_option(formats, height));
    }
    if formats.iter().any(|format| format.has_audio) {
        options.push(m4a_option(formats));
        if ffmpeg_available {
            options.push(mp3_option(formats));
        }
    }
    options
}

/// Chiều cao có thật (cả format gộp), cao trước; không kể format bị hạ bậc (hỏng).
fn video_heights(formats: &[Format]) -> Vec<u64> {
    let mut heights: Vec<u64> = video_tier(formats, None)
        .filter_map(|format| format.height)
        .filter(|height| *height > 0)
        .collect();
    heights.sort_unstable_by(|left, right| right.cmp(left));
    heights.dedup();
    heights
}

/// Tối đa 5 mức quen thuộc có thật, thêm mức cao nhất nếu lạ (vd. 1600p).
fn listed_heights(heights: &[u64]) -> Vec<u64> {
    let preferred = [2160_u64, 1440, 1080, 720, 480, 360];
    let mut chosen: Vec<u64> = preferred
        .into_iter()
        .filter(|height| heights.contains(height))
        .collect();
    if let Some(&maximum) = heights.first()
        && !chosen.contains(&maximum)
    {
        chosen.push(maximum);
        chosen.sort_unstable_by(|left, right| right.cmp(left));
    }
    chosen.truncate(5);
    chosen
}

fn best_option(formats: &[Format], max_height: Option<u64>) -> MediaFormatOption {
    let fallback = "bv*+ba/b";
    let choice = merge_choice(formats, None, fallback).unwrap_or_else(|| Choice {
        selector: fallback.into(),
        size: guessed_best_size(formats),
        bitrate: tallest(formats).and_then(|format| format.tbr),
    });
    MediaFormatOption {
        id: "best".into(),
        label: "Tốt nhất".into(),
        detail: match max_height {
            Some(height) => format!("Video + âm thanh · tối đa {height}p"),
            None => "Video + âm thanh".into(),
        },
        size_bytes: choice.size,
        bitrate_kbps: kbps(choice.bitrate),
        selector: choice.selector,
        audio_format: None,
        badge: Some("Khuyên dùng".into()),
        recommended: true,
    }
}

fn height_option(formats: &[Format], height: u64) -> MediaFormatOption {
    let fallback = format!("bv*[height<={height}]+ba/b[height<={height}]");
    let choice = merge_choice(formats, Some(height), &fallback).unwrap_or_else(|| Choice {
        size: guessed_size(formats, height),
        bitrate: top_bitrate(formats, height),
        selector: fallback,
    });
    MediaFormatOption {
        id: height.to_string(),
        label: format!("{height}p"),
        detail: "Video + âm thanh".into(),
        size_bytes: choice.size,
        bitrate_kbps: kbps(choice.bitrate),
        selector: choice.selector,
        audio_format: None,
        badge: (height == 1080).then(|| "Phổ biến".into()),
        recommended: false,
    }
}

fn m4a_option(formats: &[Format]) -> MediaFormatOption {
    let picked = pick_audio(formats, Some("m4a"));
    let shown = picked.or_else(|| loudest(formats));
    MediaFormatOption {
        id: "audio-m4a".into(),
        label: "Âm thanh M4A".into(),
        detail: "Chỉ âm thanh · giữ chất lượng gốc".into(),
        size_bytes: shown.and_then(|format| format.size),
        bitrate_kbps: kbps(shown.and_then(|format| format.abr.or(format.tbr))),
        selector: with_id(picked, "ba[ext=m4a]/ba"),
        audio_format: Some("m4a".into()),
        badge: None,
        recommended: false,
    }
}

fn mp3_option(formats: &[Format]) -> MediaFormatOption {
    MediaFormatOption {
        id: "audio-mp3".into(),
        label: "Âm thanh MP3".into(),
        detail: "Chuyển sang MP3 bằng ffmpeg".into(),
        size_bytes: None,
        bitrate_kbps: Some(192),
        selector: with_id(pick_audio(formats, None), "ba/b"),
        audio_format: Some("mp3".into()),
        badge: None,
        recommended: false,
    }
}

/// `<hình>+<tiếng>/<fallback>` cho mức `limit` (`None` = cao nhất): yt-dlp vẫn tự chọn khi mã không
/// còn. `None` khi trang không có luồng tách tải thẳng được, format gộp/HLS cao hơn luồng tách
/// (giữ selector cũ, không tụt độ phân giải), tiếng bị hạ bậc hoặc mã không ghim được.
fn merge_choice(formats: &[Format], limit: Option<u64>, fallback: &str) -> Option<Choice> {
    let video = pick_video(formats, limit)?;
    let tallest = video_tier(formats, limit)
        .filter_map(|format| format.height)
        .max();
    if video.height != tallest {
        return None;
    }
    // `ba` cũng xếp `preference` trước: m4a cuối danh sách mà bị hạ bậc thì để yt-dlp tự chọn.
    let audio = pick_audio(formats, Some("m4a")).filter(|audio| top_audio(formats, audio))?;
    let (video_pin, audio_pin) = (video.pin.as_deref()?, audio.pin.as_deref()?);
    Some(Choice {
        selector: format!("{video_pin}+{audio_pin}/{fallback}"),
        size: sum_size(video.size, audio.size),
        bitrate: video.tbr,
    })
}

/// Format có hình ≤ `limit` ở bậc `preference` cao nhất. yt-dlp xếp `preference` trước cả độ phân
/// giải nên `bv*` không chọn bậc dưới: YouTube gán -10 cho format hỏng (ngắn hơn nửa thời lượng).
fn video_tier(formats: &[Format], limit: Option<u64>) -> impl DoubleEndedIterator<Item = &Format> {
    let fits = move |format: &&Format| {
        format.has_video && format.height.is_some_and(|height| within(height, limit))
    };
    let top = formats
        .iter()
        .filter(fits)
        .map(|format| format.preference)
        .max();
    formats
        .iter()
        .filter(fits)
        .filter(move |format| Some(format.preference) == top)
}

/// `audio` ở bậc `preference` cao nhất trong các format chỉ có tiếng (bậc `ba` chọn).
fn top_audio(formats: &[Format], audio: &Format) -> bool {
    formats
        .iter()
        .filter(|format| format.has_audio && !format.has_video)
        .all(|format| format.preference <= audio.preference)
}

/// Hình: format chỉ có hình ở bậc yt-dlp chọn, cao nhất ≤ `limit`; ở chiều cao đó lấy mp4 cuối danh
/// sách (yt-dlp xếp kém → tốt), không có mp4 thì format cuối (webm, ffmpeg ghép). Không lọc cứng
/// mp4: video 4K chỉ có VP9 sẽ bị tụt.
fn pick_video(formats: &[Format], limit: Option<u64>) -> Option<&Format> {
    let split = || video_tier(formats, limit).filter(|format| format.plain && !format.has_audio);
    let height = split().filter_map(|format| format.height).max()?;
    let level = |format: &&Format| format.height == Some(height);
    split()
        .rev()
        .filter(level)
        .find(|format| format.ext == "mp4")
        .or_else(|| split().rev().find(level))
}

/// Tiếng: format chỉ có tiếng cuối danh sách, ưu tiên đuôi `ext`. yt-dlp xếp tiếng gốc sau lồng
/// tiếng, bản DRC trước bản thường, nên cái cuối là tiếng gốc không DRC.
fn pick_audio<'a>(formats: &'a [Format], ext: Option<&str>) -> Option<&'a Format> {
    let alone = || {
        formats
            .iter()
            .rev()
            .filter(|format| format.plain && format.has_audio && !format.has_video)
    };
    ext.and_then(|ext| alone().find(|format| format.ext == ext))
        .or_else(|| alone().next())
}

fn within(height: u64, limit: Option<u64>) -> bool {
    limit.is_none_or(|limit| height <= limit)
}

/// `<mã>/<fallback>`; không có mã cụ thể (hoặc không ghim được) thì giữ selector cũ.
fn with_id(format: Option<&Format>, fallback: &str) -> String {
    match format.and_then(|format| format.pin.as_deref()) {
        Some(pin) => format!("{pin}/{fallback}"),
        None => fallback.to_string(),
    }
}

/// Selector cũ (yt-dlp tự chọn): ước lượng như trước, format gộp cao nhất hoặc hình + tiếng.
fn guessed_best_size(formats: &[Format]) -> Option<u64> {
    formats
        .iter()
        .filter(|format| format.has_video && format.has_audio)
        .max_by_key(|format| format.height.unwrap_or(0))
        .and_then(|format| format.size)
        .or_else(|| {
            sum_size(
                tallest(formats).and_then(|format| format.size),
                loudest(formats).and_then(|format| format.size),
            )
        })
}

/// Selector cũ: format lớn nhất ở mức đó, cộng tiếng nếu chỉ có hình.
fn guessed_size(formats: &[Format], height: u64) -> Option<u64> {
    let audio = loudest(formats).and_then(|format| format.size);
    formats
        .iter()
        .filter(|format| format.has_video && format.height == Some(height))
        .max_by_key(|format| format.size.unwrap_or(0))
        .and_then(|format| {
            if format.has_audio {
                format.size
            } else {
                sum_size(format.size, audio)
            }
        })
}

fn top_bitrate(formats: &[Format], height: u64) -> Option<f64> {
    formats
        .iter()
        .filter(|format| format.has_video && format.height == Some(height))
        .filter_map(|format| format.tbr)
        .fold(None::<f64>, |current, rate| {
            Some(current.map_or(rate, |value| value.max(rate)))
        })
}

fn tallest(formats: &[Format]) -> Option<&Format> {
    formats
        .iter()
        .filter(|format| format.has_video)
        .max_by_key(|format| format.height.unwrap_or(0))
}

/// Tiếng bitrate cao nhất (ước lượng cho selector cũ).
fn loudest(formats: &[Format]) -> Option<&Format> {
    formats
        .iter()
        .filter(|format| format.has_audio && !format.has_video)
        .max_by(|left, right| audio_rank(left).total_cmp(&audio_rank(right)))
}

fn kbps(rate: Option<f64>) -> Option<u64> {
    rate.map(|rate| rate.round() as u64)
}

/// `items` = mọi format của trang, để biết mã nào do yt-dlp đánh số.
fn parse_format(value: &Value, items: &[Value]) -> Option<Format> {
    let text = |key: &str| field(value, key);
    let ext = text("ext").to_ascii_lowercase();
    if ext.is_empty() {
        return None;
    }
    let height = value.get("height").and_then(Value::as_u64);
    let vcodec = value
        .get("vcodec")
        .and_then(Value::as_str)
        .unwrap_or("none");
    let acodec = value
        .get("acodec")
        .and_then(Value::as_str)
        .unwrap_or("none");
    let id = text("format_id");
    let plain = matches!(text("protocol"), "http" | "https")
        && !resolve::flagged(value.get("has_drm"))
        && probe_cache::plain_id(id);
    let size = value
        .get("filesize")
        .and_then(Value::as_u64)
        .or_else(|| value.get("filesize_approx").and_then(Value::as_u64));
    Some(Format {
        pin: pin(id, text("language"), items),
        ext,
        height,
        has_video: vcodec != "none" && height.is_some(),
        has_audio: acodec != "none",
        plain,
        preference: value
            .get("preference")
            .and_then(Value::as_f64)
            .map_or(-1, |preference| preference.floor() as i64),
        size,
        tbr: value.get("tbr").and_then(Value::as_f64),
        abr: value.get("abr").and_then(Value::as_f64),
    })
}

/// Mã ghi vào selector. Nhiều format trùng mã thì yt-dlp thêm số thứ tự (`140-0`…`140-23`, tiếng gốc
/// số lớn nhất): số chỉ là vị trí, trang thêm bản lồng tiếng là `140-23` trỏ sang tiếng khác. Khi đó
/// ghim kèm ngôn ngữ (`140-23[language=en-US]`), lệch thì yt-dlp rơi về phần sau `/`; nhóm không có
/// ngôn ngữ riêng cho mã này thì không ghim.
fn pin(id: &str, language: &str, items: &[Value]) -> Option<String> {
    let Some(base) = numbered_base(id) else {
        return Some(id.to_string());
    };
    let group: Vec<&Value> = items
        .iter()
        .filter(|item| numbered_base(field(item, "format_id")) == Some(base))
        .collect();
    if group.len() < 2 {
        return Some(id.to_string());
    }
    let alone = group
        .iter()
        .filter(|item| field(item, "language") == language)
        .count()
        == 1;
    (alone && probe_cache::plain_language(language)).then(|| format!("{id}[language={language}]"))
}

/// `140-23` → `140`: phần trước số thứ tự yt-dlp có thể đã thêm.
fn numbered_base(id: &str) -> Option<&str> {
    let (base, number) = id.rsplit_once('-')?;
    let digits = !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit());
    (digits && !base.is_empty()).then_some(base)
}

fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).map_or("", str::trim)
}

fn audio_rank(format: &Format) -> f64 {
    format.abr.or(format.tbr).unwrap_or(0.0)
}

fn sum_size(video: Option<u64>, audio: Option<u64>) -> Option<u64> {
    match (video, audio) {
        (Some(video), Some(audio)) => video.checked_add(audio),
        (Some(video), None) => Some(video),
        (None, Some(audio)) => Some(audio),
        (None, None) => None,
    }
}

pub(super) fn classify_stderr(stderr: &str) -> String {
    let message = stderr.to_ascii_lowercase();
    if message.contains("drm") {
        return "Nội dung có DRM; UTD không vượt qua bảo vệ bản quyền.".into();
    }
    if message.contains("sign in")
        || message.contains("log in")
        || message.contains("login")
        || message.contains("cookies")
        || message.contains("not a bot")
    {
        return "Nguồn này cần đăng nhập hoặc cookie. UTD chưa gửi cookie ở bản này.".into();
    }
    if message.contains("unsupported url") || message.contains("no video formats found") {
        return "URL này chưa được hỗ trợ hoặc không có định dạng tải được.".into();
    }
    if message.contains("private video")
        || message.contains("video unavailable")
        || message.contains("has been removed")
    {
        return "Media không công khai hoặc đã bị gỡ.".into();
    }
    if message.contains("timed out")
        || message.contains("connection")
        || message.contains("network")
    {
        return "Không kết nối được tới nguồn media.".into();
    }
    "Không đọc được thông tin media từ URL này.".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_info() -> Value {
        json!({
            "title": "Video thử nghiệm",
            "webpage_url": "https://example.com/watch",
            "uploader": "UTD",
            "duration": 125.4,
            "thumbnail": "https://example.com/thumb.jpg",
            "formats": [
                {
                    "format_id": "137",
                    "protocol": "https",
                    "ext": "mp4",
                    "height": 1080,
                    "vcodec": "avc1",
                    "acodec": "none",
                    "filesize": 50_000_000,
                    "tbr": 2500.0
                },
                {
                    "format_id": "140",
                    "protocol": "https",
                    "ext": "m4a",
                    "vcodec": "none",
                    "acodec": "mp4a",
                    "filesize": 5_000_000,
                    "abr": 128.0
                },
                {
                    "format_id": "134",
                    "protocol": "https",
                    "ext": "mp4",
                    "height": 360,
                    "vcodec": "avc1",
                    "acodec": "none",
                    "filesize": 10_000_000,
                    "tbr": 800.0
                }
            ]
        })
    }

    fn video(id: &str, ext: &str, height: u64, size: u64) -> Value {
        json!({
            "format_id": id,
            "protocol": "https",
            "ext": ext,
            "height": height,
            "vcodec": if ext == "webm" { "vp9" } else { "av01.0.09M.08" },
            "acodec": "none",
            "filesize": size,
            "tbr": size as f64 / 10.0
        })
    }

    fn audio(id: &str, ext: &str, size: u64) -> Value {
        json!({
            "format_id": id,
            "protocol": "https",
            "ext": ext,
            "vcodec": "none",
            "acodec": if ext == "m4a" { "mp4a.40.2" } else { "opus" },
            "filesize": size,
            "abr": 130.0
        })
    }

    /// Luồng HLS (`m3u8_native`, đuôi mp4): engine không tải thẳng được.
    fn hls(id: &str, height: u64) -> Value {
        json!({
            "format_id": id,
            "protocol": "m3u8_native",
            "ext": "mp4",
            "height": height,
            "vcodec": "vp09.00.41.08",
            "acodec": "none"
        })
    }

    fn combined(id: &str, height: u64, size: u64) -> Value {
        json!({
            "format_id": id,
            "protocol": "https",
            "ext": "mp4",
            "height": height,
            "vcodec": "avc1",
            "acodec": "mp4a.40.2",
            "filesize": size
        })
    }

    fn dub(id: &str, ext: &str, language: &str, size: u64) -> Value {
        let mut format = audio(id, ext, size);
        format["language"] = json!(language);
        format
    }

    fn ranked(mut format: Value, preference: i64) -> Value {
        format["preference"] = json!(preference);
        format
    }

    fn info(formats: Vec<Value>) -> Value {
        json!({ "title": "Video", "webpage_url": "https://example.com/watch", "formats": formats })
    }

    fn option<'a>(result: &'a MediaProbeResult, id: &str) -> &'a MediaFormatOption {
        result
            .formats
            .iter()
            .find(|format| format.id == id)
            .unwrap_or_else(|| panic!("thiếu lựa chọn {id}"))
    }

    fn selector<'a>(result: &'a MediaProbeResult, id: &str) -> &'a str {
        option(result, id).selector.as_str()
    }

    #[test]
    fn builds_only_real_heights_and_audio_options() {
        let result =
            build_result("https://example.com/watch", &sample_info(), true).expect("probe result");
        let labels: Vec<&str> = result
            .formats
            .iter()
            .map(|format| format.label.as_str())
            .collect();
        assert!(labels.contains(&"Tốt nhất"));
        assert!(labels.contains(&"1080p"));
        assert!(labels.contains(&"360p"));
        assert!(!labels.contains(&"720p"));
        assert!(labels.contains(&"Âm thanh M4A"));
        assert!(labels.contains(&"Âm thanh MP3"));
        assert_eq!(result.duration_seconds, Some(125));
        let best = &result.formats[0];
        assert!(best.recommended);
        assert_eq!(best.size_bytes, Some(55_000_000));
        assert_eq!(best.selector, "137+140/bv*+ba/b");
    }

    #[test]
    fn picks_the_last_mp4_and_the_original_m4a_without_drc() {
        // Thứ tự như yt-dlp (kém → tốt): lồng tiếng, DRC, rồi tiếng gốc; HLS chen giữa các mức.
        let formats = vec![
            audio("140-1", "m4a", 900),
            audio("251-1", "webm", 800),
            audio("140-drc", "m4a", 1000),
            audio("251-drc", "webm", 950),
            audio("140", "m4a", 1000),
            audio("251", "webm", 950),
            combined("18", 360, 500),
            video("136", "mp4", 720, 3000),
            video("247", "webm", 720, 2500),
            video("398", "mp4", 720, 2000),
            hls("617", 1080),
            video("137", "mp4", 1080, 6000),
            video("248", "webm", 1080, 5000),
            video("399", "mp4", 1080, 4000),
            hls("616", 1080),
        ];
        let result = build_result("https://example.com/watch", &info(formats), true).unwrap();
        assert_eq!(selector(&result, "best"), "399+140/bv*+ba/b");
        assert_eq!(
            option(&result, "best").size_bytes,
            Some(5000),
            "đúng 399 + 140"
        );
        assert_eq!(option(&result, "best").bitrate_kbps, Some(400));
        assert_eq!(
            selector(&result, "1080"),
            "399+140/bv*[height<=1080]+ba/b[height<=1080]"
        );
        assert_eq!(option(&result, "1080").badge.as_deref(), Some("Phổ biến"));
        assert_eq!(
            selector(&result, "720"),
            "398+140/bv*[height<=720]+ba/b[height<=720]"
        );
        assert_eq!(option(&result, "720").size_bytes, Some(3000));
        assert_eq!(
            selector(&result, "360"),
            "bv*[height<=360]+ba/b[height<=360]",
            "mức chỉ có format gộp giữ selector cũ"
        );
        assert_eq!(selector(&result, "audio-m4a"), "140/ba[ext=m4a]/ba");
        assert_eq!(option(&result, "audio-m4a").size_bytes, Some(1000));
        assert_eq!(selector(&result, "audio-mp3"), "251/ba/b", "tiếng tốt nhất");
    }

    #[test]
    fn keeps_4k_when_it_only_exists_as_vp9() {
        let formats = vec![
            audio("140", "m4a", 100),
            audio("251", "webm", 90),
            video("137", "mp4", 1080, 600),
            video("248", "webm", 1080, 500),
            video("313", "webm", 2160, 2000),
        ];
        let result = build_result("https://example.com/watch", &info(formats), true).unwrap();
        assert_eq!(selector(&result, "best"), "313+140/bv*+ba/b");
        assert_eq!(option(&result, "best").size_bytes, Some(2100));
        assert!(selector(&result, "2160").starts_with("313+140/"));
        assert!(
            selector(&result, "1080").starts_with("137+140/"),
            "mp4 ở 1080p"
        );
    }

    #[test]
    fn keeps_old_selectors_for_pages_with_only_combined_formats() {
        let formats = vec![combined("18", 360, 100), combined("22", 720, 300)];
        let result = build_result("https://example.com/watch", &info(formats), true).unwrap();
        assert_eq!(selector(&result, "best"), "bv*+ba/b");
        assert_eq!(option(&result, "best").size_bytes, Some(300));
        assert_eq!(
            selector(&result, "720"),
            "bv*[height<=720]+ba/b[height<=720]"
        );
        assert_eq!(selector(&result, "audio-m4a"), "ba[ext=m4a]/ba");
        assert_eq!(selector(&result, "audio-mp3"), "ba/b");
    }

    #[test]
    fn never_picks_split_streams_below_a_taller_combined_or_hls_format() {
        let taller = vec![
            audio("140", "m4a", 100),
            video("136", "mp4", 720, 300),
            combined("37", 1080, 900),
            hls("625", 2160),
        ];
        let result = build_result("https://example.com/watch", &info(taller), true).unwrap();
        assert_eq!(
            selector(&result, "best"),
            "bv*+ba/b",
            "không tụt xuống 720p"
        );
        assert_eq!(
            selector(&result, "1080"),
            "bv*[height<=1080]+ba/b[height<=1080]"
        );
        assert!(selector(&result, "720").starts_with("136+140/"));
    }

    #[test]
    fn pins_ids_numbered_by_yt_dlp_together_with_their_language() {
        // yt-dlp đánh số các tiếng trùng itag theo thứ tự đã xếp: lồng tiếng trước, tiếng gốc cuối.
        let formats = vec![
            dub("140-0", "m4a", "de", 900),
            dub("251-0", "webm", "de", 800),
            dub("140-1", "m4a", "fr", 900),
            dub("251-1", "webm", "fr", 800),
            dub("140-2", "m4a", "en-US", 1000),
            dub("251-2", "webm", "en-US", 950),
            video("399", "mp4", 1080, 4000),
        ];
        let result = build_result("https://example.com/watch", &info(formats), true).unwrap();
        let best = selector(&result, "best");
        assert_eq!(best, "399+140-2[language=en-US]/bv*+ba/b");
        assert_eq!(option(&result, "best").size_bytes, Some(5000));
        assert_eq!(
            probe_cache::selector_ids(best),
            Some(vec!["399", "140-2"]),
            "cache vẫn đọc được mã"
        );
        assert_eq!(
            selector(&result, "audio-m4a"),
            "140-2[language=en-US]/ba[ext=m4a]/ba"
        );
        assert_eq!(selector(&result, "audio-mp3"), "251-2[language=en-US]/ba/b");
    }

    #[test]
    fn keeps_old_selectors_when_a_numbered_id_has_no_unique_language() {
        let formats = vec![
            audio("140-0", "m4a", 900),
            audio("140-1", "m4a", 1000),
            video("399", "mp4", 1080, 4000),
        ];
        let result = build_result("https://example.com/watch", &info(formats), true).unwrap();
        assert_eq!(selector(&result, "best"), "bv*+ba/b");
        assert_eq!(selector(&result, "audio-m4a"), "ba[ext=m4a]/ba");

        let items = vec![
            dub("140-0", "m4a", "en", 1),
            dub("140-1", "m4a", "en", 1),
            dub("140-2", "m4a", "en US", 1),
            dub("140-drc", "m4a", "en", 1),
            video("http-1080", "mp4", 1080, 1),
        ];
        assert_eq!(pin("140-1", "en", &items), None, "trùng ngôn ngữ");
        assert_eq!(pin("140-2", "en US", &items), None, "ngôn ngữ cần ngoặc");
        assert_eq!(pin("140-drc", "en", &items).as_deref(), Some("140-drc"));
        assert_eq!(
            pin("http-1080", "", &items).as_deref(),
            Some("http-1080"),
            "đuôi số của chính extractor, không trùng ai"
        );
    }

    #[test]
    fn skips_formats_yt_dlp_marks_as_damaged() {
        // YouTube: `preference` -10 khi format ngắn hơn nửa thời lượng; yt-dlp xếp chúng dưới hết.
        let formats = vec![
            audio("140", "m4a", 100),
            ranked(video("401", "mp4", 2160, 500), -10),
            video("248", "webm", 1080, 300),
            video("313", "webm", 2160, 2000),
        ];
        let result = build_result("https://example.com/watch", &info(formats), true).unwrap();
        assert_eq!(selector(&result, "best"), "313+140/bv*+ba/b");
        assert_eq!(option(&result, "best").size_bytes, Some(2100));
        assert!(selector(&result, "2160").starts_with("313+140/"));
        assert!(selector(&result, "1080").starts_with("248+140/"));

        let top_damaged = vec![
            audio("140", "m4a", 100),
            ranked(video("401", "mp4", 2160, 500), -10),
            ranked(video("399", "mp4", 1080, 400), -1),
        ];
        let result = build_result("https://example.com/watch", &info(top_damaged), true).unwrap();
        assert_eq!(
            selector(&result, "best"),
            "399+140/bv*+ba/b",
            "-1 = mặc định"
        );
        assert_eq!(
            option(&result, "best").detail,
            "Video + âm thanh · tối đa 1080p"
        );
        assert!(
            !result.formats.iter().any(|format| format.id == "2160"),
            "không có 2160p lành"
        );

        let damaged_m4a = vec![
            ranked(audio("140", "m4a", 100), -10),
            audio("251", "webm", 90),
            video("399", "mp4", 1080, 400),
        ];
        let result = build_result("https://example.com/watch", &info(damaged_m4a), true).unwrap();
        assert_eq!(
            selector(&result, "best"),
            "bv*+ba/b",
            "để yt-dlp chọn tiếng lành"
        );
        assert_eq!(selector(&result, "audio-mp3"), "251/ba/b");
    }

    /// Chạy: đặt `UTD_PROBE_JSON` = file `yt-dlp -J`, rồi
    /// `cargo test -p app --lib real_probe -- --ignored --nocapture`.
    #[test]
    #[ignore = "cần JSON thật của yt-dlp (UTD_PROBE_JSON)"]
    fn real_probe_json() {
        let path = std::env::var("UTD_PROBE_JSON").expect("UTD_PROBE_JSON");
        let value: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let result = build_result("https://www.youtube.com/watch?v=09OGSRptb0M", &value, true)
            .expect("probe result");
        for format in &result.formats {
            println!(
                "{:<14} {:<50} {:?}",
                format.label, format.selector, format.size_bytes
            );
        }
        // Giờ lúc tạo JSON, không phải giờ chạy: link trong file hết hạn sau ~6 giờ.
        let now = value
            .get("epoch")
            .and_then(Value::as_i64)
            .unwrap_or_else(resolve::now_secs);
        probe_cache::remember(&value, &result.url, now);
        let best = selector(&result, "best");
        let cached = probe_cache::lookup(&[result.webpage_url.as_str()], best, now);
        for stream in cached.iter().flat_map(|resolved| &resolved.streams) {
            println!(
                "cache {} {} {:?} headers={}",
                stream.format_id,
                stream.protocol,
                stream.exact_size,
                stream.headers.len()
            );
        }
        if result.webpage_url.contains("09OGSRptb0M") {
            assert_eq!(cached.map(|resolved| resolved.streams.len()), Some(2));
            assert!(selector(&result, "best").starts_with("399+140/"));
            assert!(selector(&result, "1080").starts_with("399+140/"));
            assert!(selector(&result, "720").starts_with("398+140/"));
            assert!(selector(&result, "480").starts_with("397+140/"));
            assert_eq!(selector(&result, "audio-m4a"), "140/ba[ext=m4a]/ba");
        }
    }

    #[test]
    fn omits_mp3_without_ffmpeg() {
        let result =
            build_result("https://example.com/watch", &sample_info(), false).expect("probe result");
        assert!(!result
            .formats
            .iter()
            .any(|format| format.audio_format.as_deref() == Some("mp3")));
    }

    #[test]
    fn rejects_drm_and_live_content() {
        let mut drm = sample_info();
        drm["_has_drm"] = json!(true);
        assert!(build_result("https://example.com", &drm, true).is_err());

        let mut live = sample_info();
        live["is_live"] = json!(true);
        assert!(build_result("https://example.com", &live, true).is_err());
    }

    #[test]
    fn playlists_point_to_the_channel_flow() {
        let playlist = json!({
            "_type": "playlist",
            "id": "MS4wLjABAAAA",
            "title": "tiktok",
            "entries": [],
            "webpage_url": "https://www.tiktok.com/@tiktok",
        });
        let error = build_result("https://vm.tiktok.com/ZMabc/", &playlist, true).unwrap_err();
        assert_eq!(error, "channel_page: https://www.tiktok.com/@tiktok");
    }

    #[test]
    fn rejects_info_without_usable_formats() {
        let value = json!({ "title": "Empty", "formats": [] });
        assert!(build_result("https://example.com", &value, true).is_err());
    }
}
