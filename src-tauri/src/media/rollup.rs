//! Gộp tiến độ các luồng con (ẩn) thành một dòng của task media trên giao diện.

use utd_engine::{DownloadTask, EngineEvent, HashState, ProgressSnapshot, TaskStatus};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Part {
    id: String,
    downloaded: u64,
    total: Option<u64>,
    /// Kích thước yt-dlp đoán, dùng khi engine chưa dò ra.
    estimate: Option<u64>,
    speed: u64,
    connections: u16,
    segments_done: u32,
    segments_total: u32,
    completed: bool,
}

#[derive(Debug, Default)]
pub(super) struct Rollup {
    parts: Vec<Part>,
}

impl Rollup {
    /// `estimates[i]` ứng với `children[i]`.
    pub fn new(children: &[DownloadTask], estimates: &[Option<u64>]) -> Self {
        let parts = children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                let completed = child.status == TaskStatus::Completed;
                Part {
                    id: child.id.clone(),
                    downloaded: if completed {
                        child.total_bytes.unwrap_or(child.downloaded_bytes)
                    } else {
                        child.downloaded_bytes
                    },
                    total: child.total_bytes,
                    estimate: estimates.get(index).copied().flatten(),
                    completed,
                    ..Part::default()
                }
            })
            .collect();
        Self { parts }
    }

    pub fn apply(&mut self, event: &EngineEvent) {
        match event {
            EngineEvent::Progress { progress } => {
                if let Some(part) = self.part(&progress.task_id) {
                    part.downloaded = progress.downloaded_bytes;
                    if progress.total_bytes.is_some() {
                        part.total = progress.total_bytes;
                    }
                    part.speed = progress.bytes_per_second;
                    part.connections = progress.connections;
                    part.segments_done = progress.segments_done;
                    part.segments_total = progress.segments_total;
                }
            }
            EngineEvent::Task { task } => self.update(task),
        }
    }

    /// Trạng thái mới của một task con; dừng/xong/lỗi thì hết tốc độ và kết nối.
    pub fn update(&mut self, task: &DownloadTask) {
        let Some(part) = self.part(&task.id) else {
            return;
        };
        if task.total_bytes.is_some() {
            part.total = task.total_bytes;
        }
        part.downloaded = task.downloaded_bytes;
        part.completed = task.status == TaskStatus::Completed;
        if part.completed {
            part.downloaded = part.total.unwrap_or(part.downloaded);
            part.segments_done = part.segments_total;
        }
        if !task.status.is_active() {
            part.speed = 0;
            part.connections = 0;
        }
    }

    fn part(&mut self, id: &str) -> Option<&mut Part> {
        self.parts.iter_mut().find(|part| part.id == id)
    }

    pub fn downloaded(&self) -> u64 {
        self.parts
            .iter()
            .map(|part| part.downloaded)
            .fold(0, u64::saturating_add)
    }

    /// Tổng kích thước; thiếu một luồng (chưa dò, yt-dlp không đoán) thì chưa biết.
    pub fn total(&self) -> Option<u64> {
        self.parts
            .iter()
            .map(|part| part.total.or(part.estimate))
            .try_fold(0u64, |sum, size| size.map(|size| sum.saturating_add(size)))
    }

    pub fn snapshot(&self, parent: &DownloadTask) -> ProgressSnapshot {
        let total = self.total();
        let downloaded = total.map_or(self.downloaded(), |total| self.downloaded().min(total));
        ProgressSnapshot {
            task_id: parent.id.clone(),
            status: parent.status,
            downloaded_bytes: downloaded,
            total_bytes: total,
            bytes_per_second: self.parts.iter().map(|part| part.speed).sum(),
            connections: self
                .parts
                .iter()
                .map(|part| part.connections)
                .fold(0, u16::saturating_add),
            segments_done: self.parts.iter().map(|part| part.segments_done).sum(),
            segments_total: self.parts.iter().map(|part| part.segments_total).sum(),
            hash_state: HashState::Unavailable,
        }
    }

    /// Ảnh tiến độ khi không còn kết nối nào (vừa dừng, đang ghép).
    pub fn idle(&self, parent: &DownloadTask) -> ProgressSnapshot {
        let mut snapshot = self.snapshot(parent);
        snapshot.bytes_per_second = 0;
        snapshot.connections = 0;
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child(name: &str) -> DownloadTask {
        DownloadTask::new("https://media.example/x", name, "C:\\UTD", 16).unwrap()
    }

    fn progress(
        task: &DownloadTask,
        downloaded: u64,
        total: u64,
        speed: u64,
        live: u16,
    ) -> EngineEvent {
        EngineEvent::Progress {
            progress: ProgressSnapshot {
                task_id: task.id.clone(),
                status: TaskStatus::Downloading,
                downloaded_bytes: downloaded,
                total_bytes: Some(total),
                bytes_per_second: speed,
                connections: live,
                segments_done: 1,
                segments_total: 4,
                hash_state: HashState::Pending,
            },
        }
    }

    #[test]
    fn sums_bytes_speed_and_live_connections_of_both_streams() {
        let mut parent = child("Clip.mp4");
        parent.status = TaskStatus::Downloading;
        let (video, audio) = (child("Clip.f399.mp4"), child("Clip.f251.webm"));
        let mut rollup = Rollup::new(&[video.clone(), audio.clone()], &[Some(1000), Some(200)]);
        assert_eq!(
            rollup.total(),
            Some(1200),
            "trước khi dò dùng kích thước yt-dlp"
        );

        rollup.apply(&progress(&video, 400, 1100, 50, 8));
        rollup.apply(&progress(&audio, 100, 200, 10, 3));
        let snapshot = rollup.snapshot(&parent);
        assert_eq!(snapshot.task_id, parent.id);
        assert_eq!(snapshot.downloaded_bytes, 500);
        assert_eq!(
            snapshot.total_bytes,
            Some(1300),
            "kích thước engine dò thay số đoán"
        );
        assert_eq!(snapshot.bytes_per_second, 60);
        assert_eq!(snapshot.connections, 11);
        assert_eq!((snapshot.segments_done, snapshot.segments_total), (2, 8));
        assert_eq!(snapshot.hash_state, HashState::Unavailable);
    }

    #[test]
    fn a_finished_stream_stops_counting_speed() {
        let parent = child("Clip.mp4");
        let (video, audio) = (child("Clip.f399.mp4"), child("Clip.f251.webm"));
        let mut rollup = Rollup::new(&[video.clone(), audio.clone()], &[None, None]);
        assert_eq!(rollup.total(), None, "chưa biết kích thước luồng nào");
        rollup.apply(&progress(&audio, 150, 200, 30, 4));
        let mut done = audio.clone();
        done.status = TaskStatus::Completed;
        done.total_bytes = Some(200);
        done.downloaded_bytes = 200;
        rollup.apply(&EngineEvent::Task {
            task: Box::new(done),
        });
        rollup.apply(&progress(&video, 10, 1000, 70, 6));
        let snapshot = rollup.snapshot(&parent);
        assert_eq!(snapshot.downloaded_bytes, 210);
        assert_eq!(snapshot.total_bytes, Some(1200));
        assert_eq!(snapshot.bytes_per_second, 70);
        assert_eq!(snapshot.connections, 6);
    }

    #[test]
    fn resumed_parts_start_from_the_journal() {
        let mut video = child("Clip.f399.mp4");
        video.total_bytes = Some(1000);
        video.downloaded_bytes = 600;
        let mut audio = child("Clip.f251.webm");
        audio.status = TaskStatus::Completed;
        audio.total_bytes = Some(200);
        let rollup = Rollup::new(&[video, audio], &[]);
        assert_eq!(rollup.downloaded(), 800);
        assert_eq!(rollup.total(), Some(1200));
    }
}
