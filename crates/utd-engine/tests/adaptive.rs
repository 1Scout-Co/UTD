//! Số kết nối tự động (kiểu IDM): server giới hạn tốc độ mỗi kết nối, giới hạn tổng băng
//! thông, trả 429/503 một lúc, và tạm dừng/tiếp tục giữa chừng.

use std::fmt::Write as _;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{sleep_until, Instant};
use utd_engine::{
    DownloadControl, DownloadEngine, DownloadOutcome, DownloadRequest, DownloadTask, EngineEvent,
    EngineOptions, HashState, Journal,
};

const MIB: usize = 1024 * 1024;
const CHUNK: usize = 16 * 1024;

// ---------- server thử: giới hạn tốc độ theo kết nối / tổng, và "bão" 429/503 ----------

#[derive(Clone, Copy, Default)]
struct Limits {
    /// B/s cho mỗi kết nối (CDN giới hạn theo kết nối).
    per_connection: Option<u64>,
    /// B/s cho tất cả kết nối cộng lại (đường truyền đã đầy).
    total: Option<u64>,
    storm: Option<Storm>,
}

/// Khi đã có ít nhất `crowd` kết nối cùng lúc trong `crowd_for`: cắt mọi kết nối đang chạy
/// và trả 429/503 (Retry-After: 1) trong `length`.
#[derive(Clone, Copy)]
struct Storm {
    crowd: usize,
    crowd_for: Duration,
    length: Duration,
}

/// Hàng đợi của đường truyền: kết nối đến trễ một chút vẫn bù được, không mất băng thông.
const BUCKET_BURST: Duration = Duration::from_millis(40);

/// Bài trần băng thông so hai bậc chỉ hơn kém vài phần trăm: các bài khác mở kết nối, tải, băm cùng lúc
/// làm máy CI 4 vCPU khựng đúng lúc đo. Bài đó giữ khoá ghi (chạy một mình), các bài khác giữ khoá đọc.
static ALONE: tokio::sync::RwLock<()> = tokio::sync::RwLock::const_new(());

struct Shared {
    body: Vec<u8>,
    limits: Limits,
    served: AtomicU64,
    rejected: AtomicUsize,
    streams: AtomicUsize,
    next_slot: Mutex<Instant>,
    crowded_since: Mutex<Option<Instant>>,
    storm_started: Mutex<Option<Instant>>,
}

impl Shared {
    fn storm_start(&self) -> Option<Instant> {
        *self.storm_started.lock().expect("storm lock")
    }

    fn storm_active(&self, now: Instant) -> bool {
        match (self.limits.storm, self.storm_start()) {
            (Some(storm), Some(started)) => now < started + storm.length,
            _ => false,
        }
    }

    fn maybe_start_storm(&self) {
        let Some(storm) = self.limits.storm else {
            return;
        };
        let now = Instant::now();
        let mut since = self.crowded_since.lock().expect("crowd lock");
        if self.streams.load(Ordering::SeqCst) < storm.crowd {
            *since = None;
            return;
        }
        let crowded = *since.get_or_insert(now);
        if now.duration_since(crowded) < storm.crowd_for {
            return;
        }
        let mut started = self.storm_started.lock().expect("storm lock");
        if started.is_none() {
            *started = Some(now);
        }
    }
}

/// Đếm số kết nối đang gửi thân file.
struct StreamGuard<'a>(&'a AtomicUsize);

impl<'a> StreamGuard<'a> {
    fn new(counter: &'a AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(counter)
    }
}

impl Drop for StreamGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct ThrottledServer {
    addr: SocketAddr,
    shared: Arc<Shared>,
}

impl ThrottledServer {
    async fn start(body: Vec<u8>, limits: Limits) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let shared = Arc::new(Shared {
            body,
            limits,
            served: AtomicU64::new(0),
            rejected: AtomicUsize::new(0),
            streams: AtomicUsize::new(0),
            next_slot: Mutex::new(Instant::now()),
            crowded_since: Mutex::new(None),
            storm_started: Mutex::new(None),
        });
        let accept = shared.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(handle(stream, accept.clone()));
            }
        });
        Self { addr, shared }
    }

    fn url(&self) -> String {
        format!("http://{}/file.bin", self.addr)
    }

    fn served(&self) -> u64 {
        self.shared.served.load(Ordering::SeqCst)
    }

    fn rejected(&self) -> usize {
        self.shared.rejected.load(Ordering::SeqCst)
    }

    fn storm_window(&self) -> Option<(Instant, Instant)> {
        let storm = self.shared.limits.storm?;
        let started = self.shared.storm_start()?;
        Some((started, started + storm.length))
    }
}

struct Request {
    head: bool,
    range: Option<(u64, Option<u64>)>,
}

async fn handle(mut stream: TcpStream, shared: Arc<Shared>) {
    let Some(request) = read_request(&mut stream).await else {
        return;
    };
    let total = shared.body.len() as u64;
    if request.head {
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nAccept-Ranges: bytes\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(header.as_bytes()).await;
        return;
    }
    if shared.storm_active(Instant::now()) {
        let count = shared.rejected.fetch_add(1, Ordering::SeqCst);
        let status = if count.is_multiple_of(2) {
            "429 Too Many Requests"
        } else {
            "503 Service Unavailable"
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nRetry-After: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(response.as_bytes()).await;
        return;
    }
    let (start, end, status) = match request.range {
        Some((start, end)) => (
            start,
            end.unwrap_or(total - 1).min(total - 1),
            "206 Partial Content",
        ),
        None => (0, total - 1, "200 OK"),
    };
    if start > end {
        let response = format!(
            "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{total}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(response.as_bytes()).await;
        return;
    }
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{total}\r\nAccept-Ranges: bytes\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n",
        end - start + 1
    );
    if stream.write_all(header.as_bytes()).await.is_err() {
        return;
    }
    send_paced(&mut stream, &shared, start as usize, end as usize + 1).await;
}

async fn send_paced(stream: &mut TcpStream, shared: &Shared, from: usize, to: usize) {
    let _counted = StreamGuard::new(&shared.streams);
    let opened = Instant::now();
    let mut sent = 0u64;
    let mut position = from;
    while position < to {
        let end = (position + CHUNK).min(to);
        let length = (end - position) as u64;
        let mut due = opened;
        if let Some(rate) = shared.limits.per_connection {
            due = opened + Duration::from_secs_f64(sent as f64 / rate as f64);
        }
        if let Some(rate) = shared.limits.total {
            let mut next = shared.next_slot.lock().expect("bucket lock");
            let now = Instant::now();
            let slot = now
                .checked_sub(BUCKET_BURST)
                .map_or(now, |floor| (*next).max(floor));
            *next = slot + Duration::from_secs_f64(length as f64 / rate as f64);
            due = due.max(slot);
        }
        sleep_until(due).await;
        if shared.storm_active(Instant::now()) {
            let _ = stream.shutdown().await;
            return;
        }
        if stream.write_all(&shared.body[position..end]).await.is_err() {
            return;
        }
        sent += length;
        position = end;
        shared.served.fetch_add(length, Ordering::SeqCst);
        shared.maybe_start_storm();
    }
    let _ = stream.flush().await;
}

async fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer[..position]);
            let mut lines = head.lines();
            let method = lines.next()?.split_whitespace().next()?.to_string();
            let range = lines.find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("range")
                    .then(|| parse_range(value.trim()))
                    .flatten()
            });
            return Some(Request {
                head: method.eq_ignore_ascii_case("HEAD"),
                range,
            });
        }
        if buffer.len() > 32 * 1024 {
            return None;
        }
    }
}

fn parse_range(value: &str) -> Option<(u64, Option<u64>)> {
    let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
    let end = end.trim();
    Some((
        start.trim().parse().ok()?,
        if end.is_empty() {
            None
        } else {
            Some(end.parse().ok()?)
        },
    ))
}

// ---------- tiện ích ----------

fn payload(size: usize) -> Vec<u8> {
    (0..size)
        .map(|index| ((index.wrapping_mul(31).wrapping_add(7)) % 251) as u8)
        .collect()
}

fn sha256(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(&mut hex, "{byte:02x}").expect("hex");
    }
    hex
}

fn options(initial: u16, max: u16) -> EngineOptions {
    EngineOptions {
        initial_connections: initial,
        large_initial_connections: initial,
        max_connections: max,
        host_connections: 16,
        global_connections: 32,
        max_attempts: 8,
        connect_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(5),
        // Nhỏ hơn idle_timeout như cấu hình thật: luật "kết nối treo" chạy trong mọi bài.
        stall_timeout: Duration::from_secs(2),
        sample_interval: Duration::from_millis(50),
        checkpoint_interval: Duration::from_millis(200),
        balance_interval: Duration::from_millis(300),
        user_agent: "utd-test/0.1".into(),
        quarantine_dir: None,
    }
}

fn engine(directory: &Path, options: EngineOptions) -> (Arc<Journal>, DownloadEngine) {
    std::fs::create_dir_all(directory.join("downloads")).expect("downloads");
    let journal = Arc::new(Journal::open(directory.join("utd.db")).expect("journal"));
    let engine = DownloadEngine::new(journal.clone(), options).expect("engine");
    (journal, engine)
}

/// Không đặt `connections` ⇒ tự động như app (lệnh thêm tải không gửi số kết nối).
fn request(server: &ThrottledServer, directory: &Path, name: &str, data: &[u8]) -> DownloadRequest {
    let mut request = DownloadRequest::new(
        server.url(),
        directory.join("downloads").to_string_lossy().into_owned(),
    );
    request.file_name = Some(name.into());
    request.expected_sha256 = Some(sha256(data));
    request
}

#[derive(Clone, Copy, Debug)]
struct Point {
    at: Instant,
    connections: u16,
    downloaded: u64,
}

/// Điều khiển có ghi lại số kết nối của mọi sự kiện tiến độ.
fn traced() -> (DownloadControl, JoinHandle<Vec<Point>>) {
    let (events, mut receiver) = mpsc::unbounded_channel();
    let collector = tokio::spawn(async move {
        let mut points = Vec::new();
        while let Some(event) = receiver.recv().await {
            if let EngineEvent::Progress { progress } = event {
                points.push(Point {
                    at: Instant::now(),
                    connections: progress.connections,
                    downloaded: progress.downloaded_bytes,
                });
            }
        }
        points
    });
    (DownloadControl::with_events(events), collector)
}

fn assert_complete(outcome: &DownloadOutcome, data: &[u8]) -> DownloadTask {
    let DownloadOutcome::Completed(task) = outcome else {
        panic!("expected completion: {outcome:?}");
    };
    assert_eq!(task.hash_state, HashState::Verified);
    let bytes = std::fs::read(Path::new(&task.dest_dir).join(&task.file_name)).expect("file");
    assert_eq!(bytes.len(), data.len());
    assert_eq!(sha256(&bytes), sha256(data));
    task.clone()
}

fn peak(points: &[Point]) -> u16 {
    points
        .iter()
        .map(|point| point.connections)
        .max()
        .unwrap_or(0)
}

fn summary(points: &[Point]) -> String {
    let start = points.first().map(|point| point.at);
    points
        .iter()
        .map(|point| {
            let at = start.map_or(0, |start| point.at.duration_since(start).as_millis());
            format!("{at}:{}", point.connections)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------- bài thử ----------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ramps_past_four_when_each_connection_is_throttled() {
    let _shared = ALONE.read().await;
    let data = payload(16 * MIB);
    let limits = Limits {
        per_connection: Some(MIB as u64),
        ..Limits::default()
    };
    let auto_server = ThrottledServer::start(data.clone(), limits).await;
    let fixed_server = ThrottledServer::start(data.clone(), limits).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let auto_dir = directory.path().join("auto");
    let fixed_dir = directory.path().join("fixed");
    let (_auto_journal, auto_engine) = engine(&auto_dir, options(4, 16));
    let (_fixed_journal, fixed_engine) = engine(&fixed_dir, options(4, 4));
    let auto_request = request(&auto_server, &auto_dir, "auto.bin", &data);
    let mut fixed_request = request(&fixed_server, &fixed_dir, "fixed.bin", &data);
    fixed_request.connections = 4;
    let (control, trace) = traced();

    let started = Instant::now();
    let (auto, fixed) = tokio::join!(
        async {
            let outcome = auto_engine.download(auto_request, control).await;
            (outcome, started.elapsed())
        },
        async {
            let outcome = fixed_engine
                .download(fixed_request, DownloadControl::new())
                .await;
            (outcome, started.elapsed())
        },
    );
    let points = trace.await.expect("trace");
    let (auto_outcome, auto_elapsed) = auto;
    let (fixed_outcome, fixed_elapsed) = fixed;
    let task = assert_complete(&auto_outcome.expect("auto download"), &data);
    assert_complete(&fixed_outcome.expect("fixed download"), &data);
    eprintln!(
        "per-connection: auto {auto_elapsed:?} vs fixed-4 {fixed_elapsed:?}, peak {} [{}]",
        peak(&points),
        summary(&points)
    );
    assert_eq!(
        task.connections, 16,
        "default must be automatic up to the engine max"
    );
    assert!(
        peak(&points) >= 8,
        "controller should ramp well past 4: [{}]",
        summary(&points)
    );
    assert!(
        auto_elapsed.as_secs_f64() * 1.4 < fixed_elapsed.as_secs_f64(),
        "auto {auto_elapsed:?} should clearly beat fixed-4 {fixed_elapsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stops_adding_connections_when_total_bandwidth_is_capped() {
    let _alone = ALONE.write().await;
    // Bài này so tốc độ hai bậc gần bằng nhau (trần chung) nên dùng cửa sổ gần cấu hình thật hơn
    // (thật: 500 ms / 3 s) và tải lâu hơn tương ứng.
    let data = payload(24 * MIB);
    let server = ThrottledServer::start(
        data.clone(),
        Limits {
            total: Some(4 * MIB as u64),
            ..Limits::default()
        },
    )
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let options = EngineOptions {
        sample_interval: Duration::from_millis(150),
        balance_interval: Duration::from_millis(750),
        ..options(4, 16)
    };
    let (_journal, engine) = engine(directory.path(), options);
    let (control, trace) = traced();

    let started = Instant::now();
    let outcome = engine
        .download(
            request(&server, directory.path(), "capped.bin", &data),
            control,
        )
        .await
        .expect("download");
    let elapsed = started.elapsed();
    let points = trace.await.expect("trace");
    assert_complete(&outcome, &data);
    let half = data.len() as u64 / 2;
    let late: Vec<u16> = points
        .iter()
        .filter(|point| point.downloaded >= half && point.connections > 0)
        .map(|point| point.connections)
        .collect();
    let late_mean =
        late.iter().map(|value| f64::from(*value)).sum::<f64>() / late.len().max(1) as f64;
    eprintln!(
        "total cap: {elapsed:?}, peak {}, late mean {late_mean:.1} [{}]",
        peak(&points),
        summary(&points)
    );
    assert!(
        peak(&points) <= 9,
        "should not keep adding useless connections: [{}]",
        summary(&points)
    );
    // Dưới trần chung bậc 8 đo ra nhanh hơn bậc 4 khoảng 0% ± nhiễu; nhiễu rơi vào vùng giữ (KEEP_GAIN..
    // CLIMB_GAIN, 3–8%) thì bộ điều khiển được phép ở lại 8. Bài này chỉ chứng minh không leo tiếp lên 12/16;
    // lùi đúng về 4 khi bậc trên không nhanh hơn thì test đơn vị của `engine::adaptive` kiểm (không nhiễu).
    assert!(
        late_mean <= 8.0,
        "should not stay above 8: [{}]",
        summary(&points)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn backs_off_on_429_and_503_then_recovers() {
    let _shared = ALONE.read().await;
    let data = payload(24 * MIB);
    let server = ThrottledServer::start(
        data.clone(),
        Limits {
            per_connection: Some(MIB as u64),
            storm: Some(Storm {
                crowd: 12,
                crowd_for: Duration::from_millis(250),
                length: Duration::from_millis(600),
            }),
            ..Limits::default()
        },
    )
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(4, 16));
    let (control, trace) = traced();

    let started = Instant::now();
    let outcome = engine
        .download(
            request(&server, directory.path(), "storm.bin", &data),
            control,
        )
        .await
        .expect("download");
    let elapsed = started.elapsed();
    let points = trace.await.expect("trace");
    assert_complete(&outcome, &data);
    let (storm_start, storm_end) = server.storm_window().expect("storm happened");
    assert!(
        server.rejected() > 0,
        "server should have rejected requests"
    );

    let before = peak(
        &points
            .iter()
            .copied()
            .filter(|point| point.at < storm_start)
            .collect::<Vec<_>>(),
    );
    let resumed = points
        .iter()
        .find(|point| point.at >= storm_end && point.connections > 0)
        .map(|point| point.at)
        .expect("downloads again after the storm");
    let window = Duration::from_millis(300);
    let right_after = peak(
        &points
            .iter()
            .copied()
            .filter(|point| point.at >= resumed && point.at < resumed + window)
            .collect::<Vec<_>>(),
    );
    let later = peak(
        &points
            .iter()
            .copied()
            .filter(|point| point.at >= resumed + window)
            .collect::<Vec<_>>(),
    );
    eprintln!(
        "storm: {elapsed:?}, rejected {}, before {before}, right after {right_after}, later {later} [{}]",
        server.rejected(),
        summary(&points)
    );
    assert!(
        before >= 6,
        "should ramp before the storm: [{}]",
        summary(&points)
    );
    assert!(
        u32::from(right_after) * 3 <= u32::from(before) * 2,
        "concurrency should drop after 429/503: [{}]",
        summary(&points)
    );
    assert!(
        later > right_after,
        "concurrency should recover: [{}]",
        summary(&points)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_and_resume_stay_byte_exact_while_adapting() {
    let _shared = ALONE.read().await;
    let data = payload(16 * MIB);
    let server = ThrottledServer::start(
        data.clone(),
        Limits {
            per_connection: Some(MIB as u64),
            ..Limits::default()
        },
    )
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), options(4, 16));
    let engine = Arc::new(engine);
    let (events, mut receiver) = mpsc::unbounded_channel();
    let control = DownloadControl::with_events(events);
    let cancel = control.cancel.clone();
    let request = request(&server, directory.path(), "paused.bin", &data);

    let started = Instant::now();
    let handle = tokio::spawn({
        let engine = engine.clone();
        async move { engine.download(request, control).await }
    });
    let watched = async {
        while let Some(event) = receiver.recv().await {
            if let EngineEvent::Progress { progress } = event
                && progress.connections >= 6
                && progress.downloaded_bytes >= 3 * MIB as u64
            {
                cancel.cancel();
                return progress.connections;
            }
        }
        0
    };
    let at_pause = tokio::time::timeout(Duration::from_secs(8), watched)
        .await
        .expect("ramped before timeout");
    let outcome = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("pause before timeout")
        .expect("download task")
        .expect("download result");
    let DownloadOutcome::Paused(task) = outcome else {
        panic!("expected paused outcome: {outcome:?}");
    };
    let segments = journal.load_segments(&task.id).expect("segments");
    let saved: u64 = segments.iter().map(|segment| segment.downloaded).sum();
    assert_eq!(saved, task.downloaded_bytes);
    assert!(task.downloaded_bytes > 0 && task.downloaded_bytes < data.len() as u64);

    let resumed = tokio::time::timeout(
        Duration::from_secs(8),
        engine.resume(&task.id, DownloadControl::new()),
    )
    .await
    .expect("resume before timeout")
    .expect("resume");
    let elapsed = started.elapsed();
    assert_complete(&resumed, &data);
    let served = server.served();
    eprintln!(
        "pause/resume: {elapsed:?}, paused at {} conns / {} B, server sent {served} B for {} B",
        at_pause,
        task.downloaded_bytes,
        data.len()
    );
    // Tiếp tục không tải lại phần đã có (chỉ thừa phần đang bay lúc dừng).
    assert!(
        served <= data.len() as u64 + 4 * MIB as u64,
        "resume re-downloaded too much: {served}"
    );
}
