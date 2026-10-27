use std::process::Stdio;

use serde::Serialize;
use serde_json::Value;
use tokio::time::{timeout, Duration};

use crate::media::{
    auto, probe_cache, resolve, run_signed, sign_in_pipe_error, tools, yt_dlp_signed,
};

/// Lỗi có mã ⇒ câu "[mã] lời" như các lỗi Phân tích khác (giao diện dịch theo mã).
fn coded(error: utd_engine::TaskError) -> ProbeFailure {
    ProbeFailure::from(format!("[{}] {}", error.code, error.message))
}

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
    /// Như `formats` nhưng đủ mức chiều cao tới 144p: bảng chọn định dạng trên trang (tiện ích, GĐ3).
    #[serde(skip)]
    pub every_height: Vec<MediaFormatOption>,
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
    /// Mã đánh số mà cả nhóm cùng một ngôn ngữ (hai CDN mirror của TikTok: `…-0`, `…-1` cùng chất lượng):
    /// format có hình (gộp hoặc chỉ hình) dùng khi `pin` là `None`; tiếng riêng không dùng.
    mirror_pin: Option<String>,
    ext: String,
    /// Chiều cao pixel thật (video dọc 1080×1920 là 1920): đúng thứ yt-dlp lọc bằng `[height<=…]`.
    height: Option<u64>,
    width: Option<u64>,
    has_video: bool,
    has_audio: bool,
    /// Chỉ tiếng: có tiếng và `vcodec` là `none` (đúng thứ `ba` của yt-dlp chọn). Khác `!has_video`: format có
    /// codec hình mà thiếu `height` (TikTok `download`) không phải hình, cũng không phải chỉ tiếng.
    audio_only: bool,
    /// http(s), không nghi DRM, mã ghi được vào selector: engine UTD tải thẳng được.
    plain: bool,
    /// `preference` của extractor, thiếu là -1 như yt-dlp; YouTube gán -10 cho format hỏng.
    preference: i64,
    size: Option<u64>,
    tbr: Option<f64>,
    abr: Option<f64>,
    /// Nhạc nền TikTok (format `audio`): nhạc của bài đăng, không phải tiếng của video. Video dùng bài hát thịnh
    /// hành thì nhạc dài hơn video nhiều (đo: 397 s cho hình 175 s), bắt đầu từ đầu bài nên lệch đoạn và mất lời
    /// nói đè lên nhạc. Chỉ đánh dấu khi trang có format gộp để lấy tiếng thật (`sound_source`); bài ảnh chỉ
    /// có nhạc thì nhạc vẫn là tiếng riêng.
    soundtrack: bool,
}

impl Format {
    /// Mức độ phân giải theo cạnh ngắn, như yt-dlp xếp `res`: video dọc 1080×1920 là 1080p, ngang không
    /// đổi. Thiếu `width` thì lấy `height`.
    fn level(&self) -> Option<u64> {
        let height = self.height?;
        Some(self.width.map_or(height, |width| width.min(height)))
    }

    /// Mã ghi vào selector cho format có hình (gộp hoặc chỉ hình): `pin`, không được thì mã mirror. Hai mirror
    /// cùng chất lượng, không có chuyện lồng tiếng ở hình, nên ghim mirror cuối (mirror yt-dlp xếp sau). Tiếng
    /// riêng chỉ theo `pin`.
    fn video_pin(&self) -> Option<&str> {
        self.pin.as_deref().or(self.mirror_pin.as_deref())
    }
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
/// `grant`: đăng nhập của trình duyệt cho trang này (S-14), cookie qua pipe.
pub async fn probe(
    tools: &tools::MediaTools,
    url: &str,
    ffmpeg_available: bool,
    grant: Option<&crate::auth::Grant>,
) -> Result<MediaProbeResult, ProbeFailure> {
    let (mut command, pipe) = yt_dlp_signed(tools, grant).map_err(coded)?;
    command
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
    let (output, cookies_read) = timeout(Duration::from_secs(45), run_signed(command, pipe.as_ref()))
        .await
        .map_err(|_| "[probe.timeout] Nguồn media phản hồi quá lâu.".to_string())?
        .map_err(|error| format!("[media.spawn] Không chạy được yt-dlp: {error}"))?;
    // Có đăng nhập mà yt-dlp không đọc cookie: kết quả là của người chưa đăng nhập, không dùng.
    if !cookies_read {
        return Err(coded(sign_in_pipe_error()));
    }
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ProbeFailure {
            message: classify_stderr(&stderr),
            outdated: auto::outdated(&stderr),
        });
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "[probe.bad_output] yt-dlp trả về dữ liệu không đọc được.".to_string())?;
    let result = build_result(url, &value, ffmpeg_available)?;
    // Lượt tải ngay sau đó dùng lại các luồng này, khỏi chạy yt-dlp lần hai.
    probe_cache::remember(&value, url, resolve::now_secs());
    Ok(result)
}

pub(super) fn build_result(
    original_url: &str,
    value: &Value,
    ffmpeg_available: bool,
) -> Result<MediaProbeResult, String> {
    if value.get("_has_drm").and_then(Value::as_bool) == Some(true) {
        return Err("[media.drm] Nội dung có DRM; UTD không vượt qua bảo vệ bản quyền.".into());
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
        return Err("[probe.live] Chưa hỗ trợ tải livestream.".into());
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
    let formats = parse_formats(value, is_tiktok(value, original_url))?;
    Ok(MediaProbeResult {
        url: original_url.to_string(),
        webpage_url,
        title,
        uploader,
        duration_seconds,
        thumbnail,
        formats: format_options(&formats, ffmpeg_available, false),
        ffmpeg_available,
        every_height: format_options(&formats, ffmpeg_available, true),
    })
}

/// Trang TikTok. yt-dlp ghi `extractor_key` = `TikTok`; JSON thiếu trường đó thì xét tên miền của link.
fn is_tiktok(value: &Value, original_url: &str) -> bool {
    if let Some(key) = value.get("extractor_key").and_then(Value::as_str) {
        return key.eq_ignore_ascii_case("tiktok");
    }
    [
        value.get("webpage_url").and_then(Value::as_str),
        Some(original_url),
    ]
    .into_iter()
    .flatten()
    .filter_map(|link| utd_engine::validate_url(link).ok())
    .any(|url| {
        url.host_str()
            .is_some_and(|host| host == "tiktok.com" || host.ends_with(".tiktok.com"))
    })
}

/// Giữ đúng thứ tự của yt-dlp: kém trước, tốt sau.
fn parse_formats(value: &Value, tiktok: bool) -> Result<Vec<Format>, String> {
    let items = value
        .get("formats")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut formats: Vec<Format> = items
        .iter()
        .filter(|item| item.get("has_drm").and_then(Value::as_bool) != Some(true))
        .filter_map(|item| parse_format(item, items))
        .collect();
    if formats
        .iter()
        .all(|format| !format.has_video && !format.has_audio)
    {
        return Err("[probe.no_formats] Không tìm thấy định dạng media có thể tải.".into());
    }
    if tiktok && sound_source(&formats, None).is_some() {
        for format in formats
            .iter_mut()
            .filter(|format| format.audio_only && format.pin.as_deref() == Some("audio"))
        {
            format.soundtrack = true;
        }
    }
    Ok(formats)
}

fn format_options(formats: &[Format], ffmpeg_available: bool, every: bool) -> Vec<MediaFormatOption> {
    let levels = video_levels(formats);
    let mut options = vec![best_option(formats, levels.first().copied())];
    // Chỉ một mức để chọn thì dòng mức giống hệt "Tốt nhất": không thêm.
    let listed = listed_levels(&levels, every);
    if listed.len() > 1 {
        for level in listed {
            options.push(level_option(formats, level));
        }
    }
    if formats.iter().any(|format| format.has_audio) {
        options.push(m4a_option(formats));
        if ffmpeg_available {
            options.push(mp3_option(formats));
        }
    }
    options
}

/// Mức phân giải (cạnh ngắn) có thật, cả format gộp, cao trước; không kể format bị hạ bậc (hỏng).
fn video_levels(formats: &[Format]) -> Vec<u64> {
    let mut levels: Vec<u64> = video_tier(formats, None)
        .filter_map(Format::level)
        .filter(|level| *level > 0)
        .collect();
    levels.sort_unstable_by(|left, right| right.cmp(left));
    levels.dedup();
    levels
}

/// Mức quen thuộc có thật, thêm mức cao nhất nếu lạ (vd. 1600p). Hộp Tải media: tối đa 5 mức; `every` (bảng
/// trên trang): mọi mức có thật từ 144p lên (video dọc TikTok ra thêm 576p).
fn listed_levels(levels: &[u64], every: bool) -> Vec<u64> {
    const FAMILIAR: &[u64] = &[2160, 1440, 1080, 720, 480, 360];
    let mut chosen: Vec<u64> = levels
        .iter()
        .copied()
        .filter(|level| {
            if every {
                *level >= 144
            } else {
                FAMILIAR.contains(level)
            }
        })
        .collect();
    if let Some(&maximum) = levels.first()
        && !chosen.contains(&maximum)
    {
        chosen.push(maximum);
        chosen.sort_unstable_by(|left, right| right.cmp(left));
    }
    if !every {
        chosen.truncate(5);
    }
    chosen
}

/// Phần dự phòng sau `/` (yt-dlp tự chọn khi mã ghim không còn).
struct Fallback {
    /// yt-dlp tự ghép hình + tiếng riêng.
    open: String,
    /// Chỉ format gộp sẵn: tiếng đúng của video. Trang có nhạc nền (TikTok) không được rơi về `+ba`, vì `ba` là
    /// nhạc nền.
    whole: String,
}

impl Fallback {
    /// `filter` = bộ lọc chiều cao (`[height<=1280]`) hoặc rỗng.
    fn new(filter: &str) -> Self {
        Self {
            open: format!("bv*{filter}+ba/b{filter}"),
            whole: format!("b{filter}"),
        }
    }

    /// Selector khi không ghim được mã nào.
    fn unpinned(&self, formats: &[Format]) -> &str {
        if has_soundtrack(formats) {
            &self.whole
        } else {
            &self.open
        }
    }
}

fn best_option(formats: &[Format], max_level: Option<u64>) -> MediaFormatOption {
    let fallback = Fallback::new("");
    let choice = merge_choice(formats, None, &fallback).unwrap_or_else(|| Choice {
        selector: fallback.unpinned(formats).into(),
        size: guessed_best_size(formats),
        bitrate: tallest(formats).and_then(|format| format.tbr),
    });
    MediaFormatOption {
        id: "best".into(),
        label: "Tốt nhất".into(),
        detail: match max_level {
            Some(level) => format!("Video + âm thanh · tối đa {level}p"),
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

/// Dòng một mức (cạnh ngắn): `id` và nhãn là mức (`1080` / `1080p`, video dọc 1080×1920 cũng vậy).
fn level_option(formats: &[Format], level: u64) -> MediaFormatOption {
    let height = level_height(formats, level);
    let fallback = Fallback::new(&format!("[height<={height}]"));
    let choice = merge_choice(formats, Some(level), &fallback).unwrap_or_else(|| Choice {
        size: guessed_size(formats, level),
        bitrate: top_bitrate(formats, level),
        selector: fallback.unpinned(formats).into(),
    });
    MediaFormatOption {
        id: level.to_string(),
        label: format!("{level}p"),
        detail: "Video + âm thanh".into(),
        size_bytes: choice.size,
        bitrate_kbps: kbps(choice.bitrate),
        selector: choice.selector,
        audio_format: None,
        badge: (level == 1080).then(|| "Phổ biến".into()),
        recommended: false,
    }
}

/// Bộ lọc dự phòng `[height<=…]` của yt-dlp so với chiều cao pixel thật, không phải cạnh ngắn: chiều cao lớn
/// nhất của các format ở mức `level` (ngang = chính mức đó; dọc 1080×1920 ở mức 1080 là 1920).
fn level_height(formats: &[Format], level: u64) -> u64 {
    formats
        .iter()
        .filter(|format| format.has_video && format.level() == Some(level))
        .filter_map(|format| format.height)
        .max()
        .unwrap_or(level)
}

fn m4a_option(formats: &[Format]) -> MediaFormatOption {
    // Trang có nhạc nền (TikTok): tiếng của video nằm trong format gộp. Cỡ và bitrate của format gộp là của cả
    // hình, không phải của tiếng (file ra chỉ còn tiếng) ⇒ không ghi.
    let from_video = video_sound(formats);
    let picked = pick_audio(formats, Some("m4a"));
    let shown = from_video.or(picked).or_else(|| loudest(formats));
    MediaFormatOption {
        id: "audio-m4a".into(),
        label: "Âm thanh M4A".into(),
        detail: "Chỉ âm thanh · giữ chất lượng gốc".into(),
        size_bytes: shown
            .filter(|_| from_video.is_none())
            .and_then(|format| format.size),
        bitrate_kbps: kbps(
            shown
                .filter(|_| from_video.is_none())
                .and_then(|format| format.abr.or(format.tbr)),
        ),
        selector: match from_video {
            Some(source) => sound_selector(source),
            None => with_id(picked, "ba[ext=m4a]/ba"),
        },
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
        selector: match video_sound(formats) {
            Some(source) => sound_selector(source),
            None => with_id(pick_audio(formats, None), "ba/b"),
        },
        audio_format: Some("mp3".into()),
        badge: None,
        recommended: false,
    }
}

/// `<mã format gộp>/b`: lấy tiếng từ file có cả hình lẫn tiếng (ffmpeg bỏ hình khi tách tiếng). Mã mất thì `b`,
/// không phải `ba` (nhạc nền).
fn sound_selector(source: &Format) -> String {
    source
        .video_pin()
        .map_or_else(|| "b".into(), |pin| format!("{pin}/b"))
}

/// Selector ghim mã cho mức `limit` (cạnh ngắn, `None` = cao nhất), kèm `/<fallback>`: yt-dlp vẫn tự chọn khi
/// mã không còn. Ưu tiên luồng tách `<hình>+<tiếng>`, không có thì một format gộp sẵn hình + tiếng (TikTok…).
/// `None` (giữ selector cũ, yt-dlp tự chọn) khi không ghim được mà không tụt độ phân giải.
fn merge_choice(formats: &[Format], limit: Option<u64>, fallback: &Fallback) -> Option<Choice> {
    split_choice(formats, limit, &fallback.open)
        .or_else(|| combined_choice(formats, limit, &fallback.open))
        .or_else(|| borrowed_sound_choice(formats, limit, &fallback.whole))
}

/// `<hình>+<tiếng>/<fallback>`. `None` khi trang không có luồng tách tải thẳng được, format gộp/HLS cao hơn
/// luồng tách (không tụt độ phân giải), tiếng bị hạ bậc hoặc mã không ghim được. Hình chỉ hình có hai mirror
/// (TikTok `…-0`, `…-1`) ghim mirror cuối (`Format::video_pin`).
fn split_choice(formats: &[Format], limit: Option<u64>, fallback: &str) -> Option<Choice> {
    let video = pick_video(formats, limit)?;
    let tallest = video_tier(formats, limit).filter_map(Format::level).max();
    if video.level() != tallest {
        return None;
    }
    // `ba` cũng xếp `preference` trước: m4a cuối danh sách mà bị hạ bậc thì để yt-dlp tự chọn.
    let audio = pick_audio(formats, Some("m4a")).filter(|audio| top_audio(formats, audio))?;
    let (video_pin, audio_pin) = (video.video_pin()?, audio.pin.as_deref()?);
    Some(Choice {
        selector: format!("{video_pin}+{audio_pin}/{fallback}"),
        size: sum_size(video.size, audio.size),
        bitrate: video.tbr,
    })
}

/// `<mã>/<fallback>` cho một format gộp sẵn hình + tiếng, khi không có luồng tách để ghim (TikTok: mọi format
/// đều gộp, nên trước đây selector là từ khoá và lượt tải phải hỏi yt-dlp lần hai). Cùng luật `split_choice`:
/// bậc `preference` cao nhất, cao nhất ≤ `limit`, và không có luồng nào cao hơn (HLS, hình tách) ⇒ không tụt
/// độ phân giải. Ở mức đó lấy mp4 cuối danh sách (yt-dlp xếp kém → tốt). Mã đánh số của hai CDN mirror
/// (`…-0`, `…-1`) ghim thẳng; lệch thì yt-dlp rơi về `<fallback>`.
fn combined_choice(formats: &[Format], limit: Option<u64>, fallback: &str) -> Option<Choice> {
    let tallest = video_tier(formats, limit).filter_map(Format::level).max()?;
    let whole = || {
        video_tier(formats, limit)
            .rev()
            .filter(|format| format.plain && format.has_audio && format.level() == Some(tallest))
    };
    let picked = whole()
        .find(|format| format.ext == "mp4")
        .or_else(|| whole().next())?;
    let pin = picked.video_pin()?;
    Some(Choice {
        selector: format!("{pin}/{fallback}"),
        size: picked.size,
        bitrate: picked.tbr,
    })
}

/// `<hình>+<format gộp>/<fallback>` khi bậc cao nhất (≤ `limit`) chỉ có hình mà tiếng riêng là nhạc nền (TikTok
/// video dài: 720p h265 chỉ hình, tiếng riêng `audio` là nhạc nền): hình cao nhất có thật, tiếng lấy từ format
/// gộp tốt nhất (`sound_source`). Tốn thêm byte (tải cả format gộp) nhưng đúng tiếng của video; ffmpeg ghép
/// `-map 0:v:0 -map 1:a:0` nên hình của format gộp bị bỏ, cỡ file ra ≈ cỡ hình (tiếng aac nhỏ, không biết trước).
/// `None` khi không phải trang có nhạc nền, hoặc format gộp/HLS cao hơn hình tách (khi đó `combined_choice` lo),
/// hoặc không ghim được mã.
fn borrowed_sound_choice(formats: &[Format], limit: Option<u64>, fallback: &str) -> Option<Choice> {
    if !has_soundtrack(formats) {
        return None;
    }
    let video = pick_video(formats, limit)?;
    let tallest = video_tier(formats, limit).filter_map(Format::level).max();
    if video.level() != tallest {
        return None;
    }
    let sound = sound_source(formats, limit)?;
    let (video_pin, sound_pin) = (video.video_pin()?, sound.video_pin()?);
    Some(Choice {
        selector: format!("{video_pin}+{sound_pin}/{fallback}"),
        size: video.size,
        bitrate: video.tbr,
    })
}

/// Trang có nhạc nền TikTok bị loại khỏi tiếng riêng.
fn has_soundtrack(formats: &[Format]) -> bool {
    formats.iter().any(|format| format.soundtrack)
}

/// Format gộp sẵn hình + tiếng tải thẳng được, tốt nhất ≤ `limit` (cạnh ngắn; `None` = cao nhất): nguồn tiếng thật
/// của video khi tiếng riêng là nhạc nền. Cùng cách `b` của yt-dlp chọn: bậc `preference` cao nhất, mức cao nhất,
/// mp4 cuối danh sách (yt-dlp xếp kém → tốt). Bản `download` (watermark) thiếu chiều cao nên không được kể.
fn sound_source(formats: &[Format], limit: Option<u64>) -> Option<&Format> {
    let whole = || {
        formats.iter().filter(move |format| {
            format.plain
                && format.has_video
                && format.has_audio
                && format.level().is_some_and(|level| within(level, limit))
        })
    };
    let top = whole().map(|format| format.preference).max()?;
    let level = whole()
        .filter(|format| format.preference == top)
        .filter_map(Format::level)
        .max()?;
    let best = || {
        whole()
            .rev()
            .filter(move |format| format.preference == top && format.level() == Some(level))
    };
    best()
        .find(|format| format.ext == "mp4")
        .or_else(|| best().next())
}

/// Nguồn tiếng của video khi tiếng riêng là nhạc nền; `None` ở mọi trang khác.
fn video_sound(formats: &[Format]) -> Option<&Format> {
    sound_source(formats, None).filter(|_| has_soundtrack(formats))
}

/// Format có hình ở mức ≤ `limit` (cạnh ngắn) và bậc `preference` cao nhất. yt-dlp xếp `preference` trước cả
/// độ phân giải nên `bv*` không chọn bậc dưới: YouTube gán -10 cho format hỏng (ngắn hơn nửa thời lượng).
fn video_tier(formats: &[Format], limit: Option<u64>) -> impl DoubleEndedIterator<Item = &Format> {
    let fits = move |format: &&Format| {
        format.has_video && format.level().is_some_and(|level| within(level, limit))
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
        .filter(|format| format.audio_only)
        .all(|format| format.preference <= audio.preference)
}

/// Hình: format chỉ có hình ở bậc yt-dlp chọn, cao nhất ≤ `limit`; ở mức đó lấy mp4 cuối danh
/// sách (yt-dlp xếp kém → tốt), không có mp4 thì format cuối (webm, ffmpeg ghép). Không lọc cứng
/// mp4: video 4K chỉ có VP9 sẽ bị tụt.
fn pick_video(formats: &[Format], limit: Option<u64>) -> Option<&Format> {
    let split = || video_tier(formats, limit).filter(|format| format.plain && !format.has_audio);
    let top = split().filter_map(Format::level).max()?;
    let level = |format: &&Format| format.level() == Some(top);
    split()
        .rev()
        .filter(level)
        .find(|format| format.ext == "mp4")
        .or_else(|| split().rev().find(level))
}

/// Tiếng: format chỉ có tiếng cuối danh sách, ưu tiên đuôi `ext`. yt-dlp xếp tiếng gốc sau lồng
/// tiếng, bản DRC trước bản thường, nên cái cuối là tiếng gốc không DRC. Nhạc nền TikTok không phải tiếng
/// của video nên không được chọn.
fn pick_audio<'a>(formats: &'a [Format], ext: Option<&str>) -> Option<&'a Format> {
    let alone = || {
        formats
            .iter()
            .rev()
            .filter(|format| format.plain && format.audio_only && !format.soundtrack)
    };
    ext.and_then(|ext| alone().find(|format| format.ext == ext))
        .or_else(|| alone().next())
}

fn within(level: u64, limit: Option<u64>) -> bool {
    limit.is_none_or(|limit| level <= limit)
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
        .max_by_key(|format| format.level().unwrap_or(0))
        .and_then(|format| format.size)
        .or_else(|| {
            sum_size(
                tallest(formats).and_then(|format| format.size),
                loudest(formats).and_then(|format| format.size),
            )
        })
}

/// Selector cũ: format lớn nhất ở mức đó, cộng tiếng nếu chỉ có hình.
fn guessed_size(formats: &[Format], level: u64) -> Option<u64> {
    let audio = loudest(formats).and_then(|format| format.size);
    formats
        .iter()
        .filter(|format| format.has_video && format.level() == Some(level))
        .max_by_key(|format| format.size.unwrap_or(0))
        .and_then(|format| {
            if format.has_audio {
                format.size
            } else {
                sum_size(format.size, audio)
            }
        })
}

fn top_bitrate(formats: &[Format], level: u64) -> Option<f64> {
    formats
        .iter()
        .filter(|format| format.has_video && format.level() == Some(level))
        .filter_map(|format| format.tbr)
        .fold(None::<f64>, |current, rate| {
            Some(current.map_or(rate, |value| value.max(rate)))
        })
}

fn tallest(formats: &[Format]) -> Option<&Format> {
    formats
        .iter()
        .filter(|format| format.has_video)
        .max_by_key(|format| format.level().unwrap_or(0))
}

/// Tiếng bitrate cao nhất (ước lượng cho selector cũ), không kể nhạc nền TikTok.
fn loudest(formats: &[Format]) -> Option<&Format> {
    formats
        .iter()
        .filter(|format| format.audio_only && !format.soundtrack)
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
    let width = value
        .get("width")
        .and_then(Value::as_u64)
        .filter(|width| *width > 0);
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
        mirror_pin: mirror_pin(id, items),
        ext,
        height,
        width,
        has_video: vcodec != "none" && height.is_some(),
        has_audio: acodec != "none",
        audio_only: acodec != "none" && vcodec == "none",
        plain,
        preference: value
            .get("preference")
            .and_then(Value::as_f64)
            .map_or(-1, |preference| preference.floor() as i64),
        size,
        tbr: value.get("tbr").and_then(Value::as_f64),
        abr: value.get("abr").and_then(Value::as_f64),
        soundtrack: false,
    })
}

/// Mã ghi vào selector. Nhiều format trùng mã thì yt-dlp thêm số thứ tự (`140-0`…`140-23`, tiếng gốc
/// số lớn nhất): số chỉ là vị trí, trang thêm bản lồng tiếng là `140-23` trỏ sang tiếng khác. Khi đó
/// ghim kèm ngôn ngữ (`140-23[language=en-US]`), lệch thì yt-dlp rơi về phần sau `/`; nhóm không có
/// ngôn ngữ riêng cho mã này thì không ghim. YouTube có khi trả mỗi tiếng hai lần (hai client, chỉ khác cờ
/// "default", cỡ trùng từng byte): vẫn là một tiếng, ghim được; cùng ngôn ngữ mà khác cỡ (vd. bản thuyết
/// minh) hoặc thiếu cỡ thì không.
fn pin(id: &str, language: &str, items: &[Value]) -> Option<String> {
    let Some(base) = numbered_base(id) else {
        return Some(id.to_string());
    };
    let group = numbered_group(base, items);
    if group.len() < 2 {
        return Some(id.to_string());
    }
    let size = |item: &&Value| item.get("filesize").and_then(Value::as_u64);
    let mut same = group
        .iter()
        .filter(|item| field(item, "language") == language);
    let first = same.next().and_then(size);
    let alone = same.all(|item| first.is_some() && size(item) == first);
    (alone && probe_cache::plain_language(language)).then(|| format!("{id}[language={language}]"))
}

/// Mã đánh số mà cả nhóm cùng một ngôn ngữ: không phải bản lồng tiếng mà là các mirror CDN của cùng một
/// format (TikTok: `h264_540p_1730569-0`, `-1`), số chỉ là vị trí mirror và `pin` không ghim được. Ghim thẳng
/// mã; mã lệch (mirror đổi chỗ) thì yt-dlp rơi về phần sau `/`. Chỉ dùng cho format gộp hình + tiếng: tiếng
/// riêng vẫn theo `pin` (lồng tiếng YouTube).
fn mirror_pin(id: &str, items: &[Value]) -> Option<String> {
    let group = numbered_group(numbered_base(id)?, items);
    let first = group.first()?;
    let same = group
        .iter()
        .all(|item| field(item, "language") == field(first, "language"));
    (group.len() >= 2 && same).then(|| id.to_string())
}

/// Các format cùng mã gốc (`140-0`, `140-1`… ⇒ `140`) trong `items`.
fn numbered_group<'a>(base: &str, items: &'a [Value]) -> Vec<&'a Value> {
    items
        .iter()
        .filter(|item| numbered_base(field(item, "format_id")) == Some(base))
        .collect()
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
        return "[media.drm] Nội dung có DRM; UTD không vượt qua bảo vệ bản quyền.".into();
    }
    if message.contains("sign in")
        || message.contains("log in")
        || message.contains("login")
        || message.contains("cookies")
        || message.contains("not a bot")
    {
        return "[media.login] Nguồn này cần đăng nhập.".into();
    }
    if message.contains("unsupported url") || message.contains("no video formats found") {
        return "[probe.unsupported] URL này chưa được hỗ trợ hoặc không có định dạng tải được.".into();
    }
    if message.contains("private video")
        || message.contains("video unavailable")
        || message.contains("has been removed")
    {
        return "[media.unavailable] Media không công khai hoặc đã bị gỡ.".into();
    }
    if message.contains("timed out")
        || message.contains("connection")
        || message.contains("network")
    {
        return "[probe.connect] Không kết nối được tới nguồn media.".into();
    }
    "[probe.failed] Không đọc được thông tin media từ URL này.".into()
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

    /// Video dọc: `height` là chiều cao pixel thật (1920 cho video 1080×1920).
    fn portrait(id: &str, ext: &str, width: u64, height: u64, size: u64) -> Value {
        let mut format = video(id, ext, height, size);
        format["width"] = json!(width);
        format
    }

    /// Format của TikTok như yt-dlp trả: gộp hình + tiếng, https, không `language`, mã có số thứ tự vì
    /// hai CDN mirror (`-0`, `-1`).
    fn tiktok(id: &str, width: u64, height: u64, size: u64) -> Value {
        json!({
            "format_id": id,
            "protocol": "https",
            "ext": "mp4",
            "width": width,
            "height": height,
            "vcodec": if id.starts_with("h264") { "h264" } else { "h265" },
            "acodec": "aac",
            "filesize_approx": size,
            "tbr": size as f64 / 10.0
        })
    }

    /// Thứ tự yt-dlp (kém → tốt): tiếng riêng, rồi 540p (576×1024), 720p, 1080p, mỗi mức hai mirror.
    fn tiktok_ladder() -> Vec<Value> {
        let mut formats = vec![audio("audio", "m4a", 200)];
        for (name, width, height, size) in [
            ("h264_540p_1730569", 576, 1024, 1400),
            ("bytevc1_540p_408497", 576, 1024, 1200),
            ("bytevc1_720p_509915", 720, 1280, 2000),
            ("bytevc1_1080p_1146746", 1080, 1920, 3200),
        ] {
            for mirror in 0..2 {
                formats.push(tiktok(&format!("{name}-{mirror}"), width, height, size));
            }
        }
        formats
    }

    /// Video TikTok dài, `yt-dlp -J` thật (2026-10-27), đúng thứ tự: tiếng mp3 riêng; `download` (bản có
    /// watermark) có hình nhưng thiếu `width`/`height`; 540p h264 gộp hình + tiếng; 720p h265 CHỈ HÌNH. Hai mức
    /// cuối đều có mã mirror `-0`, `-1`.
    fn tiktok_long() -> Vec<Value> {
        let mut formats = vec![
            json!({
                "format_id": "audio", "protocol": "https", "ext": "mp3",
                "vcodec": "none", "acodec": "mp3"
            }),
            json!({
                "format_id": "download", "protocol": "https", "ext": "mp4",
                "vcodec": "h264", "acodec": "aac", "preference": -2,
                "filesize": 40_000_000, "tbr": 1500.0
            }),
        ];
        for (name, width, height, vcodec, acodec, size) in [
            ("h264_540p_1526080", 576, 1024, "h264", "aac", 33_376_714u64),
            ("bytevc1_720p_698386", 720, 1280, "h265", "none", 11_107_596),
        ] {
            for mirror in 0..2 {
                formats.push(json!({
                    "format_id": format!("{name}-{mirror}"), "protocol": "https", "ext": "mp4",
                    "width": width, "height": height, "vcodec": vcodec, "acodec": acodec,
                    "preference": -1, "filesize": size
                }));
            }
        }
        formats
    }

    fn ids_of(options: &[MediaFormatOption]) -> Vec<&str> {
        options.iter().map(|option| option.id.as_str()).collect()
    }

    fn panel<'a>(result: &'a MediaProbeResult, id: &str) -> &'a MediaFormatOption {
        result
            .every_height
            .iter()
            .find(|format| format.id == id)
            .unwrap_or_else(|| panic!("thiếu dòng {id} trong bảng trên trang"))
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
    fn the_page_panel_lists_every_height_down_to_144p() {
        let mut formats: Vec<Value> = [2160_u64, 1440, 1080, 720, 480, 360, 240, 144]
            .into_iter()
            .map(|height| video(&format!("v{height}"), "mp4", height, height * 1000))
            .collect();
        formats.push(audio("140", "m4a", 1000));
        let result = build_result("https://example.com/watch", &info(formats), true).unwrap();
        let ids = |options: &[MediaFormatOption]| -> Vec<String> {
            options.iter().map(|option| option.id.clone()).collect()
        };
        assert_eq!(
            ids(&result.formats),
            ["best", "2160", "1440", "1080", "720", "480", "audio-m4a", "audio-mp3"],
            "hộp Tải media giữ tối đa 5 mức"
        );
        assert_eq!(
            ids(&result.every_height),
            ["best", "2160", "1440", "1080", "720", "480", "360", "240", "144", "audio-m4a", "audio-mp3"]
        );
        assert!(result.every_height.iter().skip(1).take(8).all(|option| option.size_bytes.is_some()));
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
            "18/bv*[height<=360]+ba/b[height<=360]",
            "mức chỉ có format gộp: ghim format gộp, phần dự phòng giữ selector cũ"
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
    fn pins_the_combined_format_on_pages_with_only_combined_formats() {
        // Phần dự phòng sau `/` vẫn là selector cũ; chỉ thêm mã format gộp ở đầu để lượt tải dùng lại luồng.
        let formats = vec![combined("18", 360, 100), combined("22", 720, 300)];
        let result = build_result("https://example.com/watch", &info(formats), true).unwrap();
        assert_eq!(selector(&result, "best"), "22/bv*+ba/b");
        assert_eq!(option(&result, "best").size_bytes, Some(300));
        assert_eq!(
            selector(&result, "720"),
            "22/bv*[height<=720]+ba/b[height<=720]"
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
            "37/bv*[height<=1080]+ba/b[height<=1080]",
            "format gộp 1080p cao hơn luồng tách 720p"
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
    fn pins_the_original_language_when_youtube_lists_each_track_twice() {
        // Shorts `Zhz7bQb9saM` (tiếng Việt gốc + lồng tiếng Anh): mỗi tiếng có hai bản (hai client, chỉ khác cờ
        // "default"), cỡ trùng từng byte. Trước đây không ghim được ⇒ bảng lấy cỡ bản 360p, lượt tải hỏi lại yt-dlp.
        let formats = vec![
            dub("140-0", "m4a", "en-US", 2_610_332),
            dub("140-1", "m4a", "en-US", 2_610_332),
            dub("140-2", "m4a", "vi", 2_609_107),
            dub("140-3", "m4a", "vi", 2_609_107),
            dub("251-0", "webm", "en-US", 2_257_774),
            dub("251-1", "webm", "en-US", 2_257_774),
            dub("251-2", "webm", "vi", 2_700_777),
            dub("251-3", "webm", "vi", 2_700_777),
            combined("18", 360, 12_108_842),
            video("400", "mp4", 1440, 33_173_910),
        ];
        let result = build_result("https://example.com/watch", &info(formats), true).unwrap();
        let best = selector(&result, "best");
        assert_eq!(best, "400+140-3[language=vi]/bv*+ba/b");
        assert_eq!(option(&result, "best").size_bytes, Some(35_783_017));
        assert_eq!(
            probe_cache::selector_ids(best),
            Some(vec!["400", "140-3"]),
            "lượt tải lấy luồng từ bộ nhớ Phân tích"
        );
        assert_eq!(
            selector(&result, "audio-m4a"),
            "140-3[language=vi]/ba[ext=m4a]/ba"
        );
        assert_eq!(selector(&result, "audio-mp3"), "251-3[language=vi]/ba/b");
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
            dub("140-1", "m4a", "en", 2),
            dub("140-2", "m4a", "en US", 1),
            dub("140-drc", "m4a", "en", 1),
            video("http-1080", "mp4", 1080, 1),
        ];
        assert_eq!(pin("140-1", "en", &items), None, "trùng ngôn ngữ, khác tiếng (cỡ khác)");
        let no_size = vec![
            json!({ "format_id": "140-0", "language": "en" }),
            json!({ "format_id": "140-1", "language": "en" }),
        ];
        assert_eq!(pin("140-1", "en", &no_size), None, "không biết cỡ ⇒ không chắc là một tiếng");
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

    #[test]
    fn pins_the_best_combined_tiktok_format_so_the_download_reuses_the_analysed_stream() {
        let result = build_result(
            "https://www.tiktok.com/@a/video/1",
            &info(tiktok_ladder()),
            true,
        )
        .unwrap();
        let best = selector(&result, "best");
        assert_eq!(
            best, "bytevc1_1080p_1146746-1/bv*+ba/b",
            "mirror cuối = mirror yt-dlp xếp sau"
        );
        assert_eq!(option(&result, "best").size_bytes, Some(3200));
        assert_eq!(
            option(&result, "best").detail,
            "Video + âm thanh · tối đa 1080p"
        );
        assert_eq!(
            probe_cache::selector_ids(best),
            Some(vec!["bytevc1_1080p_1146746-1"]),
            "cache đọc được mã"
        );
        assert_eq!(
            selector(&result, "audio-m4a"),
            "bytevc1_1080p_1146746-1/b",
            "tiếng của video (format gộp), không phải nhạc nền `audio`"
        );
    }

    #[test]
    fn names_levels_by_the_short_side_so_vertical_videos_read_like_landscape_ones() {
        let result = build_result(
            "https://www.tiktok.com/@a/video/1",
            &info(tiktok_ladder()),
            true,
        )
        .unwrap();
        assert_eq!(
            ids_of(&result.formats),
            ["best", "1080", "720", "audio-m4a", "audio-mp3"]
        );
        assert_eq!(option(&result, "1080").label, "1080p");
        assert_eq!(option(&result, "720").label, "720p");
        assert_eq!(option(&result, "1080").badge.as_deref(), Some("Phổ biến"));
        assert_eq!(
            ids_of(&result.every_height),
            ["best", "1080", "720", "576", "audio-m4a", "audio-mp3"]
        );
        assert_eq!(panel(&result, "576").label, "576p");
        // `height` của yt-dlp là chiều cao pixel thật: lọc dự phòng theo đó, không theo cạnh ngắn.
        assert_eq!(
            selector(&result, "1080"),
            "bytevc1_1080p_1146746-1/bv*[height<=1920]+ba/b[height<=1920]"
        );
        assert_eq!(
            selector(&result, "720"),
            "bytevc1_720p_509915-1/bv*[height<=1280]+ba/b[height<=1280]"
        );
        assert_eq!(
            panel(&result, "576").selector,
            "bytevc1_540p_408497-1/bv*[height<=1024]+ba/b[height<=1024]"
        );
        assert_eq!(option(&result, "1080").size_bytes, Some(3200));
        assert_eq!(option(&result, "720").size_bytes, Some(2000));
        assert_eq!(panel(&result, "576").size_bytes, Some(1200));
    }

    #[test]
    fn lists_no_level_row_that_would_repeat_the_only_level() {
        let mut one = vec![audio("audio", "m4a", 200)];
        one.extend(
            (0..2).map(|mirror| tiktok(&format!("h264_1080p_1-{mirror}"), 1080, 1920, 3200)),
        );
        let result = build_result("https://www.tiktok.com/@a/video/1", &info(one), true).unwrap();
        let only_best = ["best", "audio-m4a", "audio-mp3"];
        assert_eq!(ids_of(&result.formats), only_best);
        assert_eq!(ids_of(&result.every_height), only_best);
        assert_eq!(selector(&result, "best"), "h264_1080p_1-1/bv*+ba/b");

        // Video ngang một mức cũng vậy.
        let landscape = vec![audio("140", "m4a", 100), video("399", "mp4", 1080, 400)];
        let result = build_result("https://example.com/watch", &info(landscape), true).unwrap();
        assert_eq!(ids_of(&result.every_height), only_best);

        // Hộp Tải media chỉ liệt kê mức quen thuộc: còn một dòng thì trùng "Tốt nhất", bỏ; bảng trên trang thì đủ.
        let mut odd = vec![audio("audio", "m4a", 200)];
        odd.push(tiktok("a-0", 1080, 1920, 3200));
        odd.push(tiktok("b-0", 576, 1024, 1400));
        odd.sort_by_key(|format| format["height"].as_u64().unwrap_or(0));
        let result = build_result("https://www.tiktok.com/@a/video/1", &info(odd), true).unwrap();
        assert_eq!(ids_of(&result.formats), only_best);
        assert_eq!(
            ids_of(&result.every_height),
            ["best", "1080", "576", "audio-m4a", "audio-mp3"]
        );
    }

    #[test]
    fn labels_vertical_dash_videos_by_the_short_side_and_keeps_the_pins() {
        // YouTube Shorts: hình và tiếng tách, 1080×1920.
        let formats = vec![
            audio("140", "m4a", 100),
            audio("251", "webm", 90),
            portrait("247", "webm", 720, 1280, 250),
            portrait("398", "mp4", 720, 1280, 200),
            portrait("137", "mp4", 1080, 1920, 600),
            portrait("248", "webm", 1080, 1920, 500),
            portrait("399", "mp4", 1080, 1920, 400),
        ];
        let result = build_result("https://example.com/shorts", &info(formats), true).unwrap();
        assert_eq!(selector(&result, "best"), "399+140/bv*+ba/b");
        assert_eq!(
            option(&result, "best").detail,
            "Video + âm thanh · tối đa 1080p"
        );
        assert_eq!(
            ids_of(&result.formats),
            ["best", "1080", "720", "audio-m4a", "audio-mp3"]
        );
        assert_eq!(option(&result, "1080").label, "1080p");
        assert_eq!(
            selector(&result, "1080"),
            "399+140/bv*[height<=1920]+ba/b[height<=1920]"
        );
        assert_eq!(
            selector(&result, "720"),
            "398+140/bv*[height<=1280]+ba/b[height<=1280]"
        );
        assert_eq!(option(&result, "720").size_bytes, Some(300), "398 + 140");
    }

    #[test]
    fn a_combined_format_never_hides_a_taller_stream_or_a_better_container() {
        // Tách cao hơn mà tiếng không ghim được ⇒ giữ selector cũ, không tụt xuống format gộp 720p.
        let formats = vec![
            audio("140-0", "m4a", 1),
            audio("140-1", "m4a", 1),
            combined("22", 720, 300),
            video("399", "mp4", 1080, 4000),
        ];
        let result = build_result("https://example.com/watch", &info(formats), true).unwrap();
        assert_eq!(selector(&result, "best"), "bv*+ba/b");
        assert_eq!(
            selector(&result, "720"),
            "22/bv*[height<=720]+ba/b[height<=720]"
        );

        // Cùng mức: mp4 cuối danh sách, dù có webm xếp sau; mã mirror ghim thẳng.
        let mut webm = tiktok("w-0", 576, 1024, 900);
        webm["ext"] = json!("webm");
        let formats = vec![
            tiktok("a-0", 576, 1024, 1000),
            tiktok("a-1", 576, 1024, 1000),
            webm,
        ];
        let result = build_result("https://example.com/watch", &info(formats), true).unwrap();
        assert_eq!(selector(&result, "best"), "a-1/bv*+ba/b");

        // Format gộp ở bậc `preference` thấp hơn (hỏng) không được chọn.
        let formats = vec![
            ranked(tiktok("bad", 1080, 1920, 3000), -10),
            tiktok("good", 576, 1024, 1000),
        ];
        let result = build_result("https://example.com/watch", &info(formats), true).unwrap();
        assert_eq!(selector(&result, "best"), "good/bv*+ba/b");
    }

    #[test]
    fn pins_numbered_combined_ids_directly_only_when_the_whole_group_is_mirrors() {
        let combined_in = |id: &str, language: &str| {
            let mut format = tiktok(id, 576, 1024, 1000);
            format["language"] = json!(language);
            format
        };
        let mirrors = vec![combined_in("a-0", ""), combined_in("a-1", "")];
        assert_eq!(mirror_pin("a-1", &mirrors).as_deref(), Some("a-1"));
        let mixed = vec![combined_in("a-0", "en"), combined_in("a-1", "fr")];
        assert_eq!(
            mirror_pin("a-1", &mixed),
            None,
            "ngôn ngữ khác nhau: ghim kèm ngôn ngữ"
        );
        assert_eq!(
            pin("a-1", "fr", &mixed).as_deref(),
            Some("a-1[language=fr]")
        );
        let alone = vec![combined_in("a-1", "")];
        assert_eq!(
            mirror_pin("a-1", &alone),
            None,
            "không có nhóm: pin() đã ghim thẳng"
        );
        assert_eq!(mirror_pin("plain", &alone), None);
    }

    const LONG_TIKTOK: &str = "https://www.tiktok.com/@a/video/7677185063775194386";

    /// Video TikTok dài (bản đo thật 2026-10-27, 174 s): hình 720p h265 CHỈ HÌNH dài 175 s, còn tiếng riêng `audio`
    /// là NHẠC NỀN của bài (đo trên hai video: dài gấp ~2,3 lần hình, bắt đầu từ đầu bài hát nên lệch đoạn, mất
    /// lời nói đè lên nhạc). Tiếng thật của video chỉ nằm trong format gộp h264 540p (aac).
    #[test]
    fn pairs_a_video_only_top_tier_with_the_sound_of_the_best_combined_format() {
        let result = build_result(LONG_TIKTOK, &info(tiktok_long()), true).unwrap();
        let best = selector(&result, "best");
        assert_eq!(
            best, "bytevc1_720p_698386-1+h264_540p_1526080-1/b",
            "hình 720p ghim mirror cuối + tiếng của format gộp tốt nhất; không `audio` (nhạc nền), dự phòng `b`"
        );
        assert_eq!(
            probe_cache::selector_ids(best),
            Some(vec!["bytevc1_720p_698386-1", "h264_540p_1526080-1"]),
            "cache đọc được cả hai mã, hình trước"
        );
        assert_eq!(
            option(&result, "best").size_bytes,
            Some(11_107_596),
            "cỡ file ra ≈ hình 720p: hình của format gộp bị bỏ, tiếng aac của nó nhỏ (QA: file 11,9 MB, bảng cũ 42 MB)"
        );
        assert_eq!(
            option(&result, "best").detail,
            "Video + âm thanh · tối đa 720p"
        );
        // Chỉ một mức quen thuộc ⇒ hộp Tải media không thêm dòng mức; bảng trên trang đủ 720p và 576p.
        assert_eq!(ids_of(&result.formats), ["best", "audio-m4a", "audio-mp3"]);
        assert_eq!(
            ids_of(&result.every_height),
            ["best", "720", "576", "audio-m4a", "audio-mp3"]
        );
        assert_eq!(
            panel(&result, "720").selector,
            "bytevc1_720p_698386-1+h264_540p_1526080-1/b[height<=1280]"
        );
        assert_eq!(panel(&result, "720").size_bytes, Some(11_107_596));
        // 576p chỉ có format gộp (h264 + aac): ghim mirror như trước.
        assert_eq!(
            panel(&result, "576").selector,
            "h264_540p_1526080-1/bv*[height<=1024]+ba/b[height<=1024]"
        );
        // Dòng chỉ âm thanh cũng lấy tiếng của video, không lấy nhạc nền. Cỡ file gộp là của cả hình (file ra chỉ còn
        // tiếng, QA: 0,7 MB so với bảng cũ 32 MB) ⇒ không ghi cỡ.
        assert_eq!(selector(&result, "audio-m4a"), "h264_540p_1526080-1/b");
        assert_eq!(selector(&result, "audio-mp3"), "h264_540p_1526080-1/b");
        assert_eq!(option(&result, "audio-m4a").size_bytes, None);
        assert_eq!(option(&result, "audio-m4a").bitrate_kbps, None);
    }

    #[test]
    fn never_lets_the_background_music_back_in_through_a_fallback() {
        for formats in [tiktok_long(), tiktok_ladder()] {
            let result = build_result(LONG_TIKTOK, &info(formats), true).unwrap();
            for option in result.formats.iter().chain(&result.every_height) {
                let ids = probe_cache::selector_ids(&option.selector).unwrap_or_default();
                assert!(!ids.contains(&"audio"), "{}: {}", option.id, option.selector);
                // Hình chỉ hình + format gộp: dự phòng `b` (gộp sẵn), không `+ba` ghép hình với nhạc nền.
                if ids.len() == 2 {
                    let tail = option.selector.split_once('/').map_or("", |(_, tail)| tail);
                    assert!(!tail.contains("ba"), "{}: {}", option.id, option.selector);
                }
                // Dòng chỉ âm thanh: lấy từ format gộp, dự phòng `b`.
                if option.audio_format.is_some() {
                    assert!(option.selector.ends_with("/b"), "{}", option.selector);
                    assert!(!option.selector.contains("ba["), "{}", option.selector);
                }
            }
        }
    }

    #[test]
    fn only_tiktok_treats_the_audio_format_as_background_music() {
        // Cùng danh sách format nhưng trang khác: `audio` là tiếng riêng thật, ghép như mọi tiếng riêng.
        let other = build_result("https://example.com/watch", &info(tiktok_long()), true).unwrap();
        assert_eq!(
            selector(&other, "best"),
            "bytevc1_720p_698386-1+audio/bv*+ba/b"
        );
        assert_eq!(selector(&other, "audio-m4a"), "audio/ba[ext=m4a]/ba");
        // YouTube vẫn `hình+tiếng` như cũ.
        let youtube = vec![
            audio("140", "m4a", 100),
            video("399", "mp4", 1080, 400),
            video("398", "mp4", 720, 200),
        ];
        let result = build_result("https://www.youtube.com/watch?v=x", &info(youtube), true).unwrap();
        assert_eq!(selector(&result, "best"), "399+140/bv*+ba/b");
        assert_eq!(selector(&result, "audio-m4a"), "140/ba[ext=m4a]/ba");

        // yt-dlp ghi `extractor_key` thì tin nó hơn tên miền của link.
        let mut value = info(tiktok_long());
        value["extractor_key"] = json!("TikTok");
        let result = build_result("https://example.com/watch", &value, true).unwrap();
        assert_eq!(
            selector(&result, "best"),
            "bytevc1_720p_698386-1+h264_540p_1526080-1/b"
        );
        value["extractor_key"] = json!("Generic");
        let result = build_result(LONG_TIKTOK, &value, true).unwrap();
        assert_eq!(
            selector(&result, "best"),
            "bytevc1_720p_698386-1+audio/bv*+ba/b"
        );
    }

    #[test]
    fn a_short_tiktok_whose_top_tier_is_combined_keeps_its_selectors_apart_from_the_audio_rows() {
        let result = build_result(LONG_TIKTOK, &info(tiktok_ladder()), true).unwrap();
        assert_eq!(
            selector(&result, "best"),
            "bytevc1_1080p_1146746-1/bv*+ba/b",
            "bậc cao nhất đã gộp sẵn: không đổi"
        );
        assert_eq!(
            selector(&result, "720"),
            "bytevc1_720p_509915-1/bv*[height<=1280]+ba/b[height<=1280]"
        );
        assert_eq!(selector(&result, "audio-m4a"), "bytevc1_1080p_1146746-1/b");
        assert_eq!(option(&result, "audio-m4a").size_bytes, None);
        assert_eq!(selector(&result, "audio-mp3"), "bytevc1_1080p_1146746-1/b");
    }

    #[test]
    fn a_tiktok_post_with_nothing_but_its_soundtrack_still_offers_it_as_audio() {
        // Bài ảnh: yt-dlp chỉ ra nhạc của bài. Không có format gộp nào để lấy tiếng thay ⇒ dùng nhạc như tiếng riêng.
        let result = build_result(
            LONG_TIKTOK,
            &info(vec![audio("audio", "m4a", 200)]),
            true,
        )
        .unwrap();
        assert_eq!(selector(&result, "audio-m4a"), "audio/ba[ext=m4a]/ba");
        assert_eq!(selector(&result, "audio-mp3"), "audio/ba/b");
    }

    #[test]
    fn never_takes_a_format_with_picture_but_no_height_as_audio_only() {
        let result = build_result(LONG_TIKTOK, &info(tiktok_long()), true).unwrap();
        // `download` có h264 nhưng thiếu `height`: không phải "chỉ tiếng" (yt-dlp `ba` chỉ lấy `vcodec=none`),
        // cũng không phải nguồn tiếng của video (bản có watermark).
        for option in result.formats.iter().chain(&result.every_height) {
            assert!(
                !option.selector.contains("download"),
                "{}: {}",
                option.id,
                option.selector
            );
        }
        let formats = parse_formats(&info(tiktok_long()), false).unwrap();
        let download = formats
            .iter()
            .find(|format| format.pin.as_deref() == Some("download"));
        assert!(download.is_some_and(|format| !format.has_video && !format.audio_only));
        let loud = loudest(&formats).and_then(|format| format.pin.as_deref());
        assert_eq!(loud, Some("audio"));
        // Không có tiếng riêng nào: ba mức "tiếng" đều không chọn `download`.
        let without_audio: Vec<Value> = tiktok_long().into_iter().skip(1).collect();
        let formats = parse_formats(&info(without_audio), false).unwrap();
        assert!(pick_audio(&formats, Some("m4a")).is_none());
        assert!(loudest(&formats).is_none());
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
