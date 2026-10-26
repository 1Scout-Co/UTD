//! Luồng media tiện ích đọc thẳng từ trang (nút "Tải video này", notes/fb-media.md): trang yt-dlp không đọc được
//! (Tin Facebook, video trong bảng tin…) mà trình phát của trang đã có link luồng thật. Một luồng có cả hình lẫn
//! tiếng, hoặc hai luồng DASH tách rời (hình + tiếng) ⇒ đường tải nhanh của UTD tải song song rồi ffmpeg ghép.
//! Link luồng lưu trong journal (`page-streams:<task>`) để tải tiếp sau khi mở lại app; không có yt-dlp nào chạy.

use serde::{Deserialize, Serialize};
use utd_engine::{validate_url, Journal, MediaJob, TaskError};

use crate::media::fast::{ResolveFuture, Resolver};
use crate::media::resolve::{Resolved, Stream};

/// Selector của job đọc từ trang: không phải selector yt-dlp, runner nhận ra để dùng luồng đã lưu.
pub(crate) const SELECTOR: &str = "utd-page";
const SETTING_PREFIX: &str = "page-streams:";
/// Link luồng dài hơn thì từ chối (link fbcdn có chữ ký ~600 ký tự).
const MAX_URL_CHARS: usize = 4096;
const MAX_CODECS_CHARS: usize = 64;

/// Một luồng tiện ích gửi: `video` (chỉ hình), `audio` (chỉ tiếng) hoặc `both` (file thường có cả hai).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageStream {
    pub url: String,
    pub kind: Kind,
    #[serde(default)]
    pub codecs: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    Video,
    Audio,
    Both,
}

/// Kiểm đầu vào: một luồng có hình (`both`, hoặc `video` khi video không có tiếng), hoặc đúng một `video` + một
/// `audio` (thứ tự ra luôn hình trước). Link phải là http(s) hợp lệ.
pub(crate) fn checked(mut streams: Vec<PageStream>) -> Result<Vec<PageStream>, String> {
    const BAD: &str = "[media.page_streams] Luồng video trên trang không hợp lệ.";
    for stream in &mut streams {
        stream.url = stream.url.trim().to_string();
        if stream.url.len() > MAX_URL_CHARS {
            return Err(BAD.into());
        }
        stream.url = validate_url(&stream.url).map_err(|_| BAD)?.to_string();
        stream.codecs = stream
            .codecs
            .take()
            .map(|codecs| codecs.trim().to_ascii_lowercase())
            .filter(|codecs| {
                !codecs.is_empty()
                    && codecs.len() <= MAX_CODECS_CHARS
                    && codecs
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_'))
            });
    }
    match streams.as_slice() {
        [one] if one.kind != Kind::Audio => Ok(streams),
        [first, second] => match (first.kind, second.kind) {
            (Kind::Video, Kind::Audio) => Ok(streams),
            (Kind::Audio, Kind::Video) => {
                streams.swap(0, 1);
                Ok(streams)
            }
            _ => Err(BAD.into()),
        },
        _ => Err(BAD.into()),
    }
}

fn key(task_id: &str) -> String {
    format!("{SETTING_PREFIX}{task_id}")
}

pub(crate) fn save(journal: &Journal, task_id: &str, streams: &[PageStream]) -> Result<(), String> {
    let value = serde_json::to_string(streams).map_err(|error| error.to_string())?;
    journal
        .set_setting(&key(task_id), &value)
        .map_err(|error| error.to_string())
}

/// Luồng đã lưu của job đọc từ trang; job thường (selector yt-dlp) ⇒ `None`.
pub(crate) fn load(journal: &Journal, job: &MediaJob) -> Option<Vec<PageStream>> {
    if job.selector != SELECTOR {
        return None;
    }
    let value = journal.get_setting(&key(&job.task_id)).ok().flatten()?;
    serde_json::from_str(&value).ok()
}

/// Xoá task: bỏ luồng đã lưu theo (không có thì thôi).
pub(crate) fn forget(journal: &Journal, task_id: &str) {
    let _ = journal.delete_setting(&key(task_id));
}

/// Luồng cho đường tải nhanh: tên luồng con `<tên>.fpage-v.mp4` / `.fpage-a.m4a`; luồng tách rời là DASH nên
/// ffmpeg sửa vỏ khi ghép.
pub(super) fn resolved(streams: &[PageStream], page_url: Option<&str>) -> Resolved {
    // Máy chủ media (fbcdn…) có thể đòi trang gốc; như trình duyệt (strict-origin-when-cross-origin) chỉ gửi gốc
    // `https://www.facebook.com/`, không gửi địa chỉ Tin đầy đủ.
    let referer = page_url
        .and_then(|page| validate_url(page).ok())
        .map(|page| format!("{}/", page.origin().ascii_serialization()));
    let streams = streams
        .iter()
        .map(|stream| {
            let (format_id, ext, vcodec, acodec) = match stream.kind {
                Kind::Video => ("page-v", "mp4", stream.codecs.clone().unwrap_or_default(), "none".into()),
                Kind::Audio => ("page-a", "m4a", "none".into(), stream.codecs.clone().unwrap_or_default()),
                Kind::Both => ("page", "mp4", String::new(), String::new()),
            };
            Stream {
                format_id: format_id.into(),
                url: stream.url.clone(),
                protocol: "https".into(),
                ext: ext.into(),
                container: (stream.kind != Kind::Both).then(|| "mp4_dash".into()),
                vcodec,
                acodec,
                size: stream.size_bytes,
                exact_size: None,
                headers: referer
                    .iter()
                    .map(|origin| ("Referer".to_string(), origin.clone()))
                    .collect(),
                chunk: None,
                available_at: None,
                drm: false,
            }
        })
        .collect();
    Resolved {
        streams,
        drm: false,
        live: false,
    }
}

/// Thay yt-dlp trong đường tải nhanh: luôn trả đúng các luồng tiện ích đã gửi (link hết hạn thì máy chủ từ chối,
/// task báo lỗi; người dùng mở lại trang và bấm nút lần nữa).
pub(super) struct Fixed(pub Resolved);

impl Resolver for Fixed {
    fn resolve<'a>(&'a self, _url: &'a str, _selector: &'a str) -> ResolveFuture<'a> {
        let resolved = self.0.clone();
        Box::pin(async move { Ok::<Resolved, TaskError>(resolved) })
    }

    fn cached(&self, _job: &MediaJob) -> Option<Resolved> {
        Some(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIDEO: &str = "https://video.fsgn2-1.fna.fbcdn.net/o1/v/t2/f2/m69/v.mp4?efg=x&oh=00_A&oe=6720";
    const AUDIO: &str = "https://video.fsgn2-1.fna.fbcdn.net/o1/v/t2/f2/m69/a.mp4?efg=y&oh=00_B&oe=6720";

    fn stream(url: &str, kind: Kind) -> PageStream {
        PageStream {
            url: url.into(),
            kind,
            codecs: None,
            size_bytes: None,
        }
    }

    #[test]
    fn accepts_one_playable_stream_or_a_video_audio_pair() {
        assert!(checked(vec![stream(VIDEO, Kind::Both)]).is_ok());
        assert!(checked(vec![stream(VIDEO, Kind::Video)]).is_ok(), "video không tiếng");
        let pair = checked(vec![stream(AUDIO, Kind::Audio), stream(VIDEO, Kind::Video)]).unwrap();
        assert_eq!(pair[0].kind, Kind::Video, "hình luôn đứng trước");
        assert!(checked(vec![stream(AUDIO, Kind::Audio)]).is_err(), "chỉ tiếng thì không phải video");
        assert!(checked(vec![stream(VIDEO, Kind::Video), stream(VIDEO, Kind::Video)]).is_err());
        assert!(checked(vec![]).is_err());
        assert!(checked(vec![stream("blob:https://www.facebook.com/4c1a", Kind::Both)]).is_err());
        assert!(checked(vec![stream("file:///C:/x.mp4", Kind::Both)]).is_err());
        let mut odd = stream(VIDEO, Kind::Video);
        odd.codecs = Some("avc1.64001F\"; rm".into());
        assert_eq!(checked(vec![odd]).unwrap()[0].codecs, None, "codec lạ bị bỏ");
    }

    #[test]
    fn pair_becomes_a_dash_merge_for_the_fast_path() {
        let mut video = stream(VIDEO, Kind::Video);
        video.codecs = Some("avc1.64001f".into());
        video.size_bytes = Some(5_000_000);
        let resolved = resolved(&[video, stream(AUDIO, Kind::Audio)], Some("https://www.facebook.com/stories/1/2/"));
        let (layout, streams) = crate::media::resolve::check(&resolved, None, true, 0).unwrap();
        assert_eq!(layout, crate::media::resolve::Layout::Merge);
        assert_eq!(streams[0].format_id, "page-v");
        assert_eq!(streams[1].ext, "m4a");
        assert_eq!(streams[0].headers[0].1, "https://www.facebook.com/", "chỉ gốc trang, như trình duyệt");
        assert!(crate::media::resolve::check(&resolved, None, false, 0).is_err(), "ghép cần ffmpeg");
        let single = super::resolved(&[stream(VIDEO, Kind::Both)], None);
        let (layout, _) = crate::media::resolve::check(&single, None, false, 0).unwrap();
        assert_eq!(layout, crate::media::resolve::Layout::Single, "file thường không cần ffmpeg");
    }

    #[test]
    fn streams_survive_in_the_journal_until_the_task_is_removed() {
        let root = std::env::temp_dir().join(format!("utd-page-streams-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let journal = Journal::open(root.join("utd.db")).unwrap();
        let streams = checked(vec![stream(VIDEO, Kind::Video), stream(AUDIO, Kind::Audio)]).unwrap();
        save(&journal, "task-1", &streams).unwrap();
        let mut job = MediaJob {
            task_id: "task-1".into(),
            url: "https://www.facebook.com/stories/1/2/".into(),
            page_url: None,
            selector: SELECTOR.into(),
            audio_format: None,
            title: "Tin".into(),
            created_at: 0,
        };
        assert_eq!(load(&journal, &job), Some(streams));
        job.selector = "bv*+ba/b".into();
        assert_eq!(load(&journal, &job), None, "job yt-dlp không dùng luồng đã lưu");
        job.selector = SELECTOR.into();
        forget(&journal, "task-1");
        assert_eq!(load(&journal, &job), None);
        drop(journal);
        let _ = std::fs::remove_dir_all(root);
    }
}
