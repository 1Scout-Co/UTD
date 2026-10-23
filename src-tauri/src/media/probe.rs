use std::path::Path;
use std::process::Stdio;

use serde::Serialize;
use serde_json::Value;
use tokio::time::{timeout, Duration};

use crate::media::{tokio_command, tools};

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
    height: Option<u64>,
    has_video: bool,
    has_audio: bool,
    size: Option<u64>,
    tbr: Option<f64>,
    abr: Option<f64>,
}

pub async fn probe(
    yt_dlp: &Path,
    tools: &tools::MediaTools,
    url: &str,
) -> Result<MediaProbeResult, String> {
    let mut command = tokio_command(yt_dlp);
    command
        .args([
            "--encoding",
            "utf-8",
            "--no-playlist",
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
        return Err(classify_stderr(&String::from_utf8_lossy(&output.stderr)));
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "yt-dlp trả về dữ liệu không đọc được.".to_string())?;
    build_result(url, &value, tools.ffmpeg.is_some())
}

fn build_result(
    original_url: &str,
    value: &Value,
    ffmpeg_available: bool,
) -> Result<MediaProbeResult, String> {
    if value.get("_has_drm").and_then(Value::as_bool) == Some(true) {
        return Err("Nội dung có DRM; UTD không vượt qua bảo vệ bản quyền.".into());
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

    let formats: Vec<Format> = value
        .get("formats")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("has_drm").and_then(Value::as_bool) != Some(true))
                .filter_map(parse_format)
                .collect()
        })
        .unwrap_or_default();
    if formats.is_empty() {
        return Err("Không tìm thấy định dạng media có thể tải.".into());
    }
    if formats.iter().all(|format| !format.has_video && !format.has_audio) {
        return Err("Không tìm thấy định dạng media có thể tải.".into());
    }

    let mut heights: Vec<u64> = formats
        .iter()
        .filter(|format| format.has_video)
        .filter_map(|format| format.height)
        .filter(|height| *height > 0)
        .collect();
    heights.sort_unstable_by(|left, right| right.cmp(left));
    heights.dedup();
    let max_height = heights.first().copied();

    let best_audio = formats
        .iter()
        .filter(|format| format.has_audio && !format.has_video)
        .max_by(|left, right| audio_rank(left).partial_cmp(&audio_rank(right)).unwrap());
    let best_audio_size = best_audio.and_then(|format| format.size);
    let best_combined = formats
        .iter()
        .filter(|format| format.has_video && format.has_audio)
        .max_by_key(|format| format.height.unwrap_or(0));
    let best_video = formats
        .iter()
        .filter(|format| format.has_video)
        .max_by_key(|format| format.height.unwrap_or(0));

    let mut options = Vec::new();
    let best_size = best_combined
        .and_then(|format| format.size)
        .or_else(|| sum_size(best_video.and_then(|format| format.size), best_audio_size));
    let best_bitrate = best_video.and_then(|format| format.tbr);
    options.push(MediaFormatOption {
        id: "best".into(),
        label: "Tốt nhất".into(),
        detail: match max_height {
            Some(height) => format!("Video + âm thanh · tối đa {height}p"),
            None => "Video + âm thanh".into(),
        },
        size_bytes: best_size,
        bitrate_kbps: best_bitrate.map(|rate| rate.round() as u64),
        selector: "bv*+ba/b".into(),
        audio_format: None,
        badge: Some("Khuyên dùng".into()),
        recommended: true,
    });

    let preferred = [2160_u64, 1440, 1080, 720, 480, 360];
    let mut chosen: Vec<u64> = Vec::new();
    for height in preferred {
        if heights.contains(&height) {
            chosen.push(height);
        }
    }
    if let Some(maximum) = max_height
        && !chosen.contains(&maximum)
    {
        chosen.push(maximum);
        chosen.sort_unstable_by(|left, right| right.cmp(left));
    }
    chosen.truncate(5);
    for height in chosen {
        let size = formats
            .iter()
            .filter(|format| format.has_video && format.height == Some(height))
            .max_by_key(|format| format.size.unwrap_or(0))
            .and_then(|format| {
                if format.has_audio {
                    format.size
                } else {
                    sum_size(format.size, best_audio_size)
                }
            });
        let bitrate = formats
            .iter()
            .filter(|format| format.has_video && format.height == Some(height))
            .filter_map(|format| format.tbr)
            .fold(None::<f64>, |current, rate| {
                Some(current.map_or(rate, |value| value.max(rate)))
            });
        options.push(MediaFormatOption {
            id: height.to_string(),
            label: format!("{height}p"),
            detail: "Video + âm thanh".into(),
            size_bytes: size,
            bitrate_kbps: bitrate.map(|rate| rate.round() as u64),
            selector: format!("bv*[height<={height}]+ba/b[height<={height}]"),
            audio_format: None,
            badge: (height == 1080).then(|| "Phổ biến".into()),
            recommended: false,
        });
    }

    if formats.iter().any(|format| format.has_audio) {
        options.push(MediaFormatOption {
            id: "audio-m4a".into(),
            label: "Âm thanh M4A".into(),
            detail: "Chỉ âm thanh · giữ chất lượng gốc".into(),
            size_bytes: best_audio_size,
            bitrate_kbps: best_audio
                .and_then(|format| format.abr.or(format.tbr))
                .map(|rate| rate.round() as u64),
            selector: "ba[ext=m4a]/ba".into(),
            audio_format: Some("m4a".into()),
            badge: None,
            recommended: false,
        });
        if ffmpeg_available {
            options.push(MediaFormatOption {
                id: "audio-mp3".into(),
                label: "Âm thanh MP3".into(),
                detail: "Chuyển sang MP3 bằng ffmpeg".into(),
                size_bytes: None,
                bitrate_kbps: Some(192),
                selector: "ba/b".into(),
                audio_format: Some("mp3".into()),
                badge: None,
                recommended: false,
            });
        }
    }

    Ok(MediaProbeResult {
        url: original_url.to_string(),
        webpage_url,
        title,
        uploader,
        duration_seconds,
        thumbnail,
        formats: options,
        ffmpeg_available,
    })
}

fn parse_format(value: &Value) -> Option<Format> {
    let ext = value.get("ext").and_then(Value::as_str).unwrap_or("");
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
    let has_video = vcodec != "none" && height.is_some();
    let has_audio = acodec != "none";
    let size = value
        .get("filesize")
        .and_then(Value::as_u64)
        .or_else(|| value.get("filesize_approx").and_then(Value::as_u64));
    let tbr = value.get("tbr").and_then(Value::as_f64);
    let abr = value.get("abr").and_then(Value::as_f64);
    Some(Format {
        height,
        has_video,
        has_audio,
        size,
        tbr,
        abr,
    })
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

fn classify_stderr(stderr: &str) -> String {
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
                    "ext": "mp4",
                    "height": 1080,
                    "vcodec": "avc1",
                    "acodec": "none",
                    "filesize": 50_000_000,
                    "tbr": 2500.0
                },
                {
                    "ext": "m4a",
                    "vcodec": "none",
                    "acodec": "mp4a",
                    "filesize": 5_000_000,
                    "abr": 128.0
                },
                {
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

    #[test]
    fn builds_only_real_heights_and_audio_options() {
        let result = build_result("https://example.com/watch", &sample_info(), true)
            .expect("probe result");
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
    }

    #[test]
    fn omits_mp3_without_ffmpeg() {
        let result = build_result("https://example.com/watch", &sample_info(), false)
            .expect("probe result");
        assert!(
            !result
                .formats
                .iter()
                .any(|format| format.audio_format.as_deref() == Some("mp3"))
        );
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
    fn rejects_info_without_usable_formats() {
        let value = json!({ "title": "Empty", "formats": [] });
        assert!(build_result("https://example.com", &value, true).is_err());
    }
}
