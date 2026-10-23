//! Task con ẩn (luồng của một task media): không vào danh sách/đếm/khôi phục, xoá theo cha.

use std::path::Path;

use tempfile::TempDir;
use utd_engine::{DownloadTask, Journal, MediaJob, TaskKind, TaskStatus, now_ms, plan_segments};

fn setup() -> (TempDir, Journal) {
    let directory = tempfile::tempdir().expect("tempdir");
    let journal = Journal::open(directory.path().join("utd.db")).expect("journal");
    (directory, journal)
}

fn downloads(directory: &Path) -> String {
    directory.join("downloads").to_string_lossy().into_owned()
}

fn parent(directory: &Path) -> DownloadTask {
    DownloadTask::new(
        "https://www.youtube.com/watch?v=09OGSRptb0M",
        "Video.mp4",
        downloads(directory),
        1,
    )
    .expect("parent")
    .with_kind(TaskKind::Media)
}

fn child(directory: &Path, parent: &DownloadTask, name: &str) -> DownloadTask {
    let mut task = DownloadTask::new(
        format!("https://rr1.googlevideo.com/videoplayback?file={name}"),
        name,
        downloads(directory),
        16,
    )
    .expect("child")
    .with_parent(parent.id.clone());
    task.max_request_bytes = Some(10 * 1024 * 1024);
    task
}

#[test]
fn children_stay_out_of_lists_counts_and_recovery() {
    let (directory, journal) = setup();
    let media = parent(directory.path());
    journal.save_task(&media).expect("parent");
    let other = DownloadTask::new(
        "https://example.com/a.iso",
        "a.iso",
        downloads(directory.path()),
        8,
    )
    .expect("other");
    journal.save_task(&other).expect("other");
    let video = child(directory.path(), &media, "Video.f399.webm");
    let mut audio = child(directory.path(), &media, "Video.f251.webm");
    audio.created_at = video.created_at + 1;
    audio.updated_at = audio.created_at;
    journal.save_task(&video).expect("video");
    journal.save_task(&audio).expect("audio");

    let listed: Vec<String> = journal
        .list_tasks()
        .expect("list")
        .into_iter()
        .map(|task| task.id)
        .collect();
    assert_eq!(listed.len(), 2);
    assert!(listed.contains(&media.id) && listed.contains(&other.id));

    let queued = journal
        .status_counts()
        .expect("counts")
        .into_iter()
        .find(|(status, _)| *status == TaskStatus::Queued)
        .map(|(_, count)| count);
    assert_eq!(queued, Some(2));
    assert_eq!(journal.recoverable_tasks().expect("recoverable").len(), 2);

    let children = journal.list_children(&media.id).expect("children");
    assert_eq!(children, vec![video.clone(), audio.clone()]);
    assert!(journal.list_children(&other.id).expect("none").is_empty());
    let loaded = journal.load_task(&video.id).expect("load").expect("video");
    assert_eq!(loaded.kind, TaskKind::MediaPart);
    assert_eq!(loaded.parent_id.as_deref(), Some(media.id.as_str()));
}

#[test]
fn deleting_the_parent_removes_children_segments_and_media_job() {
    let (directory, journal) = setup();
    let media = parent(directory.path());
    journal.save_task(&media).expect("parent");
    journal
        .save_media_job(&MediaJob {
            task_id: media.id.clone(),
            url: media.url.clone(),
            page_url: None,
            selector: "bv*+ba/b".into(),
            audio_format: None,
            title: "Video".into(),
            created_at: now_ms(),
        })
        .expect("job");
    let mut video = child(directory.path(), &media, "Video.f399.webm");
    video.total_bytes = Some(40 * 1024 * 1024);
    journal.save_task(&video).expect("video");
    journal
        .save_segments(&video.id, &plan_segments(40 * 1024 * 1024, 8))
        .expect("segments");
    let audio = child(directory.path(), &media, "Video.f251.webm");
    journal.save_task(&audio).expect("audio");

    let removed = journal
        .delete_task_with_children(&media.id)
        .expect("delete");
    let mut removed_ids: Vec<String> = removed.into_iter().map(|task| task.id).collect();
    removed_ids.sort();
    let mut expected = vec![video.id.clone(), audio.id.clone()];
    expected.sort();
    assert_eq!(removed_ids, expected);
    for id in [&media.id, &video.id, &audio.id] {
        assert!(
            journal.load_task(id).expect("load").is_none(),
            "{id} survived"
        );
    }
    assert!(
        journal
            .load_segments(&video.id)
            .expect("segments")
            .is_empty()
    );
    assert!(journal.load_media_job(&media.id).expect("job").is_none());
    assert!(journal.integrity_check().expect("integrity").is_empty());

    // `delete_task` thường cũng xoá theo cha.
    let media = parent(directory.path());
    journal.save_task(&media).expect("parent");
    let part = child(directory.path(), &media, "Video.f140.m4a");
    journal.save_task(&part).expect("part");
    journal.delete_task(&media.id).expect("delete");
    assert!(journal.load_task(&part.id).expect("load").is_none());
}

#[test]
fn mark_interrupted_also_pauses_children() {
    let (directory, journal) = setup();
    let mut media = parent(directory.path());
    media.status = TaskStatus::Downloading;
    journal.save_task(&media).expect("parent");
    let mut video = child(directory.path(), &media, "Video.f399.webm");
    video.status = TaskStatus::Downloading;
    journal.save_task(&video).expect("video");

    assert_eq!(journal.mark_interrupted().expect("mark"), 2);
    let video = journal.load_task(&video.id).expect("load").expect("video");
    assert_eq!(video.status, TaskStatus::Paused);
}

#[test]
fn a_child_needs_an_existing_parent() {
    let (directory, journal) = setup();
    let ghost = parent(directory.path());
    let orphan = child(directory.path(), &ghost, "orphan.webm");
    assert!(
        journal.save_task(&orphan).is_err(),
        "orphan child was saved"
    );
    assert!(journal.list_tasks().expect("list").is_empty());
}
