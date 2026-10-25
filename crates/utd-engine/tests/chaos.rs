//! Chaos (SPEC "Xong nghĩa là gì", AC-02, AC-03, AC-04, AC-28): 100 lần mất mạng, 20 lần kill tiến trình,
//! 5 lần mô phỏng rút điện, link ký hết hạn rồi cập nhật link, file đổi giữa chừng, 429 chờ lâu, máy chủ im,
//! đổi mạng, `.part` bị xoá. File cuối phải đúng từng byte và SHA-256.
//!
//! Kill / rút điện chạy engine trong tiến trình con thật (chính file test này, `chaos_child_worker`) rồi
//! TerminateProcess/SIGKILL nó; "rút điện" thêm: cắt cụt WAL của journal giữa một frame, bỏ `-shm`, ghi rác
//! vào mọi vùng `.part` mà journal chưa nhận, và (ổ nói dối fsync) xoá trắng đuôi các đoạn journal đã nhận.

mod kit;

use std::io::{BufRead, BufReader, Seek, SeekFrom, Write as _};
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use kit::*;
use tokio::sync::mpsc;
use tokio::time::sleep;
use utd_engine::recovery::{self, Plan, RouteProbe};
use utd_engine::{
    now_ms, DownloadControl, DownloadEngine, DownloadOutcome, DownloadRequest, DownloadTask,
    EngineEvent, EngineOptions, Journal, LinkError, RecoveryWait, TaskStatus,
};

const CHILD_ENV: &str = "UTD_CHAOS_CHILD";
const WIFI: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20));
const MOBILE: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 64, 3, 9));

/// Ngẫu nhiên đủ dùng cho bài thử (không thêm crate); in hạt giống để chạy lại đúng kịch bản.
struct Dice(u64);

impl Dice {
    fn new(label: &str) -> Self {
        let seed = std::env::var("UTD_CHAOS_SEED")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| now_ms() as u64 ^ 0x9E37_79B9_7F4A_7C15);
        eprintln!("chaos {label}: seed={seed}");
        Self(seed.max(1))
    }

    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    fn between(&mut self, low: u64, high: u64) -> u64 {
        low + self.next() % (high - low + 1)
    }
}

/// Đường ra mạng giả của engine: `None` = mất mạng, đổi địa chỉ = đổi Wi-Fi↔4G.
#[derive(Clone)]
struct FakeRoute(Arc<Mutex<Option<IpAddr>>>);

impl FakeRoute {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Some(WIFI))))
    }

    fn set(&self, address: Option<IpAddr>) {
        *self.0.lock().expect("route") = address;
    }

    fn get(&self) -> Option<IpAddr> {
        *self.0.lock().expect("route")
    }

    fn probe(&self) -> RouteProbe {
        let route = self.clone();
        RouteProbe::new(move || route.get())
    }
}

fn chaos_options() -> EngineOptions {
    EngineOptions {
        idle_timeout: Duration::from_secs(5),
        connect_timeout: Duration::from_secs(3),
        ..options(4, 4)
    }
}

fn request(url: String, directory: &Path, name: &str) -> DownloadRequest {
    let mut request = DownloadRequest::new(url, downloads(directory));
    request.file_name = Some(name.into());
    request
}

fn assert_exact(task: &DownloadTask, data: &[u8]) {
    assert_eq!(task.status, TaskStatus::Completed, "{task:?}");
    let bytes = std::fs::read(final_path(task)).expect("final file");
    assert_eq!(bytes.len(), data.len(), "size");
    assert_eq!(sha256(&bytes), sha256(data), "SHA-256");
    assert!(!Path::new(&task.temp_path).exists(), ".part left behind");
}

/// Phục vụ `data` có Range, mỗi phản hồi `rate` B/s, theo công tắc mạng.
fn network_handler(data: Arc<Vec<u8>>, switch: Arc<Switch>, rate: u64) -> Handler {
    let opts = FileOpts::default();
    Arc::new(move |request| {
        if switch.is_down() {
            return Reply::reset();
        }
        serve_file(request, &data, &opts)
            .header("ETag", "\"chaos-v1\"")
            .rate(rate)
            .on(&switch)
    })
}

/// Vòng chạy giống app (src-tauri/src/recover.rs): lỗi ⇒ `recovery::plan`; chờ mạng thì chờ tới khi có
/// route; chờ thử lại thì chờ ngắn (bài thử không cần đợi đủ lịch giãn). Không bao giờ được "bỏ cuộc".
async fn run_like_the_app(
    engine: &DownloadEngine,
    journal: &Journal,
    task_id: &str,
    route: &FakeRoute,
) -> (DownloadTask, Vec<(String, RecoveryWait)>) {
    let mut failures = Vec::new();
    for _ in 0..400 {
        let outcome = engine
            .resume(task_id, DownloadControl::new())
            .await
            .expect("run");
        let task = match outcome {
            DownloadOutcome::Completed(task) => return (task, failures),
            DownloadOutcome::Failed(task) => task,
            other => panic!("unexpected outcome {other:?}"),
        };
        let error = task.error.clone().expect("failure reason");
        let plan = recovery::plan(&task, &error, route.get().is_some(), now_ms());
        let Plan::Wait(wait) = plan else {
            panic!("gave up on {error:?} after {failures:?}");
        };
        failures.push((error.code.clone(), wait.wait));
        let mut saved = journal.load_task(task_id).expect("load").expect("task");
        saved.recovery = Some(wait.clone());
        saved.status = TaskStatus::Paused;
        journal.save_task(&saved).expect("save");
        match wait.wait {
            RecoveryWait::Network => {
                while route.get().is_none() {
                    sleep(Duration::from_millis(20)).await;
                }
            }
            RecoveryWait::Retry => sleep(Duration::from_millis(100)).await,
            other => panic!("unexpected wait {other:?} for {error:?}"),
        }
    }
    panic!("never finished: {failures:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hundred_network_drops_still_end_byte_exact() {
    let data = Arc::new(payload(20 * MIB));
    let switch = Switch::new();
    let server = Server::start(network_handler(data.clone(), switch.clone(), 128 * KIB as u64)).await;
    let route = FakeRoute::new();
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), chaos_options());
    let engine = engine.with_route(route.probe());
    let task = engine
        .enqueue(request(server.url("/drops.bin"), directory.path(), "drops.bin"))
        .expect("enqueue");

    let drops = Arc::new(AtomicUsize::new(0));
    let driver = {
        let (switch, route, drops) = (switch.clone(), route.clone(), drops.clone());
        tokio::spawn(async move {
            let mut dice = Dice::new("network");
            sleep(Duration::from_millis(400)).await;
            for drop in 1..=100usize {
                sleep(Duration::from_millis(dice.between(60, 150))).await;
                // Mỗi 25 lần một lần mất mạng lâu (modem khởi động lại): không còn route.
                let long = drop % 25 == 0;
                switch.set_down(true);
                if long {
                    route.set(None);
                    sleep(Duration::from_millis(3_600)).await;
                } else {
                    sleep(Duration::from_millis(dice.between(40, 120))).await;
                }
                // Mỗi 10 lần đổi mạng (Wi-Fi↔4G): IP nguồn khác.
                let address = if drop % 20 < 10 { WIFI } else { MOBILE };
                route.set(Some(address));
                switch.set_down(false);
                drops.store(drop, Ordering::SeqCst);
            }
        })
    };

    let started = Instant::now();
    let (done, failures) = run_like_the_app(&engine, &journal, &task.id, &route).await;
    let elapsed = started.elapsed();
    let dropped = drops.load(Ordering::SeqCst);
    driver.abort();
    eprintln!(
        "chaos network: {dropped} drops in {elapsed:?}, {} runs ended early: {failures:?}",
        failures.len()
    );
    assert_eq!(dropped, 100, "the download finished before all 100 drops");
    assert!(
        failures
            .iter()
            .filter(|(_, wait)| *wait == RecoveryWait::Network)
            .count()
            >= 3,
        "long outages: the app waits for the network instead of failing: {failures:?}"
    );
    assert_exact(&done, &data);
}

// --- Kill tiến trình và rút điện: engine chạy trong tiến trình con thật. ---

/// Tiến trình con (chỉ chạy khi có `UTD_CHAOS_CHILD`): như app vừa mở lại sau khi tắt ngang
/// (`mark_interrupted`), tải tiếp tới xong; in `TASK`, `PROGRESS <byte>`, `DONE <trạng thái>`.
#[test]
fn chaos_child_worker() {
    let Ok(directory) = std::env::var(CHILD_ENV) else {
        return;
    };
    let url = std::env::var("UTD_CHAOS_URL").expect("url");
    let name = std::env::var("UTD_CHAOS_NAME").expect("name");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async move {
        let directory = PathBuf::from(directory);
        let journal = Arc::new(Journal::open(directory.join("utd.db")).expect("journal"));
        journal.mark_interrupted().expect("interrupted");
        // Cấu hình thật (checkpoint 1 s), chỉ giới hạn 4 kết nối cho máy chủ chậm của bài thử.
        let options = EngineOptions {
            initial_connections: 4,
            large_initial_connections: 4,
            max_connections: 4,
            ..EngineOptions::default()
        };
        let engine = DownloadEngine::new(journal.clone(), options).expect("engine");
        let existing = journal
            .list_tasks()
            .expect("tasks")
            .into_iter()
            .find(|task| task.file_name == name);
        let task = match existing {
            Some(task) => task,
            None => engine
                .enqueue(request(url, &directory, &name))
                .expect("enqueue"),
        };
        say(&format!("TASK {}", task.id));
        let (events, mut inbox) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(event) = inbox.recv().await {
                if let EngineEvent::Progress { progress } = event {
                    say(&format!("PROGRESS {}", progress.downloaded_bytes));
                }
            }
        });
        let outcome = engine
            .resume(&task.id, DownloadControl::with_events(events))
            .await
            .expect("resume");
        say(&format!("DONE {:?}", outcome.task().status));
    });
}

fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

struct Child {
    process: std::process::Child,
    lines: std_mpsc::Receiver<String>,
    progress: Vec<(Instant, u64)>,
    done: Option<String>,
}

impl Child {
    fn spawn(directory: &Path, url: &str, name: &str) -> Self {
        let mut process = Command::new(std::env::current_exe().expect("test exe"))
            .args(["chaos_child_worker", "--exact", "--nocapture", "--test-threads", "1"])
            .env(CHILD_ENV, directory)
            .env("UTD_CHAOS_URL", url)
            .env("UTD_CHAOS_NAME", name)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("child");
        let stdout = process.stdout.take().expect("stdout");
        let (sender, lines) = std_mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            process,
            lines,
            progress: Vec::new(),
            done: None,
        }
    }

    fn drain(&mut self) {
        while let Ok(line) = self.lines.try_recv() {
            if let Some(bytes) = line.strip_prefix("PROGRESS ") {
                if let Ok(bytes) = bytes.trim().parse() {
                    self.progress.push((Instant::now(), bytes));
                }
            } else if let Some(status) = line.strip_prefix("DONE ") {
                self.done = Some(status.trim().to_string());
            }
        }
    }

    fn last(&self) -> u64 {
        self.progress.last().map_or(0, |(_, bytes)| *bytes)
    }

    /// Byte/giây của lượt chạy này (theo tiến độ con báo).
    fn rate(&self) -> f64 {
        match (self.progress.first(), self.progress.last()) {
            (Some((start, first)), Some((end, last))) if end > start => {
                last.saturating_sub(*first) as f64 / end.duration_since(*start).as_secs_f64()
            }
            _ => 0.0,
        }
    }

    /// Chờ con tải thêm qua mốc `bytes` (đang tải thật), tối đa `limit`.
    async fn wait_past(&mut self, bytes: u64, limit: Duration) {
        let deadline = Instant::now() + limit;
        loop {
            self.drain();
            if self.last() > bytes || self.done.is_some() {
                return;
            }
            assert!(Instant::now() < deadline, "child made no progress");
            sleep(Duration::from_millis(20)).await;
        }
    }

    fn kill(mut self) -> Self {
        self.drain();
        self.process.kill().expect("kill");
        let _ = self.process.wait();
        self.drain();
        self
    }

    async fn finish(mut self, limit: Duration) -> String {
        let deadline = Instant::now() + limit;
        loop {
            self.drain();
            if let Some(done) = self.done.take() {
                let _ = self.process.wait();
                return done;
            }
            assert!(Instant::now() < deadline, "child did not finish");
            sleep(Duration::from_millis(50)).await;
        }
    }
}

fn saved_bytes(directory: &Path, task_id: &str) -> u64 {
    let journal = Journal::open(directory.join("utd.db")).expect("journal");
    journal
        .load_task(task_id)
        .expect("load")
        .map_or(0, |task| task.downloaded_bytes)
}

fn task_by_name(directory: &Path, name: &str) -> DownloadTask {
    let journal = Journal::open(directory.join("utd.db")).expect("journal");
    journal
        .list_tasks()
        .expect("tasks")
        .into_iter()
        .find(|task| task.file_name == name)
        .expect("task")
}

/// Rút điện: WAL cụt giữa một frame, `-shm` mất; vùng `.part` journal chưa nhận thành rác; `liar` = ổ nói
/// dối fsync: đuôi mỗi đoạn journal đã nhận (theo trang 4 KiB) thành byte 0.
fn power_cut(directory: &Path, task_id: &str, dice: &mut Dice, liar: bool) -> (u64, u64) {
    // Điện mất giữa lúc ghi journal: một hai frame cuối của WAL ghi dở (frame = 24 B đầu + trang 4 KiB).
    let wal = directory.join("utd.db-wal");
    if let Ok(metadata) = std::fs::metadata(&wal)
        && metadata.len() > 32
    {
        let torn = dice.between(1, (metadata.len() - 32).min(2 * 4120));
        let keep = metadata.len() - torn;
        let file = std::fs::OpenOptions::new().write(true).open(&wal).expect("wal");
        file.set_len(keep).expect("truncate wal");
    }
    let _ = std::fs::remove_file(directory.join("utd.db-shm"));

    let journal = Journal::open(directory.join("utd.db")).expect("journal after cut");
    assert!(journal.integrity_check().expect("integrity").is_empty());
    let task = journal.load_task(task_id).expect("load").expect("task survives");
    let segments = journal.load_segments(task_id).expect("segments");
    drop(journal);

    let mut part = std::fs::OpenOptions::new()
        .write(true)
        .open(&task.temp_path)
        .expect("part");
    let mut garbage = 0u64;
    let mut zeroed = 0u64;
    for segment in &segments {
        let claimed_end = segment.start + segment.downloaded;
        // Vùng chưa nhận: dữ liệu ghi dở có thể là bất cứ gì.
        let length = segment.end + 1 - claimed_end;
        if length > 0 {
            let noise: Vec<u8> = (0..length).map(|_| dice.next() as u8).collect();
            part.seek(SeekFrom::Start(claimed_end)).expect("seek");
            part.write_all(&noise).expect("garbage");
            garbage += length;
        }
        if liar && segment.downloaded > 0 {
            let from = (claimed_end.saturating_sub(64 * 1024).div_ceil(4096) * 4096).max(segment.start);
            if from < claimed_end {
                part.seek(SeekFrom::Start(from)).expect("seek");
                part.write_all(&vec![0u8; (claimed_end - from) as usize]).expect("zero");
                zeroed += claimed_end - from;
            }
        }
    }
    part.sync_all().expect("sync");
    (garbage, zeroed)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn twenty_kills_and_five_power_cuts_keep_the_file_byte_exact() {
    let data = Arc::new(payload(40 * MIB));
    let switch = Switch::new();
    let server = Server::start(network_handler(data.clone(), switch, 192 * KIB as u64)).await;
    let directory = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(downloads(directory.path())).expect("downloads");
    let url = server.url("/crash.bin");
    let name = "crash.bin";
    let mut dice = Dice::new("crash");

    // Lượt đầu tạo task; journal được checkpoint vào file chính trước khi bắt đầu "rút điện".
    let child = Child::spawn(directory.path(), &url, name);
    let mut child = child;
    child.wait_past(0, Duration::from_secs(20)).await;
    let child = child.kill();
    drop(child);
    let task = task_by_name(directory.path(), name);

    let mut lost_seconds: Vec<f64> = Vec::new();
    for _ in 0..20 {
        let before = saved_bytes(directory.path(), &task.id);
        let mut child = Child::spawn(directory.path(), &url, name);
        child.wait_past(before, Duration::from_secs(20)).await;
        sleep(Duration::from_millis(dice.between(300, 1_500))).await;
        let child = child.kill();
        assert!(child.done.is_none(), "finished too early: make the payload bigger");
        let saved = saved_bytes(directory.path(), &task.id);
        let lost = child.last().saturating_sub(saved);
        let rate = child.rate().max(1.0);
        lost_seconds.push(lost as f64 / rate);
    }
    let worst = lost_seconds.iter().copied().fold(0.0, f64::max);
    eprintln!(
        "chaos kill: 20 kills, worst lost progress {worst:.2} s, per kill {:?}",
        lost_seconds.iter().map(|value| format!("{value:.2}")).collect::<Vec<_>>()
    );
    assert!(worst <= 5.0, "AC-02: lost {worst:.2} s of progress");

    let mut damage = Vec::new();
    for cut in 0..5 {
        let before = saved_bytes(directory.path(), &task.id);
        let mut child = Child::spawn(directory.path(), &url, name);
        child.wait_past(before, Duration::from_secs(20)).await;
        // Đủ lâu cho vài checkpoint: WAL rách chỉ mất commit cuối.
        sleep(Duration::from_millis(dice.between(1_500, 3_000))).await;
        let child = child.kill();
        assert!(child.done.is_none(), "finished too early: make the payload bigger");
        damage.push(power_cut(directory.path(), &task.id, &mut dice, cut % 2 == 1));
    }
    eprintln!("chaos power cut: (garbage, zeroed) bytes per cut {damage:?}");
    assert!(damage.iter().any(|(_, zeroed)| *zeroed > 0), "a lying disk was simulated");

    let status = Child::spawn(directory.path(), &url, name)
        .finish(Duration::from_secs(180))
        .await;
    assert_eq!(status, "Completed");
    let done = task_by_name(directory.path(), name);
    assert_exact(&done, &data);
}

// --- Link ký hết hạn rồi cập nhật link (kiểu IDM "Refresh download address"). ---

struct Signed {
    data: Arc<Vec<u8>>,
    other: Arc<Vec<u8>>,
    sig: AtomicU64,
    validators: bool,
    switch: Arc<Switch>,
}

fn signed_handler(state: Arc<Signed>) -> Handler {
    let opts = FileOpts::default();
    Arc::new(move |request| {
        let sig = request
            .path
            .split_once("sig=")
            .and_then(|(_, value)| value.parse::<u64>().ok());
        let (body, etag) = if request.path.starts_with("/other") {
            (&state.other, "\"other\"")
        } else if request.path.starts_with("/short") {
            return serve_file(request, &state.data[..1024], &opts);
        } else {
            (&state.data, "\"model-v1\"")
        };
        if !request.path.starts_with("/other") && sig != Some(state.sig.load(Ordering::SeqCst)) {
            return Reply::new(403);
        }
        let reply = serve_file(request, body, &opts).rate(256 * KIB as u64).on(&state.switch);
        if state.validators {
            reply
                .header("ETag", etag)
                .header("Last-Modified", "Tue, 20 Oct 2026 08:00:00 GMT")
        } else {
            reply
        }
    })
}

async fn expire_midway(validators: bool) {
    let data = Arc::new(payload(6 * MIB));
    let mut other = payload(6 * MIB);
    other.reverse();
    let state = Arc::new(Signed {
        data: data.clone(),
        other: Arc::new(other),
        sig: AtomicU64::new(1),
        validators,
        switch: Switch::new(),
    });
    let server = Server::start(signed_handler(state.clone())).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), chaos_options());
    let engine = Arc::new(engine);
    let mut first = request(server.url("/model.bin?sig=1"), directory.path(), "model.bin");
    // Mỗi yêu cầu ≤ 512 KiB như link ký của CDN: hết hạn là yêu cầu kế tiếp nhận 403 ngay.
    first.max_request_bytes = Some(512 * KIB as u64);
    let task = engine.enqueue(first).expect("enqueue");

    let run = {
        let engine = engine.clone();
        let id = task.id.clone();
        tokio::spawn(async move { engine.resume(&id, DownloadControl::new()).await })
    };
    loop {
        let saved = journal.load_task(&task.id).expect("load").expect("task");
        if saved.downloaded_bytes > MIB as u64 {
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    // Link hết hạn: yêu cầu mới bị 403, kết nối đang chạy bị cắt.
    let expired = Instant::now();
    state.sig.store(2, Ordering::SeqCst);
    state.switch.cut();
    let outcome = run.await.expect("join").expect("run");
    let DownloadOutcome::Failed(failed) = outcome else {
        panic!("expected failure, got {outcome:?}");
    };
    assert_eq!(failed.error.as_ref().unwrap().code, "http.403");
    assert!(expired.elapsed() < Duration::from_secs(6), "403 must not be retried for a minute");
    let kept = failed.downloaded_bytes;
    assert!(kept > MIB as u64 && kept < data.len() as u64);
    let Plan::Wait(wait) = recovery::plan(&failed, failed.error.as_ref().unwrap(), true, now_ms())
    else {
        panic!("an expired link waits for a new one");
    };
    assert_eq!(wait.wait, RecoveryWait::Link);

    // Link mới trỏ tới file khác: không nhận, phần đã tải còn nguyên.
    let before = std::fs::read(&failed.temp_path).expect("part");
    let wrong_size = engine
        .replace_link(&task.id, &server.url("/short.bin?sig=2"), Vec::new(), None)
        .await;
    assert!(matches!(wrong_size, Err(LinkError::Size { .. })), "{wrong_size:?}");
    let different = engine
        .replace_link(&task.id, &server.url("/other.bin"), Vec::new(), None)
        .await;
    assert_eq!(different.unwrap_err(), LinkError::Changed);
    let still_expired = engine
        .replace_link(&task.id, &server.url("/model.bin?sig=1"), Vec::new(), None)
        .await;
    assert!(matches!(still_expired, Err(LinkError::Unreachable(_))), "{still_expired:?}");
    assert_eq!(std::fs::read(&failed.temp_path).expect("part"), before);
    assert_eq!(saved_bytes(directory.path(), &task.id), kept);

    // Link mới đúng file: nhận, tải tiếp từ phần đã có.
    let relinked = engine
        .replace_link(&task.id, &server.url("/model.bin?sig=2"), Vec::new(), None)
        .await
        .expect("same file");
    assert_eq!(relinked.downloaded_bytes, kept);
    assert!(relinked.url.ends_with("sig=2"));
    let gets_before = server.gets().len();
    let outcome = engine
        .resume(&task.id, DownloadControl::new())
        .await
        .expect("resume");
    assert_exact(outcome.task(), &data);
    let resent: u64 = server.gets()[gets_before..]
        .iter()
        .filter_map(Req::range_len)
        .sum();
    assert!(
        resent < data.len() as u64 - kept / 2,
        "kept part was downloaded again: {resent} B resent, {kept} B kept"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_expired_signed_link_is_updated_and_keeps_the_downloaded_part() {
    expire_midway(true).await;
}

/// Máy chủ không gửi ETag/Last-Modified: so vài đoạn byte đã tải với link mới.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_validators_the_new_link_is_checked_by_sampling_bytes() {
    expire_midway(false).await;
}

// --- File đổi giữa chừng, 429 chờ lâu, máy chủ im, mất mạng / đổi mạng, `.part` bị xoá. ---

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_replaced_mid_download_stops_without_mixing_versions() {
    let old = Arc::new(payload(4 * MIB));
    let mut fresh = payload(4 * MIB);
    fresh.reverse();
    let fresh = Arc::new(fresh);
    let served = Arc::new(AtomicU64::new(0));
    let handler: Handler = {
        let (old, fresh, served) = (old.clone(), fresh.clone(), served.clone());
        let opts = FileOpts::default();
        Arc::new(move |request| {
            // Sau ~1 MiB, máy chủ thay file cùng kích thước (ETag mới).
            let replaced = served.load(Ordering::SeqCst) > MIB as u64;
            let (body, etag) = if replaced {
                (&fresh, "\"v2\"")
            } else {
                (&old, "\"v1\"")
            };
            let reply = serve_file(request, body, &opts).header("ETag", etag);
            if let Some(length) = request.range_len() {
                served.fetch_add(length.min(256 * KIB as u64), Ordering::SeqCst);
            }
            reply.rate(512 * KIB as u64)
        })
    };
    let server = Server::start(handler).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), chaos_options());
    let mut changing = request(server.url("/release.zip"), directory.path(), "release.zip");
    changing.max_request_bytes = Some(256 * KIB as u64);
    let outcome = engine
        .download(changing, DownloadControl::new())
        .await
        .expect("download");
    let DownloadOutcome::Failed(task) = outcome else {
        panic!("AC-04: expected a stop, got {outcome:?}");
    };
    assert_eq!(task.error.as_ref().unwrap().code, "file.changed");
    assert!(!final_path(&task).exists(), "never place a mixed file");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_long_retry_after_ends_the_run_with_a_scheduled_retry() {
    let data = Arc::new(payload(2 * MIB));
    let limited = Arc::new(AtomicU64::new(0));
    let handler: Handler = {
        let (data, limited) = (data.clone(), limited.clone());
        let opts = FileOpts::default();
        Arc::new(move |request| {
            if request.method == "HEAD" {
                return serve_file(request, &data, &opts);
            }
            limited.fetch_add(1, Ordering::SeqCst);
            Reply::new(429).header("Retry-After", "1800")
        })
    };
    let server = Server::start(handler).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), chaos_options());
    let started = Instant::now();
    let outcome = engine
        .download(
            request(server.url("/busy.bin"), directory.path(), "busy.bin"),
            DownloadControl::new(),
        )
        .await
        .expect("download");
    let elapsed = started.elapsed();
    let DownloadOutcome::Failed(task) = outcome else {
        panic!("expected a failure, got {outcome:?}");
    };
    assert_eq!(task.error.as_ref().unwrap().code, "http.429");
    assert!(elapsed < Duration::from_secs(5), "did not wait in-run: {elapsed:?}");
    let hinted = task.recovery.as_ref().and_then(|wait| wait.retry_at).expect("hint");
    let ahead = hinted - now_ms();
    assert!((1_790_000..=1_800_500).contains(&ahead), "Retry-After kept: {ahead} ms");
    let Plan::Wait(wait) = recovery::plan(&task, task.error.as_ref().unwrap(), true, now_ms()) else {
        panic!("429 is retried later");
    };
    assert!(wait.retry_at.unwrap() >= hinted);
    assert!(limited.load(Ordering::SeqCst) <= 8, "hammered the server");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_that_accepts_then_stays_silent_is_given_up_quickly() {
    let handler: Handler = Arc::new(|_request| {
        let mut reply = Reply::new(200);
        reply.delay = Some(Duration::from_secs(120));
        reply
    });
    let server = Server::start(handler).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), chaos_options());
    let started = Instant::now();
    let outcome = engine
        .download(
            request(server.url("/silent.bin"), directory.path(), "silent.bin"),
            DownloadControl::new(),
        )
        .await
        .expect("download");
    let elapsed = started.elapsed();
    let DownloadOutcome::Failed(task) = outcome else {
        panic!("expected a failure, got {outcome:?}");
    };
    assert_eq!(task.error.as_ref().unwrap().code, "network.timeout");
    // HEAD (≤ hạn kết nối) + GET (hạn kết nối), không thử lại trong lượt: app hẹn giờ thử lại.
    assert!(elapsed < Duration::from_secs(12), "stuck resolving for {elapsed:?}");
}

/// Mạng chết lặng (kết nối mở mà không còn byte) kèm mất route: dừng lượt `network.offline` sau vài giây
/// thay vì chờ hết hạn im 30 s × số lượt; có mạng lại thì tải tiếp đúng byte.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn going_offline_ends_the_run_fast_and_resumes_when_back() {
    let data = Arc::new(payload(8 * MIB));
    let outage = Outage::new(Duration::from_millis(800), Duration::from_secs(600));
    let quiet = Arc::new(AtomicU64::new(1));
    let handler: Handler = {
        let (data, outage, quiet) = (data.clone(), outage.clone(), quiet.clone());
        let opts = FileOpts::default();
        Arc::new(move |request| {
            let mut reply = serve_file(request, &data, &opts).rate(512 * KIB as u64);
            if quiet.load(Ordering::SeqCst) == 1 {
                reply.outage = Some(outage.clone());
            }
            reply
        })
    };
    let server = Server::start(handler).await;
    let route = FakeRoute::new();
    let directory = tempfile::tempdir().expect("tempdir");
    let options = EngineOptions {
        idle_timeout: Duration::from_secs(30),
        stall_timeout: Duration::from_secs(5),
        ..chaos_options()
    };
    let (journal, engine) = engine(directory.path(), options);
    let engine = engine.with_route(route.probe());
    let task = engine
        .enqueue(request(server.url("/offline.bin"), directory.path(), "offline.bin"))
        .expect("enqueue");
    let cutter = {
        let route = route.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(900)).await;
            route.set(None);
        })
    };
    let started = Instant::now();
    let outcome = engine
        .resume(&task.id, DownloadControl::new())
        .await
        .expect("run");
    let elapsed = started.elapsed();
    cutter.await.expect("cutter");
    let DownloadOutcome::Failed(failed) = outcome else {
        panic!("expected offline, got {outcome:?}");
    };
    assert_eq!(failed.error.as_ref().unwrap().code, "network.offline");
    assert!(elapsed < Duration::from_secs(10), "offline detected after {elapsed:?}");
    let Plan::Wait(wait) = recovery::plan(&failed, failed.error.as_ref().unwrap(), false, now_ms())
    else {
        panic!("offline waits for the network");
    };
    assert_eq!(wait.wait, RecoveryWait::Network);

    quiet.store(0, Ordering::SeqCst);
    route.set(Some(WIFI));
    let (done, _) = run_like_the_app(&engine, &journal, &task.id, &route).await;
    assert_exact(&done, &data);
}

/// Đổi Wi-Fi↔4G: kết nối cũ chết lặng; engine thấy IP nguồn đổi thì mở lại ngay, không chờ 30 s.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn switching_networks_reconnects_without_waiting_for_the_idle_timeout() {
    let data = Arc::new(payload(6 * MIB));
    let first_wave = Arc::new(AtomicU64::new(1));
    let handler: Handler = {
        let (data, first_wave) = (data.clone(), first_wave.clone());
        let opts = FileOpts::default();
        Arc::new(move |request| {
            let mut reply = serve_file(request, &data, &opts).rate(1024 * KIB as u64);
            if request.is_get() && first_wave.load(Ordering::SeqCst) == 1 && request.range.is_some() {
                // Kết nối của mạng cũ: gửi một ít rồi im mãi (gói tin không còn tới).
                reply.stall_after = Some(256 * KIB);
            }
            reply
        })
    };
    let server = Server::start(handler).await;
    let route = FakeRoute::new();
    let directory = tempfile::tempdir().expect("tempdir");
    let options = EngineOptions {
        idle_timeout: Duration::from_secs(30),
        stall_timeout: Duration::from_secs(20),
        ..chaos_options()
    };
    let (_journal, engine) = engine(directory.path(), options);
    let engine = engine.with_route(route.probe());
    let switcher = {
        let (route, first_wave) = (route.clone(), first_wave.clone());
        tokio::spawn(async move {
            sleep(Duration::from_millis(1_200)).await;
            first_wave.store(0, Ordering::SeqCst);
            route.set(Some(MOBILE));
        })
    };
    let started = Instant::now();
    let outcome = engine
        .download(
            request(server.url("/switch.bin"), directory.path(), "switch.bin"),
            DownloadControl::new(),
        )
        .await
        .expect("download");
    let elapsed = started.elapsed();
    switcher.await.expect("switcher");
    assert_exact(outcome.task(), &data);
    assert!(elapsed < Duration::from_secs(20), "waited for the idle timeout: {elapsed:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_part_file_deleted_mid_download_is_reported_then_downloaded_again() {
    let data = Arc::new(payload(6 * MIB));
    let switch = Switch::new();
    let server = Server::start(network_handler(data.clone(), switch, 512 * KIB as u64)).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), chaos_options());
    let engine = Arc::new(engine);
    let task = engine
        .enqueue(request(server.url("/gone.bin"), directory.path(), "gone.bin"))
        .expect("enqueue");
    let run = {
        let engine = engine.clone();
        let id = task.id.clone();
        tokio::spawn(async move { engine.resume(&id, DownloadControl::new()).await })
    };
    loop {
        let saved = journal.load_task(&task.id).expect("load").expect("task");
        if saved.downloaded_bytes > MIB as u64 {
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    // Người dùng xoá `.part` (Windows cho xoá vì engine mở kèm FILE_SHARE_DELETE).
    std::fs::remove_file(&task.temp_path).expect("delete part");
    let outcome = run.await.expect("join").expect("run");
    let DownloadOutcome::Failed(failed) = outcome else {
        panic!("expected a clear failure, got {outcome:?}");
    };
    assert_eq!(failed.error.as_ref().unwrap().code, "file.part_missing");
    // Mở lại (vd sau khi khởi động lại app) vẫn báo, không âm thầm tải lại từ 0.
    let again = engine
        .resume(&task.id, DownloadControl::new())
        .await
        .expect("resume");
    assert_eq!(again.task().error.as_ref().unwrap().code, "file.part_missing");
    // Người dùng bấm "Tải lại từ đầu".
    engine.reset_progress(&task.id).expect("reset");
    let outcome = engine
        .resume(&task.id, DownloadControl::new())
        .await
        .expect("resume");
    assert_exact(outcome.task(), &data);
}
