//! Đường tải nhanh kiểu IDM: yt-dlp chỉ cho URL của từng luồng; engine UTD tải mỗi luồng bằng
//! nhiều kết nối (mỗi yêu cầu HTTP ≤ `http_chunk_size`, YouTube bóp yêu cầu lớn hơn), video và
//! âm thanh cùng lúc, rồi ffmpeg ghép. Không dùng được (HLS, DRM, thiếu ffmpeg…) hoặc hỏng trước
//! khi có byte nào thì để yt-dlp tải như cũ.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tokio::task::{JoinError, JoinSet};
use tokio::time::{interval, sleep, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use utd_engine::{
    now_ms, DownloadControl, DownloadEngine, DownloadOutcome, DownloadTask, EngineEvent,
    ErrorClass, Journal, MediaJob, TaskError, TaskStatus,
};

use crate::media::finish::{self, error, Sink};
use crate::media::mux::Hint;
use crate::media::resolve::{self, Layout, Resolved, Stream};
use crate::media::rollup::Rollup;
use crate::media::tools::MediaTools;
use crate::media::{output, parts};

/// Lấy link mới tối đa chừng này lần mỗi lượt khi máy chủ báo link hết hạn (403/410).
const MAX_REFRESHES: u32 = 2;
const TICK: Duration = Duration::from_millis(500);
const CHECKPOINT: Duration = Duration::from_secs(1);

pub(super) type ResolveFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Resolved, TaskError>> + Send + 'a>>;

/// Hỏi URL các luồng; bài thử thay yt-dlp bằng kết quả cố định.
pub(super) trait Resolver: Send + Sync {
    fn resolve<'a>(&'a self, url: &'a str, selector: &'a str) -> ResolveFuture<'a>;
}

pub(super) struct YtDlp<'a>(pub &'a MediaTools);

impl Resolver for YtDlp<'_> {
    fn resolve<'a>(&'a self, url: &'a str, selector: &'a str) -> ResolveFuture<'a> {
        Box::pin(resolve::resolve(self.0, url, selector))
    }
}

pub(super) struct Fast<'a> {
    pub engine: Arc<DownloadEngine>,
    pub journal: Arc<Journal>,
    pub sink: &'a dyn Sink,
    pub resolver: &'a dyn Resolver,
    pub ffmpeg: Option<PathBuf>,
    /// Token của task media: huỷ ⇒ mọi task con dừng, giữ đoạn đã tải.
    pub cancel: CancellationToken,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum FastEnd {
    /// Task đã tới trạng thái của lượt này (xong, dừng, lỗi, cách ly).
    Handled,
    /// Chưa ghi byte nào: chạy yt-dlp như cũ.
    Fallback,
}

enum Step<T> {
    Go(T),
    Fallback,
    /// Đã dừng hoặc báo lỗi task cha.
    Stop,
}

struct Plan {
    layout: Layout,
    children: Vec<DownloadTask>,
    hints: Vec<Hint>,
    estimates: Vec<Option<u64>>,
}

enum Batch {
    Done,
    Paused,
    Failed(TaskError),
    Quarantined(TaskError),
}

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

pub(super) async fn run(
    fast: &Fast<'_>,
    task: &mut DownloadTask,
    job: &MediaJob,
) -> Result<FastEnd, String> {
    let existing = fast.journal.list_children(&task.id).map_err(text)?;
    let step = if existing.is_empty() {
        // yt-dlp đã tải dở ở lượt trước: để nó tải tiếp file .part của nó.
        if task.downloaded_bytes > 0 {
            return Ok(FastEnd::Fallback);
        }
        start(fast, task, job).await?
    } else {
        resume_plan(fast, task, job, existing).await?
    };
    let mut plan = match step {
        Step::Go(plan) => plan,
        Step::Fallback => return Ok(FastEnd::Fallback),
        Step::Stop => return Ok(FastEnd::Handled),
    };
    let mut refreshes = 0;
    loop {
        match download(fast, task, &plan).await? {
            Batch::Done => break,
            Batch::Paused => {
                let downloaded = task.downloaded_bytes;
                finish::pause(fast.sink, &fast.journal, task, downloaded)?;
                return Ok(FastEnd::Handled);
            }
            Batch::Quarantined(guard) => {
                task.status = TaskStatus::Quarantined;
                task.error = Some(error(
                    ErrorClass::Security,
                    &guard.code,
                    "Luồng media có nội dung nguy hiểm nên đã bị cách ly.",
                ));
                task.updated_at = now_ms();
                fast.journal.save_task(task).map_err(text)?;
                finish::emit_task(fast.sink, task);
                return Ok(FastEnd::Handled);
            }
            Batch::Failed(failure) => {
                if !parts::has_bytes(&fast.journal, &task.id) {
                    parts::discard_parts(&fast.journal, &task.id);
                    // yt-dlp tự báo kích thước của nó.
                    task.total_bytes = None;
                    return Ok(FastEnd::Fallback);
                }
                if link_refused(&failure) && refreshes < MAX_REFRESHES {
                    refreshes += 1;
                    let children = fast.journal.list_children(&task.id).map_err(text)?;
                    match refresh(fast, task, job, children).await? {
                        Step::Go(next) => plan = next,
                        Step::Fallback => return Ok(FastEnd::Fallback),
                        Step::Stop => return Ok(FastEnd::Handled),
                    }
                    continue;
                }
                finish::fail(fast.sink, &fast.journal, task, part_error(&failure))?;
                return Ok(FastEnd::Handled);
            }
        }
    }
    output::mux_and_finish(fast, task, plan.layout, &plan.hints).await?;
    Ok(FastEnd::Handled)
}

/// Lượt đầu: hỏi yt-dlp, kiểm tải thẳng được không, tạo task con.
async fn start(
    fast: &Fast<'_>,
    task: &mut DownloadTask,
    job: &MediaJob,
) -> Result<Step<Plan>, String> {
    let resolved = match ask(fast, task, job).await? {
        Step::Go(Ok(resolved)) => resolved,
        Step::Go(Err(_)) | Step::Fallback => return Ok(Step::Fallback),
        Step::Stop => return Ok(Step::Stop),
    };
    let ffmpeg = fast.ffmpeg.is_some();
    let audio = job.audio_format.as_deref();
    let (layout, streams) = match resolve::check(&resolved, audio, ffmpeg, resolve::now_secs()) {
        Ok(checked) => checked,
        Err(reason) => {
            log::info!("media: dùng yt-dlp ({reason})");
            return Ok(Step::Fallback);
        }
    };
    if !wait_available(&streams, &fast.cancel).await {
        pause_now(fast, task)?;
        return Ok(Step::Stop);
    }
    let connections = fast.engine.options().max_connections;
    match parts::create(&fast.journal, task, &streams, connections) {
        Ok(children) => Ok(Step::Go(plan_for(layout, children, &streams))),
        Err(message) => {
            log::warn!("media: không tạo được luồng con ({message})");
            Ok(Step::Fallback)
        }
    }
}

fn plan_for(layout: Layout, children: Vec<DownloadTask>, streams: &[Stream]) -> Plan {
    Plan {
        layout,
        children,
        hints: parts::hints_from_streams(streams),
        estimates: streams.iter().map(|stream| stream.size).collect(),
    }
}

/// Chạy yt-dlp `-J` (huỷ được). `Go(Err)` = yt-dlp báo lỗi.
async fn ask(
    fast: &Fast<'_>,
    task: &mut DownloadTask,
    job: &MediaJob,
) -> Result<Step<Result<Resolved, TaskError>>, String> {
    finish::set_status(fast.sink, &fast.journal, task, TaskStatus::Resolving)?;
    tokio::select! {
        resolved = fast.resolver.resolve(&job.url, &job.selector) => Ok(Step::Go(resolved)),
        _ = fast.cancel.cancelled() => {
            pause_now(fast, task)?;
            Ok(Step::Stop)
        }
    }
}

fn pause_now(fast: &Fast<'_>, task: &mut DownloadTask) -> Result<(), String> {
    let downloaded = task.downloaded_bytes;
    finish::pause(fast.sink, &fast.journal, task, downloaded)
}

/// Lượt tiếp tục: dùng lại task con; link sắp hết hạn thì hỏi yt-dlp link mới trước.
async fn resume_plan(
    fast: &Fast<'_>,
    task: &mut DownloadTask,
    job: &MediaJob,
    mut children: Vec<DownloadTask>,
) -> Result<Step<Plan>, String> {
    let Some(layout) = Layout::infer(job.audio_format.as_deref(), children.len()) else {
        // Không khớp cấu hình media (dữ liệu cũ): làm lại từ đầu.
        parts::discard_all(&fast.journal, &children);
        return start(fast, task, job).await;
    };
    parts::repair_missing(&fast.journal, &mut children)?;
    let now = resolve::now_secs();
    let stale = children
        .iter()
        .any(|child| child.status != TaskStatus::Completed && resolve::expired(&child.url, now));
    if stale {
        return refresh(fast, task, job, children).await;
    }
    Ok(Step::Go(Plan {
        layout,
        hints: parts::hints_from_children(&children),
        estimates: Vec::new(),
        children,
    }))
}

/// Hỏi lại yt-dlp rồi thay link các task con (cùng luồng, cùng kích thước ⇒ giữ đoạn đã tải).
async fn refresh(
    fast: &Fast<'_>,
    task: &mut DownloadTask,
    job: &MediaJob,
    children: Vec<DownloadTask>,
) -> Result<Step<Plan>, String> {
    let resolved = match ask(fast, task, job).await? {
        Step::Go(resolved) => resolved,
        Step::Fallback => return Ok(Step::Fallback),
        Step::Stop => return Ok(Step::Stop),
    };
    let checked = resolved.map_err(Some).and_then(|resolved| {
        let audio = job.audio_format.as_deref();
        resolve::check(&resolved, audio, fast.ffmpeg.is_some(), resolve::now_secs())
            .map_err(|_| None)
    });
    let (layout, streams) = match checked {
        Ok(checked) => checked,
        Err(failure) => {
            if !parts::has_bytes(&fast.journal, &task.id) {
                parts::discard_all(&fast.journal, &children);
                return Ok(Step::Fallback);
            }
            let failure = failure.unwrap_or_else(|| {
                error(
                    ErrorClass::Permanent,
                    "media.expired",
                    "Link media đã hết hạn và không lấy lại được. Xoá rồi tải lại.",
                )
            });
            finish::fail(fast.sink, &fast.journal, task, failure)?;
            return Ok(Step::Stop);
        }
    };
    if !wait_available(&streams, &fast.cancel).await {
        pause_now(fast, task)?;
        return Ok(Step::Stop);
    }
    let connections = fast.engine.options().max_connections;
    let children = parts::reconcile(&fast.journal, task, children, &streams, connections)?;
    Ok(Step::Go(plan_for(layout, children, &streams)))
}

/// Luồng chưa mở (`available_at`, tối đa 60 s): chờ. `false` = người dùng tạm dừng.
async fn wait_available(streams: &[Stream], cancel: &CancellationToken) -> bool {
    let now = resolve::now_secs();
    let wait = streams
        .iter()
        .filter_map(|stream| stream.available_at)
        .max()
        .map(|at| (at - now).clamp(0, resolve::MAX_WAIT_SECS))
        .unwrap_or(0);
    if wait == 0 {
        return !cancel.is_cancelled();
    }
    tokio::select! {
        _ = sleep(Duration::from_secs(wait as u64)) => true,
        _ = cancel.cancelled() => false,
    }
}

/// Tải song song mọi luồng chưa xong; gộp tiến độ vào task cha.
async fn download(fast: &Fast<'_>, task: &mut DownloadTask, plan: &Plan) -> Result<Batch, String> {
    let mut rollup = Rollup::new(&plan.children, &plan.estimates);
    let pending: Vec<String> = plan
        .children
        .iter()
        .filter(|child| child.status != TaskStatus::Completed)
        .map(|child| child.id.clone())
        .collect();
    if pending.is_empty() {
        return Ok(Batch::Done);
    }
    task.total_bytes = rollup.total();
    task.downloaded_bytes = finish::clamp(rollup.downloaded(), task.total_bytes);
    finish::set_status(fast.sink, &fast.journal, task, TaskStatus::Downloading)?;

    // Một luồng hỏng thì dừng luồng kia (giữ đoạn đã tải); tạm dừng task cha dừng cả hai.
    let siblings = fast.cancel.child_token();
    let (events, mut inbox) = mpsc::unbounded_channel();
    let mut workers = spawn_parts(fast, pending, &siblings, &events);
    drop(events);
    let mut outcomes = Outcomes::default();
    let mut ticker = interval(TICK);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut checkpoint = Instant::now();
    loop {
        tokio::select! {
            joined = workers.join_next() => {
                let Some(joined) = joined else { break };
                if outcomes.record(joined, &mut rollup) {
                    siblings.cancel();
                }
            }
            Some(event) = inbox.recv() => rollup.apply(&event),
            _ = ticker.tick() => report(fast, task, &rollup, &mut checkpoint)?,
        }
    }
    settle(fast, task, &rollup, outcomes)
}

/// Mỗi luồng một task engine; số kết nối do bộ điều khiển tự chọn (trần = mức tối đa).
fn spawn_parts(
    fast: &Fast<'_>,
    pending: Vec<String>,
    siblings: &CancellationToken,
    events: &mpsc::UnboundedSender<EngineEvent>,
) -> JoinSet<utd_engine::Result<DownloadOutcome>> {
    let mut workers = JoinSet::new();
    for id in pending {
        let engine = fast.engine.clone();
        let control = DownloadControl {
            cancel: siblings.child_token(),
            events: Some(events.clone()),
        };
        workers.spawn(async move { engine.resume(&id, control).await });
    }
    workers
}

/// Kết quả các luồng con của một lượt.
#[derive(Default)]
struct Outcomes {
    failure: Option<TaskError>,
    quarantine: Option<TaskError>,
}

impl Outcomes {
    fn troubled(&self) -> bool {
        self.failure.is_some() || self.quarantine.is_some()
    }

    /// Ghi kết quả một luồng; `true` khi đây là sự cố đầu tiên (phải dừng luồng kia).
    fn record(
        &mut self,
        joined: Result<utd_engine::Result<DownloadOutcome>, JoinError>,
        rollup: &mut Rollup,
    ) -> bool {
        let before = self.troubled();
        match joined {
            Ok(Ok(outcome)) => {
                rollup.update(outcome.task());
                match outcome {
                    DownloadOutcome::Failed(child) => {
                        self.failure
                            .get_or_insert(child.error.unwrap_or_else(worker_error));
                    }
                    DownloadOutcome::Quarantined(child) => {
                        self.quarantine
                            .get_or_insert(child.error.unwrap_or_else(worker_error));
                    }
                    DownloadOutcome::Completed(_) | DownloadOutcome::Paused(_) => {}
                }
            }
            Ok(Err(problem)) => {
                log::warn!("media: luồng con lỗi journal ({problem})");
                self.failure.get_or_insert_with(worker_error);
            }
            Err(_) => {
                self.failure.get_or_insert_with(worker_error);
            }
        }
        !before && self.troubled()
    }
}

/// Sau khi mọi luồng dừng: lưu tiến độ gộp rồi xếp loại lượt này.
fn settle(
    fast: &Fast<'_>,
    task: &mut DownloadTask,
    rollup: &Rollup,
    outcomes: Outcomes,
) -> Result<Batch, String> {
    task.total_bytes = rollup.total();
    task.downloaded_bytes = finish::clamp(rollup.downloaded(), task.total_bytes);
    task.updated_at = now_ms();
    fast.journal.save_task(task).map_err(text)?;
    fast.sink.progress(rollup.idle(task));
    if let Some(guard) = outcomes.quarantine {
        return Ok(Batch::Quarantined(guard));
    }
    if fast.cancel.is_cancelled() {
        return Ok(Batch::Paused);
    }
    if let Some(failure) = outcomes.failure {
        return Ok(Batch::Failed(failure));
    }
    let children = fast.journal.list_children(&task.id).map_err(text)?;
    if children
        .iter()
        .all(|child| child.status == TaskStatus::Completed)
    {
        Ok(Batch::Done)
    } else {
        Ok(Batch::Failed(worker_error()))
    }
}

/// Tiến độ gộp lên giao diện mỗi nhịp; ghi journal mỗi giây (tiếp tục sau khi tắt app).
fn report(
    fast: &Fast<'_>,
    task: &mut DownloadTask,
    rollup: &Rollup,
    checkpoint: &mut Instant,
) -> Result<(), String> {
    let snapshot = rollup.snapshot(task);
    task.total_bytes = snapshot.total_bytes;
    task.downloaded_bytes = snapshot.downloaded_bytes;
    if checkpoint.elapsed() >= CHECKPOINT {
        task.updated_at = now_ms();
        fast.journal.save_task(task).map_err(text)?;
        *checkpoint = Instant::now();
    }
    fast.sink.progress(snapshot);
    Ok(())
}

fn worker_error() -> TaskError {
    error(
        ErrorClass::Transient,
        "media.failed",
        "Không tải được luồng media. Bấm Tiếp tục để thử lại.",
    )
}

/// Máy chủ từ chối link (thường do hết hạn): hỏi yt-dlp link mới.
fn link_refused(failure: &TaskError) -> bool {
    matches!(failure.code.as_str(), "http.401" | "http.403" | "http.410")
}

/// Lỗi của task con (tiếng Anh, mã engine) → câu tiếng Việt ngắn, mã Doctor hiểu.
fn part_error(failure: &TaskError) -> TaskError {
    let code = failure.code.as_str();
    let (class, code, message) = match code {
        "http.401" | "http.403" | "http.407" => (
            ErrorClass::User,
            "media.forbidden",
            "Nguồn media từ chối truy cập.",
        ),
        "http.404" | "http.410" => (
            ErrorClass::Permanent,
            "media.not_found",
            "Không tìm thấy media trên máy chủ.",
        ),
        "http.429" => (
            ErrorClass::Transient,
            "media.rate_limited",
            "Nguồn media đang giới hạn tốc độ. Thử lại sau.",
        ),
        "file.exists" => (
            ErrorClass::User,
            "file.exists",
            "Đã có file trùng tên luồng media trong thư mục tải.",
        ),
        _ if code.starts_with("disk.") => (
            ErrorClass::Permanent,
            code,
            "Không ghi được file media. Kiểm tra dung lượng ổ đĩa.",
        ),
        _ if code.starts_with("guard.") => (
            failure.class,
            code,
            "Không kiểm tra an toàn được luồng media.",
        ),
        _ if code.starts_with("network.") || code.starts_with("http.5") || code == "http.408" => (
            ErrorClass::Transient,
            "media.network",
            "Kết nối tới nguồn media bị gián đoạn. Bấm Tiếp tục để tải tiếp.",
        ),
        _ if code.starts_with("http.") => (
            ErrorClass::Transient,
            "media.changed",
            "Luồng media trên máy chủ đã đổi. Bấm Tiếp tục để thử lại.",
        ),
        _ if code.starts_with("media.") => (failure.class, code, failure.message.as_str()),
        _ => (
            ErrorClass::Transient,
            "media.failed",
            "Không tải được luồng media. Bấm Tiếp tục để thử lại.",
        ),
    };
    error(class, code, message)
}

#[cfg(test)]
#[path = "fast_tests.rs"]
mod tests;
