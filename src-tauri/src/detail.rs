//! Màn Chi tiết download (S-15..S-21): task, bản đồ đoạn và lịch sử đọc từ journal. Bản đồ đoạn mới
//! được đẩy (`utd://segments`) cho đúng MỘT task đang mở, theo nhịp tiến độ engine vốn đã báo — giao
//! diện không hỏi dồn, 50 download chạy cùng lúc chỉ tốn một phép so tên mỗi lần báo tiến độ (AC-23).

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use utd_engine::{DownloadTask, Journal, Segment, TaskEvent, TaskKind, TaskStatus};

use crate::state::AppState;

pub const SEGMENTS_EVENT: &str = "utd://segments";

/// Journal ghi mốc đoạn mỗi giây (`checkpoint_interval`): đẩy dày hơn cũng không có gì mới.
const PUSH_EVERY: Duration = Duration::from_millis(900);

/// Vai của một luồng trong bản đồ đoạn. Task media có task con ẩn theo thứ tự đầu vào ffmpeg:
/// hai luồng ⇒ hình rồi tiếng, một luồng ⇒ luồng gộp (media/parts.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PartRole {
    File,
    Video,
    Audio,
    Stream,
}

/// Một luồng tải (task HTTP, hoặc task con của media): không có link (link luồng media là link ký có hạn).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailPart {
    pub id: String,
    pub role: PartRole,
    pub file_name: String,
    pub status: TaskStatus,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    /// Trống: chưa chia đoạn, hoặc tải một luồng (server không cho Range / không báo kích thước).
    pub segments: Vec<Segment>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadDetail {
    pub task: DownloadTask,
    pub parts: Vec<DetailPart>,
    pub events: Vec<TaskEvent>,
    /// Nơi file nằm khi xong (`dest_dir` + tên).
    pub target_path: String,
    /// Task đã xong: file còn ở đó không (người dùng xoá/chuyển bằng Explorer). Chưa xong: None.
    pub file_exists: Option<bool>,
    /// Tiêu đề video (task media).
    pub media_title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartsUpdate {
    pub task_id: String,
    pub parts: Vec<DetailPart>,
}

/// Task đang mở ở màn Chi tiết (nhiều nhất một) và lần đẩy gần nhất.
#[derive(Default)]
pub struct DetailWatch {
    inner: Mutex<Watch>,
}

#[derive(Default)]
struct Watch {
    task_id: Option<String>,
    pushed: Option<Instant>,
}

impl DetailWatch {
    pub fn set(&self, task_id: Option<String>) {
        if let Ok(mut watch) = self.inner.lock() {
            *watch = Watch {
                task_id,
                pushed: None,
            };
        }
    }

    /// `task_id` đang mở và đã đủ nhịp: ghi nhận lần đẩy này.
    fn due(&self, task_id: &str, now: Instant) -> bool {
        let Ok(mut watch) = self.inner.lock() else {
            return false;
        };
        if watch.task_id.as_deref() != Some(task_id) {
            return false;
        }
        if watch
            .pushed
            .is_some_and(|last| now.saturating_duration_since(last) < PUSH_EVERY)
        {
            return false;
        }
        watch.pushed = Some(now);
        true
    }
}

/// Một lần đọc cho màn Chi tiết. `None` = task không còn (S-16); lỗi đọc journal ⇒ Err (S-18).
#[tauri::command]
pub async fn download_detail(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<Option<DownloadDetail>, String> {
    let journal = state.journal.clone();
    tauri::async_runtime::spawn_blocking(move || read_detail(&journal, &task_id))
        .await
        .map_err(|error| error.to_string())?
}

/// Màn Chi tiết mở task nào (`None` = đã đóng). Chỉ task này được đẩy bản đồ đoạn.
#[tauri::command]
pub fn watch_download(app: AppHandle, task_id: Option<String>) {
    if let Some(watch) = app.try_state::<DetailWatch>() {
        watch.set(task_id);
    }
}

/// Nhật ký một task để dán vào báo lỗi ("Sao chép log"): link bỏ query (token), thư mục người dùng
/// thành `~`, không có header.
#[tauri::command]
pub async fn download_log(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> Result<String, String> {
    let journal = state.journal.clone();
    let home = app.path().home_dir().ok();
    tauri::async_runtime::spawn_blocking(move || {
        let detail = read_detail(&journal, &task_id)?
            .ok_or_else(|| "download not found".to_string())?;
        Ok(crate::detail_log::render(&detail, home.as_deref()))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Engine (HTTP) và runner media gọi sau mỗi lần báo tiến độ: task đang mở thì đẩy bản đồ đoạn.
pub(crate) fn progressed(app: &AppHandle, task_id: &str) {
    let Some(watch) = app.try_state::<DetailWatch>() else {
        return;
    };
    if !watch.due(task_id, Instant::now()) {
        return;
    }
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    match read_task_parts(&state.journal, task_id) {
        Ok(Some(parts)) => {
            let update = PartsUpdate {
                task_id: task_id.to_string(),
                parts,
            };
            let _ = app.emit(SEGMENTS_EVENT, update);
        }
        Ok(None) => {}
        Err(error) => log::warn!("Detail segments read failed: {error}"),
    }
}

pub(crate) fn read_detail(journal: &Journal, task_id: &str) -> Result<Option<DownloadDetail>, String> {
    let text = |error: utd_engine::Error| error.to_string();
    let Some(task) = journal.load_task(task_id).map_err(text)? else {
        return Ok(None);
    };
    let parts = parts_of(journal, &task)?;
    let events = journal.task_events(task_id).map_err(text)?;
    let media_title = if task.kind == TaskKind::Media {
        journal.load_media_job(task_id).map_err(text)?.map(|job| job.title)
    } else {
        None
    };
    let target = Path::new(&task.dest_dir).join(&task.file_name);
    let file_exists = (task.status == TaskStatus::Completed).then(|| target.is_file());
    Ok(Some(DownloadDetail {
        target_path: target.to_string_lossy().into_owned(),
        file_exists,
        media_title,
        parts,
        events,
        task,
    }))
}

fn read_task_parts(journal: &Journal, task_id: &str) -> Result<Option<Vec<DetailPart>>, String> {
    match journal.load_task(task_id).map_err(|error| error.to_string())? {
        Some(task) => parts_of(journal, &task).map(Some),
        None => Ok(None),
    }
}

/// Task HTTP: chính nó. Task media: các task con (hình, tiếng); tải bằng yt-dlp thì không có luồng nào.
fn parts_of(journal: &Journal, task: &DownloadTask) -> Result<Vec<DetailPart>, String> {
    let text = |error: utd_engine::Error| error.to_string();
    let streams = if task.kind == TaskKind::Media {
        journal.list_children(&task.id).map_err(text)?
    } else {
        vec![task.clone()]
    };
    let roles = roles(task.kind, streams.len());
    streams
        .into_iter()
        .zip(roles)
        .map(|(stream, role)| {
            let segments = journal.load_segments(&stream.id).map_err(text)?;
            Ok(DetailPart {
                id: stream.id,
                role,
                file_name: stream.file_name,
                status: stream.status,
                total_bytes: stream.total_bytes,
                downloaded_bytes: stream.downloaded_bytes,
                segments,
            })
        })
        .collect()
}

fn roles(kind: TaskKind, count: usize) -> Vec<PartRole> {
    match (kind, count) {
        (TaskKind::Media, 2) => vec![PartRole::Video, PartRole::Audio],
        (TaskKind::Media, _) => vec![PartRole::Stream; count],
        _ => vec![PartRole::File; count],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use utd_engine::{now_ms, plan_segments, ErrorClass, HashState, TaskError};

    fn scratch() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("utd-detail-{}", uuid::Uuid::new_v4()))
    }

    fn journal(root: &Path) -> Journal {
        Journal::open(root.join("data").join("utd.db")).unwrap()
    }

    fn task(root: &Path, url: &str, name: &str) -> DownloadTask {
        DownloadTask::new(url, name, root.to_string_lossy().into_owned(), 8).unwrap()
    }

    #[test]
    fn missing_task_is_empty_not_an_error() {
        let root = scratch();
        let journal = journal(&root);
        assert!(read_detail(&journal, "khong-co").unwrap().is_none());
        drop(journal);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn http_task_has_one_part_with_its_segment_map_and_history() {
        let root = scratch();
        let journal = journal(&root);
        let total = 8 * 1024 * 1024;
        let mut download = task(&root, "https://example.com/a.iso", "a.iso").with_total_bytes(total);
        journal.save_task(&download).unwrap();
        let mut plan = plan_segments(total, 4);
        plan[0].downloaded = plan[0].byte_len();
        plan[0].status = utd_engine::SegmentStatus::Done;
        plan[1].downloaded = 1024;
        plan[1].status = utd_engine::SegmentStatus::Active;
        journal.save_segments(&download.id, &plan).unwrap();
        download.downloaded_bytes = plan[0].downloaded + 1024;
        download.status = TaskStatus::Failed;
        download.error =
            Some(TaskError::new(ErrorClass::Transient, "network.closed", "Closed early").unwrap());
        journal.save_task(&download).unwrap();

        let detail = read_detail(&journal, &download.id).unwrap().unwrap();
        assert_eq!(detail.parts.len(), 1);
        let part = &detail.parts[0];
        assert_eq!(part.role, PartRole::File);
        assert_eq!(part.segments.len(), 4);
        assert_eq!(part.segments[1].downloaded, 1024);
        assert_eq!(detail.file_exists, None, "chưa xong thì không hỏi đĩa");
        assert_eq!(detail.events.last().unwrap().code.as_deref(), Some("network.closed"));
        assert!(detail.target_path.ends_with("a.iso"));
        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["parts"][0]["role"], "file");
        assert_eq!(json["events"][0]["status"], "queued");
        drop(journal);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn media_task_shows_video_then_audio_without_stream_links() {
        let root = scratch();
        let journal = journal(&root);
        let media = task(&root, "https://www.youtube.com/watch?v=x", "Clip.mp4").with_kind(TaskKind::Media);
        journal.save_task(&media).unwrap();
        journal
            .save_media_job(&utd_engine::MediaJob {
                task_id: media.id.clone(),
                url: media.url.clone(),
                page_url: None,
                selector: "bv*+ba/b".into(),
                audio_format: None,
                title: "Một clip".into(),
                created_at: now_ms(),
            })
            .unwrap();
        for (offset, name) in [(2, "Clip.f399.mp4"), (1, "Clip.f251.webm")] {
            let mut child = task(&root, "https://rr1.googlevideo.com/videoplayback?sig=SECRET", name)
                .with_parent(media.id.clone());
            child.created_at = now_ms() - offset;
            journal.save_task(&child).unwrap();
        }
        let detail = read_detail(&journal, &media.id).unwrap().unwrap();
        let roles: Vec<PartRole> = detail.parts.iter().map(|part| part.role).collect();
        assert_eq!(roles, [PartRole::Video, PartRole::Audio]);
        assert_eq!(detail.parts[0].file_name, "Clip.f399.mp4");
        assert_eq!(detail.media_title.as_deref(), Some("Một clip"));
        assert!(!serde_json::to_string(&detail.parts).unwrap().contains("SECRET"));
        drop(journal);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn completed_task_reports_whether_the_file_is_still_there() {
        let root = scratch();
        std::fs::create_dir_all(&root).unwrap();
        let journal = journal(&root);
        let mut done = task(&root, "https://example.com/b.zip", "b.zip").with_total_bytes(3);
        done.downloaded_bytes = 3;
        done.status = TaskStatus::Completed;
        done.hash_state = HashState::Unavailable;
        done.completed_at = Some(now_ms());
        journal.save_task(&done).unwrap();
        assert_eq!(read_detail(&journal, &done.id).unwrap().unwrap().file_exists, Some(false));
        std::fs::write(root.join("b.zip"), b"abc").unwrap();
        assert_eq!(read_detail(&journal, &done.id).unwrap().unwrap().file_exists, Some(true));
        drop(journal);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_the_open_task_is_pushed_and_not_more_than_once_a_beat() {
        let watch = DetailWatch::default();
        let start = Instant::now();
        assert!(!watch.due("a", start), "chưa mở gì");
        watch.set(Some("a".into()));
        assert!(!watch.due("b", start));
        assert!(watch.due("a", start));
        assert!(!watch.due("a", start + Duration::from_millis(300)));
        assert!(watch.due("a", start + PUSH_EVERY));
        watch.set(Some("a".into()));
        assert!(watch.due("a", start + PUSH_EVERY), "mở lại thì đẩy ngay");
        watch.set(None);
        assert!(!watch.due("a", start + PUSH_EVERY * 3));
    }
}
