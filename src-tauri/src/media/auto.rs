//! Công cụ media tự lo, để người không rành công nghệ cài UTD xong là tải được video:
//! - mở app mà thiếu yt-dlp/deno/ffmpeg ⇒ tải nền, không hỏi; lỗi (mất mạng…) thì thử lại giãn dần;
//! - yt-dlp của UTD tự kiểm bản mới tối đa mỗi ngày một lần (deno mỗi 30 ngày);
//! - task media gặp lúc công cụ đang tải thì chờ, không báo lỗi;
//! - yt-dlp lỗi kiểu extractor/403 ⇒ kiểm bản mới, có thì cài rồi chạy lại một lần.
//!
//! Tắt được trong Cài đặt (mặc định bật). Không sửa bản người dùng tự cài (PATH, WinGet…): bản đó
//! được dùng như cũ; chỉ khi nó cũ hơn bản mới nhất và vừa lỗi thì UTD tải bản riêng vào `<data>\bin`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Notify, watch};
use tokio_util::sync::CancellationToken;
use utd_engine::{Journal, now_ms};

use crate::events::TOOLS_AUTO_EVENT;
use crate::media::install::{self, InstallError, Job, Outcome};
use crate::media::sources::{self, Platform, Tool};
use crate::media::tools;

/// Khoá trong bảng settings: "off" = tắt tự tải/cập nhật; không có/khác = bật.
const SETTING_KEY: &str = "media_tools_auto";
/// Giờ (ms) của lần kiểm bản mới gần nhất thành công.
const CHECKED_KEY: &str = "media_tools_checked";
const DENO_CHECKED_KEY: &str = "media_deno_checked";
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const DENO_EVERY_MS: i64 = 30 * DAY_MS;
/// Chờ trước lần thử lại thứ n sau lỗi (giây); hết bảng thì giữ mức cuối.
const BACKOFF_SECS: [u64; 7] = [5, 15, 30, 60, 120, 300, 600];
/// Kiểm bản mới sau khi mở app một lúc (không tranh mạng lúc khởi động), rồi mỗi giờ xem tới hạn chưa.
const UPDATE_FIRST: Duration = Duration::from_secs(60);
const UPDATE_TICK: Duration = Duration::from_secs(60 * 60);
/// yt-dlp lỗi kiểu extractor: kiểm bản mới tối đa mỗi 30 phút (nhiều task lỗi cùng lúc chỉ kiểm một lần).
const FAILURE_GAP: Duration = Duration::from_secs(30 * 60);
/// Task tải chờ yt-dlp qua tối đa chừng này lượt tự tải (~9 phút theo bảng chờ ở trên) rồi mới báo lỗi.
pub(crate) const TASK_PASSES: u64 = 6;
/// Đang có lần cài khác (người dùng bấm trong Cài đặt): chờ rồi thử lại, tối đa ~10 phút.
const BUSY_WAIT: Duration = Duration::from_secs(2);
const BUSY_TRIES: u32 = 300;

/// Trạng thái cho giao diện (sự kiện `utd://tools-auto`).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoState {
    pub enabled: bool,
    /// Đang dò công cụ lúc mở app (chưa biết thiếu gì).
    pub checking: bool,
    /// Công cụ còn phải tải, theo thứ tự.
    pub pending: Vec<Tool>,
    /// Đang tải thật (không phải đang chờ thử lại).
    pub active: Option<Tool>,
    /// Đang cập nhật nền (đã có bản dùng được, không chặn gì).
    pub updating: Option<Tool>,
    /// Công cụ mà lượt tải vừa rồi không tải được.
    pub failed: Vec<Tool>,
    /// Câu lỗi của công cụ đầu tiên trong `failed`; `retry_at` = giờ thử lại (ms).
    pub error: Option<String>,
    pub retry_at: Option<i64>,
    /// Số lượt tải đã xong (để người chờ biết đã có lượt thử mới sau lúc mình bắt đầu chờ).
    pub rounds: u64,
    /// Người dùng bấm Huỷ: thôi tự tải tới khi cần lại (bấm tải media).
    pub paused: bool,
}

/// Kết quả chờ công cụ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Wait {
    /// Không còn gì phải chờ (có thể vẫn thiếu nếu tự tải đang tắt hoặc công cụ phụ lỗi).
    Ready,
    Cancelled,
    /// Công cụ bắt buộc vẫn không tải được sau số lượt cho phép: câu lỗi ngắn.
    Failed(String),
}

pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Phần chạm máy thật (dò công cụ, tải): bài thử thay bằng bản giả hoặc server giả.
pub(crate) trait Toolbox: Send + Sync {
    /// Công cụ còn thiếu, theo thứ tự nên tải.
    fn missing(&self) -> BoxFuture<'_, Vec<Tool>>;
    /// Phiên bản đang dùng và có phải bản của UTD (`<data>\bin`) không.
    fn active(&self, tool: Tool) -> BoxFuture<'_, Option<(String, bool)>>;
    /// Cài bản mới nhất; `current` = bản đang dùng (đã mới nhất thì `Outcome::Current`).
    fn install(
        &self,
        tool: Tool,
        current: Option<String>,
    ) -> BoxFuture<'_, Result<Outcome, InstallError>>;
}

type EmitState = Box<dyn Fn(&AutoState) + Send + Sync>;

struct Inner {
    state: watch::Sender<AutoState>,
    kick: Notify,
    toolbox: Box<dyn Toolbox>,
    journal: Arc<Journal>,
    emit: Option<EmitState>,
    /// Giờ lần kiểm bản mới vì lỗi gần nhất; khoá để nhiều task lỗi cùng lúc chỉ kiểm một lần.
    failure_check: tokio::sync::Mutex<Option<Instant>>,
    backoff: Vec<Duration>,
}

#[derive(Clone)]
pub struct Auto {
    inner: Arc<Inner>,
}

impl Auto {
    pub(crate) fn new(
        toolbox: Box<dyn Toolbox>,
        journal: Arc<Journal>,
        emit: Option<EmitState>,
    ) -> Self {
        let enabled = journal
            .get_setting(SETTING_KEY)
            .ok()
            .flatten()
            .is_none_or(|value| value != "off");
        let state = AutoState {
            enabled,
            checking: enabled,
            ..AutoState::default()
        };
        let backoff = BACKOFF_SECS
            .iter()
            .map(|secs| Duration::from_secs(*secs))
            .collect();
        Self {
            inner: Arc::new(Inner {
                state: watch::Sender::new(state),
                kick: Notify::new(),
                toolbox,
                journal,
                emit,
                failure_check: tokio::sync::Mutex::new(None),
                backoff,
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_backoff(self, backoff: Vec<Duration>) -> Self {
        let inner = Arc::into_inner(self.inner).expect("chưa chia sẻ");
        Self {
            inner: Arc::new(Inner { backoff, ..inner }),
        }
    }

    pub fn state(&self) -> AutoState {
        self.inner.state.borrow().clone()
    }

    fn update(&self, change: impl FnOnce(&mut AutoState)) {
        let changed = self.inner.state.send_if_modified(|state| {
            let before = state.clone();
            change(state);
            *state != before
        });
        if changed && let Some(emit) = &self.inner.emit {
            emit(&self.state());
        }
    }

    /// Bật/tắt (công tắc trong Cài đặt). Tắt giữa lúc đang tự tải ⇒ dừng lần tải đó.
    pub fn set_enabled(&self, enabled: bool) -> Result<AutoState, String> {
        self.inner
            .journal
            .set_setting(SETTING_KEY, if enabled { "on" } else { "off" })
            .map_err(|_| "Không lưu được cài đặt.".to_string())?;
        let stop = !enabled && self.state().active.is_some();
        self.update(|state| {
            state.enabled = enabled;
            state.paused = false;
            if !enabled {
                state.error = None;
                state.retry_at = None;
                state.failed.clear();
            }
        });
        if stop {
            install::cancel();
        }
        self.inner.kick.notify_one();
        Ok(self.state())
    }

    /// Có việc cần công cụ: bỏ thời gian chờ thử lại, bỏ tạm dừng (người dùng từng bấm Huỷ).
    pub(crate) fn kick(&self) {
        self.update(|state| state.paused = false);
        self.inner.kick.notify_one();
    }

    /// Công cụ sắp được tự tải (đang tải hoặc chờ lượt, lần thử gần nhất chưa lỗi).
    pub(crate) fn will_install(&self, tool: Tool) -> bool {
        let state = self.state();
        state.enabled
            && !state.paused
            && state.pending.contains(&tool)
            && !state.failed.contains(&tool)
    }

    /// Hàng đợi chung chưa chạy task media lúc công cụ nó cần còn đang tự tải: task vẫn "Đang chờ lượt"
    /// mà không giữ chỗ chạy (file HTTP phía sau chạy trước). Như `wait_for` của runner: công cụ bắt buộc
    /// (yt-dlp; ffmpeg khi `audio`) chờ qua cả các lần thử lại sau lỗi, công cụ phụ (deno; ffmpeg khi tải
    /// video) chỉ chờ lượt đang tải chưa lỗi. Tự tải tắt hoặc người dùng bấm Huỷ ⇒ không chờ.
    pub(crate) fn holds_media(&self, audio: bool) -> bool {
        let state = self.state();
        if !state.enabled || state.paused {
            return false;
        }
        let essential = |tool: Tool| tool == Tool::YtDlp || (audio && tool == Tool::Ffmpeg);
        state.checking
            || state
                .pending
                .iter()
                .any(|tool| essential(*tool) || !state.failed.contains(tool))
    }

    /// Theo dõi trạng thái tự tải (hàng đợi xem lại task media đang chờ công cụ khi trạng thái đổi).
    pub(crate) fn subscribe(&self) -> watch::Receiver<AutoState> {
        self.inner.state.subscribe()
    }

    /// Chờ công cụ đang/sắp được tự tải. `essential` (yt-dlp, ffmpeg khi tách âm thanh): chờ qua tối
    /// đa `passes` lượt tải xong kể từ lúc gọi, vẫn thiếu thì `Failed`. `optional` (deno, ffmpeg):
    /// chỉ chờ một lượt; lỗi thì chạy tiếp không có nó. Người dùng đang đợi (Phân tích, liệt kê kênh)
    /// nên lần tự tải đã bị Huỷ được bật lại.
    pub(crate) async fn wait_for(
        &self,
        essential: &[Tool],
        optional: &[Tool],
        passes: u64,
        cancel: &CancellationToken,
    ) -> Wait {
        self.wait(essential, optional, passes, cancel, true).await
    }

    /// Như `wait_for`, cho task hàng đợi vừa chạy: lần tự tải đã bị Huỷ thì thôi chờ ngay, không bật lại
    /// (hàng đợi thả task media ra chính vì người dùng bấm Huỷ; thêm hay tiếp tục task media mới bật lại).
    pub(crate) async fn wait_for_task(
        &self,
        essential: &[Tool],
        optional: &[Tool],
        passes: u64,
        cancel: &CancellationToken,
    ) -> Wait {
        self.wait(essential, optional, passes, cancel, false).await
    }

    async fn wait(
        &self,
        essential: &[Tool],
        optional: &[Tool],
        passes: u64,
        cancel: &CancellationToken,
        revive: bool,
    ) -> Wait {
        let mut receiver = self.inner.state.subscribe();
        let mut start: Option<u64> = None;
        loop {
            let state = receiver.borrow_and_update().clone();
            let done = start.map_or(0, |start| state.rounds.saturating_sub(start));
            let needed = state.pending.iter().any(|tool| essential.contains(tool));
            let wanted = done == 0 && state.pending.iter().any(|tool| optional.contains(tool));
            if !state.enabled || (!state.checking && !needed && !wanted) {
                return Wait::Ready;
            }
            if needed && !state.checking && done >= passes.max(1) {
                return Wait::Failed(state.error.unwrap_or_default());
            }
            // Người dùng bấm Huỷ (Cài đặt) sau lúc mình nhờ tải, hoặc trước đó mà mình không được bật lại.
            if state.paused && (start.is_some() || !revive) {
                return if needed {
                    Wait::Failed("Đã huỷ tải công cụ.".into())
                } else {
                    Wait::Ready
                };
            }
            if start.is_none() {
                start = Some(state.rounds);
                self.kick();
                continue;
            }
            tokio::select! {
                changed = receiver.changed() => if changed.is_err() { return Wait::Ready },
                _ = cancel.cancelled() => return Wait::Cancelled,
            }
        }
    }

    /// Câu cho người dùng khi vẫn thiếu `tool` sau lúc chờ; tự tải tắt hoặc đã bị Huỷ (UTD không tự thử
    /// lại) thì giữ câu `fallback`.
    pub(crate) fn missing_message(&self, tool: Tool, fallback: String) -> String {
        let state = self.state();
        if !state.enabled || state.paused || !state.pending.contains(&tool) {
            return fallback;
        }
        match state.error {
            Some(reason) => format!("Chưa tải được {}: {reason} UTD sẽ tự thử lại.", tool.name()),
            None => format!("Chưa tải xong {}. UTD sẽ tự thử lại.", tool.name()),
        }
    }

    async fn pause_for(&self, delay: Duration) {
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = self.inner.kick.notified() => {}
        }
    }

    /// Cài, chờ nếu đang có lần cài khác (người dùng bấm trong Cài đặt).
    async fn install_when_free(
        &self,
        tool: Tool,
        current: Option<String>,
    ) -> Result<Outcome, InstallError> {
        let mut tries = 0;
        loop {
            match self.inner.toolbox.install(tool, current.clone()).await {
                Err(InstallError::Busy(_)) if tries < BUSY_TRIES => {
                    tries += 1;
                    tokio::time::sleep(BUSY_WAIT).await;
                }
                result => return result,
            }
        }
    }

    /// Vòng tự tải: chạy suốt đời app. Mỗi lượt thử lần lượt mọi công cụ còn thiếu (một cái lỗi không
    /// chặn cái sau), lượt có lỗi thì chờ giãn dần rồi thử lại; không thiếu gì thì ngủ tới khi có việc.
    pub(crate) async fn bootstrap(&self) {
        let mut failures = 0usize;
        loop {
            let state = self.state();
            if !state.enabled || state.paused {
                self.update(|state| state.checking = false);
                self.inner.kick.notified().await;
                continue;
            }
            let missing = self.inner.toolbox.missing().await;
            self.update(|state| {
                state.checking = false;
                state.pending.clone_from(&missing);
            });
            if missing.is_empty() {
                failures = 0;
                self.inner.kick.notified().await;
                continue;
            }
            let Some(errors) = self.pass(&missing).await else {
                continue;
            };
            // Dò lại: cài "xong" mà vẫn không thấy (vd không chạy được) cũng tính là lỗi.
            let still = self.inner.toolbox.missing().await;
            let message = still.first().map(|tool| {
                errors
                    .iter()
                    .find(|(failed, _)| failed == tool)
                    .map(|(_, message)| message.clone())
                    .unwrap_or_else(|| format!("Đã tải {} nhưng không chạy được.", tool.name()))
            });
            let delay = message.as_ref().map(|_| {
                let delay = self.inner.backoff[failures.min(self.inner.backoff.len() - 1)];
                failures += 1;
                delay
            });
            self.update(|state| {
                state.pending.clone_from(&still);
                state.failed.clone_from(&still);
                state.error = message;
                state.retry_at =
                    delay.map(|delay| now_ms() + i64::try_from(delay.as_millis()).unwrap_or(0));
                state.rounds += 1;
            });
            match delay {
                Some(delay) => {
                    log::info!("Auto tools: còn thiếu {still:?}, thử lại sau {delay:?}");
                    self.pause_for(delay).await;
                }
                None => failures = 0,
            }
        }
    }

    /// Một lượt tải mọi công cụ còn thiếu. `None` = người dùng huỷ hoặc tắt tự tải giữa chừng.
    async fn pass(&self, missing: &[Tool]) -> Option<Vec<(Tool, String)>> {
        let mut errors = Vec::new();
        for &tool in missing {
            let state = self.state();
            if !state.enabled || state.paused {
                return None;
            }
            self.update(|state| state.active = Some(tool));
            let result = self.install_when_free(tool, None).await;
            self.update(|state| {
                state.active = None;
                if result.is_ok() {
                    state.pending.retain(|pending| *pending != tool);
                    state.failed.retain(|failed| *failed != tool);
                }
            });
            match result {
                Ok(_) => {}
                Err(InstallError::Cancelled(_)) => {
                    self.update(|state| state.paused = true);
                    return None;
                }
                Err(InstallError::Busy(message) | InstallError::Failed(message)) => {
                    errors.push((tool, message));
                }
            }
        }
        Some(errors)
    }

    /// Kiểm bản mới định kỳ (yt-dlp mỗi ngày, deno mỗi 30 ngày), chỉ cho bản của UTD.
    pub(crate) async fn update_loop(&self) {
        tokio::time::sleep(UPDATE_FIRST).await;
        loop {
            self.check_updates().await;
            tokio::time::sleep(UPDATE_TICK).await;
        }
    }

    pub(crate) async fn check_updates(&self) {
        for (tool, key, every) in [
            (Tool::YtDlp, CHECKED_KEY, DAY_MS),
            (Tool::Deno, DENO_CHECKED_KEY, DENO_EVERY_MS),
        ] {
            let state = self.state();
            // Công cụ khác còn lỗi (vd ffmpeg thiếu chỗ) không chặn cập nhật yt-dlp.
            let idle = state.active.is_none() && !state.checking;
            if !state.enabled || !idle || !self.due(key, every) {
                continue;
            }
            // Bản người dùng tự cài: không đụng, họ tự cập nhật.
            let Some((version, true)) = self.inner.toolbox.active(tool).await else {
                continue;
            };
            self.update(|state| state.updating = Some(tool));
            let result = self.install_when_free(tool, Some(version)).await;
            self.update(|state| state.updating = None);
            match result {
                Ok(_) => self.mark(key),
                Err(error) => log::warn!("Auto update {} failed: {error:?}", tool.name()),
            }
        }
    }

    fn due(&self, key: &str, every: i64) -> bool {
        let last = self
            .inner
            .journal
            .get_setting(key)
            .ok()
            .flatten()
            .and_then(|value| value.parse::<i64>().ok());
        last.is_none_or(|last| now_ms().saturating_sub(last) >= every)
    }

    fn mark(&self, key: &str) {
        let _ = self.inner.journal.set_setting(key, &now_ms().to_string());
    }

    /// yt-dlp (bản `used`) vừa lỗi kiểu extractor/403: kiểm bản mới, có thì cài. `true` = chạy lại.
    /// Bản người dùng tự cài cũ hơn bản mới nhất ⇒ tải bản riêng vào `<data>\bin` (bản của họ giữ
    /// nguyên; từ đó UTD dùng bản của mình vì bản trong `<data>\bin` luôn được ưu tiên).
    pub(crate) async fn refresh_after_failure(&self, used: &str) -> bool {
        if !self.state().enabled {
            return false;
        }
        let mut last = self.inner.failure_check.lock().await;
        // Task khác vừa cập nhật trong lúc chờ khoá: chạy lại bằng bản mới.
        if let Some((version, _)) = self.inner.toolbox.active(Tool::YtDlp).await
            && version != used
        {
            return true;
        }
        if last.is_some_and(|at| at.elapsed() < FAILURE_GAP) {
            return false;
        }
        *last = Some(Instant::now());
        self.update(|state| state.updating = Some(Tool::YtDlp));
        let result = self
            .install_when_free(Tool::YtDlp, Some(used.to_string()))
            .await;
        self.update(|state| state.updating = None);
        log::info!("yt-dlp lỗi kiểu extractor, kiểm bản mới: {result:?}");
        if result.is_ok() {
            self.mark(CHECKED_KEY);
        }
        result == Ok(Outcome::Installed)
    }
}

/// Lỗi của yt-dlp mà bản mới hay sửa được: mọi lỗi của extractor (`ERROR: [youtube] …`, vd YouTube
/// đổi trang: "The page needs to be reloaded", "Unable to extract …", thử thách nsig) và 403 khi tải
/// luồng. Không gồm lỗi bản mới không cứu được (đăng nhập, riêng tư, DRM) hay lỗi mạng. Đoán sai chỉ
/// tốn một lần đọc file checksum (kiểm tối đa mỗi 30 phút, chỉ chạy lại khi thật có bản mới).
pub(crate) fn outdated(stderr: &str) -> bool {
    let message = stderr.to_ascii_lowercase();
    let hopeless = [
        "drm",
        "sign in",
        "log in",
        "cookies",
        "private video",
        "video unavailable",
        "has been removed",
        "unsupported url",
        // Mạng hỏng: bản mới cũng vậy.
        "urlopen error",
        "timed out",
        "getaddrinfo",
        "connection refused",
        "connection reset",
        "network is unreachable",
    ];
    if hopeless.iter().any(|needle| message.contains(needle)) {
        return false;
    }
    let extractor = message
        .lines()
        .any(|line| line.trim_start().starts_with("error: ["));
    extractor
        || [
            "unable to extract",
            "nsig",
            "signature",
            "challenge",
            "requested format is not available",
            "no video formats found",
            "http error 403",
            "precondition check failed",
            "confirm you are on the latest version",
            "please report this issue",
        ]
        .iter()
        .any(|needle| message.contains(needle))
}

/// Máy thật: dò bằng `tools::detect`, tải theo bảng nguồn của nền tảng vào `<data>\bin`.
struct Machine {
    app: AppHandle,
    platform: Option<Platform>,
}

async fn detect() -> Option<tools::MediaToolsStatus> {
    tauri::async_runtime::spawn_blocking(tools::detect)
        .await
        .ok()
}

/// Công cụ còn thiếu theo thứ tự tải (`Tool::ALL`).
pub(crate) fn missing_from(status: &tools::MediaToolsStatus) -> Vec<Tool> {
    Tool::ALL
        .into_iter()
        .filter(|tool| status.info(*tool).is_none())
        .collect()
}

impl Toolbox for Machine {
    fn missing(&self) -> BoxFuture<'_, Vec<Tool>> {
        Box::pin(async move {
            if self.platform.is_none() {
                return Vec::new();
            }
            detect()
                .await
                .map(|status| missing_from(&status))
                .unwrap_or_default()
        })
    }

    fn active(&self, tool: Tool) -> BoxFuture<'_, Option<(String, bool)>> {
        Box::pin(async move {
            let info = detect().await?.info(tool)?;
            Some((info.version, info.source == tools::ToolSource::Utd))
        })
    }

    fn install(
        &self,
        tool: Tool,
        current: Option<String>,
    ) -> BoxFuture<'_, Result<Outcome, InstallError>> {
        Box::pin(async move {
            let platform = self.platform.ok_or_else(|| {
                InstallError::Failed("Chưa tự tải được công cụ trên hệ điều hành này.".into())
            })?;
            let release = sources::release(tool, platform);
            let bin = tools::bin_dir();
            let job = Job {
                release: &release,
                bin: &bin,
                policy: release.policy(),
                current,
                emit: install::emitter(&self.app),
                retry_delay: install::RETRY_DELAY,
            };
            install::install_job(&job).await
        })
    }
}

/// Gọi trong `setup` sau khi có journal: tạo bộ tự tải, chạy vòng tải lần đầu và vòng cập nhật.
pub fn start(app: &AppHandle, journal: Arc<Journal>) -> Auto {
    let emitter = app.clone();
    let auto = Auto::new(
        Box::new(Machine {
            app: app.clone(),
            platform: Platform::current(),
        }),
        journal,
        Some(Box::new(move |state: &AutoState| {
            let _ = emitter.emit(TOOLS_AUTO_EVENT, state);
        })),
    );
    let bootstrap = auto.clone();
    tauri::async_runtime::spawn(async move { bootstrap.bootstrap().await });
    let updates = auto.clone();
    tauri::async_runtime::spawn(async move { updates.update_loop().await });
    auto
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Máy giả: phiên bản đang dùng, kết quả cài lần lượt, và nhật ký các lần cài.
    struct Fake {
        active: Mutex<Option<(String, bool)>>,
        results: Mutex<Vec<Result<Outcome, InstallError>>>,
        installs: Mutex<Vec<(Tool, Option<String>)>>,
    }

    impl Fake {
        fn new(
            active: Option<(&str, bool)>,
            results: Vec<Result<Outcome, InstallError>>,
        ) -> Arc<Self> {
            Arc::new(Self {
                active: Mutex::new(active.map(|(version, own)| (version.to_string(), own))),
                results: Mutex::new(results),
                installs: Mutex::new(Vec::new()),
            })
        }
    }

    struct Shared(Arc<Fake>);

    impl Toolbox for Shared {
        fn missing(&self) -> BoxFuture<'_, Vec<Tool>> {
            Box::pin(async { Vec::new() })
        }

        fn active(&self, _tool: Tool) -> BoxFuture<'_, Option<(String, bool)>> {
            Box::pin(async move { self.0.active.lock().unwrap().clone() })
        }

        fn install(
            &self,
            tool: Tool,
            current: Option<String>,
        ) -> BoxFuture<'_, Result<Outcome, InstallError>> {
            Box::pin(async move {
                self.0.installs.lock().unwrap().push((tool, current));
                let mut results = self.0.results.lock().unwrap();
                if results.is_empty() {
                    Ok(Outcome::Current)
                } else {
                    results.remove(0)
                }
            })
        }
    }

    fn auto_with(fake: &Arc<Fake>) -> (Auto, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("utd-auto-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let journal = Arc::new(Journal::open(dir.join("utd.db")).unwrap());
        (
            Auto::new(Box::new(Shared(fake.clone())), journal, None),
            dir,
        )
    }

    #[test]
    fn recognises_errors_a_newer_yt_dlp_can_fix() {
        assert!(outdated(
            "ERROR: [youtube] abc: Unable to extract nsig function code"
        ));
        assert!(outdated(
            "ERROR: unable to download video data: HTTP Error 403: Forbidden"
        ));
        assert!(outdated(
            "ERROR: [youtube] x: Requested format is not available. Use --list-formats"
        ));
        assert!(outdated(
            "ERROR: x; please report this issue on https://github.com/yt-dlp/yt-dlp/issues . Confirm you are on the latest version using yt-dlp -U"
        ));
        assert!(!outdated(
            "ERROR: [youtube] x: Sign in to confirm you're not a bot"
        ));
        assert!(!outdated(
            "ERROR: [youtube] x: Private video. HTTP Error 403"
        ));
        assert!(!outdated("ERROR: Unsupported URL: https://example.com"));
        assert!(!outdated(
            "ERROR: unable to download: <urlopen error timed out>"
        ));
        // Câu thật của yt-dlp 2025.01.15 với YouTube tháng 10/2026.
        assert!(outdated("ERROR: [youtube] jNQXAC9IVRw: The page needs to be reloaded."));
        assert!(!outdated(
            "ERROR: [youtube] x: Unable to download webpage: <urlopen error [Errno 11001] getaddrinfo failed>"
        ));
        assert!(!outdated("ERROR: Postprocessing: Conversion failed!"));
    }

    #[tokio::test]
    async fn refreshes_once_per_gap_and_reports_a_new_version() {
        let fake = Fake::new(Some(("2026.08.19", true)), vec![Ok(Outcome::Installed)]);
        let (auto, dir) = auto_with(&fake);
        assert!(auto.refresh_after_failure("2026.08.19").await);
        assert_eq!(
            fake.installs.lock().unwrap().as_slice(),
            [(Tool::YtDlp, Some("2026.08.19".to_string()))]
        );
        // Lỗi tiếp ngay sau đó (bản mới cũng hỏng): không kiểm lại trong 30 phút.
        assert!(!auto.refresh_after_failure("2026.08.19").await);
        assert_eq!(fake.installs.lock().unwrap().len(), 1);
        // Task khác chạy bản cũ trong lúc đã có bản mới: chạy lại luôn, không cần tải.
        *fake.active.lock().unwrap() = Some(("2026.10.22".into(), true));
        assert!(auto.refresh_after_failure("2026.08.19").await);
        assert!(auto.state().updating.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn no_refresh_when_current_or_switched_off() {
        let fake = Fake::new(Some(("2026.10.22", false)), vec![Ok(Outcome::Current)]);
        let (auto, dir) = auto_with(&fake);
        // Bản trên PATH đã mới nhất: không có gì để chạy lại.
        assert!(!auto.refresh_after_failure("2026.10.22").await);
        let (off, other) = auto_with(&fake);
        off.set_enabled(false).unwrap();
        assert!(!off.state().enabled);
        assert!(!off.refresh_after_failure("2026.10.22").await);
        assert_eq!(fake.installs.lock().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(other);
    }

    #[tokio::test]
    async fn daily_check_updates_only_the_copy_utd_manages() {
        let fake = Fake::new(Some(("2026.08.19", false)), vec![]);
        let (auto, dir) = auto_with(&fake);
        auto.update(|state| state.checking = false);
        auto.check_updates().await;
        assert!(
            fake.installs.lock().unwrap().is_empty(),
            "bản người dùng tự cài: không đụng"
        );

        *fake.active.lock().unwrap() = Some(("2026.08.19".into(), true));
        auto.check_updates().await;
        assert_eq!(
            fake.installs.lock().unwrap().len(),
            2,
            "yt-dlp và deno của UTD"
        );
        // Đã kiểm hôm nay: không kiểm lại.
        auto.check_updates().await;
        assert_eq!(fake.installs.lock().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn waiting_task_can_be_paused_and_off_means_no_wait() {
        let fake = Fake::new(None, vec![]);
        let (auto, dir) = auto_with(&fake);
        auto.update(|state| {
            state.checking = false;
            state.pending = vec![Tool::Ffmpeg];
        });
        // Phân tích chỉ cần yt-dlp, deno: ffmpeg đang tải không chặn.
        let never = CancellationToken::new();
        let ready = auto.wait_for(&[Tool::YtDlp], &[Tool::Deno], 1, &never);
        assert_eq!(ready.await, Wait::Ready);
        assert!(auto.will_install(Tool::Ffmpeg));
        let cancel = CancellationToken::new();
        cancel.cancel();
        let waited = auto.wait_for(&[Tool::Ffmpeg], &[], TASK_PASSES, &cancel);
        assert_eq!(waited.await, Wait::Cancelled);
        // Người dùng bấm Huỷ lần tự tải trong lúc task chờ: task thôi chờ, báo lỗi rõ.
        let waiting = auto.clone();
        let task = tokio::spawn(async move {
            let never = CancellationToken::new();
            waiting.wait_for(&[Tool::Ffmpeg], &[], TASK_PASSES, &never).await
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        auto.update(|state| state.paused = true);
        assert_eq!(task.await.unwrap(), Wait::Failed("Đã huỷ tải công cụ.".into()));
        auto.update(|state| state.paused = false);
        // ffmpeg vừa lỗi: không còn "sắp có" (Phân tích không hứa định dạng cần ffmpeg).
        auto.update(|state| state.failed = vec![Tool::Ffmpeg]);
        assert!(!auto.will_install(Tool::Ffmpeg));
        auto.set_enabled(false).unwrap();
        let off = auto.wait_for(&[Tool::Ffmpeg], &[], TASK_PASSES, &never);
        assert_eq!(off.await, Wait::Ready);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Bấm Huỷ lúc hàng đợi đang giữ task media: hàng đợi thả task ra, task không được bật lại lần tự
    /// tải (trước đây task vừa thả gọi `kick` ⇒ tải lại ngay, task giữ chỗ chờ thêm tới 6 lượt).
    #[tokio::test]
    async fn cancel_is_not_undone_by_the_task_the_queue_just_released() {
        let fake = Fake::new(None, vec![]);
        let (auto, dir) = auto_with(&fake);
        auto.update(|state| {
            state.checking = false;
            state.pending = vec![Tool::YtDlp, Tool::Deno, Tool::Ffmpeg];
        });
        assert!(auto.holds_media(false));
        auto.update(|state| state.paused = true);
        assert!(!auto.holds_media(false), "Huỷ ⇒ hàng đợi thả task media");
        let never = CancellationToken::new();
        let task = auto.wait_for_task(
            &[Tool::YtDlp],
            &[Tool::Deno, Tool::Ffmpeg],
            TASK_PASSES,
            &never,
        );
        let waited = tokio::time::timeout(Duration::from_millis(500), task).await;
        assert_eq!(waited, Ok(Wait::Failed("Đã huỷ tải công cụ.".into())));
        assert!(auto.state().paused, "vẫn tạm dừng, không tải lại");
        assert!(!auto.holds_media(false), "task media khác không bị giữ lại");
        assert_eq!(
            auto.missing_message(Tool::YtDlp, "Thiếu yt-dlp.".into()),
            "Thiếu yt-dlp.",
            "đã Huỷ thì không hứa tự thử lại"
        );
        // Phân tích (người dùng đang đợi) thì bật lại lần tự tải.
        let probe = auto.wait_for(&[Tool::YtDlp], &[], 1, &never);
        assert!(tokio::time::timeout(Duration::from_millis(50), probe).await.is_err());
        assert!(!auto.state().paused);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Hàng đợi giữ task media (không giữ chỗ) đúng lúc runner sẽ phải chờ công cụ.
    #[test]
    fn queue_holds_media_tasks_only_while_their_tools_are_coming() {
        let fake = Fake::new(None, vec![]);
        let (auto, dir) = auto_with(&fake);
        assert!(auto.holds_media(false), "đang dò công cụ lúc mở app");
        auto.update(|state| {
            state.checking = false;
            state.pending = vec![Tool::Deno];
        });
        assert!(auto.holds_media(false), "deno đang tải lượt đầu: chờ một lượt");
        auto.update(|state| state.failed = vec![Tool::Deno]);
        assert!(!auto.holds_media(true), "deno lỗi: chạy tiếp không có nó");
        auto.update(|state| {
            state.pending = vec![Tool::Ffmpeg];
            state.failed = vec![Tool::Ffmpeg];
        });
        assert!(!auto.holds_media(false), "ffmpeg lỗi: video vẫn chạy");
        assert!(auto.holds_media(true), "tách âm thanh cần ffmpeg: chờ qua các lần thử lại");
        auto.update(|state| {
            state.pending = vec![Tool::YtDlp];
            state.failed = vec![Tool::YtDlp];
        });
        assert!(auto.holds_media(false));
        auto.update(|state| state.paused = true);
        assert!(!auto.holds_media(true), "bấm Huỷ: task chạy và báo thiếu công cụ");
        auto.update(|state| state.paused = false);
        auto.set_enabled(false).unwrap();
        assert!(!auto.holds_media(true), "tắt tự tải: không chờ");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Kệ công cụ giả: tải là có, trừ công cụ "hỏng" luôn lỗi.
    struct Shelf {
        have: Mutex<Vec<Tool>>,
        broken: Vec<Tool>,
    }

    struct OnShelf(Arc<Shelf>);

    impl Toolbox for OnShelf {
        fn missing(&self) -> BoxFuture<'_, Vec<Tool>> {
            Box::pin(async move {
                let have = self.0.have.lock().unwrap();
                Tool::ALL
                    .into_iter()
                    .filter(|tool| !have.contains(tool))
                    .collect()
            })
        }

        fn active(&self, _tool: Tool) -> BoxFuture<'_, Option<(String, bool)>> {
            Box::pin(async { None })
        }

        fn install(
            &self,
            tool: Tool,
            _current: Option<String>,
        ) -> BoxFuture<'_, Result<Outcome, InstallError>> {
            Box::pin(async move {
                if self.0.broken.contains(&tool) {
                    return Err(InstallError::Failed("Nguồn tải hỏng.".into()));
                }
                self.0.have.lock().unwrap().push(tool);
                Ok(Outcome::Installed)
            })
        }
    }

    #[tokio::test]
    async fn a_broken_tool_does_not_block_the_others_and_waits_are_bounded() {
        let shelf = Arc::new(Shelf {
            have: Mutex::new(Vec::new()),
            broken: vec![Tool::Deno],
        });
        let dir = std::env::temp_dir().join(format!("utd-auto-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let journal = Arc::new(Journal::open(dir.join("utd.db")).unwrap());
        let auto = Auto::new(Box::new(OnShelf(shelf.clone())), journal, None)
            .with_backoff(vec![Duration::from_millis(20)]);
        let runner = auto.clone();
        let looping = tokio::spawn(async move { runner.bootstrap().await });
        let never = CancellationToken::new();

        // Task video: chờ yt-dlp; deno hỏng chỉ được chờ một lượt, rồi chạy tiếp không có nó.
        let task = auto.wait_for(
            &[Tool::YtDlp],
            &[Tool::Deno, Tool::Ffmpeg],
            TASK_PASSES,
            &never,
        );
        let waited = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap();
        assert_eq!(waited, Wait::Ready);
        assert_eq!(
            *shelf.have.lock().unwrap(),
            [Tool::YtDlp, Tool::Ffmpeg],
            "ffmpeg không bị deno chặn"
        );
        let state = auto.state();
        assert_eq!(state.failed, [Tool::Deno]);
        assert_eq!(state.error.as_deref(), Some("Nguồn tải hỏng."));
        assert!(state.retry_at.is_some());

        // Công cụ bắt buộc vẫn hỏng sau số lượt cho phép: báo lỗi chứ không chờ mãi.
        let essential = auto.wait_for(&[Tool::Deno], &[], 2, &never);
        let failed = tokio::time::timeout(Duration::from_secs(5), essential)
            .await
            .unwrap();
        assert_eq!(failed, Wait::Failed("Nguồn tải hỏng.".into()));
        assert!(
            auto.missing_message(Tool::Deno, String::new())
                .contains("UTD sẽ tự thử lại")
        );
        looping.abort();
        let _ = std::fs::remove_dir_all(dir);
    }
}
