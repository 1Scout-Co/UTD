//! Task con ẩn: mỗi luồng yt-dlp chọn (video, âm thanh) là một task engine tải nhiều kết nối,
//! ghi `<thư mục tải>\<tên>.f<mã định dạng>.<đuôi>` như yt-dlp. Thứ tự task con (`created_at`)
//! là thứ tự đầu vào của ffmpeg: hình trước, tiếng sau.

use std::path::{Path, PathBuf};

use utd_engine::{allowed_request_headers, now_ms, DownloadTask, Journal, TaskStatus};

use crate::media::mux::Hint;
use crate::media::resolve::{part_id, Stream};

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// Tên file gốc bỏ đuôi: `Clip.mp4` → `Clip`.
pub(super) fn stem(task: &DownloadTask) -> String {
    Path::new(&task.file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("media")
        .to_string()
}

pub(super) fn part_name(stem: &str, stream: &Stream) -> String {
    format!("{stem}.f{}.{}", part_id(&stream.format_id), stream.ext)
}

/// File luồng đã tải xong của task con.
pub(super) fn stream_path(child: &DownloadTask) -> PathBuf {
    Path::new(&child.dest_dir).join(&child.file_name)
}

fn build(parent: &DownloadTask, stream: &Stream, connections: u16) -> Result<DownloadTask, String> {
    let mut child = DownloadTask::new(
        stream.url.clone(),
        part_name(&stem(parent), stream),
        parent.dest_dir.clone(),
        connections,
    )
    .map_err(text)?
    .with_parent(parent.id.clone());
    apply_stream(&mut child, stream);
    Ok(child)
}

/// URL, header của yt-dlp (không cookie) và trần byte mỗi yêu cầu.
fn apply_stream(child: &mut DownloadTask, stream: &Stream) {
    child.url = stream.url.clone();
    child.final_url = None;
    child.max_request_bytes = Some(stream.request_cap());
    child.request_headers = allowed_request_headers(stream.headers.clone());
}

/// Lưu task con theo đúng thứ tự `streams` (cha phải có sẵn trong journal).
pub(super) fn create(
    journal: &Journal,
    parent: &DownloadTask,
    streams: &[Stream],
    connections: u16,
) -> Result<Vec<DownloadTask>, String> {
    let mut children = Vec::with_capacity(streams.len());
    for stream in streams {
        match build(parent, stream, connections) {
            Ok(child) => children.push(child),
            Err(message) => {
                discard_all(journal, &children);
                return Err(message);
            }
        }
    }
    if let Err(message) = order(journal, &mut children) {
        discard_all(journal, &children);
        return Err(message);
    }
    Ok(children)
}

/// Đặt `created_at` tăng dần theo vị trí rồi lưu: `list_children` trả đúng thứ tự này.
/// Các mốc nằm trong quá khứ: engine ghi `updated_at = now` ngay sau đó, không được nhỏ hơn; và
/// không sau `completed_at` của luồng đã xong ở lượt trước (journal từ chối xong trước khi tạo).
fn order(journal: &Journal, children: &mut [DownloadTask]) -> Result<(), String> {
    let now = now_ms();
    let floor = children
        .iter()
        .filter_map(|child| child.completed_at)
        .min()
        .map_or(now, |at| at.min(now));
    let base = floor - children.len() as i64;
    for (index, child) in children.iter_mut().enumerate() {
        child.created_at = base + index as i64;
        child.updated_at = now;
        journal.save_task(child).map_err(text)?;
    }
    Ok(())
}

/// Sau khi hỏi lại yt-dlp: giữ task con cùng luồng (cùng kích thước ⇒ tải tiếp đúng các đoạn đã
/// có, chỉ thay link), tải lại từ đầu luồng đổi kích thước, bỏ luồng không còn được chọn.
pub(super) fn reconcile(
    journal: &Journal,
    parent: &DownloadTask,
    children: Vec<DownloadTask>,
    streams: &[Stream],
    connections: u16,
) -> Result<Vec<DownloadTask>, String> {
    let stem = stem(parent);
    let mut old = children;
    let mut kept = Vec::with_capacity(streams.len());
    for stream in streams {
        let name = part_name(&stem, stream);
        let found = old.iter().position(|child| child.file_name == name);
        let reused = found
            .map(|position| old.remove(position))
            .and_then(|mut child| {
                if size_conflict(&child, stream) {
                    discard(journal, &child);
                    return None;
                }
                if child.status != TaskStatus::Completed {
                    apply_stream(&mut child, stream);
                }
                Some(child)
            });
        match reused {
            Some(child) => kept.push(child),
            None => kept.push(build(parent, stream, connections)?),
        }
    }
    discard_all(journal, &old);
    order(journal, &mut kept)?;
    Ok(kept)
}

/// Kích thước đã dò khác kích thước chắc chắn mới ⇒ không ghép đoạn cũ với luồng mới.
fn size_conflict(child: &DownloadTask, stream: &Stream) -> bool {
    matches!((child.total_bytes, stream.exact_size), (Some(old), Some(new)) if old != new)
}

/// Task con xong mà file luồng đã mất: tải lại luồng đó.
pub(super) fn repair_missing(
    journal: &Journal,
    children: &mut [DownloadTask],
) -> Result<(), String> {
    for child in children.iter_mut() {
        if child.status == TaskStatus::Completed && !stream_path(child).is_file() {
            journal.reset_task_progress(&child.id).map_err(text)?;
            child.downloaded_bytes = 0;
            child.status = TaskStatus::Paused;
            child.completed_at = None;
            child.updated_at = now_ms();
            journal.save_task(child).map_err(text)?;
        }
    }
    Ok(())
}

/// Đã có byte nào trên đĩa chưa (đọc lại journal: engine vừa ghi tiến độ).
pub(super) fn has_bytes(journal: &Journal, parent_id: &str) -> bool {
    journal
        .list_children(parent_id)
        .map(|children| {
            children
                .iter()
                .any(|child| child.downloaded_bytes > 0 || child.status == TaskStatus::Completed)
        })
        // Không đọc được journal: coi như đã có, giữ nguyên để không mất tiến độ.
        .unwrap_or(true)
}

/// File tạm luôn là của UTD (tên có mã task). File luồng chỉ xoá khi chính task con đã tải xong:
/// chưa xong mà có file trùng tên (lỗi `file.exists`) thì đó là file khác, không đụng tới.
fn remove_files(child: &DownloadTask) {
    let _ = std::fs::remove_file(&child.temp_path);
    if child.status == TaskStatus::Completed {
        let _ = std::fs::remove_file(stream_path(child));
    }
}

/// Bỏ một task con cùng file tạm và file luồng của nó.
pub(super) fn discard(journal: &Journal, child: &DownloadTask) {
    remove_files(child);
    let _ = journal.delete_task(&child.id);
}

pub(super) fn discard_all(journal: &Journal, children: &[DownloadTask]) {
    for child in children {
        discard(journal, child);
    }
}

/// Dọn file tạm và file luồng của mọi task con trước khi xoá task media khỏi danh sách
/// (journal tự xoá dòng task con theo task cha). Gọi khi task không chạy.
pub(crate) fn discard_parts(journal: &Journal, parent_id: &str) {
    if let Ok(children) = journal.list_children(parent_id) {
        discard_all(journal, &children);
    }
}

/// File tạm yt-dlp (đường dự phòng) để lại khi task media chưa xong bị xoá: `<tên>.f<mã>.<đuôi>` và
/// bản `.part`/`.ytdl`/`.part-FragN` của nó, `<tên>.<đuôi>.part`, `<tên>.<đuôi>.ytdl`, `<tên>.temp.<đuôi>`.
/// Chỉ tên khớp đúng mẫu; file cuối `<tên>.<đuôi>` không bao giờ bị đụng. Gọi khi task không chạy.
pub(crate) fn discard_leftovers(task: &DownloadTask) {
    if task.status == TaskStatus::Completed {
        return;
    }
    let stem = stem(task);
    let Ok(entries) = std::fs::read_dir(&task.dest_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
        if is_file && entry.file_name().to_str().is_some_and(|name| is_leftover(&stem, name)) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn is_leftover(stem: &str, name: &str) -> bool {
    let Some(rest) = name.strip_prefix(stem).and_then(|rest| rest.strip_prefix('.')) else {
        return false;
    };
    let core = rest
        .strip_suffix(".part")
        .or_else(|| rest.strip_suffix(".ytdl"))
        .unwrap_or(rest);
    let core = core.find(".part-Frag").map_or(core, |at| &core[..at]);
    let temporary = core.len() != rest.len();
    let extension = |value: &str| {
        (1..=5).contains(&value.len()) && value.chars().all(|c| c.is_ascii_alphanumeric())
    };
    let format = |value: &str| {
        value.len() > 1
            && value.starts_with('f')
            && value[1..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    };
    match core.split('.').collect::<Vec<_>>().as_slice() {
        [id, ext] => (format(id) || *id == "temp") && extension(ext),
        [ext] => temporary && extension(ext),
        _ => false,
    }
}

pub(super) fn hints_from_streams(streams: &[Stream]) -> Vec<Hint> {
    streams
        .iter()
        .map(|stream| Hint {
            vcodec: stream.vcodec.clone(),
            acodec: stream.acodec.clone(),
            ext: stream.ext.clone(),
            remux: stream
                .container
                .as_deref()
                .is_some_and(|container| container.ends_with("_dash")),
        })
        .collect()
}

/// Lượt tiếp tục không hỏi yt-dlp: chỉ biết đuôi file, codec để ffmpeg thử; không rõ có phải DASH
/// không nên vẫn sửa vỏ.
pub(super) fn hints_from_children(children: &[DownloadTask]) -> Vec<Hint> {
    children
        .iter()
        .map(|child| Hint {
            ext: Path::new(&child.file_name)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_lowercase(),
            remux: true,
            ..Hint::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use utd_engine::{HashState, TaskKind};

    /// Thư mục thử, xoá sau cùng (journal SQLite phải đóng trước thì Windows mới cho xoá).
    struct TempRoot(PathBuf);

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct Fixture {
        journal: Journal,
        parent: DownloadTask,
        _root: TempRoot,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("utd-parts-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let journal = Journal::open(root.join("utd.db")).unwrap();
            let mut parent = DownloadTask::new(
                "https://www.youtube.com/watch?v=x",
                "Clip.mp4",
                root.to_string_lossy().into_owned(),
                1,
            )
            .unwrap()
            .with_kind(TaskKind::Media);
            parent.hash_state = HashState::Unavailable;
            journal.save_task(&parent).unwrap();
            Self {
                journal,
                parent,
                _root: TempRoot(root),
            }
        }
    }


    fn stream(format_id: &str, ext: &str, size: Option<u64>) -> Stream {
        Stream {
            format_id: format_id.into(),
            url: format!("https://rr1.googlevideo.com/videoplayback?itag={format_id}&expire=1"),
            protocol: "https".into(),
            ext: ext.into(),
            container: None,
            vcodec: if ext == "mp4" {
                "av01".into()
            } else {
                "none".into()
            },
            acodec: if ext == "mp4" {
                "none".into()
            } else {
                "opus".into()
            },
            size,
            exact_size: size,
            headers: vec![
                ("User-Agent".into(), "Mozilla/5.0".into()),
                ("Cookie".into(), "SID=secret".into()),
            ],
            chunk: Some(10 * 1024 * 1024),
            available_at: None,
            drm: false,
            cookies: Default::default(),
        }
    }

    #[test]
    fn yt_dlp_leftovers_match_only_temporary_names() {
        let stem = "Đi Cùng Pew";
        for name in [
            "Đi Cùng Pew.f399.mp4.part",
            "Đi Cùng Pew.f399.mp4",
            "Đi Cùng Pew.f251.webm.ytdl",
            "Đi Cùng Pew.f251-drc.webm.part-Frag12",
            "Đi Cùng Pew.f251.webm.part-Frag12.part",
            "Đi Cùng Pew.mp4.part",
            "Đi Cùng Pew.mp4.ytdl",
            "Đi Cùng Pew.temp.mp4",
        ] {
            assert!(is_leftover(stem, name), "{name}");
        }
        for name in [
            "Đi Cùng Pew.mp4",
            "Đi Cùng Pew (1).mp4.part",
            "Đi Cùng Pew.mp4.ab12cd34.part",
            "Khác.f399.mp4.part",
            "Đi Cùng Pew.ghi chú.txt",
        ] {
            assert!(!is_leftover(stem, name), "{name}");
        }
    }

    #[test]
    fn creates_hidden_children_in_input_order_without_cookies() {
        let fx = Fixture::new();
        let streams = [
            stream("399", "mp4", Some(900)),
            stream("251", "webm", Some(100)),
        ];
        let children = create(&fx.journal, &fx.parent, &streams, 16).unwrap();
        let saved = fx.journal.list_children(&fx.parent.id).unwrap();
        let names: Vec<&str> = saved.iter().map(|child| child.file_name.as_str()).collect();
        assert_eq!(names, ["Clip.f399.mp4", "Clip.f251.webm"]);
        assert!(saved.iter().all(|child| child.kind == TaskKind::MediaPart));
        assert_eq!(saved[0].max_request_bytes, Some(10 * 1024 * 1024));
        let headers: Vec<&str> = saved[0]
            .request_headers
            .iter()
            .map(|header| header.name.as_str())
            .collect();
        assert_eq!(headers, ["User-Agent"], "không bao giờ gửi cookie");
        assert_eq!(
            fx.journal.list_tasks().unwrap().len(),
            1,
            "danh sách chỉ có task cha"
        );
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn reconcile_keeps_same_size_streams_and_restarts_changed_ones() {
        let fx = Fixture::new();
        let first = [
            stream("399", "mp4", Some(900)),
            stream("251", "webm", Some(100)),
        ];
        let mut children = create(&fx.journal, &fx.parent, &first, 16).unwrap();
        children[0].total_bytes = Some(900);
        children[0].downloaded_bytes = 300;
        children[1].total_bytes = Some(100);
        for child in &children {
            fx.journal.save_task(child).unwrap();
        }
        std::fs::write(&children[1].temp_path, b"part").unwrap();
        let (video_id, audio_id) = (children[0].id.clone(), children[1].id.clone());
        let audio_part = PathBuf::from(&children[1].temp_path);

        let mut fresh = [
            stream("399", "mp4", Some(900)),
            stream("251", "webm", Some(120)),
        ];
        fresh[0].url = "https://rr2.googlevideo.com/videoplayback?itag=399&expire=9".into();
        let kept = reconcile(&fx.journal, &fx.parent, children, &fresh, 16).unwrap();
        assert_eq!(
            kept[0].id, video_id,
            "cùng kích thước ⇒ giữ các đoạn đã tải"
        );
        assert_eq!(kept[0].url, fresh[0].url);
        assert_eq!(kept[0].downloaded_bytes, 300);
        assert_ne!(kept[1].id, audio_id, "đổi kích thước ⇒ tải lại luồng đó");
        assert!(fx.journal.load_task(&audio_id).unwrap().is_none());
        assert!(!audio_part.exists(), "file tạm của luồng cũ bị dọn");
        let order: Vec<String> = fx
            .journal
            .list_children(&fx.parent.id)
            .unwrap()
            .into_iter()
            .map(|child| child.id)
            .collect();
        assert_eq!(
            order,
            [kept[0].id.clone(), kept[1].id.clone()],
            "hình vẫn ở đầu"
        );

        let only_audio = [stream("140", "m4a", None)];
        let replaced = reconcile(&fx.journal, &fx.parent, kept, &only_audio, 16).unwrap();
        assert_eq!(replaced.len(), 1);
        assert_eq!(fx.journal.list_children(&fx.parent.id).unwrap().len(), 1);
    }

    #[test]
    fn reconcile_keeps_a_stream_that_finished_earlier() {
        // Tiếng xong từ lượt trước, hình hết hạn link: hỏi lại yt-dlp không được làm hỏng journal.
        let fx = Fixture::new();
        let streams = [
            stream("399", "mp4", Some(900)),
            stream("251", "webm", Some(100)),
        ];
        let mut children = create(&fx.journal, &fx.parent, &streams, 16).unwrap();
        let earlier = now_ms() - 60_000;
        for child in children.iter_mut() {
            child.created_at = earlier;
            child.updated_at = earlier;
        }
        children[0].total_bytes = Some(900);
        children[0].downloaded_bytes = 300;
        let audio = &mut children[1];
        audio.status = TaskStatus::Completed;
        audio.hash_state = HashState::Unavailable;
        audio.total_bytes = Some(100);
        audio.downloaded_bytes = 100;
        audio.completed_at = Some(earlier + 1_000);
        audio.updated_at = earlier + 1_000;
        for child in &children {
            fx.journal.save_task(child).unwrap();
        }
        let (video_id, audio_id) = (children[0].id.clone(), children[1].id.clone());

        let mut fresh = streams.clone();
        fresh[0].url = "https://rr2.googlevideo.com/videoplayback?itag=399&expire=9".into();
        let kept = reconcile(&fx.journal, &fx.parent, children, &fresh, 16).unwrap();
        assert_eq!(kept[0].id, video_id);
        assert_eq!(kept[0].url, fresh[0].url);
        assert_eq!(kept[1].id, audio_id, "luồng đã xong được giữ");
        assert_eq!(kept[1].status, TaskStatus::Completed);
        let saved = fx.journal.list_children(&fx.parent.id).unwrap();
        let order: Vec<&str> = saved.iter().map(|child| child.id.as_str()).collect();
        assert_eq!(order, [video_id.as_str(), audio_id.as_str()], "hình vẫn ở đầu");
        for child in &saved {
            child.validate().unwrap();
        }
        // Lượt sau engine ghi `updated_at = now` cho luồng hình: vẫn hợp lệ.
        let mut video = saved[0].clone();
        video.updated_at = now_ms();
        fx.journal.save_task(&video).unwrap();
    }

    #[test]
    fn discards_part_files_and_rows() {
        let fx = Fixture::new();
        let streams = [stream("399", "mp4", None), stream("251", "webm", None)];
        let mut children = create(&fx.journal, &fx.parent, &streams, 16).unwrap();
        assert!(!has_bytes(&fx.journal, &fx.parent.id), "chưa tải byte nào");
        // Luồng hình đã tải xong (file luồng là của UTD); luồng tiếng còn dở mà có file trùng tên.
        children[0].status = TaskStatus::Completed;
        children[0].hash_state = HashState::Unavailable;
        fx.journal.save_task(&children[0]).unwrap();
        assert!(has_bytes(&fx.journal, &fx.parent.id));
        for child in &children {
            std::fs::write(&child.temp_path, b"part").unwrap();
            std::fs::write(stream_path(child), b"done").unwrap();
        }
        discard_parts(&fx.journal, &fx.parent.id);
        assert!(children.iter().all(|child| !Path::new(&child.temp_path).exists()));
        assert!(!stream_path(&children[0]).exists());
        assert!(
            stream_path(&children[1]).exists(),
            "file trùng tên của task chưa xong không phải của UTD"
        );
        assert!(fx.journal.list_children(&fx.parent.id).unwrap().is_empty());
        assert!(
            fx.journal.load_task(&fx.parent.id).unwrap().is_some(),
            "cha do người gọi xoá"
        );
    }

    #[test]
    fn hints_come_from_extensions_when_codecs_are_unknown() {
        let fx = Fixture::new();
        let streams = [stream("399", "mp4", None), stream("251", "webm", None)];
        let children = create(&fx.journal, &fx.parent, &streams, 16).unwrap();
        let hints = hints_from_children(&children);
        assert_eq!(hints[0].ext, "mp4");
        assert_eq!(hints[1].ext, "webm");
        assert!(hints[0].vcodec.is_empty());
        assert!(hints[0].remux, "không rõ DASH ⇒ vẫn sửa vỏ");
        let from_streams = hints_from_streams(&streams);
        assert_eq!(from_streams[1].acodec, "opus");
        assert!(!from_streams[0].remux, "luồng thường không cần sửa vỏ");
        let mut dash = streams[0].clone();
        dash.container = Some("mp4_dash".into());
        assert!(hints_from_streams(&[dash])[0].remux);
    }
}
