//! Hỏi yt-dlp URL thật của từng luồng (video, âm thanh) để engine của UTD tự tải nhiều kết nối.
//! yt-dlp tải luồng https lần lượt, mỗi luồng một kết nối; UTD tải song song như IDM.

use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::time::timeout;
use utd_engine::{validate_url, ErrorClass, TaskError};

use crate::media::{
    ffmpeg_location, probe_cache, run_signed, runner, sign_in_pipe_error, tools, yt_dlp_signed,
};

/// YouTube bóp tốc độ yêu cầu Range lớn: mỗi yêu cầu xin tối đa chừng này khi yt-dlp không nói.
pub(super) const DEFAULT_CHUNK: u64 = 10 * 1024 * 1024;
/// Link còn sống ít hơn chừng này (giây) thì coi như hết hạn, lấy link mới trước khi tải tiếp.
const EXPIRY_MARGIN: i64 = 300;
/// Luồng chưa mở (`available_at`) quá chừng này giây nữa: để yt-dlp tự chờ.
pub(super) const MAX_WAIT_SECS: i64 = 60;
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(60);
/// Đuôi luồng nhận tải thẳng; đuôi khác (lạ, chạy được mã…) để yt-dlp lo.
const STREAM_EXTENSIONS: &[&str] = &[
    "mp4", "m4a", "m4v", "webm", "mkv", "mov", "mp3", "ogg", "opus", "aac", "flac", "wav", "3gp",
];

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Stream {
    pub format_id: String,
    pub url: String,
    pub protocol: String,
    pub ext: String,
    /// `mp4_dash`, `webm_dash`… (luồng DASH cần ffmpeg sửa vỏ).
    pub container: Option<String>,
    /// Viết thường; `none` = không có, rỗng = không rõ.
    pub vcodec: String,
    pub acodec: String,
    /// `filesize`, không có thì `filesize_approx`.
    pub size: Option<u64>,
    /// Kích thước chắc chắn: `filesize` hoặc `clen` trong URL.
    pub exact_size: Option<u64>,
    pub headers: Vec<(String, String)>,
    pub chunk: Option<u64>,
    pub available_at: Option<i64>,
    pub drm: bool,
    pub cookies: StreamCookies,
}

/// Cookie yt-dlp đặt cho luồng (`cookies` của format): CDN TikTok trả 403 nếu thiếu `tt_chain_token`.
/// Mỗi cookie một dòng kiểu `Set-Cookie`, giá trị bỏ ngoặc kép như yt-dlp gửi (giữ ngoặc thì CDN cũng
/// trả 403). Chỉ ở RAM (probe_cache, hũ cookie của lượt tải), không vào journal.
#[derive(Clone, Default, PartialEq)]
pub(super) struct StreamCookies(Vec<String>);

impl std::fmt::Debug for StreamCookies {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "StreamCookies({})", self.0.len())
    }
}

impl StreamCookies {
    const MAX: usize = 32;

    /// `a=1; Domain=.x.com; Path=/; Secure; Expires=…; b="2"; Domain=…` ⇒ `["a=1; Domain=.x.com; …", "b=2; …"]`.
    pub fn parse(text: &str) -> Self {
        let mut lines: Vec<String> = Vec::new();
        for part in text.split(';').map(str::trim).filter(|part| !part.is_empty()) {
            let (name, value) = part.split_once('=').unwrap_or((part, ""));
            let attribute = matches!(
                name.trim().to_ascii_lowercase().as_str(),
                "domain" | "path" | "secure" | "expires" | "max-age" | "httponly" | "samesite" | "version" | "comment"
            );
            match lines.last_mut() {
                Some(line) if attribute => {
                    line.push_str("; ");
                    line.push_str(part);
                }
                _ if attribute || !part.contains('=') => {}
                _ => lines.push(format!("{}={}", name.trim(), value.trim().trim_matches('"'))),
            }
        }
        lines.truncate(Self::MAX);
        Self(lines)
    }

    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Stream {
    pub fn has_video(&self) -> bool {
        self.vcodec != "none"
    }

    pub fn has_audio(&self) -> bool {
        self.acodec != "none"
    }

    /// Trần byte mỗi yêu cầu HTTP: `http_chunk_size` của yt-dlp, mặc định 10 MiB.
    pub fn request_cap(&self) -> u64 {
        self.chunk
            .unwrap_or(DEFAULT_CHUNK)
            .max(utd_engine::MIN_REQUEST_BYTES)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Resolved {
    pub streams: Vec<Stream>,
    pub drm: bool,
    pub live: bool,
}

/// Cách ra file cuối từ các luồng đã tải.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Layout {
    /// Hai luồng `[video, âm thanh]`, ffmpeg ghép.
    Merge,
    /// Một luồng có hình: giữ đuôi của luồng.
    Single,
    /// Một luồng, ra âm thanh `m4a`/`mp3`.
    Audio(AudioFormat),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AudioFormat {
    M4a,
    Mp3,
}

impl AudioFormat {
    pub fn parse(value: Option<&str>) -> Option<Self> {
        match value {
            Some("m4a") => Some(Self::M4a),
            Some("mp3") => Some(Self::Mp3),
            _ => None,
        }
    }
}

impl Layout {
    /// Đoán lại cách ghép từ task con đã có (lượt tiếp tục không hỏi lại yt-dlp).
    pub fn infer(audio_format: Option<&str>, parts: usize) -> Option<Self> {
        match (AudioFormat::parse(audio_format), parts) {
            (Some(format), 1) => Some(Self::Audio(format)),
            (None, 1) => Some(Self::Single),
            (None, 2) => Some(Self::Merge),
            _ => None,
        }
    }
}

/// Đối số yt-dlp cho selector ghim ĐÚNG hai mã (`399+140/…`; TikTok video dài: `<hình chỉ hình>+<format gộp>/…`,
/// hình 720p ghép với tiếng của bản gộp 540p). Mặc định yt-dlp chỉ giữ MỘT luồng hình khi ghép: format thứ hai
/// cũng có hình nên bị bỏ, kết quả chỉ còn hình và MẤT TIẾNG (đo thật 2026-10-27). `--video-multistreams` giữ cả
/// hai. Cờ này chỉ đổi kết quả khi hai vế đều có hình: `bv*+ba/b`, `399+140`, `b`, `ba` ra đúng cùng luồng có và
/// không có cờ (đo trên YouTube và TikTok). Selector không ghim đúng hai mã thì không thêm gì.
///
/// `merging` = yt-dlp tải và tự ghép (đường dự phòng của runner): ffmpeg của yt-dlp map cả hình của format gộp ⇒
/// file ra hai luồng hình. `-map -1:v:0` (sau các `-map` của yt-dlp) bỏ luồng hình thứ hai; không khớp luồng nào
/// (hình + tiếng riêng như YouTube) thì ffmpeg bỏ qua, file ra giống hệt byte (đo trên YouTube `134+140`).
pub(super) fn pair_args(selector: &str, merging: bool) -> &'static [&'static str] {
    const KEEP_BOTH: &[&str] = &["--video-multistreams"];
    const KEEP_ONE_PICTURE: &[&str] = &[
        "--video-multistreams",
        "--ppa",
        "Merger+ffmpeg_o:-map -1:v:0",
    ];
    if !probe_cache::selector_ids(selector).is_some_and(|ids| ids.len() == 2) {
        return &[];
    }
    if merging {
        KEEP_ONE_PICTURE
    } else {
        KEEP_BOTH
    }
}

/// `yt-dlp -J -f <selector>`: chỉ đọc thông tin, không tải media.
pub(super) async fn resolve(
    tools: &tools::MediaTools,
    url: &str,
    selector: &str,
    grant: Option<&crate::auth::Grant>,
) -> Result<Resolved, TaskError> {
    let (mut command, pipe) = yt_dlp_signed(tools, grant)?;
    command.args([
        "--encoding",
        "utf-8",
        "--no-playlist",
        "--no-warnings",
        "--socket-timeout",
        "15",
        "-J",
        "-f",
        selector,
    ]);
    command.args(pair_args(selector, false));
    // Không có ffmpeg, yt-dlp chọn định dạng khác: chỉ chỗ giống hệt lượt tải thật.
    if let Some(location) = ffmpeg_location(tools) {
        command.arg("--ffmpeg-location").arg(location);
    }
    command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let (output, cookies_read) = timeout(RESOLVE_TIMEOUT, run_signed(command, pipe.as_ref()))
        .await
        .map_err(|_| failure("media.network", "Nguồn media phản hồi quá lâu."))?
        .map_err(|error| failure("media.spawn", &format!("Không chạy được yt-dlp: {error}")))?;
    if !cookies_read {
        return Err(sign_in_pipe_error());
    }
    if !output.status.success() {
        return Err(runner::classify_failure(&String::from_utf8_lossy(
            &output.stderr,
        )));
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| failure("media.failed", "yt-dlp trả về dữ liệu không đọc được."))?;
    parse(&value).ok_or_else(|| failure("media.failed", "yt-dlp không trả về luồng tải được."))
}

fn failure(code: &str, message: &str) -> TaskError {
    TaskError {
        class: ErrorClass::Transient,
        code: code.into(),
        message: message.into(),
    }
}

/// Ghép (`bv*+ba`) ⇒ `requested_formats`; một định dạng ⇒ chính thông tin gốc.
pub(super) fn parse(value: &Value) -> Option<Resolved> {
    let streams = match value.get("requested_formats").and_then(Value::as_array) {
        Some(items) if !items.is_empty() => {
            items.iter().map(parse_stream).collect::<Option<_>>()?
        }
        _ => vec![parse_stream(value)?],
    };
    let live = value.get("is_live").and_then(Value::as_bool) == Some(true)
        || value
            .get("live_status")
            .and_then(Value::as_str)
            .is_some_and(|status| status == "is_live" || status == "is_upcoming");
    Some(Resolved {
        streams,
        drm: flagged(value.get("_has_drm")),
        live,
    })
}

/// Một format trong JSON của yt-dlp: `requested_formats[i]`, `formats[i]` hoặc thông tin gốc.
pub(super) fn parse_stream(value: &Value) -> Option<Stream> {
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(|text| text.trim().to_string())
    };
    let format_id = text("format_id").filter(|id| !id.is_empty())?;
    let url = text("url").filter(|url| !url.is_empty())?;
    let codec = |key: &str| text(key).unwrap_or_default().to_ascii_lowercase();
    let filesize = number(value.get("filesize"));
    let headers = value
        .get("http_headers")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    Some(Stream {
        exact_size: filesize.or_else(|| declared_length(&url)),
        size: filesize.or_else(|| number(value.get("filesize_approx"))),
        format_id,
        protocol: text("protocol").unwrap_or_default().to_ascii_lowercase(),
        ext: text("ext").unwrap_or_default().to_ascii_lowercase(),
        container: text("container").map(|value| value.to_ascii_lowercase()),
        vcodec: codec("vcodec"),
        acodec: codec("acodec"),
        headers,
        chunk: value
            .get("downloader_options")
            .and_then(|options| number(options.get("http_chunk_size"))),
        available_at: number(value.get("available_at")).and_then(|at| i64::try_from(at).ok()),
        drm: flagged(value.get("has_drm")),
        cookies: value
            .get("cookies")
            .and_then(Value::as_str)
            .map(StreamCookies::parse)
            .unwrap_or_default(),
        url,
    })
}

fn number(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    value.as_u64().or_else(|| {
        value
            .as_f64()
            .filter(|number| *number >= 0.0)
            .map(|number| number as u64)
    })
}

/// `has_drm` có thể là `true` hoặc `"maybe"`: có nghi ngờ là không tải thẳng.
pub(super) fn flagged(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(text)) => !text.is_empty(),
        _ => false,
    }
}

/// Tải thẳng bằng engine được không. Lỗi = lý do (chỉ để ghi log), khi đó dùng yt-dlp như cũ.
pub(super) fn check(
    resolved: &Resolved,
    audio_format: Option<&str>,
    ffmpeg: bool,
    now_secs: i64,
) -> Result<(Layout, Vec<Stream>), &'static str> {
    if resolved.drm || resolved.live {
        return Err("drm or live");
    }
    let streams = &resolved.streams;
    if streams.is_empty() || streams.len() > 2 {
        return Err("unexpected stream count");
    }
    for stream in streams {
        eligible_stream(stream, now_secs)?;
    }
    let audio = AudioFormat::parse(audio_format);
    if audio_format.is_some() && audio.is_none() {
        return Err("unknown audio format");
    }
    match (audio, streams.as_slice()) {
        (Some(_), [_, _]) => Err("audio job with two streams"),
        (Some(_), _) if !ffmpeg => Err("audio needs ffmpeg"),
        (Some(_), [stream]) if !stream.has_audio() => Err("stream has no audio"),
        (Some(format), [stream]) => Ok((Layout::Audio(format), vec![stream.clone()])),
        (None, [first, second]) => {
            if !ffmpeg {
                return Err("merge needs ffmpeg");
            }
            // ffmpeg lấy hình ở đầu vào 0, tiếng ở đầu vào 1.
            let ordered = if !first.has_video() && second.has_video() {
                vec![second.clone(), first.clone()]
            } else {
                vec![first.clone(), second.clone()]
            };
            if !ordered[0].has_video() || !ordered[1].has_audio() {
                return Err("merge needs one video and one audio stream");
            }
            Ok((Layout::Merge, ordered))
        }
        (None, [stream]) => {
            let dash = stream
                .container
                .as_deref()
                .is_some_and(|container| container.ends_with("_dash"));
            if dash && !ffmpeg {
                return Err("dash stream needs ffmpeg");
            }
            Ok((Layout::Single, vec![stream.clone()]))
        }
        _ => Err("unexpected stream count"),
    }
}

fn eligible_stream(stream: &Stream, now_secs: i64) -> Result<(), &'static str> {
    if !matches!(stream.protocol.as_str(), "http" | "https") {
        return Err("not a plain http stream");
    }
    if stream.drm {
        return Err("drm stream");
    }
    if validate_url(&stream.url).is_err() {
        return Err("invalid stream url");
    }
    if !STREAM_EXTENSIONS.contains(&stream.ext.as_str()) {
        return Err("unsupported stream extension");
    }
    if part_id(&stream.format_id).is_empty() {
        return Err("invalid format id");
    }
    if stream
        .available_at
        .is_some_and(|at| at > now_secs + MAX_WAIT_SECS)
    {
        return Err("stream not available yet");
    }
    Ok(())
}

/// Mã định dạng an toàn để đặt trong tên file (`f399`, `hls-720p`…).
pub(super) fn part_id(format_id: &str) -> String {
    format_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .take(32)
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn query_number(url: &str, key: &str) -> Option<u64> {
    let url = validate_url(url).ok()?;
    url.query_pairs()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| value.parse().ok())
}

/// Giờ hết hạn (giây Unix) của link kiểu googlevideo: tham số `expire=`.
pub(super) fn expires_at(url: &str) -> Option<i64> {
    query_number(url, "expire").and_then(|at| i64::try_from(at).ok())
}

/// Link đã hoặc sắp hết hạn (còn dưới 5 phút).
pub(super) fn expired(url: &str, now_secs: i64) -> bool {
    expires_at(url).is_some_and(|at| at <= now_secs + EXPIRY_MARGIN)
}

/// Kích thước luồng ghi trong URL (`clen=` của YouTube).
pub(super) fn declared_length(url: &str) -> Option<u64> {
    query_number(url, "clen")
}

pub(super) fn now_secs() -> i64 {
    utd_engine::now_ms() / 1000
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn merged_info() -> Value {
        json!({
            "title": "Video",
            "format_id": "399+251",
            "protocol": "https+https",
            "is_live": false,
            "requested_formats": [
                {
                    "format_id": "399",
                    "url": "https://rr4.googlevideo.com/videoplayback?expire=1792793545&clen=402957281&itag=399",
                    "protocol": "https",
                    "ext": "mp4",
                    "container": "mp4_dash",
                    "vcodec": "av01.0.09M.08",
                    "acodec": "none",
                    "filesize": 402957281,
                    "available_at": 1792771943,
                    "downloader_options": { "http_chunk_size": 10485760 },
                    "http_headers": {
                        "User-Agent": "Mozilla/5.0",
                        "Accept-Language": "en-us,en;q=0.5",
                        "Sec-Fetch-Mode": "navigate"
                    },
                    "has_drm": false
                },
                {
                    "format_id": "251",
                    "url": "https://rr4.googlevideo.com/videoplayback?expire=1792793545&itag=251",
                    "protocol": "https",
                    "ext": "webm",
                    "container": "webm_dash",
                    "vcodec": "none",
                    "acodec": "opus",
                    "filesize_approx": 112987308.0,
                    "http_headers": { "User-Agent": "Mozilla/5.0" }
                }
            ]
        })
    }

    fn stream(format_id: &str, protocol: &str, ext: &str, vcodec: &str, acodec: &str) -> Stream {
        Stream {
            format_id: format_id.into(),
            url: format!("https://media.example/{format_id}"),
            protocol: protocol.into(),
            ext: ext.into(),
            container: None,
            vcodec: vcodec.into(),
            acodec: acodec.into(),
            size: Some(1000),
            exact_size: Some(1000),
            headers: Vec::new(),
            chunk: None,
            available_at: None,
            drm: false,
            cookies: Default::default(),
        }
    }

    #[test]
    fn splits_yt_dlp_stream_cookies_and_unquotes_values() {
        let cookies = StreamCookies::parse(
            "ttwid=1%7Cx; Domain=.tiktok.com; Path=/; Secure; Expires=1824135169; tt_csrf_token=y; Domain=.tiktok.com; Path=/; Secure; tt_chain_token=\"z/+=\"; Domain=.tiktok.com; Path=/; Secure; Expires=1808583169",
        );
        assert_eq!(
            cookies.lines().collect::<Vec<_>>(),
            [
                "ttwid=1%7Cx; Domain=.tiktok.com; Path=/; Secure; Expires=1824135169",
                "tt_csrf_token=y; Domain=.tiktok.com; Path=/; Secure",
                "tt_chain_token=z/+=; Domain=.tiktok.com; Path=/; Secure; Expires=1808583169",
            ]
        );
        assert_eq!(format!("{cookies:?}"), "StreamCookies(3)", "không lộ giá trị trong log");
        let format = serde_json::json!({
            "format_id": "h264_540p", "url": "https://v16-webapp-prime.tiktok.com/video/x", "ext": "mp4",
            "protocol": "https", "cookies": "tt_chain_token=\"z\"; Domain=.tiktok.com; Path=/"
        });
        assert!(!parse_stream(&format).unwrap().cookies.is_empty());
    }

    fn resolved(streams: Vec<Stream>) -> Resolved {
        Resolved {
            streams,
            drm: false,
            live: false,
        }
    }

    #[test]
    fn reads_requested_formats_with_headers_chunks_and_sizes() {
        let parsed = parse(&merged_info()).expect("resolved");
        assert_eq!(parsed.streams.len(), 2);
        let video = &parsed.streams[0];
        assert_eq!(video.format_id, "399");
        assert_eq!(video.protocol, "https");
        assert_eq!(video.ext, "mp4");
        assert_eq!(video.container.as_deref(), Some("mp4_dash"));
        assert!(video.has_video() && !video.has_audio());
        assert_eq!(video.size, Some(402_957_281));
        assert_eq!(video.exact_size, Some(402_957_281));
        assert_eq!(video.chunk, Some(10_485_760));
        assert_eq!(video.request_cap(), 10_485_760);
        assert_eq!(video.available_at, Some(1_792_771_943));
        assert_eq!(video.headers.len(), 3);
        assert!(video
            .headers
            .iter()
            .any(|(name, value)| name == "Sec-Fetch-Mode" && value == "navigate"));

        let audio = &parsed.streams[1];
        assert!(audio.has_audio() && !audio.has_video());
        assert_eq!(
            audio.size,
            Some(112_987_308),
            "filesize_approx khi thiếu filesize"
        );
        assert_eq!(
            audio.exact_size, None,
            "approx không phải kích thước chắc chắn"
        );
        assert_eq!(audio.chunk, None);
        assert_eq!(
            audio.request_cap(),
            DEFAULT_CHUNK,
            "thiếu http_chunk_size ⇒ 10 MiB"
        );
        assert!(!parsed.drm && !parsed.live);
    }

    #[test]
    fn reads_a_single_selected_format_from_the_top_level() {
        let info = json!({
            "format_id": "140",
            "url": "https://media.example/a.m4a",
            "protocol": "https",
            "ext": "m4a",
            "vcodec": "none",
            "acodec": "mp4a.40.2",
            "filesize": 5000,
            "_has_drm": "maybe",
            "live_status": "not_live"
        });
        let parsed = parse(&info).expect("resolved");
        assert_eq!(parsed.streams.len(), 1);
        assert_eq!(parsed.streams[0].acodec, "mp4a.40.2");
        assert!(parsed.drm, "\"maybe\" vẫn tính là có DRM");
        assert!(parse(&json!({ "_type": "playlist", "entries": [] })).is_none());
    }

    #[test]
    fn picks_the_fast_path_only_for_plain_http_streams() {
        let video = stream("399", "https", "mp4", "av01", "none");
        let audio = stream("251", "https", "webm", "none", "opus");
        let both = resolved(vec![audio.clone(), video.clone()]);
        let (layout, streams) = check(&both, None, true, 0).expect("eligible");
        assert_eq!(layout, Layout::Merge);
        assert_eq!(streams[0].format_id, "399", "hình luôn ở đầu vào 0");
        assert_eq!(check(&both, None, false, 0), Err("merge needs ffmpeg"));

        let hls = resolved(vec![stream("hls-1", "m3u8_native", "mp4", "avc1", "mp4a")]);
        assert!(check(&hls, None, true, 0).is_err());
        let mut drm = both.clone();
        drm.streams[0].drm = true;
        assert!(check(&drm, None, true, 0).is_err());
        let mut live = both.clone();
        live.live = true;
        assert!(check(&live, None, true, 0).is_err());
        let odd = resolved(vec![stream("1", "https", "exe", "avc1", "mp4a")]);
        assert!(check(&odd, None, true, 0).is_err(), "đuôi lạ để yt-dlp lo");
        let three = resolved(vec![video.clone(), audio.clone(), audio.clone()]);
        assert!(check(&three, None, true, 0).is_err());
        let mute = resolved(vec![video.clone(), video.clone()]);
        assert!(
            check(&mute, None, true, 0).is_err(),
            "ghép cần một luồng có tiếng"
        );
        let mut later = both.clone();
        later.streams[0].available_at = Some(1000);
        assert!(check(&later, None, true, 0).is_err());
        assert!(
            check(&later, None, true, 990).is_ok(),
            "chờ ngắn thì vẫn tải thẳng"
        );
    }

    #[test]
    fn single_and_audio_layouts_need_ffmpeg_only_when_converting() {
        let progressive = resolved(vec![stream("18", "https", "mp4", "avc1", "mp4a")]);
        assert_eq!(
            check(&progressive, None, false, 0).unwrap().0,
            Layout::Single
        );
        let mut dash = progressive.clone();
        dash.streams[0].container = Some("mp4_dash".into());
        assert!(check(&dash, None, false, 0).is_err());
        assert!(check(&dash, None, true, 0).is_ok());

        let audio = resolved(vec![stream("140", "https", "m4a", "none", "mp4a.40.2")]);
        assert_eq!(
            check(&audio, Some("m4a"), true, 0).unwrap().0,
            Layout::Audio(AudioFormat::M4a)
        );
        assert_eq!(
            check(&audio, Some("mp3"), true, 0).unwrap().0,
            Layout::Audio(AudioFormat::Mp3)
        );
        assert!(check(&audio, Some("mp3"), false, 0).is_err());
        assert!(check(&audio, Some("flac"), true, 0).is_err());
        let silent = resolved(vec![stream("137", "https", "mp4", "avc1", "none")]);
        assert!(check(&silent, Some("m4a"), true, 0).is_err());
        let two = resolved(vec![
            stream("399", "https", "mp4", "av01", "none"),
            stream("140", "https", "m4a", "none", "mp4a"),
        ]);
        assert!(check(&two, Some("m4a"), true, 0).is_err());
    }

    /// `yt-dlp -J --video-multistreams -f bytevc1_720p_698386-1+h264_540p_1526080-1/b` thật (video TikTok 174 s,
    /// 2026-10-27), bỏ phần thân: hình 720p h265 CHỈ HÌNH rồi format gộp 540p h264 + aac. Không có
    /// `--video-multistreams`, yt-dlp bỏ format thứ hai vì nó cũng có hình và chỉ trả hình (mất tiếng).
    fn tiktok_pair() -> Value {
        let format = |id: &str, vcodec: &str, acodec: &str, width: u64, height: u64, size: u64| {
            json!({
                "format_id": id,
                "url": format!("https://v19-webapp-prime.tiktok.com/video/tos/alisg/{id}/"),
                "protocol": "https",
                "ext": "mp4",
                "vcodec": vcodec,
                "acodec": acodec,
                "width": width,
                "height": height,
                "filesize": size,
                "preference": -1,
                "http_headers": { "User-Agent": "Mozilla/5.0" },
                "cookies": "tt_chain_token=\"abc\"; Domain=.tiktok.com; Path=/"
            })
        };
        json!({
            "title": "Video dài",
            "format_id": "bytevc1_720p_698386-1+h264_540p_1526080-1",
            "protocol": "https+https",
            "ext": "mkv",
            "acodec": "aac",
            "requested_formats": [
                format("bytevc1_720p_698386-1", "h265", "none", 720, 1280, 11_107_596),
                format("h264_540p_1526080-1", "h264", "aac", 576, 1024, 33_376_714),
            ]
        })
    }

    #[test]
    fn merges_a_video_only_stream_with_the_audio_of_a_combined_one_in_that_order() {
        let parsed = parse(&tiktok_pair()).expect("resolved");
        let (layout, streams) = check(&parsed, None, true, 0).expect("eligible");
        assert_eq!(layout, Layout::Merge);
        let ids: Vec<&str> = streams.iter().map(|stream| stream.format_id.as_str()).collect();
        assert_eq!(
            ids,
            ["bytevc1_720p_698386-1", "h264_540p_1526080-1"],
            "hình đầu vào 0, format gộp (lấy tiếng) đầu vào 1"
        );
        assert!(streams[0].has_video() && !streams[0].has_audio());
        assert!(streams[1].has_video() && streams[1].has_audio());
        assert!(!streams[0].cookies.is_empty() && !streams[1].cookies.is_empty());
        assert_eq!(check(&parsed, None, false, 0), Err("merge needs ffmpeg"));
        // Tiếp tục task đã dừng: hai task con ⇒ vẫn là ghép.
        assert_eq!(Layout::infer(None, streams.len()), Some(Layout::Merge));
    }

    #[test]
    fn takes_the_sound_of_a_combined_stream_for_an_audio_job() {
        // Dòng M4A/MP3 của TikTok: một format gộp, ffmpeg bỏ hình khi tách tiếng.
        let combined = resolved(vec![stream("h264_540p_1526080-1", "https", "mp4", "h264", "aac")]);
        assert_eq!(
            check(&combined, Some("m4a"), true, 0).unwrap().0,
            Layout::Audio(AudioFormat::M4a)
        );
        assert_eq!(
            check(&combined, Some("mp3"), true, 0).unwrap().0,
            Layout::Audio(AudioFormat::Mp3)
        );
    }

    #[test]
    fn asks_yt_dlp_for_several_video_streams_only_when_two_ids_are_pinned() {
        // Không có `--video-multistreams`, `A+B` mà B cũng có hình chỉ trả A (mất tiếng); có cờ thì trả cả hai.
        let tiktok = "bytevc1_720p_698386-1+h264_540p_1526080-1/b[height<=1280]";
        assert_eq!(pair_args(tiktok, false), ["--video-multistreams"]);
        // Tải bằng yt-dlp: ffmpeg của yt-dlp map cả hình của format gộp, bỏ nó đi để file chỉ có một hình.
        assert_eq!(
            pair_args(tiktok, true),
            ["--video-multistreams", "--ppa", "Merger+ffmpeg_o:-map -1:v:0"]
        );
        assert_eq!(pair_args("399+140/bv*+ba/b", false), ["--video-multistreams"]);
        for selector in [
            "bv*+ba/b",
            "bv*[height<=720]+ba/b[height<=720]",
            "h264_540p_1526080-1/bv*[height<=1024]+ba/b[height<=1024]",
            "h264_540p_1526080-1/b",
            "140/ba[ext=m4a]/ba",
            "b",
        ] {
            assert!(pair_args(selector, false).is_empty(), "{selector}");
            assert!(pair_args(selector, true).is_empty(), "{selector}");
        }
    }

    #[test]
    fn infers_the_layout_of_saved_parts() {
        assert_eq!(Layout::infer(None, 2), Some(Layout::Merge));
        assert_eq!(Layout::infer(None, 1), Some(Layout::Single));
        assert_eq!(
            Layout::infer(Some("mp3"), 1),
            Some(Layout::Audio(AudioFormat::Mp3))
        );
        assert_eq!(Layout::infer(Some("m4a"), 2), None);
        assert_eq!(Layout::infer(None, 0), None);
    }

    #[test]
    fn detects_expired_links_and_declared_lengths() {
        let url = "https://rr4.googlevideo.com/videoplayback?expire=1000&clen=4096&itag=399";
        assert_eq!(expires_at(url), Some(1000));
        assert_eq!(declared_length(url), Some(4096));
        assert!(expired(url, 1000));
        assert!(expired(url, 800), "còn dưới 5 phút ⇒ lấy link mới trước");
        assert!(!expired(url, 600));
        assert!(!expired("https://cdn.example/video.mp4", 1_000_000));
        assert!(!expired("https://cdn.example/v.mp4?expire=soon", 0));
        assert_eq!(declared_length("not a url"), None);
    }

    #[test]
    fn makes_format_ids_safe_for_file_names() {
        assert_eq!(part_id("399"), "399");
        assert_eq!(part_id("hls-720p"), "hls-720p");
        assert_eq!(part_id("dash/v=1:2"), "dash_v_1_2");
        assert_eq!(part_id("../"), "");
    }
}
