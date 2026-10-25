use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use reqwest::header::CONTENT_RANGE;
use reqwest::{Method, Response, StatusCode};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::adaptive::{is_throttle, ConnectionGate, GateSlot};
use super::file::{disk_failure, PartFile};
use super::http::{
    classify_error, failure_from_status, parse_content_range, RequestFailure, Validators,
};
use super::limiter::Limiter;
use super::net::NetEpoch;
use super::pulse::Pulse;
use super::transport::Transport;
use crate::model::{ErrorClass, Segment, SegmentStatus, TaskError};

/// Nhịp hỏi "kết nối khác có đang nhận byte không" của luật treo (tối đa; hạn treo ngắn thì nửa hạn).
const STALL_RECHECK: Duration = Duration::from_millis(500);
/// Nhịp hỏi "mạng vừa đổi chưa" khi không có luật treo (một kết nối).
const NET_RECHECK: Duration = Duration::from_millis(500);
/// Máy chủ bảo chờ (Retry-After) lâu hơn chừng này mà không còn kết nối nào chạy: dừng lượt để app hẹn
/// giờ thử lại, không giữ chỗ trong hàng đợi lúc chờ. Ngắn hơn thì chờ ngay trong lượt.
pub(crate) const MAX_INLINE_WAIT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy)]
pub(crate) struct RetryPolicy {
    pub max_attempts: u32,
    pub idle_timeout: Duration,
    pub stall_timeout: Duration,
}

impl RetryPolicy {
    /// Hạn coi là treo sau `stalls` lần cắt liền nhau mà đoạn không thêm byte: gấp đôi mỗi lần
    /// (mạng chậm thật thì không cắt mãi); chạm `idle_timeout` thì thôi, chỉ còn hạn idle.
    pub fn stall_window(&self, stalls: u32) -> Option<Duration> {
        let window = self.stall_timeout.saturating_mul(1 << stalls.min(16));
        (window < self.idle_timeout).then_some(window)
    }
}

pub(crate) struct SegmentRuntime {
    index: AtomicU32,
    start: u64,
    state: Mutex<RuntimeState>,
}

struct RuntimeState {
    end: u64,
    downloaded: u64,
    status: SegmentStatus,
    /// Lúc mở kết nối đang dùng cho đoạn (`None` khi đoạn đang chờ lượt).
    connected: Option<Instant>,
}

impl SegmentRuntime {
    pub fn new(segment: &Segment) -> Self {
        Self {
            index: AtomicU32::new(segment.index),
            start: segment.start,
            state: Mutex::new(RuntimeState {
                end: segment.end,
                downloaded: segment.downloaded,
                status: segment.status,
                connected: None,
            }),
        }
    }

    pub fn index(&self) -> u32 {
        self.index.load(Ordering::Relaxed)
    }

    pub fn set_index(&self, index: u32) {
        self.index.store(index, Ordering::Relaxed);
    }

    pub fn start(&self) -> u64 {
        self.start
    }

    pub fn end(&self) -> u64 {
        self.lock().end
    }

    pub fn downloaded(&self) -> u64 {
        self.lock().downloaded
    }

    pub fn snapshot(&self) -> Segment {
        let state = self.lock();
        Segment {
            index: self.index(),
            start: self.start,
            end: state.end,
            downloaded: state.downloaded,
            status: state.status,
        }
    }

    pub fn next_offset(&self) -> Option<u64> {
        let state = self.lock();
        let next = self.start + state.downloaded;
        (next <= state.end).then_some(next)
    }

    pub fn is_done(&self) -> bool {
        self.next_offset().is_none()
    }

    pub fn mark_status(&self, status: SegmentStatus) {
        self.lock().status = status;
    }

    /// Đánh dấu đoạn có/không có kết nối mở: đoạn 0 B/s là treo hay chỉ đang chờ tới lượt.
    pub fn set_connected(&self, connected: bool) {
        self.lock().connected = connected.then(Instant::now);
    }

    /// Đoạn đã giữ một kết nối mở liền ít nhất `window`.
    pub fn connected_for(&self, window: Duration) -> bool {
        self.lock()
            .connected
            .is_some_and(|since| since.elapsed() >= window)
    }

    /// Số byte còn phải tải.
    pub fn remaining(&self) -> u64 {
        let state = self.lock();
        (state.end + 1).saturating_sub(self.start + state.downloaded)
    }

    /// Chia đôi phần còn lại (kiểu IDM) cho kết nối đang rảnh; mỗi nửa ít nhất `minimum_piece`.
    pub fn split_half(&self, minimum_piece: u64) -> Option<(u64, u64)> {
        let mut state = self.lock();
        let next = self.start + state.downloaded;
        let end = state.end;
        if next > end {
            return None;
        }
        let remaining = end - next + 1;
        if remaining < minimum_piece.max(1).saturating_mul(2) {
            return None;
        }
        let split_start = next + remaining / 2;
        state.end = split_start - 1;
        Some((split_start, end))
    }

    pub fn split_tail(&self, minimum_remaining: u64) -> Option<(u64, u64)> {
        let mut state = self.lock();
        let next = self.start + state.downloaded;
        let end = state.end;
        if next > end {
            return None;
        }
        let remaining = end - next + 1;
        if remaining < minimum_remaining {
            return None;
        }
        let gap = (remaining / 4).min(4 * crate::model::MIN_SEGMENT_BYTES);
        let split_start = next + gap;
        if split_start > end {
            return None;
        }
        state.end = split_start - 1;
        Some((split_start, end))
    }

    fn lock(&self) -> MutexGuard<'_, RuntimeState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub(crate) struct SegmentJob {
    pub transport: Transport,
    pub total: u64,
    pub url: Url,
    pub host: String,
    pub segment: Arc<SegmentRuntime>,
    pub file: Arc<PartFile>,
    pub limiter: Arc<Limiter>,
    pub gate: Arc<ConnectionGate>,
    pub cancel: CancellationToken,
    pub retry: RetryPolicy,
    pub pulse: Arc<Pulse>,
    /// ETag / Last-Modified lúc bắt đầu lượt: phản hồi mang validator khác ⇒ file đã đổi (AC-04).
    pub validators: Arc<Validators>,
    pub net: Arc<NetEpoch>,
}

pub(crate) struct SegmentFailure {
    pub error: TaskError,
    /// Máy chủ bảo chờ chừng này (Retry-After) rồi mới thử lại.
    pub retry_after: Option<Duration>,
}

impl SegmentFailure {
    pub fn new(error: TaskError) -> Self {
        Self {
            error,
            retry_after: None,
        }
    }

    fn from_request(failure: RequestFailure) -> Self {
        Self {
            error: failure.error,
            retry_after: failure.retry_after,
        }
    }
}

/// Lỗi kết thúc lượt luôn (không thử lại trong lượt): lỗi không tự khỏi (403 link hết hạn, 404) hoặc
/// máy chủ bảo chờ lâu, khi không còn kết nối nào khác đang chạy.
fn ends_the_run(failure: &RequestFailure, alone: bool) -> bool {
    alone
        && (!failure.retryable()
            || failure
                .retry_after
                .is_some_and(|after| after > MAX_INLINE_WAIT))
}

/// Đếm lượt của một worker.
#[derive(Default)]
struct Tries {
    attempts: u32,
    /// Số lần liền nhau kết nối bị cắt vì treo mà đoạn không thêm byte nào.
    stalls: u32,
}

/// Việc tiếp theo sau một lượt kết nối.
enum Next {
    Now,
    After(Duration),
    Stop(Result<(), SegmentFailure>),
}

pub(crate) async fn run_segment(job: SegmentJob) -> Result<(), SegmentFailure> {
    let mut tries = Tries::default();
    loop {
        if job.cancel.is_cancelled() || job.finished() {
            return Ok(());
        }
        let Some(slot) = job.gate.enter(&job.cancel).await else {
            return Ok(());
        };
        if job.finished() {
            return Ok(());
        }
        let Some(limit) = job.limiter.acquire(&job.host, &job.cancel).await else {
            return Ok(());
        };
        // Trong lúc chờ hàng host/toàn cục, mục tiêu có thể đã giảm.
        if slot.yield_if_over() {
            continue;
        }
        let stall = job.retry.stall_window(tries.stalls);
        let (outcome, moved) = job.connected(&slot, stall).await;
        drop(limit);
        drop(slot);
        let delay = match job.next_step(outcome, moved, &mut tries) {
            Next::Now => continue,
            Next::After(delay) => delay,
            Next::Stop(result) => return result,
        };
        tokio::select! {
            _ = job.cancel.cancelled() => return Ok(()),
            _ = sleep(delay) => {}
        }
    }
}

impl SegmentJob {
    fn finished(&self) -> bool {
        let done = self.segment.is_done();
        if done {
            self.segment.mark_status(SegmentStatus::Done);
        }
        done
    }

    /// Sau một lượt kết nối: thử lại ngay, chờ rồi thử, hay dừng (`moved`: đoạn có thêm byte).
    fn next_step(&self, outcome: Attempt, moved: bool, tries: &mut Tries) -> Next {
        if moved {
            tries.stalls = 0;
        }
        match outcome {
            Attempt::Complete | Attempt::Yielded | Attempt::RangeDone => {
                tries.attempts = 0;
                Next::Now
            }
            Attempt::Progress(failure) => {
                self.gate.report(&failure);
                tries.attempts = 0;
                Next::Now
            }
            // Im trong khi kết nối khác vẫn nhận byte: đóng rồi mở lại sau một chút, không tính
            // lượt (đã có byte thì như Progress).
            Attempt::Stalled(failure) => {
                self.gate.report(&failure);
                if moved {
                    tries.attempts = 0;
                } else {
                    tries.stalls += 1;
                }
                Next::After(stall_delay(tries.stalls))
            }
            Attempt::Cancelled => Next::Stop(Ok(())),
            Attempt::Retry(failure) => {
                self.gate.report(&failure);
                if ends_the_run(&failure, self.gate.live() == 0) {
                    self.segment.mark_status(SegmentStatus::Failed);
                    return Next::Stop(Err(SegmentFailure::from_request(failure)));
                }
                // Server chê nhiều kết nối mà các kết nối khác vẫn chạy: chỉ cần bớt, không phải hỏng.
                if !(is_throttle(&failure) && self.gate.live() > 0) {
                    tries.attempts += 1;
                }
                if tries.attempts >= self.retry.max_attempts {
                    self.segment.mark_status(SegmentStatus::Failed);
                    return Next::Stop(Err(SegmentFailure::from_request(failure)));
                }
                let delay = retry_delay(&failure, tries.attempts.max(1), self.segment.index());
                Next::After(delay)
            }
            Attempt::Fatal(error) => {
                self.segment.mark_status(SegmentStatus::Failed);
                Next::Stop(Err(SegmentFailure::new(error)))
            }
        }
    }

    /// Mở một kết nối cho đoạn và tải tới khi xong/hỏng; kèm cờ đoạn có thêm byte không.
    async fn connected(&self, slot: &GateSlot, stall: Option<Duration>) -> (Attempt, bool) {
        let live = self.gate.connect();
        self.segment.set_connected(true);
        let before = self.segment.downloaded();
        let outcome = self.attempt(slot, stall).await;
        self.segment.set_connected(false);
        drop(live);
        (outcome, self.segment.downloaded() > before)
    }

    fn watch(&self, stall: Option<Duration>) -> Watch<'_> {
        Watch {
            pulse: &self.pulse,
            idle: self.retry.idle_timeout,
            stall,
            net: &self.net,
            net_at: self.net.current(),
        }
    }

    /// Một lượt giữ kết nối: tải tiếp đoạn bằng các yêu cầu nối nhau (mỗi yêu cầu không quá
    /// trần byte của task), dùng lại cùng kết nối keep-alive giữa các yêu cầu.
    async fn attempt(&self, slot: &GateSlot, stall: Option<Duration>) -> Attempt {
        let watch = self.watch(stall);
        let mut wrote = false;
        loop {
            let Some(next) = self.segment.next_offset() else {
                return Attempt::Complete;
            };
            let end = self.transport.request_end(next, self.segment.end());
            match self.request(next, end, slot, &watch).await {
                Attempt::RangeDone => {
                    wrote = true;
                    if self.cancel.is_cancelled() {
                        return Attempt::Cancelled;
                    }
                    if slot.yield_if_over() {
                        return Attempt::Yielded;
                    }
                }
                // Lượt này đã tải được một phần: như rớt giữa chừng, thử lại ngay không tính lượt.
                Attempt::Retry(failure) if wrote && !is_throttle(&failure) => {
                    return Attempt::Progress(failure);
                }
                other => return other,
            }
        }
    }

    async fn request(&self, next: u64, end: u64, slot: &GateSlot, watch: &Watch<'_>) -> Attempt {
        let range = format!("bytes={next}-{end}");
        let response = match send_within(&self.transport, &self.url, Some(&range), watch).await {
            Some(Ok(response)) => response,
            Some(Err(failure)) => return Attempt::Retry(failure),
            None => return Attempt::Stalled(stall_failure()),
        };
        let status = response.status();
        if status == StatusCode::PARTIAL_CONTENT {
            let range = response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|value| value.to_str().ok())
                .and_then(parse_content_range);
            let Some(range) = range else {
                return Attempt::Fatal(task_error(
                    ErrorClass::Permanent,
                    "http.content_range",
                    "The server sent an invalid byte range.",
                ));
            };
            if range.start != next || range.end < range.start {
                return Attempt::Fatal(task_error(
                    ErrorClass::Permanent,
                    "http.range_mismatch",
                    "The server returned a different byte range than requested.",
                ));
            }
            if range.total.is_some_and(|server_total| server_total != self.total) {
                return Attempt::Fatal(task_error(
                    ErrorClass::Permanent,
                    "http.size_mismatch",
                    "The file size changed on the server.",
                ));
            }
            if self
                .validators
                .differs(&Validators::from_headers(response.headers()))
            {
                return Attempt::Fatal(file_changed());
            }
            return self.stream(response, slot, end, watch).await;
        }
        if status.is_success() {
            return Attempt::Fatal(task_error(
                ErrorClass::Permanent,
                "http.range_ignored",
                "The server ignored the byte range request.",
            ));
        }
        Attempt::Retry(failure_from_status(status, response.headers()))
    }

    /// Nhận thân của yêu cầu đã xin tới byte `stop` (gồm).
    async fn stream(
        &self,
        mut response: Response,
        slot: &GateSlot,
        stop: u64,
        watch: &Watch<'_>,
    ) -> Attempt {
        let mut wrote = false;
        loop {
            let chunk = tokio::select! {
                _ = self.cancel.cancelled() => return Attempt::Cancelled,
                waited = watch.wait(response.chunk()) => match waited {
                    Waited::Idle => return retry_or_progress(idle_failure(), wrote),
                    Waited::Stalled => return Attempt::Stalled(stall_failure()),
                    Waited::Done(Err(error)) => {
                        return retry_or_progress(classify_error(&error), wrote);
                    }
                    Waited::Done(Ok(None)) => {
                        // Thân kết thúc đúng như server khai: hết phần đã xin, hoặc server trả
                        // một khúc ngắn hơn ⇒ xin tiếp từ chỗ đang dở, không phải lỗi mạng.
                        return match self.segment.next_offset() {
                            None => Attempt::Complete,
                            Some(next) if next > stop || wrote => Attempt::RangeDone,
                            Some(_) => Attempt::Retry(closed_failure()),
                        };
                    }
                    Waited::Done(Ok(Some(chunk))) => chunk,
                },
            };
            if chunk.is_empty() {
                continue;
            }
            match self.write(&chunk) {
                WriteResult::Complete => return Attempt::Complete,
                WriteResult::Progress => wrote = true,
                WriteResult::Disk(error) => return Attempt::Fatal(error),
            }
            // Bộ điều khiển vừa giảm số kết nối: đóng kết nối này, phần còn lại tải sau.
            if slot.yield_if_over() {
                return Attempt::Yielded;
            }
        }
    }

    fn write(&self, chunk: &[u8]) -> WriteResult {
        let mut state = self.segment.lock();
        let next = self.segment.start + state.downloaded;
        if next > state.end {
            return WriteResult::Complete;
        }
        let room = state.end - next + 1;
        let take = usize::try_from(room.min(chunk.len() as u64)).unwrap_or(chunk.len());
        if let Err(error) = self.file.write_at(next, &chunk[..take]) {
            return WriteResult::Disk(disk_failure(&error));
        }
        self.pulse.beat();
        state.downloaded += take as u64;
        state.status = SegmentStatus::Active;
        if self.segment.start + state.downloaded > state.end {
            state.status = SegmentStatus::Done;
            WriteResult::Complete
        } else {
            WriteResult::Progress
        }
    }
}

pub(crate) struct StreamJob {
    pub transport: Transport,
    pub url: Url,
    pub host: String,
    pub file: Arc<PartFile>,
    pub limiter: Arc<Limiter>,
    pub cancel: CancellationToken,
    pub retry: RetryPolicy,
    pub downloaded: Arc<AtomicU64>,
    pub total: Option<u64>,
    /// Server nhận Range: theo trần byte mỗi yêu cầu của task (nếu có).
    pub ranged: bool,
    pub pulse: Arc<Pulse>,
    pub validators: Arc<Validators>,
    pub net: Arc<NetEpoch>,
}

impl StreamJob {
    pub async fn run(mut self) -> Result<(), SegmentFailure> {
        let mut attempts = 0u32;
        let mut restarts = 0u32;
        loop {
            if self.cancel.is_cancelled() {
                return Ok(());
            }
            let position = self.downloaded.load(Ordering::Relaxed);
            if self.total.is_some_and(|total| position >= total) {
                return Ok(());
            }
            let Some(limit) = self.limiter.acquire(&self.host, &self.cancel).await else {
                return Ok(());
            };
            let stop = self.request_stop(position);
            let range = match stop {
                Some(stop) => Some(format!("bytes={position}-{stop}")),
                None if position > 0 => Some(format!("bytes={position}-")),
                None => None,
            };
            let sent = send_within(&self.transport, &self.url, range.as_deref(), &self.watch())
                .await
                .unwrap_or_else(|| Err(idle_failure()));
            let mut response = match sent {
                Ok(response) => response,
                Err(failure) => {
                    drop(limit);
                    attempts += 1;
                    if attempts >= self.retry.max_attempts || ends_the_run(&failure, true) {
                        return Err(SegmentFailure::from_request(failure));
                    }
                    let delay = retry_delay(&failure, attempts, 0);
                    tokio::select! {
                        _ = self.cancel.cancelled() => return Ok(()),
                        _ = sleep(delay) => {}
                    }
                    continue;
                }
            };
            let status = response.status();
            if status == StatusCode::RANGE_NOT_SATISFIABLE {
                // Có trần mà chưa biết tổng: file hết đúng ở mốc vừa xin.
                let at_end = self
                    .total
                    .map_or(stop.is_some() && position > 0, |total| position >= total);
                if at_end {
                    return Ok(());
                }
                return Err(SegmentFailure::new(task_error(
                        ErrorClass::Permanent,
                        "http.range_mismatch",
                        "The server rejected the resume offset.",
                    )));
            }
            let mut requested_stop = None;
            if status == StatusCode::PARTIAL_CONTENT {
                requested_stop = stop;
                let range = response
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|value| value.to_str().ok())
                    .and_then(parse_content_range);
                let Some(range) = range else {
                    return Err(SegmentFailure::new(task_error(
                            ErrorClass::Permanent,
                            "http.content_range",
                            "The server sent an invalid byte range.",
                        )));
                };
                if range.start != position || range.end < range.start {
                    return Err(SegmentFailure::new(task_error(
                            ErrorClass::Permanent,
                            "http.range_mismatch",
                            "The server returned a different byte range than requested.",
                        )));
                }
                match (self.total, range.total) {
                    (Some(total), Some(server_total)) if total != server_total => {
                        return Err(SegmentFailure::new(task_error(
                                ErrorClass::Permanent,
                                "http.size_mismatch",
                                "The file size changed on the server.",
                            )));
                    }
                    // Lúc dò chưa biết tổng mà phản hồi Range nói rõ: biết chỗ hết file.
                    (None, Some(server_total)) => self.total = Some(server_total),
                    _ => {}
                }
                if self
                    .validators
                    .differs(&Validators::from_headers(response.headers()))
                {
                    return Err(SegmentFailure::new(file_changed()));
                }
            } else if status.is_success() {
                if position > 0 {
                    restarts += 1;
                    if restarts > 3 {
                        return Err(SegmentFailure::new(task_error(
                                ErrorClass::Permanent,
                                "http.restart",
                                "The server keeps restarting the download from the beginning.",
                            )));
                    }
                    if let Err(error) = self.file.reset(self.total) {
                        return Err(SegmentFailure::new(disk_failure(&error)));
                    }
                    self.downloaded.store(0, Ordering::Relaxed);
                }
            } else {
                drop(limit);
                let failure = failure_from_status(status, response.headers());
                attempts += 1;
                if attempts >= self.retry.max_attempts || ends_the_run(&failure, true) {
                    return Err(SegmentFailure::from_request(failure));
                }
                let delay = retry_delay(&failure, attempts, 0);
                tokio::select! {
                    _ = self.cancel.cancelled() => return Ok(()),
                    _ = sleep(delay) => {}
                }
                continue;
            }

            let outcome = self.stream(&mut response, requested_stop).await;
            drop(limit);
            match outcome {
                Attempt::Complete | Attempt::Yielded => return Ok(()),
                Attempt::Cancelled => return Ok(()),
                Attempt::RangeDone | Attempt::Progress(_) => attempts = 0,
                Attempt::Retry(failure) | Attempt::Stalled(failure) => {
                    attempts += 1;
                    if attempts >= self.retry.max_attempts {
                        return Err(SegmentFailure::from_request(failure));
                    }
                    let delay = retry_delay(&failure, attempts, 0);
                    tokio::select! {
                        _ = self.cancel.cancelled() => return Ok(()),
                        _ = sleep(delay) => {}
                    }
                }
                Attempt::Fatal(error) => return Err(SegmentFailure::new(error)),
            }
        }
    }

    /// Chỉ một kết nối: không có kết nối khác để so, chờ đủ `idle_timeout` (mạng đổi thì mở lại ngay).
    fn watch(&self) -> Watch<'_> {
        Watch {
            pulse: &self.pulse,
            idle: self.retry.idle_timeout,
            stall: None,
            net: &self.net,
            net_at: self.net.current(),
        }
    }

    /// Byte cuối (gồm) cho yêu cầu kế tiếp khi task có trần byte và server nhận Range.
    fn request_stop(&self, position: u64) -> Option<u64> {
        let cap = self.transport.max_request().filter(|_| self.ranged)?;
        let stop = position.saturating_add(cap - 1);
        Some(self.total.map_or(stop, |total| stop.min(total.saturating_sub(1))))
    }

    /// Nhận thân; `stop` là byte cuối đã xin khi yêu cầu có trần.
    async fn stream(&self, response: &mut Response, stop: Option<u64>) -> Attempt {
        let watch = self.watch();
        let mut wrote = false;
        loop {
            let chunk = tokio::select! {
                _ = self.cancel.cancelled() => return Attempt::Cancelled,
                waited = watch.wait(response.chunk()) => match waited {
                    Waited::Idle | Waited::Stalled => {
                        return retry_or_progress(idle_failure(), wrote);
                    }
                    Waited::Done(Err(error)) => {
                        return retry_or_progress(classify_error(&error), wrote);
                    }
                    Waited::Done(Ok(None)) => return self.body_ended(stop, wrote),
                    Waited::Done(Ok(Some(chunk))) => chunk,
                },
            };
            if chunk.is_empty() {
                continue;
            }
            let position = self.downloaded.load(Ordering::Relaxed);
            let take = self
                .total
                .map_or(chunk.len() as u64, |total| total.saturating_sub(position))
                .min(chunk.len() as u64) as usize;
            if take == 0 {
                return Attempt::Complete;
            }
            if let Err(error) = self.file.write_at(position, &chunk[..take]) {
                return Attempt::Fatal(disk_failure(&error));
            }
            self.pulse.beat();
            self.downloaded
                .store(position + take as u64, Ordering::Relaxed);
            wrote = true;
        }
    }

    fn body_ended(&self, stop: Option<u64>, wrote: bool) -> Attempt {
        let position = self.downloaded.load(Ordering::Relaxed);
        let finished = match (self.total, stop) {
            (Some(total), _) => position >= total,
            // Chưa biết tổng: khúc ngắn hơn phần đã xin chưa chắc là hết file (CDN trả mỗi lần
            // một khúc) ⇒ xin tiếp; hết thật thì yêu cầu sau nhận 416 hoặc khúc rỗng.
            (None, Some(_)) => !wrote,
            (None, None) => true,
        };
        if finished {
            Attempt::Complete
        } else if stop.is_some_and(|stop| position > stop || wrote) {
            // Hết phần đã xin, hoặc server trả gọn một khúc ngắn hơn: xin tiếp.
            Attempt::RangeDone
        } else {
            retry_or_progress(closed_failure(), wrote)
        }
    }
}

enum Attempt {
    Complete,
    /// Xong phần của một yêu cầu có trần, đoạn/file còn tiếp: gửi yêu cầu kế trên cùng kết nối.
    RangeDone,
    /// Đã tải được một phần rồi kết nối hỏng: thử lại ngay, không tính lượt.
    Progress(RequestFailure),
    /// Kết nối im quá hạn treo trong khi kết nối khác vẫn nhận byte: đóng, mở lại, không tính lượt.
    Stalled(RequestFailure),
    /// Nhả kết nối vì đang thừa so với mục tiêu.
    Yielded,
    Retry(RequestFailure),
    Fatal(TaskError),
    Cancelled,
}

enum WriteResult {
    Complete,
    Progress,
    Disk(TaskError),
}

/// Hạn chờ byte của một kết nối.
struct Watch<'a> {
    pulse: &'a Pulse,
    idle: Duration,
    /// Có ⇒ im quá chừng này mà kết nối khác của lượt tải đang nhận byte (xem `stalled`) thì coi
    /// là treo (học Surge `health.go`, fast-down `multi.rs`).
    stall: Option<Duration>,
    /// Mạng đổi (Wi-Fi↔4G, máy ngủ dậy) từ lúc mở kết nối ⇒ kết nối đã chết lặng: mở lại ngay.
    net: &'a NetEpoch,
    net_at: u64,
}

enum Waited<T> {
    Done(T),
    /// Im quá `idle`.
    Idle,
    /// Im quá hạn treo trong khi kết nối khác vẫn nhận byte, hoặc mạng vừa đổi.
    Stalled,
}

impl Watch<'_> {
    async fn wait<T>(&self, work: impl Future<Output = T>) -> Waited<T> {
        let _waiting = self.pulse.waiting();
        tokio::pin!(work);
        let since = Instant::now();
        let idle_at = since + self.idle;
        let recheck = self.stall.map_or(NET_RECHECK, stall_recheck);
        // Nhịp hỏi đầu sớm hơn hạn treo một nhịp: tới hạn đã có đủ hai nhịp để xét.
        let first = self.stall.map_or(recheck, |stall| stall.saturating_sub(recheck));
        let mut check_at = (since + first).min(idle_at);
        let mut heard_from = since.max(check_at - recheck);
        let mut heard_before = false;
        loop {
            tokio::select! {
                biased;
                output = &mut work => return Waited::Done(output),
                _ = sleep(check_at.saturating_duration_since(Instant::now())) => {}
            }
            let now = Instant::now();
            if now >= idle_at {
                return Waited::Idle;
            }
            if self.net.current() != self.net_at {
                return Waited::Stalled;
            }
            if let Some(stall) = self.stall {
                let heard = self.pulse.heard_since(heard_from);
                if now >= since + stall && self.stalled(since, heard, heard_before) {
                    return Waited::Stalled;
                }
                (heard_from, heard_before) = (now, heard);
            }
            check_at = (now + recheck).min(idle_at);
        }
    }

    /// Đã im quá hạn treo: chỉ riêng kết nối này hỏng hay mạng rớt cả loạt. Còn kết nối khác đang
    /// mở thì chúng phải đang nhận byte (trong cả hai nhịp hỏi gần nhất): cả loạt cùng im là mạng
    /// rớt, chờ đủ `idle` như cũ; một kết nối chạy lại trước vài ms lúc mạng có lại cũng chưa tính.
    /// Chỉ còn mình mở: kết nối khác đã nhận byte sau lúc mình im (xong việc trong lúc mình treo).
    fn stalled(&self, since: Instant, heard: bool, heard_before: bool) -> bool {
        if self.pulse.others_waiting() {
            heard && heard_before
        } else {
            self.pulse.heard_since(since)
        }
    }
}

/// Nhịp hỏi của luật treo: nửa hạn treo, không quá `STALL_RECHECK`.
fn stall_recheck(stall: Duration) -> Duration {
    (stall / 2).clamp(Duration::from_millis(1), STALL_RECHECK)
}

/// Gửi GET, chờ đầu phản hồi trong hạn của `watch`: kết nối chết lặng (NAT rớt, server treo)
/// không giữ đoạn mãi; quá hạn thì báo như rớt mạng để thử lại, `None` khi bị coi là treo.
async fn send_within(
    transport: &Transport,
    url: &Url,
    range: Option<&str>,
    watch: &Watch<'_>,
) -> Option<Result<Response, RequestFailure>> {
    let sent = transport.send(Method::GET, url, range, None);
    match watch.wait(sent).await {
        Waited::Done(sent) => Some(sent.map_err(|error| classify_error(&error))),
        Waited::Idle => Some(Err(idle_failure())),
        Waited::Stalled => None,
    }
}

/// Chờ trước khi mở lại kết nối vừa cắt vì treo: ngắn, tăng dần để không xoáy.
fn stall_delay(stalls: u32) -> Duration {
    Duration::from_millis(200u64 << stalls.min(3))
}

fn retry_or_progress(failure: RequestFailure, wrote: bool) -> Attempt {
    if wrote {
        Attempt::Progress(failure)
    } else {
        Attempt::Retry(failure)
    }
}

fn retry_delay(failure: &RequestFailure, attempt: u32, seed: u32) -> Duration {
    if let Some(after) = failure.retry_after {
        return after.clamp(Duration::from_millis(100), MAX_INLINE_WAIT);
    }
    let exponent = attempt.saturating_sub(1).min(6);
    let base = 500u64.saturating_mul(1u64 << exponent).min(30_000);
    let jitter = 80
        + u64::from(
            seed.wrapping_mul(31)
                .wrapping_add(attempt.wrapping_mul(17))
                % 41,
        );
    Duration::from_millis(base * jitter / 100)
}

fn idle_failure() -> RequestFailure {
    RequestFailure {
        error: task_error(
            ErrorClass::Transient,
            "network.idle",
            "The server stopped sending data.",
        ),
        retry_after: None,
    }
}

fn stall_failure() -> RequestFailure {
    RequestFailure {
        error: task_error(
            ErrorClass::Transient,
            "network.stalled",
            "The connection stopped sending data while others kept going.",
        ),
        retry_after: None,
    }
}

fn closed_failure() -> RequestFailure {
    RequestFailure {
        error: task_error(
            ErrorClass::Transient,
            "network.closed",
            "The connection closed before the file finished.",
        ),
        retry_after: None,
    }
}

fn file_changed() -> TaskError {
    task_error(
        ErrorClass::Permanent,
        "file.changed",
        "The file changed on the server since the download started.",
    )
}

fn task_error(class: ErrorClass, code: &str, message: &str) -> TaskError {
    TaskError {
        class,
        code: code.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> SegmentRuntime {
        SegmentRuntime::new(&Segment {
            index: 0,
            start: 0,
            end: 64 * crate::model::MIN_SEGMENT_BYTES - 1,
            downloaded: crate::model::MIN_SEGMENT_BYTES,
            status: SegmentStatus::Active,
        })
    }

    #[test]
    fn split_keeps_a_gap_and_shrinks_the_original_tail() {
        let runtime = runtime();
        let (tail_start, tail_end) = runtime
            .split_tail(8 * crate::model::MIN_SEGMENT_BYTES)
            .expect("split");
        assert_eq!(tail_end, 64 * crate::model::MIN_SEGMENT_BYTES - 1);
        assert_eq!(runtime.end(), tail_start - 1);
        assert!(tail_start > runtime.start() + runtime.downloaded());
    }

    #[test]
    fn split_refuses_when_the_tail_is_too_small() {
        let runtime = SegmentRuntime::new(&Segment {
            index: 0,
            start: 0,
            end: 4 * crate::model::MIN_SEGMENT_BYTES - 1,
            downloaded: 0,
            status: SegmentStatus::Active,
        });
        assert!(
            runtime
                .split_tail(8 * crate::model::MIN_SEGMENT_BYTES)
                .is_none()
        );
    }

    #[test]
    fn stall_window_doubles_then_leaves_only_the_idle_timeout() {
        let policy = RetryPolicy {
            max_attempts: 8,
            idle_timeout: Duration::from_secs(30),
            stall_timeout: Duration::from_secs(5),
        };
        let windows: Vec<_> = (0..4).map(|stalls| policy.stall_window(stalls)).collect();
        let seconds = |value| Some(Duration::from_secs(value));
        assert_eq!(windows, vec![seconds(5), seconds(10), seconds(20), None]);
        assert!(policy.stall_window(u32::MAX).is_none());
    }

    static NET: NetEpoch = NetEpoch::new();

    fn watch(pulse: &Pulse, idle: Duration) -> Watch<'_> {
        Watch {
            pulse,
            idle,
            stall: Some(Duration::from_millis(200)),
            net: &NET,
            net_at: NET.current(),
        }
    }

    /// Chờ im (không bao giờ có byte) cạnh một kết nối khác chạy `other`.
    async fn silent_beside(watch: &Watch<'_>, other: impl Future<Output = ()>) -> Waited<()> {
        tokio::select! {
            waited = watch.wait(std::future::pending::<()>()) => waited,
            _ = other => unreachable!("the other connection never ends"),
        }
    }

    #[tokio::test]
    async fn waits_the_idle_timeout_when_no_other_connection_is_heard() {
        let pulse = Pulse::new("task", None);
        let watch = watch(&pulse, Duration::from_millis(600));
        let started = Instant::now();
        let waited = watch.wait(std::future::pending::<()>()).await;
        assert!(matches!(waited, Waited::Idle));
        assert!(started.elapsed() >= Duration::from_millis(600));
    }

    #[tokio::test]
    async fn cuts_a_silent_wait_once_the_others_finished_without_it() {
        // Kết nối khác nhận byte sau lúc kết nối này im rồi xong việc, đóng: mạng vẫn chạy.
        let pulse = Pulse::new("task", None);
        let watch = watch(&pulse, Duration::from_secs(30));
        let other = async {
            sleep(Duration::from_millis(20)).await;
            pulse.beat();
            std::future::pending::<()>().await
        };
        let started = Instant::now();
        assert!(matches!(silent_beside(&watch, other).await, Waited::Stalled));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn cuts_a_silent_wait_while_another_open_connection_keeps_receiving() {
        let pulse = Pulse::new("task", None);
        let watch = watch(&pulse, Duration::from_secs(30));
        let other = async {
            let _open = pulse.waiting();
            loop {
                sleep(Duration::from_millis(10)).await;
                pulse.beat();
            }
        };
        let started = Instant::now();
        assert!(matches!(silent_beside(&watch, other).await, Waited::Stalled));
        let elapsed = started.elapsed();
        assert!(elapsed >= Duration::from_millis(200), "cut after {elapsed:?}");
        assert!(elapsed < Duration::from_secs(5), "cut after {elapsed:?}");
    }

    #[tokio::test]
    async fn waits_the_idle_timeout_when_every_open_connection_goes_quiet() {
        // Mạng rớt cả loạt: kết nối khác vẫn mở, nhận khúc cuối ngay sau lúc kết nối này im rồi
        // cũng im. Không phải treo riêng một kết nối ⇒ chờ đủ idle, không cắt.
        let pulse = Pulse::new("task", None);
        let watch = watch(&pulse, Duration::from_millis(1200));
        let other = async {
            let _open = pulse.waiting();
            sleep(Duration::from_millis(20)).await;
            pulse.beat();
            std::future::pending::<()>().await
        };
        let started = Instant::now();
        assert!(matches!(silent_beside(&watch, other).await, Waited::Idle));
        assert!(started.elapsed() >= Duration::from_millis(1200));
    }

    #[test]
    fn stall_recheck_is_half_the_window_at_most_the_recheck() {
        assert_eq!(stall_recheck(Duration::from_secs(5)), STALL_RECHECK);
        assert_eq!(
            stall_recheck(Duration::from_millis(200)),
            Duration::from_millis(100)
        );
        assert_eq!(
            stall_recheck(Duration::from_nanos(1)),
            Duration::from_millis(1)
        );
    }
}
