use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
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
    empty_response, file_is_page, is_html_type, is_page_name, looks_like_html, not_a_file,
    page_check, sniff_remote, PageCheck,
};
use super::file::{disk_failure, move_no_clobber, read_at, target_path, volume_missing, PartFile};
use super::http::{host_key, probe, sanitize_suggested_name, Probe, RequestFailure, Validators};
use super::limiter::Limiter;
use super::net::{NetEpoch, NetState, NetWatch};
use super::pulse::Pulse;
use super::relink::{self, LinkError};
use super::transport::{plain_policy, scoped_policy, Transport};
use super::verify::{sha256_file, HashOutcome};
use super::worker::{
    run_segment, RetryPolicy, SegmentFailure, SegmentJob, SegmentRuntime, StreamJob,
    MAX_INLINE_WAIT,
};
use super::{
    DownloadControl, DownloadOutcome, DownloadRequest, EngineEvent, EngineOptions, ProgressSnapshot,
    RemoteFile, LARGE_FILE_BYTES,
};
use crate::db::Journal;
use crate::headers::{validate_request_headers, RequestHeader};
use crate::safety::{self, GuardAction};
use crate::model::{
    now_ms, plan_segments, validate_file_name, validate_segments, validate_url, DownloadTask,
    ErrorClass, HashState, Recovery, RecoveryWait, Segment, SegmentStatus, TaskError, TaskKind,
    TaskStatus, MAX_SEGMENTS, MIN_SEGMENT_BYTES,
};
use crate::recovery::RouteProbe;
use crate::{Error, Result};

/// Dò link tối đa chừng này lượt trong một lần chạy; hỏng nữa thì app hẹn giờ thử lại (hiện "Sẽ thử
/// lại lúc…"), không đứng "đang dò" vài phút.
const PROBE_ATTEMPTS: u32 = 2;
/// Mở lại task: lùi mốc mỗi đoạn qua đuôi toàn byte 0 tối đa chừng này (byte chưa xuống đĩa lúc rút
/// điện, ổ có bộ đệm ghi không tôn trọng fsync).
const ZERO_SCAN: u64 = 1024 * 1024;
/// Khối đọc khi soát đuôi đoạn (trang bộ đệm của hệ điều hành).
const PAGE: u64 = 4096;
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
    /// Hỏi đường ra mạng (bài thử thay bằng mạng giả).
    route: RouteProbe,
}

enum SessionEnd {
    Completed,
    Paused,
    Failed(SegmentFailure),
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
    pulse: Arc<Pulse>,
    validators: Arc<Validators>,
    net: Arc<NetEpoch>,
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
            pulse: self.pulse.clone(),
            validators: self.validators.clone(),
            net: self.net.clone(),
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
            route: RouteProbe::default(),
        })
    }

    /// Thay cách hỏi đường ra mạng (bài thử: rớt mạng, đổi mạng giả).
    pub fn with_route(mut self, route: RouteProbe) -> Self {
        self.route = route;
        self
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

    /// Dò nhanh một link mà không tạo task (HEAD, hoặc GET 1 byte): tên, kích thước, có phải trang web.
    /// Cho bảng xem trước của hộp Thêm hàng loạt: thử một lần, mỗi yêu cầu tối đa `connect_timeout`.
    pub async fn inspect(&self, raw_url: &str) -> std::result::Result<RemoteFile, TaskError> {
        let url = validate_url(raw_url.trim()).map_err(|error| TaskError {
            class: ErrorClass::User,
            code: "url.invalid".into(),
            message: error.to_string(),
        })?;
        let transport = Transport::new(&self.client, &self.scoped_client, &url, &[], None)
            .map_err(|_| internal_error())?;
        let probe = probe(&transport, &url, self.options.connect_timeout)
            .await
            .map_err(|failure| failure.error)?;
        let file_name = sanitize_suggested_name(probe.file_name.as_deref().unwrap_or(url.path()));
        let page = !probe.attachment
            && (is_page_name(&file_name)
                || probe.content_type.as_deref().is_some_and(is_html_type));
        Ok(RemoteFile {
            url: probe.url.to_string(),
            file_name,
            total_bytes: probe.total_bytes,
            content_type: probe.content_type,
            page,
        })
    }

    /// Cập nhật link của task đang dừng/lỗi (link ký hết hạn, cần đăng nhập lại): chỉ nhận khi link mới
    /// trỏ đúng file đang tải dở (relink.rs), giữ phần đã tải. `headers`: header thường cho link mới (đã
    /// lọc, không cookie). Không đổi trạng thái task; người gọi xếp nó vào hàng đợi.
    pub async fn replace_link(
        &self,
        task_id: &str,
        raw_url: &str,
        headers: Vec<RequestHeader>,
    ) -> std::result::Result<DownloadTask, LinkError> {
        let mut task = self
            .journal
            .load_task(task_id)
            .ok()
            .flatten()
            .ok_or(LinkError::NotAllowed)?;
        let editable = task.kind == TaskKind::Http
            && task.parent_id.is_none()
            && !task.status.is_active()
            && !matches!(task.status, TaskStatus::Completed | TaskStatus::Quarantined);
        if !editable {
            return Err(LinkError::NotAllowed);
        }
        let url = validate_url(raw_url.trim()).map_err(|_| LinkError::Invalid)?;
        validate_request_headers(&headers).map_err(|_| LinkError::Invalid)?;
        let transport = Transport::new(
            &self.client,
            &self.scoped_client,
            &url,
            &headers,
            task.max_request_bytes,
        )
        .map_err(|_| LinkError::Invalid)?;
        let segments = self.journal.load_segments(task_id).unwrap_or_default();
        let timeout = self.options.connect_timeout;
        let probe = relink::verify(&transport, &url, &task, &segments, timeout).await?;
        task.url = url.to_string();
        task.final_url = Some(probe.url.to_string());
        task.etag = probe.etag;
        task.last_modified = probe.last_modified;
        if task.downloaded_bytes == 0 {
            task.total_bytes = probe.total_bytes;
        }
        task.request_headers = headers;
        task.error = None;
        task.recovery = None;
        task.updated_at = now_ms();
        self.journal
            .save_task(&task)
            .map_err(|_| LinkError::NotAllowed)?;
        Ok(task)
    }

    /// Bỏ phần đã tải để lượt sau tải lại từ đầu (file trên máy chủ đã đổi, `.part` mất). Giữ link, tên.
    pub fn reset_progress(&self, task_id: &str) -> Result<DownloadTask> {
        let mut task = self
            .journal
            .load_task(task_id)?
            .ok_or_else(|| Error::NotFound(format!("task {task_id}")))?;
        if task.status.is_active() || task.status == TaskStatus::Completed {
            return Err(Error::InvalidInput("task is running or finished".into()));
        }
        self.journal.reset_task_progress(task_id)?;
        let _ = std::fs::remove_file(&task.temp_path);
        task.downloaded_bytes = 0;
        task.etag = None;
        task.last_modified = None;
        task.hash_state = HashState::Pending;
        task.error = None;
        task.recovery = None;
        task.updated_at = now_ms();
        self.journal.save_task(&task)?;
        Ok(task)
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

    /// Một lượt tải. Lỗi đĩa giữa chừng (đầy ổ lúc cấp phát `.part`, ổ USB rút, mất quyền) thành lỗi
    /// của task — không để task kẹt ở "đang tải" trong journal.
    async fn run(
        &self,
        task: DownloadTask,
        control: DownloadControl,
        allow_rename: bool,
    ) -> Result<DownloadOutcome> {
        let id = task.id.clone();
        match self.run_inner(task, &control, allow_rename).await {
            Err(Error::Io(error)) => {
                log::warn!("engine: task={id} io error {:?}", error.kind());
                match self.journal.load_task(&id)? {
                    Some(task) => self.fail(task, disk_failure(&error), &control),
                    None => Err(Error::Io(error)),
                }
            }
            other => other,
        }
    }

    async fn run_inner(
        &self,
        mut task: DownloadTask,
        control: &DownloadControl,
        allow_rename: bool,
    ) -> Result<DownloadOutcome> {
        let control = control.clone();
        if control.cancel.is_cancelled() {
            return self.pause(task, &control);
        }
        let pulse = Pulse::new(&task.id, task.parent_id.as_deref());
        task.status = TaskStatus::Resolving;
        task.error = None;
        // Giờ hẹn cũ (gợi ý Retry-After của lượt trước) đã dùng xong; số lần thử app vẫn giữ.
        if let Some(recovery) = &mut task.recovery {
            recovery.retry_at = None;
        }
        task.updated_at = now_ms();
        task.validate()?;
        self.journal.save_task(&task)?;
        emit_task(&control, &task);

        let url = validate_url(&task.url)?;
        let transport = self.transport(&task, &url)?;
        let probe = match self.probe_with_retry(&transport, &url, &control).await {
            Ok(probe) => probe,
            Err(failure) => {
                if control.cancel.is_cancelled() {
                    return self.pause(task, &control);
                }
                return self.fail_with(task, failure.error, failure.retry_after, &control);
            }
        };
        pulse.mark("probe");
        if control.cancel.is_cancelled() {
            return self.pause(task, &control);
        }

        let changed = metadata_changed(&task, &probe);
        let size_redefined = probe
            .total_bytes
            .is_some_and(|total| task.total_bytes != Some(total));
        // AC-04: file đổi trên máy chủ trong lúc tải dở ⇒ dừng và báo, người dùng chọn tải lại từ đầu.
        // Luồng con của media: app tự hỏi yt-dlp và ghép lại, ở đây chỉ bỏ đoạn cũ như trước.
        if changed && task.downloaded_bytes > 0 && task.parent_id.is_none() && probe.supports_ranges {
            return self.fail(task, file_changed(), &control);
        }
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

        // Kiểm sớm cho khỏi tải phí; lúc đưa vào chỗ vẫn không ghi đè (`move_no_clobber`).
        let target = target_path(&task.dest_dir, &task.file_name);
        if target.exists() {
            return self.fail(task, file_exists(), &control);
        }

        if task.total_bytes == Some(0) {
            self.journal.reset_task_progress(&task.id)?;
            task.downloaded_bytes = 0;
            let file = self.open_part(&task, Some(0)).await?;
            file.reset(Some(0))?;
            return self.finish(task, &control, sniff, &pulse).await;
        }

        // `task.connections` chỉ là trần; số kết nối thật do bộ điều khiển tự chọn.
        let max_connections = task
            .connections
            .min(self.options.max_connections)
            .min(self.options.host_connections)
            .min(self.options.global_connections)
            .max(1);
        let ranged = probe.supports_ranges && task.total_bytes.is_some();
        // `.part` / thư mục tải mất lúc đang dừng: báo rõ, không âm thầm tải lại từ 0. Ổ rút ra thì app
        // chờ ổ cắm lại. Luồng con của media do app sửa (tải lại luồng đó).
        if ranged
            && task.downloaded_bytes > 0
            && task.parent_id.is_none()
            && let Some(error) = missing_part(&task)
        {
            return self.fail(task, error, &control);
        }
        let mut segments = if ranged {
            self.journal.load_segments(&task.id)?
        } else {
            Vec::new()
        };
        if ranged && task.downloaded_bytes > 0 && !segments.is_empty() {
            let path = PathBuf::from(&task.temp_path);
            let mut checked = segments.clone();
            let dropped = tokio::task::spawn_blocking(move || {
                reconcile_part(&path, &mut checked).map(|dropped| (dropped, checked))
            })
            .await
            .map_err(|error| Error::Io(io::Error::other(error)))?;
            if let Ok((dropped, checked)) = dropped
                && dropped > 0
            {
                log::warn!(
                    "engine: task={} .part thiếu {dropped} byte so với journal, tải lại phần đó",
                    task.id
                );
                self.journal.save_segments(&task.id, &checked)?;
                task.downloaded_bytes = checked.iter().map(|segment| segment.downloaded).sum();
                segments = checked;
            }
        }
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
                return self.finish(task, &control, sniff, &pulse).await;
            }
            let validators = Arc::new(Validators::of(&task));
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
                    pulse.clone(),
                    validators,
                )
                .await?;
            return self.conclude(task, &control, end, sniff, &pulse).await;
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
            .run_single(&mut task, &control, file, transport, probe, pulse.clone())
            .await?;
        self.conclude(task, &control, end, sniff, &pulse).await
    }

    async fn conclude(
        &self,
        mut task: DownloadTask,
        control: &DownloadControl,
        end: SessionEnd,
        sniff: bool,
        pulse: &Pulse,
    ) -> Result<DownloadOutcome> {
        match end {
            SessionEnd::Completed => self.finish(task, control, sniff, pulse).await,
            SessionEnd::Paused => self.pause(task, control),
            SessionEnd::Failed(failure) => {
                self.fail_with(task, failure.error, failure.retry_after, control)
            }
            SessionEnd::Rejected(error) => {
                self.discard_part(&mut task).await?;
                self.fail(task, error, control)
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
    ) -> std::result::Result<Probe, RequestFailure> {
        let limit = self.options.max_attempts.clamp(1, PROBE_ATTEMPTS);
        let cancelled = || RequestFailure {
            error: cancelled_error(),
            retry_after: None,
        };
        let mut attempts = 0;
        loop {
            let probed = tokio::select! {
                result = probe(transport, url, self.options.connect_timeout) => result,
                _ = control.cancel.cancelled() => return Err(cancelled()),
            };
            match probed {
                Ok(probe) => return Ok(probe),
                Err(failure) => {
                    attempts += 1;
                    // Máy chủ im tới hết hạn, hoặc bảo chờ lâu: thôi luôn, app hẹn giờ thử lại.
                    let slow = failure.error.code == "network.timeout"
                        || failure
                            .retry_after
                            .is_some_and(|after| after > MAX_INLINE_WAIT);
                    if !failure.retryable() || attempts >= limit || slow {
                        return Err(failure);
                    }
                    let delay = failure
                        .retry_after
                        .unwrap_or_else(|| Duration::from_millis(500 * u64::from(attempts)));
                    tokio::select! {
                        _ = control.cancel.cancelled() => return Err(cancelled()),
                        _ = sleep(delay) => {}
                    }
                }
            }
        }
    }

    /// Số kết nối mở ngay: người gọi chỉ định (media chia ngân sách), file lớn mở luôn như IDM,
    /// file nhỏ bắt đầu ít rồi bộ điều khiển tự leo.
    fn initial_connections(&self, control: &DownloadControl, total: u64) -> u16 {
        control.initial_connections.unwrap_or_else(|| {
            if total >= LARGE_FILE_BYTES {
                self.options
                    .initial_connections
                    .max(self.options.large_initial_connections)
            } else {
                self.options.initial_connections
            }
        })
    }

    fn retry_policy(&self) -> RetryPolicy {
        RetryPolicy {
            max_attempts: self.options.max_attempts,
            idle_timeout: self.options.idle_timeout,
            stall_timeout: self.options.stall_timeout,
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
        pulse: Arc<Pulse>,
        validators: Arc<Validators>,
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
        let retry = self.retry_policy();
        let initial = usize::from(
            self.initial_connections(control, total)
                .min(max_connections)
                .max(1),
        );
        let gate = ConnectionGate::new(initial);
        let mut controller = Controller::new(
            initial,
            usize::from(max_connections),
            Tuning::new(self.options.sample_interval, self.options.balance_interval),
            Instant::now(),
            total_downloaded(&runtimes),
        );
        let mut net = NetWatch::new(self.route.clone());
        let context = SegmentContext {
            transport,
            total,
            url,
            host,
            file: file.clone(),
            limiter: self.limiter.clone(),
            gate: gate.clone(),
            cancel: cancel.clone(),
            retry,
            pulse,
            validators,
            net: net.epoch(),
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
        let mut saving = Saving::new(&segments);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    join_set.abort_all();
                    while join_set.join_next().await.is_some() {}
                    saving.settle().await?;
                    self.checkpoint_segments(task, &runtimes, &file).await?;
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
                            saving.settle().await?;
                            self.checkpoint_segments(task, &runtimes, &file).await?;
                            return Ok(SessionEnd::Failed(failure));
                        }
                        None => break,
                    }
                }
                _ = tick.tick() => {
                    let now = Instant::now();
                    let downloaded = total_downloaded(&runtimes);
                    let elapsed = now.duration_since(last_sample).as_secs_f64().max(0.001);
                    let speed = ((downloaded.saturating_sub(last_bytes)) as f64 / elapsed) as u64;
                    let receiving = downloaded > last_bytes;
                    last_bytes = downloaded;
                    last_sample = now;

                    let stop = if net.tick(now, receiving) == NetState::Offline {
                        Some(offline_error())
                    } else if !Path::new(&task.temp_path).exists() {
                        // Xoá / đổi tên `.part` lúc đang tải (Windows cho xoá vì mở kèm FILE_SHARE_DELETE).
                        Some(part_missing())
                    } else {
                        None
                    };
                    if let Some(error) = stop {
                        join_set.abort_all();
                        while join_set.join_next().await.is_some() {}
                        saving.settle().await?;
                        self.checkpoint_segments(task, &runtimes, &file).await?;
                        return Ok(SessionEnd::Failed(SegmentFailure::new(error)));
                    }
                    // Checkpoint chạy nền (fsync rồi ghi journal); lượt trước chưa xong (đĩa chậm) thì để nhịp sau.
                    if now.duration_since(last_checkpoint) >= self.options.checkpoint_interval
                        && !saving.busy()
                    {
                        saving.settle().await?;
                        saving.start(&self.journal, &file, &task.id, snapshot(&runtimes));
                        task.updated_at = now_ms();
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
                    let tails = self.fill_idle(&mut runtimes, controller.target());
                    if !tails.is_empty() {
                        saving.settle().await?;
                        self.save_plan(task, &saving.durable_plan(&runtimes))?;
                    }
                    for tail in tails {
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
                            && let Some(tail) =
                                split_slowest(&mut runtimes, &previous_balance, balance_elapsed)
                        {
                            saving.settle().await?;
                            self.save_plan(task, &saving.durable_plan(&runtimes))?;
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

        saving.settle().await?;
        self.checkpoint_segments(task, &runtimes, &file).await?;
        if cancel.is_cancelled() {
            return Ok(SessionEnd::Paused);
        }
        let downloaded = task.downloaded_bytes;
        if downloaded != total {
            return Ok(SessionEnd::Failed(SegmentFailure::new(client_error(
                "network.closed",
                "The download ended before all bytes arrived.",
            ))));
        }
        Ok(SessionEnd::Completed)
    }

    /// Số đoạn chưa xong ít hơn số kết nối cho phép: chia đôi đoạn còn nhiều nhất (kiểu IDM)
    /// để kết nối rảnh có việc, nhất là lúc gần xong.
    fn fill_idle(
        &self,
        runtimes: &mut Vec<Arc<SegmentRuntime>>,
        target: usize,
    ) -> Vec<Arc<SegmentRuntime>> {
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
        added
    }

    /// Lưu cách chia đoạn mới; `plan` chỉ kèm byte đã chắc nằm trên đĩa (`Saving::durable_plan`).
    fn save_plan(&self, task: &mut DownloadTask, plan: &[Segment]) -> Result<()> {
        self.journal.save_segments(&task.id, plan)?;
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
        pulse: Arc<Pulse>,
    ) -> Result<SessionEnd> {
        let cancel = control.cancel.clone();
        let downloaded = Arc::new(AtomicU64::new(0));
        let host = host_key(&probe.url);
        let mut net = NetWatch::new(self.route.clone());
        let job = StreamJob {
            transport,
            url: probe.url,
            ranged: probe.supports_ranges,
            host,
            file,
            limiter: self.limiter.clone(),
            cancel: cancel.clone(),
            retry: self.retry_policy(),
            downloaded: downloaded.clone(),
            total: task.total_bytes,
            pulse,
            validators: Arc::new(Validators::of(task)),
            net: net.epoch(),
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
                            return Ok(SessionEnd::Failed(failure));
                        }
                        Some(Err(error)) if error.is_cancelled() => break,
                        Some(Err(_)) => {
                            let bytes = downloaded.load(Ordering::Relaxed);
                            self.checkpoint_stream(task, bytes)?;
                            return Ok(SessionEnd::Failed(SegmentFailure::new(internal_error())));
                        }
                    }
                }
                _ = tick.tick() => {
                    let now = Instant::now();
                    let bytes = downloaded.load(Ordering::Relaxed);
                    let elapsed = now.duration_since(last_sample).as_secs_f64().max(0.001);
                    let speed = ((bytes.saturating_sub(last_bytes)) as f64 / elapsed) as u64;
                    let receiving = bytes > last_bytes;
                    last_bytes = bytes;
                    last_sample = now;
                    if net.tick(now, receiving) == NetState::Offline {
                        join_set.abort_all();
                        while join_set.join_next().await.is_some() {}
                        self.checkpoint_stream(task, bytes)?;
                        return Ok(SessionEnd::Failed(SegmentFailure::new(offline_error())));
                    }
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
            return Ok(SessionEnd::Failed(SegmentFailure::new(client_error(
                "http.size_mismatch",
                "The downloaded size does not match the server's declared size.",
            ))));
        }
        Ok(SessionEnd::Completed)
    }

    /// Chụp tiến độ, fsync `.part`, rồi mới ghi journal: journal không bao giờ đi trước dữ liệu trên
    /// đĩa (rút điện ngay sau đó vẫn tải tiếp đúng). Checkpoint cuối lượt; giữa lượt dùng `Saving`.
    async fn checkpoint_segments(
        &self,
        task: &mut DownloadTask,
        runtimes: &[Arc<SegmentRuntime>],
        file: &Arc<PartFile>,
    ) -> Result<()> {
        let segments = snapshot(runtimes);
        let downloaded = segments.iter().map(|segment| segment.downloaded).sum();
        sync_part(file).await?;
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
        pulse: &Pulse,
    ) -> Result<DownloadOutcome> {
        pulse.mark("downloaded");
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

        let target = target_path(&task.dest_dir, &task.file_name);
        if let Err(error) = place(&temp, &target, pulse).await {
            return self.fail(task, error, control);
        }
        pulse.mark("done");

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
            // Người dùng (hoặc app sắp tắt / cài bản mới) dừng: không còn lịch tự thử lại nào.
            task.recovery = None;
            task.updated_at = now_ms();
            self.journal.save_task(&task)?;
            emit_task(control, &task);
        }
        Ok(DownloadOutcome::Paused(task))
    }

    fn fail(
        &self,
        task: DownloadTask,
        error: TaskError,
        control: &DownloadControl,
    ) -> Result<DownloadOutcome> {
        self.fail_with(task, error, None, control)
    }

    /// `retry_after`: máy chủ bảo chờ (Retry-After) ⇒ gợi ý giờ thử lại cho app (`task.recovery`).
    fn fail_with(
        &self,
        mut task: DownloadTask,
        mut error: TaskError,
        retry_after: Option<Duration>,
        control: &DownloadControl,
    ) -> Result<DownloadOutcome> {
        if error.code.starts_with("disk.") && volume_missing(Path::new(&task.dest_dir)) {
            error = unplugged();
        }
        if let Some(after) = retry_after {
            let mut hint = task
                .recovery
                .take()
                .unwrap_or_else(|| Recovery::new(RecoveryWait::Retry));
            let after = i64::try_from(after.as_millis()).unwrap_or(i64::MAX);
            hint.retry_at = Some(now_ms().saturating_add(after));
            task.recovery = Some(hint);
        }
        task.status = TaskStatus::Failed;
        task.error = Some(error);
        task.updated_at = now_ms();
        self.journal.save_task(&task)?;
        emit_task(control, &task);
        Ok(DownloadOutcome::Failed(task))
    }
}

/// Xả file tạm xuống đĩa rồi đưa vào chỗ. Không ghi đè: tên đã có (kể cả file người dùng vừa
/// tạo sau lúc kiểm đầu lượt) ⇒ `file.exists`, file tạm giữ nguyên.
async fn place(temp: &Path, target: &Path, pulse: &Pulse) -> std::result::Result<(), TaskError> {
    let synced = async {
        let file = tokio::fs::OpenOptions::new().write(true).open(temp).await?;
        file.sync_all().await
    }
    .await;
    if synced.is_err() {
        return Err(TaskError {
            class: ErrorClass::Permanent,
            code: "disk.sync".into(),
            message: "Could not flush the download to disk.".into(),
        });
    }
    pulse.mark("synced");
    match move_no_clobber(temp, target).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(file_exists()),
        Err(_) => Err(TaskError {
            class: ErrorClass::Permanent,
            code: "file.rename".into(),
            message: "Could not move the finished file into place.".into(),
        }),
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
    let validators = Validators {
        etag: probe.etag.clone(),
        last_modified: probe.last_modified.clone(),
    };
    let size_changed = matches!(
        (task.total_bytes, probe.total_bytes),
        (Some(left), Some(right)) if left != right
    );
    size_changed || Validators::of(task).differs(&validators)
}

/// `.part` hoặc thư mục tải không còn (task đã có byte): ổ rút ra thì `disk.unplugged` (app chờ ổ
/// cắm lại), còn lại `file.part_missing` (người dùng chọn tải lại phần thiếu).
fn missing_part(task: &DownloadTask) -> Option<TaskError> {
    let folder = Path::new(&task.dest_dir);
    if !folder.is_dir() {
        return Some(if volume_missing(folder) {
            unplugged()
        } else {
            part_missing()
        });
    }
    (!Path::new(&task.temp_path).is_file()).then(part_missing)
}

/// Đối chiếu `.part` với journal trước khi tải tiếp: đoạn nào journal nhận quá độ dài file thì cắt
/// bớt; đuôi đoạn là các trang toàn byte 0 (byte chưa kịp xuống đĩa khi rút điện) thì tải lại. Trả số
/// byte phải tải lại. Byte 0 thật chỉ tốn tải lại, không bao giờ giữ vùng chưa chắc có dữ liệu.
fn reconcile_part(path: &Path, segments: &mut [Segment]) -> io::Result<u64> {
    let file = File::open(path)?;
    let length = file.metadata()?.len();
    let mut dropped = 0u64;
    let mut page = vec![0u8; PAGE as usize];
    for segment in segments.iter_mut() {
        let room = length.saturating_sub(segment.start);
        let mut end = segment.start + segment.downloaded.min(room);
        let floor = end.saturating_sub(ZERO_SCAN).max(segment.start);
        while end > floor {
            let from = ((end - 1) / PAGE * PAGE).max(floor);
            let size = (end - from) as usize;
            let read = read_at(&file, from, &mut page[..size])?;
            if read < size || page[..size].iter().any(|byte| *byte != 0) {
                break;
            }
            end = from;
        }
        let keep = end - segment.start;
        if keep < segment.downloaded {
            dropped += segment.downloaded - keep;
            segment.downloaded = keep;
            if segment.status == SegmentStatus::Done {
                segment.status = SegmentStatus::Pending;
            }
        }
    }
    Ok(dropped)
}

fn snapshot(runtimes: &[Arc<SegmentRuntime>]) -> Vec<Segment> {
    runtimes.iter().map(|runtime| runtime.snapshot()).collect()
}

/// Checkpoint giữa lượt chạy nền: fsync `.part` rồi ghi journal, không chặn vòng lượt tải (đĩa chậm, USB).
/// Nhớ byte chắc chắn đã nằm trên đĩa của mỗi đoạn để cách chia đoạn mới không nhận hơn.
struct Saving {
    handle: Option<tokio::task::JoinHandle<Result<Vec<Segment>>>>,
    /// Theo byte đầu đoạn (không đổi khi tách đoạn): số byte đã fsync và journal đã nhận.
    durable: HashMap<u64, u64>,
}

impl Saving {
    fn new(saved: &[Segment]) -> Self {
        Self {
            handle: None,
            durable: saved
                .iter()
                .map(|segment| (segment.start, segment.downloaded))
                .collect(),
        }
    }

    fn busy(&self) -> bool {
        self.handle.as_ref().is_some_and(|handle| !handle.is_finished())
    }

    fn start(&mut self, journal: &Arc<Journal>, file: &Arc<PartFile>, task_id: &str, segments: Vec<Segment>) {
        let (journal, file, task_id) = (journal.clone(), file.clone(), task_id.to_string());
        self.handle = Some(tokio::task::spawn_blocking(move || {
            let downloaded = segments.iter().map(|segment| segment.downloaded).sum();
            file.sync()?;
            journal.checkpoint(&task_id, &segments, downloaded)?;
            Ok(segments)
        }));
    }

    /// Chờ checkpoint đang chạy (nếu có) xong. Gọi trước mọi lần ghi cách chia đoạn và checkpoint cuối:
    /// checkpoint cập nhật theo số thứ tự đoạn, ghi sau cách chia mới là lệch đoạn.
    async fn settle(&mut self) -> Result<()> {
        if let Some(handle) = self.handle.take() {
            let saved = handle
                .await
                .map_err(|error| Error::Io(io::Error::other(error)))??;
            self.durable = saved
                .iter()
                .map(|segment| (segment.start, segment.downloaded))
                .collect();
        }
        Ok(())
    }

    /// Cách chia hiện tại, mỗi đoạn chỉ kèm byte đã chắc nằm trên đĩa (đoạn mới tách: 0).
    fn durable_plan(&self, runtimes: &[Arc<SegmentRuntime>]) -> Vec<Segment> {
        let mut plan = snapshot(runtimes);
        for segment in &mut plan {
            let durable = self.durable.get(&segment.start).copied().unwrap_or(0);
            segment.downloaded = segment.downloaded.min(durable);
            if segment.status == SegmentStatus::Done && !segment.is_done() {
                segment.status = SegmentStatus::Active;
            }
        }
        plan
    }
}

async fn sync_part(file: &Arc<PartFile>) -> Result<()> {
    let file = file.clone();
    tokio::task::spawn_blocking(move || file.sync())
        .await
        .map_err(|error| Error::Io(io::Error::other(error)))??;
    Ok(())
}

/// Đoạn chậm hơn một nửa đoạn nhanh nhất (kể cả đoạn treo 0 B/s): tách phần đuôi cho kết nối khác làm.
fn split_slowest(
    runtimes: &mut Vec<Arc<SegmentRuntime>>,
    previous: &HashMap<u64, u64>,
    elapsed: f64,
) -> Option<Arc<SegmentRuntime>> {
    let unfinished = runtimes.iter().filter(|runtime| !runtime.is_done()).count();
    if unfinished < 2 || runtimes.len() >= MAX_SEGMENTS {
        return None;
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
    let runtime = pick_slowest(&speeds, Duration::from_secs_f64(elapsed))?;
    let range = runtime.split_tail(MIN_SPLIT_REMAINING)?;
    Some(insert_split(runtimes, &runtime, range))
}

/// Đoạn chậm nhất còn đủ lớn để tách, chậm hơn một nửa đoạn nhanh nhất. Đoạn 0 B/s chỉ tính
/// khi giữ kết nối mở suốt cửa sổ đo (treo thật), không phải đang chờ tới lượt.
fn pick_slowest(
    speeds: &[(Arc<SegmentRuntime>, u64)],
    window: Duration,
) -> Option<Arc<SegmentRuntime>> {
    let fastest = speeds.iter().map(|(_, speed)| *speed).max().unwrap_or(0);
    if fastest == 0 {
        return None;
    }
    speeds
        .iter()
        .filter(|(runtime, per_second)| {
            (*per_second > 0 || runtime.connected_for(window))
                && per_second.saturating_mul(2) < fastest
                && !runtime.is_done()
                && runtime.remaining() >= MIN_SPLIT_REMAINING
        })
        .min_by_key(|(_, per_second)| *per_second)
        .map(|(runtime, _)| runtime.clone())
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

fn file_exists() -> TaskError {
    TaskError {
        class: ErrorClass::User,
        code: "file.exists".into(),
        message: "A file with this name already exists.".into(),
    }
}

fn file_changed() -> TaskError {
    TaskError {
        class: ErrorClass::Permanent,
        code: "file.changed".into(),
        message: "The file changed on the server since the download started.".into(),
    }
}

fn part_missing() -> TaskError {
    TaskError {
        class: ErrorClass::User,
        code: "file.part_missing".into(),
        message: "The partial download file or its folder was deleted or renamed.".into(),
    }
}

fn unplugged() -> TaskError {
    TaskError {
        class: ErrorClass::User,
        code: "disk.unplugged".into(),
        message: "The drive holding the download folder is not connected.".into(),
    }
}

fn offline_error() -> TaskError {
    client_error("network.offline", "The computer is offline.")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(start: u64, connected: bool) -> Arc<SegmentRuntime> {
        let runtime = Arc::new(SegmentRuntime::new(&Segment {
            index: 0,
            start,
            end: start + 4 * MIN_SPLIT_REMAINING - 1,
            downloaded: 0,
            status: SegmentStatus::Active,
        }));
        runtime.set_connected(connected);
        runtime
    }

    #[test]
    fn a_hung_connection_is_the_slowest() {
        let fast = runtime(0, true);
        let slow = runtime(1 << 30, true);
        let hung = runtime(2 << 30, true);
        let speeds = vec![(fast, 8_000_000), (slow, 1_000_000), (hung.clone(), 0)];
        let picked = pick_slowest(&speeds, Duration::ZERO).expect("a segment to split");
        assert!(Arc::ptr_eq(&picked, &hung));
    }

    #[test]
    fn a_segment_waiting_for_its_turn_is_not_hung() {
        let fast = runtime(0, true);
        let slow = runtime(1 << 30, true);
        let waiting = runtime(2 << 30, false);
        let speeds = vec![(fast, 8_000_000), (slow.clone(), 1_000_000), (waiting, 0)];
        let picked = pick_slowest(&speeds, Duration::ZERO).expect("a segment to split");
        assert!(Arc::ptr_eq(&picked, &slow));
        // Mới mở kết nối, chưa đủ một cửa sổ đo: chưa tính là treo.
        let fresh = runtime(3 << 30, true);
        let speeds = vec![(runtime(0, true), 8_000_000), (fresh, 0)];
        assert!(pick_slowest(&speeds, Duration::from_secs(60)).is_none());
    }

    #[test]
    fn large_files_open_more_connections_right_away() {
        let directory = tempfile::tempdir().expect("tempdir");
        let journal = Arc::new(Journal::open(directory.path().join("utd.db")).expect("journal"));
        let engine = DownloadEngine::new(journal, EngineOptions::default()).expect("engine");
        let control = DownloadControl::new();
        assert_eq!(engine.initial_connections(&control, LARGE_FILE_BYTES - 1), 4);
        assert_eq!(engine.initial_connections(&control, LARGE_FILE_BYTES), 8);
        let shared = DownloadControl {
            initial_connections: Some(12),
            ..DownloadControl::new()
        };
        assert_eq!(
            engine.initial_connections(&shared, 1024),
            12,
            "người gọi chia ngân sách thì theo người gọi"
        );
    }
}
