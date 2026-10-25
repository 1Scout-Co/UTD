//! Tự chọn số kết nối theo tốc độ thật (kiểu IDM, người dùng không phải chọn).
//!
//! Bộ điều khiển leo đồi: đo tốc độ mỗi cửa sổ, còn nhanh lên rõ (≥8%) thì thêm kết nối,
//! gần như không hơn (<3%) thì lùi một bậc và giữ, định kỳ dò lại để theo kịp mạng đổi
//! (Wi-Fi↔4G). Server chặn (429/503, Retry-After) hoặc mạng rớt hàng loạt thì giảm một nửa,
//! chờ, rồi tăng lại.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::http::RequestFailure;

/// Thêm kết nối mà nhanh hơn ít nhất chừng này thì thêm tiếp.
const CLIMB_GAIN: f64 = 0.08;
/// Nhanh hơn ít hơn chừng này thì kết nối thêm là vô ích: lùi lại.
const KEEP_GAIN: f64 = 0.03;
/// Đang giữ mà tốc độ đổi quá ±40% ⇒ mạng đã đổi, dò lại sớm.
const SHIFT_RATIO: f64 = 0.4;
/// Trung bình số kết nối đang chạy phải đạt chừng này của mục tiêu thì phép đo mới có nghĩa.
const SATURATED_SHARE: f64 = 0.8;
/// Làm mượt tốc độ tham chiếu khi đang giữ.
const SMOOTHING: f64 = 0.5;

/// Server bảo "nhiều quá" (khác với mạng rớt).
pub(crate) fn is_throttle(failure: &RequestFailure) -> bool {
    failure.retry_after.is_some() || matches!(failure.error.code.as_str(), "http.429" | "http.503")
}

/// Cổng chung của các worker trong một lượt tải: số kết nối không vượt mục tiêu.
pub(crate) struct ConnectionGate {
    target: AtomicUsize,
    /// Worker đã qua cổng (kể cả đang chờ hàng host/toàn cục).
    active: AtomicUsize,
    /// Kết nối đang thật sự mở tới server.
    live: AtomicUsize,
    throttles: AtomicU32,
    faults: AtomicU32,
    retry_after_ms: AtomicU64,
    changed: Notify,
}

impl ConnectionGate {
    pub fn new(target: usize) -> Arc<Self> {
        Arc::new(Self {
            target: AtomicUsize::new(target.max(1)),
            active: AtomicUsize::new(0),
            live: AtomicUsize::new(0),
            throttles: AtomicU32::new(0),
            faults: AtomicU32::new(0),
            retry_after_ms: AtomicU64::new(0),
            changed: Notify::new(),
        })
    }

    pub fn target(&self) -> usize {
        self.target.load(Ordering::Acquire)
    }

    pub fn live(&self) -> usize {
        self.live.load(Ordering::Acquire)
    }

    pub fn set_target(&self, target: usize) {
        self.target.store(target.max(1), Ordering::Release);
        self.changed.notify_waiters();
    }

    /// Chờ tới lượt; `None` khi bị huỷ.
    pub async fn enter(self: &Arc<Self>, cancel: &CancellationToken) -> Option<GateSlot> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.try_enter() {
                return Some(GateSlot {
                    gate: self.clone(),
                    released: AtomicBool::new(false),
                });
            }
            tokio::select! {
                _ = &mut changed => {}
                _ = cancel.cancelled() => return None,
            }
        }
    }

    fn try_enter(&self) -> bool {
        let target = self.target();
        update_if(&self.active, |active| (active < target).then_some(active + 1))
    }

    fn release(&self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
        self.changed.notify_waiters();
    }

    /// Đánh dấu một kết nối đang mở (bỏ guard khi đóng).
    pub fn connect(self: &Arc<Self>) -> LiveGuard {
        self.live.fetch_add(1, Ordering::AcqRel);
        LiveGuard { gate: self.clone() }
    }

    /// Worker báo lỗi tạm thời để bộ điều khiển biết server/mạng đang quá tải.
    pub fn report(&self, failure: &RequestFailure) {
        if !failure.retryable() {
            return;
        }
        if is_throttle(failure) {
            self.throttles.fetch_add(1, Ordering::AcqRel);
        } else {
            self.faults.fetch_add(1, Ordering::AcqRel);
        }
        if let Some(after) = failure.retry_after {
            let millis = u64::try_from(after.as_millis()).unwrap_or(u64::MAX).max(1);
            self.retry_after_ms.fetch_max(millis, Ordering::AcqRel);
        }
    }

    pub fn take_pressure(&self) -> Pressure {
        let millis = self.retry_after_ms.swap(0, Ordering::AcqRel);
        Pressure {
            throttles: self.throttles.swap(0, Ordering::AcqRel),
            faults: self.faults.swap(0, Ordering::AcqRel),
            retry_after: (millis > 0).then(|| Duration::from_millis(millis)),
        }
    }
}

/// Như `fetch_update` (Rust mới đổi tên thành `try_update`, bản cũ chưa có): ghi khi `next` trả `Some`.
/// Viết tay để build được trên mọi bản stable mà không dính cảnh báo deprecated.
fn update_if(value: &AtomicUsize, mut next: impl FnMut(usize) -> Option<usize>) -> bool {
    let mut current = value.load(Ordering::Acquire);
    while let Some(new) = next(current) {
        match value.compare_exchange_weak(current, new, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return true,
            Err(actual) => current = actual,
        }
    }
    false
}

/// Chỗ của một worker trong cổng; tự trả khi drop.
pub(crate) struct GateSlot {
    gate: Arc<ConnectionGate>,
    released: AtomicBool,
}

impl GateSlot {
    /// Đang thừa kết nối so với mục tiêu ⇒ nhả chỗ (true): worker đóng kết nối, giữ phần đã tải.
    pub fn yield_if_over(&self) -> bool {
        if self.released.load(Ordering::Acquire) {
            return false;
        }
        let gate = &self.gate;
        let shrunk = update_if(&gate.active, |active| {
            (active > gate.target()).then(|| active - 1)
        });
        if shrunk {
            self.released.store(true, Ordering::Release);
        }
        shrunk
    }
}

impl Drop for GateSlot {
    fn drop(&mut self) {
        if !self.released.swap(true, Ordering::AcqRel) {
            self.gate.release();
        }
    }
}

pub(crate) struct LiveGuard {
    gate: Arc<ConnectionGate>,
}

impl Drop for LiveGuard {
    fn drop(&mut self) {
        self.gate.live.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Nhịp của bộ điều khiển, lấy từ `EngineOptions`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tuning {
    /// Thời gian đo mỗi bậc.
    pub window: Duration,
    /// Bỏ qua lúc kết nối mới còn đang mở.
    pub settle: Duration,
    /// Giữ yên trước khi dò lại (mặc định ~12 s).
    pub hold: Duration,
}

impl Tuning {
    pub fn new(sample: Duration, balance: Duration) -> Self {
        Self {
            window: balance,
            settle: sample.min(balance),
            hold: balance.saturating_mul(4),
        }
    }

    /// Bậc từng bị server chặn thì tạm tránh trong chừng này.
    fn ceiling_ttl(&self) -> Duration {
        self.hold.saturating_mul(4)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Pressure {
    pub throttles: u32,
    pub faults: u32,
    pub retry_after: Option<Duration>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Sample {
    pub now: Instant,
    pub downloaded: u64,
    pub live: usize,
    pub pressure: Pressure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Đang thêm kết nối; còn nhanh lên thì thêm tiếp.
    Climb,
    /// Đã tới điểm tốt nhất: giữ, tới hạn thì dò lại.
    Hold(Instant),
    /// Vừa bị chặn hoặc rớt mạng: chờ rồi mới tăng lại.
    Backoff(Instant),
}

struct Window {
    start: Instant,
    bytes: u64,
    measure_from: Instant,
    /// Đã qua lúc ổn định và đang đo (chưa thì nhịp kế chỉ lấy làm mốc).
    started: bool,
    live_sum: usize,
    ticks: u32,
}

impl Window {
    fn new(now: Instant, bytes: u64, settle: Duration) -> Self {
        let mut window = Self {
            start: now,
            bytes,
            measure_from: now,
            started: false,
            live_sum: 0,
            ticks: 0,
        };
        window.settle(now, settle);
        window
    }

    /// Vừa bắt đầu, vừa đổi số kết nối hay vừa hết lùi: cửa sổ kế bắt đầu ở nhịp đầu tiên sau lúc ổn
    /// định. Mốc đặt ở nửa `settle` vì nhịp thật luôn tới trễ vài ms: mốc trọn `settle` thì nhịp đó
    /// lọt qua và lúc mở kết nối bị tính vào tốc độ của bậc mới.
    fn settle(&mut self, now: Instant, settle: Duration) {
        self.measure_from = now + settle / 2;
        self.started = false;
    }

    fn reset(&mut self, sample: &Sample) {
        self.start = sample.now;
        self.bytes = sample.downloaded;
        self.live_sum = 0;
        self.ticks = 0;
    }

    /// Đủ một cửa sổ thì trả (tốc độ B/s, đã dùng hết số kết nối cho phép chưa).
    fn sample(&mut self, sample: &Sample, target: usize, length: Duration) -> Option<(f64, bool)> {
        if !self.started {
            // Chỉ bắt đầu đo khi đã qua lúc ổn định và byte đang về: lúc mở kết nối, hay app khựng lúc
            // đầu (chưa có byte nào), không được kéo tốc độ của bậc này xuống.
            let flowing = sample.downloaded > self.bytes;
            self.reset(sample);
            self.started = flowing && sample.now >= self.measure_from;
            return None;
        }
        self.live_sum += sample.live;
        self.ticks += 1;
        let elapsed = sample.now.saturating_duration_since(self.start);
        if elapsed < length {
            return None;
        }
        let rate =
            sample.downloaded.saturating_sub(self.bytes) as f64 / elapsed.as_secs_f64().max(0.001);
        let mean_live = self.live_sum as f64 / f64::from(self.ticks.max(1));
        self.reset(sample);
        Some((rate, mean_live >= SATURATED_SHARE * target as f64))
    }
}

pub(crate) struct Controller {
    cap: usize,
    target: usize,
    tuning: Tuning,
    phase: Phase,
    /// Lúc đầu (và sau rớt mạng) nhân đôi mỗi bậc; sau đó tăng chậm hơn.
    slow_start: bool,
    /// Bậc trước và tốc độ đo được ở bậc đó.
    reference: Option<(usize, f64)>,
    /// Bậc từng bị server chặn: tạm không vượt tới khi hết hạn.
    ceiling: Option<(usize, Instant)>,
    window: Window,
    faults: u32,
    /// Một đợt lỗi chỉ giảm một lần.
    calm_until: Option<Instant>,
    /// Mục tiêu ngay trước lần giảm gần nhất.
    cut_from: usize,
}

impl Controller {
    pub fn new(initial: usize, cap: usize, tuning: Tuning, now: Instant, downloaded: u64) -> Self {
        let cap = cap.max(1);
        Self {
            cap,
            target: initial.clamp(1, cap),
            tuning,
            phase: Phase::Climb,
            slow_start: true,
            reference: None,
            ceiling: None,
            window: Window::new(now, downloaded, tuning.settle),
            faults: 0,
            calm_until: None,
            cut_from: initial.clamp(1, cap),
        }
    }

    pub fn target(&self) -> usize {
        self.target
    }

    /// Gọi mỗi nhịp đo; trả về mục tiêu mới khi cần đổi.
    pub fn on_tick(&mut self, sample: Sample) -> Option<usize> {
        if let Some(target) = self.on_pressure(&sample) {
            return Some(target);
        }
        if let Phase::Backoff(until) = self.phase {
            self.window.reset(&sample);
            if sample.now >= until {
                self.phase = Phase::Climb;
                self.reference = None;
                self.window.settle(sample.now, self.tuning.settle);
            }
            return None;
        }
        let (rate, saturated) = self
            .window
            .sample(&sample, self.target, self.tuning.window)?;
        self.faults = 0;
        match self.phase {
            Phase::Climb => self.climb(sample.now, rate, saturated),
            Phase::Hold(until) => self.hold(sample.now, until, rate, saturated),
            Phase::Backoff(_) => None,
        }
    }

    fn on_pressure(&mut self, sample: &Sample) -> Option<usize> {
        let pressure = sample.pressure;
        self.faults = self.faults.saturating_add(pressure.faults);
        let throttled = pressure.throttles > 0;
        let burst_limit = u32::try_from(self.target / 4).unwrap_or(u32::MAX).max(2);
        if !throttled && self.faults < burst_limit {
            return None;
        }
        let now = sample.now;
        let wait = pressure.retry_after.unwrap_or(Duration::ZERO);
        self.faults = 0;
        if self.calm_until.is_some_and(|until| now < until) {
            // Cùng một đợt lỗi: không giảm thêm, chỉ chờ lâu hơn nếu server bảo vậy.
            if let Phase::Backoff(until) = self.phase {
                self.phase = Phase::Backoff(until.max(now + wait));
            }
            if throttled && self.ceiling.is_none() {
                self.ceiling = Some((self.cut_from, now + self.tuning.ceiling_ttl()));
                self.slow_start = false;
            }
            return None;
        }
        self.cut_from = self.target;
        if throttled {
            // Server chê nhiều kết nối: nhớ bậc này, lên lại từ từ.
            self.ceiling = Some((self.target, now + self.tuning.ceiling_ttl()));
            self.slow_start = false;
        } else {
            // Mạng rớt (không phải server chê): lên lại nhanh khi mạng ổn.
            self.slow_start = true;
        }
        self.calm_until = Some(now + wait.max(self.tuning.window));
        self.phase = Phase::Backoff(now + wait.max(self.tuning.window.saturating_mul(2)));
        self.reference = None;
        self.set((self.target / 2).max(1), now)
    }

    fn climb(&mut self, now: Instant, rate: f64, saturated: bool) -> Option<usize> {
        if !saturated {
            // Chưa dùng hết số kết nối đang cho (hết việc hoặc vướng giới hạn host): thêm vô ích.
            self.reference = None;
            return None;
        }
        let previous = self.reference.filter(|(level, _)| *level < self.target);
        let Some((level, before)) = previous else {
            self.reference = Some((self.target, rate));
            return self.step_up(now);
        };
        let gain = gain(before, rate);
        if gain >= CLIMB_GAIN {
            self.reference = Some((self.target, rate));
            return self.step_up(now);
        }
        self.slow_start = false;
        self.phase = Phase::Hold(now + self.tuning.hold);
        if gain < KEEP_GAIN {
            // Thêm kết nối mà không nhanh hơn: lùi về bậc cũ.
            self.reference = Some((level, before));
            return self.set(level, now);
        }
        self.reference = Some((self.target, rate));
        None
    }

    fn hold(&mut self, now: Instant, until: Instant, rate: f64, saturated: bool) -> Option<usize> {
        if !saturated {
            return None;
        }
        let shifted = match self.reference {
            Some((level, before)) if level == self.target => {
                let shifted = before > 0.0 && (rate - before).abs() / before > SHIFT_RATIO;
                let smoothed = if shifted {
                    rate
                } else {
                    before + (rate - before) * SMOOTHING
                };
                self.reference = Some((level, smoothed));
                shifted
            }
            _ => {
                self.reference = Some((self.target, rate));
                false
            }
        };
        if now < until && !shifted {
            return None;
        }
        // Dò lại một bậc nhỏ: mạng hoặc server có thể đã đổi.
        let limit = self.limit(now);
        if self.target >= limit {
            self.phase = Phase::Hold(now + self.tuning.hold);
            return None;
        }
        self.phase = Phase::Climb;
        let step = (self.target / 4).max(1);
        self.set((self.target + step).min(limit), now)
    }

    fn step_up(&mut self, now: Instant) -> Option<usize> {
        let limit = self.limit(now);
        if self.target >= limit {
            self.phase = Phase::Hold(now + self.tuning.hold);
            return None;
        }
        let step = if self.slow_start {
            self.target
        } else {
            (self.target / 2).max(1)
        };
        self.phase = Phase::Climb;
        self.set((self.target + step).min(limit), now)
    }

    fn limit(&mut self, now: Instant) -> usize {
        if let Some((level, until)) = self.ceiling {
            if now < until {
                return level.saturating_sub(1).clamp(1, self.cap);
            }
            self.ceiling = None;
        }
        self.cap
    }

    fn set(&mut self, target: usize, now: Instant) -> Option<usize> {
        let target = target.clamp(1, self.cap);
        if target == self.target {
            return None;
        }
        self.target = target;
        self.window.settle(now, self.tuning.settle);
        Some(target)
    }
}

fn gain(before: f64, after: f64) -> f64 {
    if before > 0.0 {
        after / before - 1.0
    } else if after > 0.0 {
        1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: Duration = Duration::from_millis(50);

    fn tuning() -> Tuning {
        Tuning::new(TICK, Duration::from_millis(300))
    }

    struct Sim {
        controller: Controller,
        now: Instant,
        bytes: f64,
    }

    impl Sim {
        fn new(initial: usize, cap: usize) -> Self {
            let now = Instant::now();
            Self {
                controller: Controller::new(initial, cap, tuning(), now, 0),
                now,
                bytes: 0.0,
            }
        }

        /// Chạy `ticks` nhịp với tốc độ phụ thuộc số kết nối; trả về các mục tiêu đã qua.
        fn run(&mut self, ticks: usize, rate: impl Fn(usize) -> f64) -> Vec<usize> {
            (0..ticks)
                .map(|_| self.tick(&rate, Pressure::default()))
                .collect()
        }

        fn tick(&mut self, rate: &impl Fn(usize) -> f64, pressure: Pressure) -> usize {
            let target = self.controller.target();
            self.now += TICK;
            self.bytes += rate(target) * TICK.as_secs_f64();
            self.controller.on_tick(Sample {
                now: self.now,
                downloaded: self.bytes as u64,
                live: target,
                pressure,
            });
            self.controller.target()
        }
    }

    const MIB: f64 = 1024.0 * 1024.0;

    #[test]
    fn climbs_to_the_cap_when_each_connection_is_throttled() {
        let mut sim = Sim::new(4, 16);
        let targets = sim.run(40, |connections| connections as f64 * MIB);
        assert_eq!(sim.controller.target(), 16, "{targets:?}");
        // 4 → 8 → 16: mỗi bậc một cửa sổ đo (+ một nhịp chờ kết nối mới).
        let reached = targets
            .iter()
            .position(|target| *target == 16)
            .expect("reaches 16");
        assert!(reached <= 16, "too slow: {targets:?}");
    }

    #[test]
    fn stops_adding_when_total_bandwidth_is_capped() {
        let mut sim = Sim::new(4, 16);
        let targets = sim.run(200, |_| 4.0 * MIB);
        assert!(targets.iter().all(|target| *target <= 8), "{targets:?}");
        // Giữ ở 4, thỉnh thoảng dò lên 5 rồi lùi.
        assert!(sim.controller.target() <= 5, "{targets:?}");
        let settled = &targets[100..];
        let busy = settled.iter().filter(|target| **target > 4).count();
        assert!(busy * 3 < settled.len(), "probes too often: {targets:?}");
    }

    /// Như trên nhưng giống máy thật: nhịp tới trễ vài ms và lúc đầu khựng 250 ms chưa có byte nào (mở
    /// kết nối, máy bận). Phần đó không được tính vào bậc 4, nếu không bậc 8 trông nhanh hơn dù trần chung
    /// đã chạm và bộ điều khiển leo lên 16 (lỗi từng làm bài trần băng thông đỏ trên CI).
    #[test]
    fn late_ticks_and_a_slow_start_do_not_fake_a_gain() {
        let start = Instant::now();
        let mut controller = Controller::new(4, 16, tuning(), start, 0);
        let tick = TICK + Duration::from_millis(3);
        let mut now = start + Duration::from_millis(250);
        let mut bytes = 0.0;
        let mut targets = Vec::new();
        for step in 0..200 {
            if step > 0 {
                now += tick;
                bytes += 4.0 * MIB * tick.as_secs_f64();
            }
            let live = controller.target();
            controller.on_tick(Sample {
                now,
                downloaded: bytes as u64,
                live,
                pressure: Pressure::default(),
            });
            targets.push(controller.target());
        }
        assert!(targets.iter().all(|target| *target <= 8), "{targets:?}");
        assert!(controller.target() <= 5, "{targets:?}");
    }

    #[test]
    fn settles_near_the_knee() {
        let mut sim = Sim::new(4, 16);
        let targets = sim.run(200, |connections| connections.min(6) as f64 * MIB);
        assert_eq!(sim.controller.target(), 8, "{targets:?}");
        // Sau lần dò đầu, chỉ thử nhích lên một bậc nhỏ rồi lùi.
        assert!(
            targets[40..].iter().all(|target| *target <= 10),
            "{targets:?}"
        );
    }

    #[test]
    fn throttling_halves_then_recovers_below_the_ceiling() {
        let mut sim = Sim::new(4, 16);
        let rate = |connections: usize| connections as f64 * MIB;
        sim.run(40, rate);
        assert_eq!(sim.controller.target(), 16);
        let throttled = Pressure {
            throttles: 3,
            faults: 0,
            retry_after: Some(Duration::from_secs(1)),
        };
        assert_eq!(sim.tick(&rate, throttled), 8);
        // Lỗi cùng đợt không giảm thêm.
        assert_eq!(sim.tick(&rate, throttled), 8);
        let after = sim.run(80, rate);
        assert!(after.iter().all(|target| *target < 16), "{after:?}");
        assert!(sim.controller.target() > 8, "{after:?}");
    }

    #[test]
    fn a_single_reset_is_ignored_but_a_burst_cuts() {
        let mut sim = Sim::new(8, 16);
        let rate = |connections: usize| connections as f64 * MIB;
        let one = Pressure {
            faults: 1,
            ..Pressure::default()
        };
        sim.tick(&rate, one);
        assert_eq!(sim.controller.target(), 8);
        let burst = Pressure {
            faults: 4,
            ..Pressure::default()
        };
        assert_eq!(sim.tick(&rate, burst), 4);
        // Rớt mạng không đặt trần: lên lại tới mức tối đa.
        sim.run(80, rate);
        assert_eq!(sim.controller.target(), 16);
    }

    #[test]
    fn does_not_grow_while_connections_are_unused() {
        let mut controller = Controller::new(4, 16, tuning(), Instant::now(), 0);
        let mut now = Instant::now();
        let mut bytes = 0u64;
        for _ in 0..60 {
            now += TICK;
            bytes += 200_000;
            controller.on_tick(Sample {
                now,
                downloaded: bytes,
                live: 2,
                pressure: Pressure::default(),
            });
        }
        assert_eq!(controller.target(), 4);
    }

    #[test]
    fn reprobes_after_the_network_gets_faster() {
        let mut sim = Sim::new(4, 16);
        sim.run(100, |connections| connections.min(4) as f64 * MIB);
        assert_eq!(sim.controller.target(), 4);
        sim.run(120, |connections| connections as f64 * MIB);
        assert!(sim.controller.target() >= 12, "{}", sim.controller.target());
    }
}
