use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::Client;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{interval, sleep, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use url::Url;

use super::adaptive::{ConnectionGate, Controller, Sample, Tuning};
use super::content::{
    empty_response, file_is_page, looks_like_html, not_a_file, page_check, sniff_remote,
    PageCheck,
};
use super::file::{move_no_clobber, target_path, PartFile};
use super::http::{host_key, probe, sanitize_suggested_name, Probe};
use super::limiter::Limiter;
use super::transport::{plain_policy, scoped_policy, Transport};
use super::verify::{sha256_file, HashOutcome};
use super::worker::{
    run_segment, RetryPolicy, SegmentFailure, SegmentJob, SegmentRuntime, StreamJob,
};
use super::{
    DownloadControl, DownloadOutcome, DownloadRequest, EngineEvent, EngineOptions, ProgressSnapshot,
};
use crate::db::Journal;
use crate::safety::{self, GuardAction};
use crate::model::{
    now_ms, plan_segments, validate_file_name, validate_segments, validate_url, DownloadTask,
    ErrorClass, HashState, Segment, SegmentStatus, TaskError, TaskStatus, MAX_SEGMENTS,
    MIN_SEGMENT_BYTES,
};
use crate::{Error, Result};

const MIN_SPLIT_REMAINING: u64 = 8 * MIN_SEGMENT_BYTES;
const MAX_SPLITS: usize = 8;
/// Kết nối rảnh mà hết đoạn chờ: chia đôi đoạn còn nhiều nhất, mỗi nửa ít nhất chừng này.
const MIN_IDLE_PIECE: u64 = MIN_SEGMENT_BYTES;

pub struct DownloadEngine {
    journal: Arc<Journal>,
    client: Client,
    /// Cho task có header riêng: dừng ở chuyển hướng sang origin khác (xem `Transport`).
    scoped_client: Client,
    limiter: Arc<Limiter>,
    options: EngineOptions,
}

enum SessionEnd {
    Completed,
    Paused,
    Failed(TaskError),
    /// Không phải file (AC-06): bỏ file tạm rồi báo lỗi.
    Rejected(TaskError),
}

/// Kết quả soát "link là trang web" sau khi dò.
enum Screen {
    /// Là file; `true` ⇒ xem đầu file lúc tải xong.
    Pass(bool),
    Reject(TaskError),
    Cancelled,
}

#[derive(Clone)]
struct SegmentContext {
    transport: Transport,
    total: u64,
    url: Url,
    host: String,
    file: Arc<PartFile>,
    limiter: Arc<Limiter>,
    gate: Arc<ConnectionGate>,
    cancel: CancellationToken,
    retry: RetryPolicy,
}

impl SegmentContext {
    fn spawn(
        &self,
        join_set: &mut JoinSet<()>,
        results: &mpsc::UnboundedSender<Option<SegmentFailure>>,
        segment: Arc<SegmentRuntime>,
    ) {
        let job = SegmentJob {
            transport: self.transport.clone(),
            total: self.total,
            url: self.url.clone(),
            host: self.host.clone(),
            segment,
            file: self.file.clone(),
            limiter: self.limiter.clone(),
            gate: self.gate.clone(),
            cancel: self.cancel.clone(),
            retry: self.retry,
        };
        let results = results.clone();
        join_set.spawn(async move {
            let outcome = run_segment(job).await;
            let _ = results.send(outcome.err());
        });
    }
}

impl DownloadEngine {
    pub fn new(journal: Arc<Journal>, options: EngineOptions) -> Result<Self> {
        options.validate()?;
        let client = build_client(&options, plain_policy())?;
        let scoped_client = build_client(&options, scoped_policy())?;
        let limiter = Arc::new(Limiter::new(
            usize::from(options.global_connections),
            usize::from(options.host_connections),
        ));
        Ok(Self {
            journal,
            client,
            scoped_client,
            limiter,
            options,
        })
    }

    fn transport(&self, task: &DownloadTask, url: &Url) -> Result<Transport> {
        Transport::new(
            &self.client,
            &self.scoped_client,
            url,
            &task.request_headers,
            task.max_request_bytes,
        )
    }

    pub fn options(&self) -> &EngineOptions {
        &self.options
    }

    pub async fn download(
        &self,
        request: DownloadRequest,
        control: DownloadControl,
    ) -> Result<DownloadOutcome> {
        let allow_rename = request.file_name.is_none();
        let task = self.enqueue(request)?;
        self.run(task, control, allow_rename).await
    }

    pub fn enqueue(&self, request: DownloadRequest) -> Result<DownloadTask> {
        let url = validate_url(&request.url)?;
        let file_name = match request.file_name {
            Some(name) => {
                validate_file_name(&name)?;
                name
            }
            None => sanitize_suggested_name(url.path()),
        };
        let mut task = DownloadTask::new(
            request.url,
            file_name,
            request.dest_dir,
            request.connections,
        )?;
        if let Some(hash) = request.expected_sha256 {
            task.expected_sha256 = Some(hash.to_ascii_lowercase());
            task.hash_state = HashState::Pending;
        }
        if let Some(parent_id) = request.parent_id {
            task = task.with_parent(parent_id);
        }
        task.max_request_bytes = request.max_request_bytes;
        task.request_headers = request.request_headers;
        task.validate()?;
        self.journal.save_task(&task)?;
        Ok(task)
    }

    pub async fn run_new(
        &self,
        task_id: &str,
        control: DownloadControl,
        allow_rename: bool,
    ) -> Result<DownloadOutcome> {
        let task = self
            .journal
            .load_task(task_id)?
            .ok_or_else(|| Error::NotFound(format!("task {task_id}")))?;
        if task.status == TaskStatus::Completed {
            return Ok(DownloadOutcome::Completed(task));
        }
        self.run(task, control, allow_rename).await
    }

    pub async fn resume(
        &self,
        task_id: &str,
        control: DownloadControl,
    ) -> Result<DownloadOutcome> {
        let mut task = self
            .journal
            .load_task(task_id)?
            .ok_or_else(|| Error::NotFound(format!("task {task_id}")))?;
        if task.status == TaskStatus::Completed {
            return Ok(DownloadOutcome::Completed(task));
        }
        if task.hash_state == HashState::Mismatch {
            self.journal.reset_task_progress(&task.id)?;
            task.downloaded_bytes = 0;
            task.hash_state = HashState::Pending;
        }
        self.run(task, control, false).await
    }

    async fn run(
        &self,
        mut task: DownloadTask,
        control: DownloadControl,
        allow_rename: bool,
    ) -> Result<DownloadOutcome> {
        if control.cancel.is_cancelled() {
            return self.pause(task, &control);
        }
        task.status = TaskStatus::Resolving;
        task.error = None;
        task.updated_at = now_ms();
        task.validate()?;
        self.journal.save_task(&task)?;
        emit_task(&control, &task);

        let url = validate_url(&task.url)?;
        let transport = self.transport(&task, &url)?;
        let probe = match self.probe_with_retry(&transport, &url, &control).await {
            Ok(probe) => probe,
            Err(error) => {
                if control.cancel.is_cancelled() {
                    return self.pause(task, &control);
                }
                return self.fail(task, error, &control);
            }
        };
        if control.cancel.is_cancelled() {
            return self.pause(task, &control);
        }

        let changed = metadata_changed(&task, &probe);
        let size_redefined = probe
            .total_bytes
            .is_some_and(|total| task.total_bytes != Some(total));
        if (changed || size_redefined) && task.downloaded_bytes > 0 {
            self.journal.reset_task_progress(&task.id)?;
            task.downloaded_bytes = 0;
            task.hash_state = HashState::Pending;
        }
        task.final_url = Some(probe.url.to_string());
        task.etag = probe.etag.clone();
        task.last_modified = probe.last_modified.clone();
        if probe.total_bytes.is_some() {
            task.total_bytes = probe.total_bytes;
        }
        if allow_rename
            && task.downloaded_bytes == 0
            && let Some(name) = probe.file_name.as_deref()
        {
            let safe = sanitize_suggested_name(name);
            if validate_file_name(&safe).is_ok() {
                task.file_name = safe;
            }
        }
        task.validate()?;
        self.journal.save_task(&task)?;

        let sniff = match self.screen_page(&transport, &task, &probe, &control).await {
            Screen::Pass(sniff) => sniff,
            Screen::Reject(error) => return self.fail(task, error, &control),
            Screen::Cancelled => return self.pause(task, &control),
        };

        let target = target_path(&task.dest_dir, &task.file_name);
        if target.exists() {
            return self.fail(
                task,
                TaskError {
                    class: ErrorClass::User,
                    code: "file.exists".into(),
                    message: "A file with this name already exists.".into(),
                },
                &control,
            );
        }

        if task.total_bytes == Some(0) {
            self.journal.reset_task_progress(&task.id)?;
            task.downloaded_bytes = 0;
            let file = self.open_part(&task, Some(0)).await?;
            file.reset(Some(0))?;
            return self.finish(task, &control, sniff).await;
        }

        // `task.connections` chỉ là trần; số kết nối thật do bộ điều khiển tự chọn.
        let max_connections = task
            .connections
            .min(self.options.max_connections)
            .min(self.options.host_connections)
            .min(self.options.global_connections)
            .max(1);
        let ranged = probe.supports_ranges && task.total_bytes.is_some();
        let mut segments = if ranged {
            self.journal.load_segments(&task.id)?
        } else {
            Vec::new()
        };
        let saved: u64 = segments.iter().map(|segment| segment.downloaded).sum();
        let temp_len = std::fs::metadata(&task.temp_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let usable = ranged
            && !segments.is_empty()
            && validate_segments(task.total_bytes, &segments).is_ok()
            && saved == task.downloaded_bytes
            && (saved == 0 || temp_len >= saved);

        if ranged {
            let total = task.total_bytes.unwrap_or(0);
            let mut reset_file = !usable;
            if !usable {
                if task.downloaded_bytes > 0 {
                    self.journal.reset_task_progress(&task.id)?;
                }
                task.downloaded_bytes = 0;
                segments.clear();
                reset_file = true;
            }
            let file = self.open_part(&task, Some(total)).await?;
            if reset_file {
                file.reset(Some(total))?;
            }
            if segments.is_empty() {
                let plan = plan_segments(total, max_connections);
                self.journal.save_segments(&task.id, &plan)?;
                segments = plan;
            }
            task.status = TaskStatus::Downloading;
            task.updated_at = now_ms();
            self.journal.save_task(&task)?;
            emit_task(&control, &task);
            if task.downloaded_bytes >= total {
                return self.finish(task, &control, sniff).await;
            }
            let end = self
                .run_segmented(
                    &mut task,
                    &control,
                    segments,
                    file,
                    transport,
                    probe.url,
                    total,
                    max_connections,
                )
                .await?;
            return self.conclude(task, control, end, sniff).await;
        }

        self.journal.reset_task_progress(&task.id)?;
        task.downloaded_bytes = 0;
        task.status = TaskStatus::Downloading;
        task.updated_at = now_ms();
        self.journal.save_task(&task)?;
        emit_task(&control, &task);
        let file = self.open_part(&task, task.total_bytes).await?;
        file.reset(task.total_bytes)?;
        let end = self
            .run_single(&mut task, &control, file, transport, probe)
            .await?;
        self.conclude(task, control, end, sniff).await
    }

    async fn conclude(
        &self,
        mut task: DownloadTask,
        control: DownloadControl,
        end: SessionEnd,
        sniff: bool,
    ) -> Result<DownloadOutcome> {
        match end {
            SessionEnd::Completed => self.finish(task, &control, sniff).await,
            SessionEnd::Paused => self.pause(task, &control),
            SessionEnd::Failed(error) => self.fail(task, error, &control),
            SessionEnd::Rejected(error) => {
                self.discard_part(&mut task).await?;
                self.fail(task, error, &control)
            }
        }
    }

    /// AC-06: server nói HTML (mà tên không phải trang web) ⇒ đọc thử đầu nội dung; là trang
    /// thật hoặc rỗng thì từ chối trước khi tải. Server không nói kiểu ⇒ xem đầu file khi xong.
    async fn screen_page(
        &self,
        transport: &Transport,
        task: &DownloadTask,
        probe: &Probe,
        control: &DownloadControl,
    ) -> Screen {
        match page_check(&task.file_name, probe.content_type.as_deref(), probe.attachment) {
            PageCheck::Skip => Screen::Pass(false),
            PageCheck::Finish => Screen::Pass(true),
            PageCheck::Probe if task.total_bytes == Some(0) => Screen::Reject(not_a_file()),
            PageCheck::Probe => {
                let sniffed = tokio::select! {
                    sniffed = sniff_remote(transport, &probe.url, self.options.connect_timeout) => sniffed,
                    _ = control.cancel.cancelled() => return Screen::Cancelled,
                };
                match sniffed {
                    Some(prefix) if prefix.is_empty() || looks_like_html(&prefix) => {
                        Screen::Reject(not_a_file())
                    }
                    Some(_) => Screen::Pass(false),
                    None => Screen::Pass(true),
                }
            }
        }
    }

    /// Bỏ file tạm và tiến độ của một lượt tải không ra file.
    async fn discard_part(&self, task: &mut DownloadTask) -> Result<()> {
        let _ = tokio::fs::remove_file(&task.temp_path).await;
        self.journal.reset_task_progress(&task.id)?;
        task.downloaded_bytes = 0;
        Ok(())
    }

    async fn probe_with_retry(
        &self,
        transport: &Transport,
        url: &Url,
        control: &DownloadControl,
    ) -> std::result::Result<Probe, TaskError> {
        let limit = self.options.max_attempts.clamp(1, 4);
        let mut attempts = 0;
        loop {
            let probed = tokio::select! {
                result = probe(transport, url, self.options.connect_timeout) => result,
                _ = control.cancel.cancelled() => return Err(cancelled_error()),
            };
            match probed {
                Ok(probe) => return Ok(probe),
                Err(failure) => {
                    attempts += 1;
                    if !failure.retryable() || attempts >= limit {
                        return Err(failure.error);
                    }
                    let delay = failure
                        .retry_after
                        .unwrap_or_else(|| Duration::from_millis(500 * u64::from(attempts)));
                    tokio::select! {
                        _ = control.cancel.cancelled() => return Err(cancelled_error()),
                        _ = sleep(delay) => {}
                    }
                }
            }
        }
    }

    async fn open_part(&self, task: &DownloadTask, total: Option<u64>) -> Result<Arc<PartFile>> {
        let path = PathBuf::from(&task.temp_path);
        let file = tokio::task::spawn_blocking(move || PartFile::open(&path, total))
            .await
            .map_err(|error| Error::Io(std::io::Error::other(error)))??;
        Ok(Arc::new(file))
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_segmented(
        &self,
        task: &mut DownloadTask,
        control: &DownloadControl,
        segments: Vec<Segment>,
        file: Arc<PartFile>,
        transport: Transport,
        url: Url,
        total: u64,
        max_connections: u16,
    ) -> Result<SessionEnd> {
        let mut runtimes: Vec<Arc<SegmentRuntime>> = segments
            .iter()
            .map(|segment| Arc::new(SegmentRuntime::new(segment)))
            .collect();
        if runtimes.iter().all(|runtime| runtime.is_done()) {
            task.downloaded_bytes = total;
            return Ok(SessionEnd::Completed);
        }
        let host = host_key(&url);
        let cancel = control.cancel.clone();
        let retry = RetryPolicy {
            max_attempts: self.options.max_attempts,
            idle_timeout: self.options.idle_timeout,
        };
        let initial = usize::from(self.options.initial_connections.min(max_connections).max(1));
        let gate = ConnectionGate::new(initial);
        let mut controller = Controller::new(
            initial,
            usize::from(max_connections),
            Tuning::new(self.options.sample_interval, self.options.balance_interval),
            Instant::now(),
            total_downloaded(&runtimes),
        );
        let context = SegmentContext {
            transport,
            total,
            url,
            host,
            file,
            limiter: self.limiter.clone(),
            gate: gate.clone(),
            cancel: cancel.clone(),
            retry,
        };
        let (results_tx, mut results_rx) = mpsc::unbounded_channel();
        let mut pending_workers = 0usize;
        let mut join_set: JoinSet<()> = JoinSet::new();
        for runtime in &runtimes {
            context.spawn(&mut join_set, &results_tx, runtime.clone());
            pending_workers += 1;
        }

        let mut tick = interval(self.options.sample_interval);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut last_bytes = total_downloaded(&runtimes);
        let mut last_sample = Instant::now();
        let mut last_checkpoint = Instant::now();
        let mut last_balance = Instant::now();
        let mut previous_balance: HashMap<u64, u64> = runtimes
            .iter()
            .map(|runtime| (runtime.start(), runtime.downloaded()))
            .collect();
        let mut splits_left = MAX_SPLITS.min(MAX_SEGMENTS.saturating_sub(runtimes.len()));

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    join_set.abort_all();
                    while join_set.join_next().await.is_some() {}
                    self.checkpoint_segments(task, &runtimes)?;
                    return Ok(SessionEnd::Paused);
                }
                message = results_rx.recv() => {
                    match message {
                        Some(None) => {
                            pending_workers = pending_workers.saturating_sub(1);
                            if pending_workers == 0 {
                                break;
                            }
                        }
                        Some(Some(failure)) => {
                            join_set.abort_all();
                            while join_set.join_next().await.is_some() {}
                            self.checkpoint_segments(task, &runtimes)?;
                            return Ok(SessionEnd::Failed(failure.error));
                        }
                        None => break,
                    }
                }
                _ = tick.tick() => {
                    let now = Instant::now();
                    let downloaded = total_downloaded(&runtimes);
                    let elapsed = now.duration_since(last_sample).as_secs_f64().max(0.001);
                    let speed = ((downloaded.saturating_sub(last_bytes)) as f64 / elapsed) as u64;
                    last_bytes = downloaded;
                    last_sample = now;

                    if now.duration_since(last_checkpoint) >= self.options.checkpoint_interval {
                        self.checkpoint_segments(task, &runtimes)?;
                        last_checkpoint = now;
                    }
                    let sample = Sample {
                        now,
                        downloaded,
                        live: gate.live(),
                        pressure: gate.take_pressure(),
                    };
                    if let Some(target) = controller.on_tick(sample) {
                        gate.set_target(target);
                    }
                    for tail in self.fill_idle(task, &mut runtimes, controller.target())? {
                        previous_balance.insert(tail.start(), 0);
                        context.spawn(&mut join_set, &results_tx, tail);
                        pending_workers += 1;
                    }
                    emit_progress(
                        control,
                        task,
                        downloaded,
                        speed,
                        u16::try_from(gate.live()).unwrap_or(u16::MAX),
                        runtimes.iter().filter(|runtime| runtime.is_done()).count() as u32,
                        runtimes.len() as u32,
                    );

                    if now.duration_since(last_balance) >= self.options.balance_interval {
                        let balance_elapsed =
                            now.duration_since(last_balance).as_secs_f64().max(0.001);
                        if splits_left > 0
                            && let Some(tail) = self.split_slowest(
                                task,
                                &mut runtimes,
                                &previous_balance,
                                balance_elapsed,
                            )?
                        {
                            context.spawn(&mut join_set, &results_tx, tail);
                            pending_workers += 1;
                            splits_left -= 1;
                        }
                        previous_balance = runtimes
                            .iter()
                            .map(|runtime| (runtime.start(), runtime.downloaded()))
                            .collect();
                        last_balance = now;
                    }
                }
            }
        }

        self.checkpoint_segments(task, &runtimes)?;
        if cancel.is_cancelled() {
            return Ok(SessionEnd::Paused);
        }
        let downloaded = task.downloaded_bytes;
        if downloaded != total {
            return Ok(SessionEnd::Failed(client_error(
                "network.closed",
                "The download ended before all bytes arrived.",
            )));
        }
        Ok(SessionEnd::Completed)
    }

    /// Số đoạn chưa xong ít hơn số kết nối cho phép: chia đôi đoạn còn nhiều nhất (kiểu IDM)
    /// để kết nối rảnh có việc, nhất là lúc gần xong.
    fn fill_idle(
        &self,
        task: &mut DownloadTask,
        runtimes: &mut Vec<Arc<SegmentRuntime>>,
        target: usize,
    ) -> Result<Vec<Arc<SegmentRuntime>>> {
        let mut added = Vec::new();
        while runtimes.len() < MAX_SEGMENTS {
            let unfinished = runtimes.iter().filter(|runtime| !runtime.is_done()).count();
            if unfinished >= target {
                break;
            }
            let Some(largest) = runtimes
                .iter()
                .filter(|runtime| !runtime.is_done())
                .max_by_key(|runtime| runtime.remaining())
                .cloned()
            else {
                break;
            };
            let Some(range) = largest.split_half(MIN_IDLE_PIECE) else {
                break;
            };
            added.push(insert_split(runtimes, &largest, range));
        }
        if !added.is_empty() {
            self.save_plan(task, runtimes)?;
        }
        Ok(added)
    }

    /// Đoạn chậm hơn một nửa đoạn nhanh nhất: tách phần đuôi cho kết nối khác làm.
    fn split_slowest(
        &self,
        task: &mut DownloadTask,
        runtimes: &mut Vec<Arc<SegmentRuntime>>,
        previous: &HashMap<u64, u64>,
        elapsed: f64,
    ) -> Result<Option<Arc<SegmentRuntime>>> {
        let unfinished = runtimes.iter().filter(|runtime| !runtime.is_done()).count();
        if unfinished < 2 || runtimes.len() >= MAX_SEGMENTS {
            return Ok(None);
        }
        let speeds: Vec<(Arc<SegmentRuntime>, u64)> = runtimes
            .iter()
            .map(|runtime| {
                let current = runtime.downloaded();
                let before = previous.get(&runtime.start()).copied().unwrap_or(current);
                let per_second = (current.saturating_sub(before) as f64 / elapsed) as u64;
                (runtime.clone(), per_second)
            })
            .collect();
        let fastest = speeds.iter().map(|(_, speed)| *speed).max().unwrap_or(0);
        if fastest == 0 {
            return Ok(None);
        }
        let slowest = speeds
            .iter()
            .filter(|(runtime, per_second)| {
                *per_second > 0 && per_second.saturating_mul(2) < fastest && !runtime.is_done()
            })
            .min_by_key(|(_, per_second)| *per_second)
            .map(|(runtime, _)| runtime.clone());
        let Some(runtime) = slowest else {
            return Ok(None);
        };
        if runtime.remaining() < MIN_SPLIT_REMAINING {
            return Ok(None);
        }
        let Some(range) = runtime.split_tail(MIN_SPLIT_REMAINING) else {
            return Ok(None);
        };
        let tail = insert_split(runtimes, &runtime, range);
        self.save_plan(task, runtimes)?;
        Ok(Some(tail))
    }

    fn save_plan(&self, task: &mut DownloadTask, runtimes: &[Arc<SegmentRuntime>]) -> Result<()> {
        let plan: Vec<Segment> = runtimes.iter().map(|runtime| runtime.snapshot()).collect();
        self.journal.save_segments(&task.id, &plan)?;
        task.downloaded_bytes = plan.iter().map(|segment| segment.downloaded).sum();
        task.updated_at = now_ms();
        Ok(())
    }

    async fn run_single(
        &self,
        task: &mut DownloadTask,
        control: &DownloadControl,
        file: Arc<PartFile>,
        transport: Transport,
        probe: Probe,
    ) -> Result<SessionEnd> {
        let cancel = control.cancel.clone();
        let downloaded = Arc::new(AtomicU64::new(0));
        let host = host_key(&probe.url);
        let job = StreamJob {
            transport,
            url: probe.url,
            ranged: probe.supports_ranges,
            host,
            file,
            limiter: self.limiter.clone(),
            cancel: cancel.clone(),
            retry: RetryPolicy {
                max_attempts: self.options.max_attempts,
                idle_timeout: self.options.idle_timeout,
            },
            downloaded: downloaded.clone(),
            total: task.total_bytes,
        };
        let mut join_set = JoinSet::new();
        join_set.spawn(job.run());

        let mut tick = interval(self.options.sample_interval);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut last_bytes = 0u64;
        let mut last_sample = Instant::now();
        let mut last_checkpoint = Instant::now();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    join_set.abort_all();
                    while join_set.join_next().await.is_some() {}
                    let bytes = downloaded.load(Ordering::Relaxed);
                    self.checkpoint_stream(task, bytes)?;
                    return Ok(SessionEnd::Paused);
                }
                joined = join_set.join_next() => {
                    match joined {
                        None => break,
                        Some(Ok(Ok(()))) => {}
                        Some(Ok(Err(failure))) => {
                            let bytes = downloaded.load(Ordering::Relaxed);
                            self.checkpoint_stream(task, bytes)?;
                            return Ok(SessionEnd::Failed(failure.error));
                        }
                        Some(Err(error)) if error.is_cancelled() => break,
                        Some(Err(_)) => {
                            let bytes = downloaded.load(Ordering::Relaxed);
                            self.checkpoint_stream(task, bytes)?;
                            return Ok(SessionEnd::Failed(internal_error()));
                        }
                    }
                }
                _ = tick.tick() => {
                    let now = Instant::now();
                    let bytes = downloaded.load(Ordering::Relaxed);
                    let elapsed = now.duration_since(last_sample).as_secs_f64().max(0.001);
                    let speed = ((bytes.saturating_sub(last_bytes)) as f64 / elapsed) as u64;
                    last_bytes = bytes;
                    last_sample = now;
                    if now.duration_since(last_checkpoint) >= self.options.checkpoint_interval {
                        self.checkpoint_stream(task, bytes)?;
                        last_checkpoint = now;
                    }
                    emit_progress(control, task, bytes, speed, 1, 0, 0);
                }
            }
        }

        let bytes = downloaded.load(Ordering::Relaxed);
        self.checkpoint_stream(task, bytes)?;
        if cancel.is_cancelled() {
            return Ok(SessionEnd::Paused);
        }
        task.downloaded_bytes = bytes;
        if task.total_bytes.is_none() {
            // Server không báo rõ 0 byte mà cũng không gửi gì (AC-06).
            if bytes == 0 {
                return Ok(SessionEnd::Rejected(empty_response()));
            }
            task.total_bytes = Some(bytes);
        }
        if task.total_bytes.is_some_and(|total| total != bytes) {
            return Ok(SessionEnd::Failed(client_error(
                "http.size_mismatch",
                "The downloaded size does not match the server's declared size.",
            )));
        }
        Ok(SessionEnd::Completed)
    }

    fn checkpoint_segments(
        &self,
        task: &mut DownloadTask,
        runtimes: &[Arc<SegmentRuntime>],
    ) -> Result<()> {
        let segments: Vec<Segment> = runtimes
            .iter()
            .map(|runtime| runtime.snapshot())
            .collect();
        let downloaded = segments.iter().map(|segment| segment.downloaded).sum();
        self.journal.checkpoint(&task.id, &segments, downloaded)?;
        task.downloaded_bytes = downloaded;
        task.updated_at = now_ms();
        Ok(())
    }

    fn checkpoint_stream(&self, task: &mut DownloadTask, downloaded: u64) -> Result<()> {
        self.journal.checkpoint(&task.id, &[], downloaded)?;
        task.downloaded_bytes = downloaded;
        task.updated_at = now_ms();
        Ok(())
    }

    /// `sniff`: server không nói kiểu nội dung ⇒ file mở đầu như trang HTML thì không lưu (AC-06).
    async fn finish(
        &self,
        mut task: DownloadTask,
        control: &DownloadControl,
        sniff: bool,
    ) -> Result<DownloadOutcome> {
        let total = task.total_bytes.unwrap_or(task.downloaded_bytes);
        task.total_bytes = Some(total);
        task.downloaded_bytes = total;
        task.status = TaskStatus::Verifying;
        task.updated_at = now_ms();
        task.validate()?;
        self.journal.save_task(&task)?;
        emit_task(control, &task);

        let temp = PathBuf::from(&task.temp_path);
        if let Some(expected) = task.expected_sha256.clone() {
            match sha256_file(&temp, &control.cancel).await {
                Ok(HashOutcome::Cancelled) => return self.pause(task, control),
                Ok(HashOutcome::Hash(actual)) => {
                    if actual != expected.to_ascii_lowercase() {
                        task.hash_state = HashState::Mismatch;
                        return self.fail(
                            task,
                            TaskError {
                                class: ErrorClass::Security,
                                code: "verify.hash_mismatch".into(),
                                message: "The downloaded file does not match the expected hash."
                                    .into(),
                            },
                            control,
                        );
                    }
                    task.hash_state = HashState::Verified;
                }
                Err(_) => {
                    return self.fail(
                        task,
                        TaskError {
                            class: ErrorClass::Permanent,
                            code: "verify.read".into(),
                            message: "Could not read the download for verification.".into(),
                        },
                        control,
                    );
                }
            }
        } else {
            task.hash_state = HashState::Unavailable;
            if sniff && file_is_page(&temp).await {
                self.discard_part(&mut task).await?;
                task.hash_state = HashState::Pending;
                return self.fail(task, not_a_file(), control);
            }
        }

        let guard = safety::inspect(&temp, &task.file_name).map_err(Error::Io)?;
        if guard.action == GuardAction::Quarantine {
            let quarantine_dir = self
                .options
                .quarantine_dir
                .as_deref()
                .map(std::path::PathBuf::from);
            return match safety::quarantine(&temp, &task.file_name, quarantine_dir.as_deref()) {
                Ok(_) => {
                    task.status = TaskStatus::Quarantined;
                    task.error = Some(TaskError {
                        class: ErrorClass::Security,
                        code: guard.code,
                        message: guard.reason,
                    });
                    task.updated_at = now_ms();
                    self.journal.save_task(&task)?;
                    emit_task(control, &task);
                    Ok(DownloadOutcome::Quarantined(task))
                }
                Err(_) => self.fail(
                    task,
                    TaskError {
                        class: ErrorClass::Permanent,
                        code: "guard.quarantine_failed".into(),
                        message: "Could not move the unsafe file to quarantine.".into(),
                    },
                    control,
                ),
            };
        }

        let synced = async {
            let file = tokio::fs::OpenOptions::new()
                .write(true)
                .open(&temp)
                .await?;
            file.sync_all().await
        }
        .await;
        if synced.is_err() {
            return self.fail(
                task,
                TaskError {
                    class: ErrorClass::Permanent,
                    code: "disk.sync".into(),
                    message: "Could not flush the download to disk.".into(),
                },
                control,
            );
        }

        let target = target_path(&task.dest_dir, &task.file_name);
        if target.exists() {
            return self.fail(
                task,
                TaskError {
                    class: ErrorClass::User,
                    code: "file.exists".into(),
                    message: "A file with this name already exists.".into(),
                },
                control,
            );
        }
        if move_no_clobber(&temp, &target).await.is_err() {
            return self.fail(
                task,
                TaskError {
                    class: ErrorClass::Permanent,
                    code: "file.rename".into(),
                    message: "Could not move the finished file into place.".into(),
                },
                control,
            );
        }

        let _ = safety::mark_of_the_web(&target);
        let timestamp = now_ms();
        task.status = TaskStatus::Completed;
        task.completed_at = Some(timestamp);
        task.updated_at = timestamp;
        task.error = None;
        self.journal.save_task(&task)?;
        emit_task(control, &task);
        Ok(DownloadOutcome::Completed(task))
    }

    fn pause(&self, mut task: DownloadTask, control: &DownloadControl) -> Result<DownloadOutcome> {
        if task.status != TaskStatus::Completed {
            task.status = TaskStatus::Paused;
            task.error = None;
            task.updated_at = now_ms();
            self.journal.save_task(&task)?;
            emit_task(control, &task);
        }
        Ok(DownloadOutcome::Paused(task))
    }

    fn fail(
        &self,
        mut task: DownloadTask,
        error: TaskError,
        control: &DownloadControl,
    ) -> Result<DownloadOutcome> {
        task.status = TaskStatus::Failed;
        task.error = Some(error);
        task.updated_at = now_ms();
        self.journal.save_task(&task)?;
        emit_task(control, &task);
        Ok(DownloadOutcome::Failed(task))
    }
}

/// HTTP/1.1: mỗi đoạn một kết nối TCP riêng như IDM. HTTP/2 dồn mọi đoạn vào một kết nối,
/// server giới hạn tốc độ theo kết nối thì thêm "kết nối" cũng không nhanh hơn.
fn build_client(options: &EngineOptions, redirect: reqwest::redirect::Policy) -> Result<Client> {
    Client::builder()
        .user_agent(&options.user_agent)
        .connect_timeout(options.connect_timeout)
        .redirect(redirect)
        .pool_max_idle_per_host(usize::from(options.max_connections))
        .http1_only()
        .build()
        .map_err(|error| Error::InvalidInput(format!("http client: {error}")))
}

fn metadata_changed(task: &DownloadTask, probe: &Probe) -> bool {
    let etag_changed = matches!(
        (task.etag.as_deref(), probe.etag.as_deref()),
        (Some(left), Some(right)) if left != right
    );
    let modified_changed = matches!(
        (task.last_modified.as_deref(), probe.last_modified.as_deref()),
        (Some(left), Some(right)) if left != right
    );
    let size_changed = matches!(
        (task.total_bytes, probe.total_bytes),
        (Some(left), Some(right)) if left != right
    );
    etag_changed || modified_changed || size_changed
}

/// Thêm đoạn đuôi vừa tách ngay sau đoạn gốc và đánh lại số thứ tự.
fn insert_split(
    runtimes: &mut Vec<Arc<SegmentRuntime>>,
    original: &Arc<SegmentRuntime>,
    (start, end): (u64, u64),
) -> Arc<SegmentRuntime> {
    let tail = Arc::new(SegmentRuntime::new(&Segment {
        index: 0,
        start,
        end,
        downloaded: 0,
        status: SegmentStatus::Pending,
    }));
    match runtimes.iter().position(|item| Arc::ptr_eq(item, original)) {
        Some(position) => runtimes.insert(position + 1, tail.clone()),
        None => runtimes.push(tail.clone()),
    }
    for (position, runtime) in runtimes.iter().enumerate() {
        runtime.set_index(position as u32);
    }
    tail
}

fn total_downloaded(runtimes: &[Arc<SegmentRuntime>]) -> u64 {
    runtimes
        .iter()
        .map(|runtime| runtime.downloaded())
        .fold(0u64, u64::saturating_add)
}

fn emit_task(control: &DownloadControl, task: &DownloadTask) {
    if let Some(events) = &control.events {
        let _ = events.send(EngineEvent::Task {
            task: Box::new(task.clone()),
        });
    }
}

fn emit_progress(
    control: &DownloadControl,
    task: &DownloadTask,
    downloaded: u64,
    speed: u64,
    connections: u16,
    segments_done: u32,
    segments_total: u32,
) {
    if let Some(events) = &control.events {
        let _ = events.send(EngineEvent::Progress {
            progress: ProgressSnapshot {
                task_id: task.id.clone(),
                status: task.status,
                downloaded_bytes: downloaded,
                total_bytes: task.total_bytes,
                bytes_per_second: speed,
                connections,
                segments_done,
                segments_total,
                hash_state: task.hash_state,
            },
        });
    }
}

fn cancelled_error() -> TaskError {
    client_error("network.cancelled", "The download was cancelled.")
}

fn client_error(code: &str, message: &str) -> TaskError {
    TaskError {
        class: ErrorClass::Transient,
        code: code.into(),
        message: message.into(),
    }
}

fn internal_error() -> TaskError {
    TaskError {
        class: ErrorClass::Permanent,
        code: "internal.worker".into(),
        message: "A download worker stopped unexpectedly.".into(),
    }
}
