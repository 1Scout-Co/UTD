use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use reqwest::header::{ACCEPT_ENCODING, CONTENT_RANGE, RANGE};
use reqwest::{Client, Response, StatusCode};
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;
use url::Url;

use super::adaptive::{is_throttle, ConnectionGate, GateSlot};
use super::file::PartFile;
use super::http::{classify_error, failure_from_status, parse_content_range, RequestFailure};
use super::limiter::Limiter;
use crate::model::{ErrorClass, Segment, SegmentStatus, TaskError};

#[derive(Debug, Clone, Copy)]
pub(crate) struct RetryPolicy {
    pub max_attempts: u32,
    pub idle_timeout: Duration,
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
    pub client: Client,
    pub total: u64,
    pub url: Url,
    pub host: String,
    pub segment: Arc<SegmentRuntime>,
    pub file: Arc<PartFile>,
    pub limiter: Arc<Limiter>,
    pub gate: Arc<ConnectionGate>,
    pub cancel: CancellationToken,
    pub retry: RetryPolicy,
}

pub(crate) struct SegmentFailure {
    pub error: TaskError,
}

pub(crate) async fn run_segment(job: SegmentJob) -> Result<(), SegmentFailure> {
    let mut attempts = 0u32;
    loop {
        if job.cancel.is_cancelled() {
            return Ok(());
        }
        if job.segment.is_done() {
            job.segment.mark_status(SegmentStatus::Done);
            return Ok(());
        }
        let Some(slot) = job.gate.enter(&job.cancel).await else {
            return Ok(());
        };
        let Some(next) = job.segment.next_offset() else {
            job.segment.mark_status(SegmentStatus::Done);
            return Ok(());
        };
        let end = job.segment.end();
        let Some(limit) = job.limiter.acquire(&job.host, &job.cancel).await else {
            return Ok(());
        };
        // Trong lúc chờ hàng host/toàn cục, mục tiêu có thể đã giảm.
        if slot.yield_if_over() {
            continue;
        }
        let live = job.gate.connect();
        let outcome = job.attempt(next, end, &slot).await;
        drop(live);
        drop(limit);
        drop(slot);
        match outcome {
            Attempt::Complete | Attempt::Yielded => attempts = 0,
            Attempt::Progress(failure) => {
                job.gate.report(&failure);
                attempts = 0;
            }
            Attempt::Cancelled => return Ok(()),
            Attempt::Retry(failure) => {
                job.gate.report(&failure);
                // Server chê nhiều kết nối mà các kết nối khác vẫn chạy: chỉ cần bớt, không phải hỏng.
                if !(is_throttle(&failure) && job.gate.live() > 0) {
                    attempts += 1;
                }
                if attempts >= job.retry.max_attempts {
                    job.segment.mark_status(SegmentStatus::Failed);
                    return Err(SegmentFailure { error: failure.error });
                }
                let delay = retry_delay(&failure, attempts.max(1), job.segment.index());
                tokio::select! {
                    _ = job.cancel.cancelled() => return Ok(()),
                    _ = sleep(delay) => {}
                }
            }
            Attempt::Fatal(error) => {
                job.segment.mark_status(SegmentStatus::Failed);
                return Err(SegmentFailure { error });
            }
        }
    }
}

impl SegmentJob {
    async fn attempt(&self, next: u64, end: u64, slot: &GateSlot) -> Attempt {
        let request = self
            .client
            .get(self.url.clone())
            .header(ACCEPT_ENCODING, "identity")
            .header(RANGE, format!("bytes={next}-{end}"));
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => return Attempt::Retry(classify_error(&error)),
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
            return self.stream(response, slot).await;
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

    async fn stream(&self, mut response: Response, slot: &GateSlot) -> Attempt {
        let mut wrote = false;
        loop {
            let chunk = tokio::select! {
                _ = self.cancel.cancelled() => return Attempt::Cancelled,
                result = timeout(self.retry.idle_timeout, response.chunk()) => match result {
                    Err(_) => return retry_or_progress(idle_failure(), wrote),
                    Ok(Err(error)) => return retry_or_progress(classify_error(&error), wrote),
                    Ok(Ok(None)) => {
                        return if self.segment.is_done() {
                            Attempt::Complete
                        } else {
                            retry_or_progress(closed_failure(), wrote)
                        };
                    }
                    Ok(Ok(Some(chunk))) => chunk,
                },
            };
            if chunk.is_empty() {
                continue;
            }
            match self.write(&chunk) {
                WriteResult::Complete => return Attempt::Complete,
                WriteResult::Progress => wrote = true,
                WriteResult::Disk => return Attempt::Fatal(disk_error()),
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
        if self.file.write_at(next, &chunk[..take]).is_err() {
            return WriteResult::Disk;
        }
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
    pub client: Client,
    pub url: Url,
    pub host: String,
    pub file: Arc<PartFile>,
    pub limiter: Arc<Limiter>,
    pub cancel: CancellationToken,
    pub retry: RetryPolicy,
    pub downloaded: Arc<AtomicU64>,
    pub total: Option<u64>,
}

impl StreamJob {
    pub async fn run(self) -> Result<(), SegmentFailure> {
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
            let mut request = self
                .client
                .get(self.url.clone())
                .header(ACCEPT_ENCODING, "identity");
            if position > 0 {
                request = request.header(RANGE, format!("bytes={position}-"));
            }
            let mut response = match request.send().await {
                Ok(response) => response,
                Err(error) => {
                    drop(limit);
                    let failure = classify_error(&error);
                    attempts += 1;
                    if attempts >= self.retry.max_attempts {
                        return Err(SegmentFailure { error: failure.error });
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
                if self.total.is_some_and(|total| position >= total) {
                    return Ok(());
                }
                return Err(SegmentFailure {
                    error: task_error(
                        ErrorClass::Permanent,
                        "http.range_mismatch",
                        "The server rejected the resume offset.",
                    ),
                });
            }
            if status == StatusCode::PARTIAL_CONTENT {
                let range = response
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|value| value.to_str().ok())
                    .and_then(parse_content_range);
                let Some(range) = range else {
                    return Err(SegmentFailure {
                        error: task_error(
                            ErrorClass::Permanent,
                            "http.content_range",
                            "The server sent an invalid byte range.",
                        ),
                    });
                };
                if range.start != position || range.end < range.start {
                    return Err(SegmentFailure {
                        error: task_error(
                            ErrorClass::Permanent,
                            "http.range_mismatch",
                            "The server returned a different byte range than requested.",
                        ),
                    });
                }
                if let (Some(total), Some(server_total)) = (self.total, range.total)
                    && total != server_total
                {
                    return Err(SegmentFailure {
                        error: task_error(
                            ErrorClass::Permanent,
                            "http.size_mismatch",
                            "The file size changed on the server.",
                        ),
                    });
                }
            } else if status.is_success() {
                if position > 0 {
                    restarts += 1;
                    if restarts > 3 {
                        return Err(SegmentFailure {
                            error: task_error(
                                ErrorClass::Permanent,
                                "http.restart",
                                "The server keeps restarting the download from the beginning.",
                            ),
                        });
                    }
                    if let Err(_error) = self.file.reset(self.total) {
                        return Err(SegmentFailure { error: disk_error() });
                    }
                    self.downloaded.store(0, Ordering::Relaxed);
                }
            } else {
                drop(limit);
                let failure = failure_from_status(status, response.headers());
                attempts += 1;
                if attempts >= self.retry.max_attempts {
                    return Err(SegmentFailure { error: failure.error });
                }
                let delay = retry_delay(&failure, attempts, 0);
                tokio::select! {
                    _ = self.cancel.cancelled() => return Ok(()),
                    _ = sleep(delay) => {}
                }
                continue;
            }

            let outcome = self.stream(&mut response).await;
            drop(limit);
            match outcome {
                Attempt::Complete | Attempt::Yielded => return Ok(()),
                Attempt::Cancelled => return Ok(()),
                Attempt::Progress(_) => attempts = 0,
                Attempt::Retry(failure) => {
                    attempts += 1;
                    if attempts >= self.retry.max_attempts {
                        return Err(SegmentFailure { error: failure.error });
                    }
                    let delay = retry_delay(&failure, attempts, 0);
                    tokio::select! {
                        _ = self.cancel.cancelled() => return Ok(()),
                        _ = sleep(delay) => {}
                    }
                }
                Attempt::Fatal(error) => return Err(SegmentFailure { error }),
            }
        }
    }

    async fn stream(&self, response: &mut Response) -> Attempt {
        let mut wrote = false;
        loop {
            let chunk = tokio::select! {
                _ = self.cancel.cancelled() => return Attempt::Cancelled,
                result = timeout(self.retry.idle_timeout, response.chunk()) => match result {
                    Err(_) => return retry_or_progress(idle_failure(), wrote),
                    Ok(Err(error)) => return retry_or_progress(classify_error(&error), wrote),
                    Ok(Ok(None)) => {
                        let position = self.downloaded.load(Ordering::Relaxed);
                        return if self.total.is_some_and(|total| position < total) {
                            retry_or_progress(closed_failure(), wrote)
                        } else {
                            Attempt::Complete
                        };
                    }
                    Ok(Ok(Some(chunk))) => chunk,
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
            if self.file.write_at(position, &chunk[..take]).is_err() {
                return Attempt::Fatal(disk_error());
            }
            self.downloaded
                .store(position + take as u64, Ordering::Relaxed);
            wrote = true;
        }
    }
}

enum Attempt {
    Complete,
    /// Đã tải được một phần rồi kết nối hỏng: thử lại ngay, không tính lượt.
    Progress(RequestFailure),
    /// Nhả kết nối vì đang thừa so với mục tiêu.
    Yielded,
    Retry(RequestFailure),
    Fatal(TaskError),
    Cancelled,
}

enum WriteResult {
    Complete,
    Progress,
    Disk,
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
        return after.max(Duration::from_millis(100));
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

fn disk_error() -> TaskError {
    task_error(
        ErrorClass::Permanent,
        "disk.write",
        "Could not write to the download file.",
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
}
