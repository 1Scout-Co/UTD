//! Chất lượng chung cho cả lô link trang video (hộp Thêm hàng loạt, tải cả kênh): không hỏi từng link.
//! Lúc thêm chỉ ghi dấu `utd:<chất lượng>` vào selector của job; tới lượt chạy (hàng đợi đã giới hạn số
//! lượt cùng lúc), runner Phân tích như hộp Tải media (`yt-dlp -J`, điền sẵn `probe_cache` cho đường tải
//! nhanh), lấy đúng selector mà probe trả cho mức gần nhất, rồi đặt tên file theo tiêu đề video.

use std::path::Path;

use tokio_util::sync::CancellationToken;
use utd_engine::fetch::Url;
use utd_engine::{
    DownloadTask, ErrorClass, Journal, MediaJob, TaskStatus, now_ms, validate_file_name,
};

use crate::media::finish::{Sink, emit_task, error, fail, pause};
use crate::media::probe::{self, MediaFormatOption};
use crate::media::tools::MediaTools;
use crate::media::{safe_base, unique_base};

const MARKER: &str = "utd:";
const NO_FFMPEG_MP3: &str =
    "Cần ffmpeg để chuyển sang MP3. Tải ffmpeg ở Cài đặt → Công cụ media rồi thử lại.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Quality {
    Best,
    /// Tối đa chừng này dòng quét (1080, 720…).
    Height(u64),
    M4a,
    Mp3,
}

impl Quality {
    /// `best` · `<chiều cao>` (144–4320) · `audio-m4a` · `audio-mp3`.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "best" => Some(Self::Best),
            "audio-m4a" => Some(Self::M4a),
            "audio-mp3" => Some(Self::Mp3),
            other => other
                .parse::<u64>()
                .ok()
                .filter(|height| (144..=4320).contains(height))
                .map(Self::Height),
        }
    }

    pub(crate) fn from_selector(selector: &str) -> Option<Self> {
        selector.strip_prefix(MARKER).and_then(Self::parse)
    }

    pub(crate) fn selector(self) -> String {
        let id = match self {
            Self::Best => "best".to_string(),
            Self::Height(height) => height.to_string(),
            Self::M4a => "audio-m4a".to_string(),
            Self::Mp3 => "audio-mp3".to_string(),
        };
        format!("{MARKER}{id}")
    }

    pub(crate) fn audio_format(self) -> Option<&'static str> {
        match self {
            Self::M4a => Some("m4a"),
            Self::Mp3 => Some("mp3"),
            _ => None,
        }
    }

    /// Định dạng probe trả cho mức này. Thiếu đúng mức ⇒ mức thấp hơn gần nhất, không có mức thấp hơn ⇒
    /// mức thấp nhất, trang không có mức nào (chỉ âm thanh) ⇒ "Tốt nhất". MP3 cần ffmpeg: thiếu ⇒ `None`.
    pub(crate) fn pick(self, formats: &[MediaFormatOption]) -> Option<&MediaFormatOption> {
        let by_id = |id: &str| formats.iter().find(|format| format.id == id);
        match self {
            Self::Best => by_id("best").or_else(|| formats.first()),
            Self::M4a => by_id("audio-m4a"),
            Self::Mp3 => by_id("audio-mp3"),
            Self::Height(limit) => {
                let sized = || {
                    formats
                        .iter()
                        .filter_map(|format| Some((format.id.parse::<u64>().ok()?, format)))
                };
                let below = sized()
                    .filter(|(height, _)| *height <= limit)
                    .max_by_key(|(height, _)| *height);
                let lowest = || sized().min_by_key(|(height, _)| *height);
                below
                    .or_else(lowest)
                    .map(|(_, format)| format)
                    .or_else(|| Self::Best.pick(formats))
            }
        }
    }
}

/// Tên tạm tới lúc Phân tích xong: `youtube.com 09OGSRptb0M` (mã video hoặc phần cuối đường dẫn).
pub(crate) fn placeholder_title(url: &Url) -> String {
    let host = url.host_str().unwrap_or("media").trim_start_matches("www.");
    let video = url
        .query_pairs()
        .find(|(key, value)| key == "v" && !value.is_empty())
        .map(|(_, value)| value.into_owned());
    let last = url
        .path_segments()
        .and_then(|mut parts| parts.rfind(|part| !part.is_empty()))
        .map(str::to_string);
    match video.or(last) {
        Some(id) => format!("{host} {id}"),
        None => host.to_string(),
    }
}

/// Kết quả [`settle`].
pub(super) enum Settled {
    /// Job chạy được: selector thật (hoặc job vốn không có dấu chất lượng).
    Ready(MediaJob),
    /// Lượt này dừng ở đây (đã báo lỗi hoặc tạm dừng trên task; Tiếp tục thì Phân tích lại).
    Stopped,
    /// yt-dlp lỗi kiểu extractor (`auto::outdated`): người gọi kiểm bản mới rồi chạy lại, không có
    /// bản mới thì báo câu lỗi này bằng [`probe_failed`].
    Outdated(String),
}

/// Job có dấu chất lượng: Phân tích, ghi selector thật và tên theo tiêu đề.
pub(super) async fn settle(
    sink: &dyn Sink,
    journal: &Journal,
    task: &mut DownloadTask,
    job: MediaJob,
    tools: &MediaTools,
    cancel: &CancellationToken,
) -> Result<Settled, String> {
    let Some(quality) = Quality::from_selector(&job.selector) else {
        return Ok(Settled::Ready(job));
    };
    task.status = TaskStatus::Resolving;
    task.error = None;
    task.updated_at = now_ms();
    journal.save_task(task).map_err(|error| error.to_string())?;
    emit_task(sink, task);

    // Runner đã chờ ffmpeg (nếu đang tự tải) trước khi tới đây: chỉ tính bản đã có.
    let probed = tokio::select! {
        result = probe::probe(tools, &job.url, tools.ffmpeg.is_some()) => result,
        () = cancel.cancelled() => {
            let downloaded = task.downloaded_bytes;
            pause(sink, journal, task, downloaded)?;
            return Ok(Settled::Stopped);
        }
    };
    let result = match probed {
        Ok(result) => result,
        Err(failure) if failure.outdated => return Ok(Settled::Outdated(failure.message)),
        Err(failure) => {
            // Link rút gọn trỏ tới kênh: không có hộp nào mở ở đây, báo cách làm thay cho mã `channel_page:`.
            let message = if crate::media_pages::is_channel_page_error(&failure.message) {
                crate::media_pages::CHANNEL_IN_BATCH
            } else {
                &failure.message
            };
            probe_failed(sink, journal, task, message)?;
            return Ok(Settled::Stopped);
        }
    };
    let Some(format) = quality.pick(&result.formats) else {
        let problem = error(ErrorClass::User, "tool.ffmpeg_missing", NO_FFMPEG_MP3);
        fail(sink, journal, task, problem)?;
        return Ok(Settled::Stopped);
    };
    let job = MediaJob {
        selector: format.selector.clone(),
        audio_format: format.audio_format.clone(),
        title: result.title.chars().take(300).collect(),
        ..job
    };
    rename_for(task, &job.title, job.audio_format.as_deref());
    journal
        .save_media_job(&job)
        .map_err(|error| error.to_string())?;
    task.updated_at = now_ms();
    journal.save_task(task).map_err(|error| error.to_string())?;
    emit_task(sink, task);
    Ok(Settled::Ready(job))
}

/// Phân tích lỗi: task `failed` (Tiếp tục thì Phân tích lại).
pub(super) fn probe_failed(
    sink: &dyn Sink,
    journal: &Journal,
    task: &mut DownloadTask,
    message: &str,
) -> Result<(), String> {
    let problem = error(ErrorClass::Transient, "media.probe_failed", message);
    fail(sink, journal, task, problem)
}

/// Đặt tên theo tiêu đề (không trùng file có sẵn); `.part` đi theo tên mới như lúc tạo task.
fn rename_for(task: &mut DownloadTask, title: &str, audio_format: Option<&str>) {
    let destination = Path::new(&task.dest_dir);
    let base = unique_base(destination, &safe_base(title));
    let name = format!("{base}.{}", audio_format.unwrap_or("mp4"));
    if validate_file_name(&name).is_err() {
        return;
    }
    let tag = task.id.get(..8).unwrap_or(&task.id);
    task.temp_path = destination
        .join(format!("{name}.{tag}.part"))
        .to_string_lossy()
        .into_owned();
    task.file_name = name;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(id: &str) -> MediaFormatOption {
        MediaFormatOption {
            id: id.into(),
            label: id.into(),
            detail: String::new(),
            size_bytes: None,
            bitrate_kbps: None,
            selector: format!("sel-{id}"),
            audio_format: id.strip_prefix("audio-").map(str::to_string),
            badge: None,
            recommended: id == "best",
        }
    }

    fn options(ids: &[&str]) -> Vec<MediaFormatOption> {
        ids.iter().map(|id| option(id)).collect()
    }

    fn picked(quality: &str, formats: &[MediaFormatOption]) -> Option<String> {
        Quality::parse(quality)
            .unwrap()
            .pick(formats)
            .map(|format| format.id.clone())
    }

    #[test]
    fn parses_only_known_qualities() {
        assert_eq!(Quality::parse("best"), Some(Quality::Best));
        assert_eq!(Quality::parse(" 1080 "), Some(Quality::Height(1080)));
        assert_eq!(Quality::parse("audio-mp3"), Some(Quality::Mp3));
        for bad in ["", "4k", "99", "99999", "audio-ogg", "bv*+ba/b"] {
            assert_eq!(Quality::parse(bad), None, "{bad}");
        }
        assert_eq!(Quality::Height(720).selector(), "utd:720");
        assert_eq!(Quality::from_selector("utd:audio-m4a"), Some(Quality::M4a));
        assert_eq!(
            Quality::from_selector("399+140/bv*+ba/b"),
            None,
            "selector thật"
        );
        assert_eq!(Quality::M4a.audio_format(), Some("m4a"));
        assert_eq!(Quality::Best.audio_format(), None);
    }

    #[test]
    fn picks_the_nearest_height_the_page_really_has() {
        let all = options(&[
            "best",
            "2160",
            "1080",
            "720",
            "360",
            "audio-m4a",
            "audio-mp3",
        ]);
        assert_eq!(picked("best", &all).as_deref(), Some("best"));
        assert_eq!(picked("1080", &all).as_deref(), Some("1080"));
        assert_eq!(picked("720", &all).as_deref(), Some("720"));
        let tall = options(&["best", "1440", "1080", "480"]);
        assert_eq!(
            picked("720", &tall).as_deref(),
            Some("480"),
            "thấp hơn gần nhất"
        );
        let only_big = options(&["best", "2160", "1440"]);
        assert_eq!(
            picked("1080", &only_big).as_deref(),
            Some("1440"),
            "không có mức thấp hơn"
        );
        let audio_only = options(&["best", "audio-m4a"]);
        assert_eq!(picked("720", &audio_only).as_deref(), Some("best"));
        assert_eq!(
            picked("audio-m4a", &audio_only).as_deref(),
            Some("audio-m4a")
        );
        assert_eq!(picked("audio-mp3", &audio_only), None, "MP3 cần ffmpeg");
    }

    #[test]
    fn placeholder_names_use_the_video_id() {
        let name = |raw: &str| placeholder_title(&Url::parse(raw).unwrap());
        assert_eq!(
            name("https://www.youtube.com/watch?v=09OGSRptb0M&t=3"),
            "youtube.com 09OGSRptb0M"
        );
        assert_eq!(name("https://youtu.be/abc123"), "youtu.be abc123");
        assert_eq!(
            name("https://www.tiktok.com/@user/video/7234567890123456789"),
            "tiktok.com 7234567890123456789"
        );
        assert_eq!(name("https://vimeo.com/"), "vimeo.com");
    }

    #[test]
    fn renaming_moves_the_part_name_too() {
        let dir = std::env::temp_dir().join(format!("utd-preset-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Clip hay.mp4"), b"x").unwrap();
        let folder = dir.to_string_lossy().into_owned();
        let mut task =
            DownloadTask::new("https://youtu.be/a", "youtu.be a.mp4", &*folder, 1).unwrap();
        rename_for(&mut task, "Clip hay", Some("m4a"));
        assert_eq!(
            task.file_name, "Clip hay (1).m4a",
            "không trùng file có sẵn"
        );
        assert!(
            task.temp_path
                .ends_with(&format!("Clip hay (1).m4a.{}.part", &task.id[..8]))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
