//! Kết nối treo (server nhận yêu cầu rồi im, không đóng kết nối). Các kết nối khác vẫn nhận byte
//! thì engine cắt kết nối treo sau `stall_timeout` rồi mở lại, không chờ hết `idle_timeout`; chỉ
//! còn một kết nối, hoặc mọi kết nối cùng im (mạng rớt cả loạt), thì vẫn chờ đủ `idle_timeout`.

mod kit;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kit::*;
use utd_engine::{DownloadControl, DownloadEngine, DownloadOutcome, DownloadRequest};

/// Mỗi phản hồi 8 MiB/s: các kết nối khác còn đang chạy suốt lúc một kết nối treo.
const RATE: u64 = 8 * MIB as u64;

/// Phục vụ file; GET thứ `nth` (đếm từ 0) bị `spoil` làm treo, `hits` đếm số lần đã treo.
fn spoiled(
    body: Arc<Vec<u8>>,
    nth: usize,
    spoil: fn(&mut Reply),
    hits: Arc<AtomicUsize>,
) -> Handler {
    let gets = AtomicUsize::new(0);
    Arc::new(move |request| {
        let mut reply = serve_file(request, &body, &FileOpts::default());
        reply.rate = Some(RATE);
        if request.is_get() && gets.fetch_add(1, Ordering::SeqCst) == nth {
            hits.fetch_add(1, Ordering::SeqCst);
            spoil(&mut reply);
        }
        reply
    })
}

fn hang_mid_body(reply: &mut Reply) {
    reply.stall_after = Some(16 * KIB);
}

fn hang_before_head(reply: &mut Reply) {
    reply.delay = Some(Duration::from_secs(60));
}

fn request(server: &Server, directory: &Path, name: &str) -> DownloadRequest {
    let mut request = DownloadRequest::new(server.url("/stall.bin"), downloads(directory));
    request.file_name = Some(name.into());
    request
}

async fn timed(engine: &DownloadEngine, request: DownloadRequest) -> (DownloadOutcome, Duration) {
    let started = Instant::now();
    let outcome = tokio::time::timeout(
        Duration::from_secs(25),
        engine.download(request, DownloadControl::new()),
    )
    .await
    .expect("a hung connection held the download")
    .expect("download");
    (outcome, started.elapsed())
}

fn assert_saved(outcome: &DownloadOutcome, data: &[u8]) {
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    let saved = std::fs::read(final_path(outcome.task())).expect("final file");
    assert_eq!(sha256(&saved), sha256(data), "content differs");
}

/// Bốn kết nối, một kết nối treo; `idle_timeout` 30 s như mặc định, hạn treo 1 s.
async fn one_of_four_hangs(spoil: fn(&mut Reply)) -> Duration {
    let data = payload(8 * MIB);
    let hits = Arc::new(AtomicUsize::new(0));
    let server = Server::start(spoiled(Arc::new(data.clone()), 1, spoil, hits.clone())).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let mut options = options(4, 4);
    options.idle_timeout = Duration::from_secs(30);
    options.stall_timeout = Duration::from_secs(1);
    // Một lượt thử: lần cắt vì treo không được tính là lượt hỏng.
    options.max_attempts = 1;
    let (_journal, engine) = engine(directory.path(), options);

    let (outcome, elapsed) = timed(&engine, request(&server, directory.path(), "stall.bin")).await;
    assert_saved(&outcome, &data);
    assert_eq!(hits.load(Ordering::SeqCst), 1, "no connection hung");
    elapsed
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_connection_that_hangs_mid_body_is_cut_while_others_run() {
    let elapsed = one_of_four_hangs(hang_mid_body).await;
    assert!(
        elapsed < Duration::from_secs(8),
        "a hung connection held the download for {elapsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_request_that_never_answers_is_cut_while_others_run() {
    let elapsed = one_of_four_hangs(hang_before_head).await;
    assert!(
        elapsed < Duration::from_secs(8),
        "a silent request held the download for {elapsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lone_connection_waits_the_full_idle_timeout() {
    // Không có kết nối nào khác nhận byte để so: im 0,3 s chưa phải treo, chờ đủ idle 2 s rồi
    // mới thử lại (tính một lượt).
    let data = payload(2 * MIB);
    let hits = Arc::new(AtomicUsize::new(0));
    let handler = spoiled(Arc::new(data.clone()), 0, hang_mid_body, hits.clone());
    let server = Server::start(handler).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let mut options = options(1, 1);
    options.idle_timeout = Duration::from_secs(2);
    options.stall_timeout = Duration::from_millis(300);
    let (_journal, engine) = engine(directory.path(), options);

    let (outcome, elapsed) = timed(&engine, request(&server, directory.path(), "lone.bin")).await;
    assert_saved(&outcome, &data);
    assert_eq!(hits.load(Ordering::SeqCst), 1, "the connection never hung");
    assert!(
        elapsed >= Duration::from_secs(2),
        "a lone connection was cut after {elapsed:?}, before the idle timeout"
    );
    assert_eq!(
        server.gets().len(),
        2,
        "expected one retry after the idle timeout"
    );
}

/// Phục vụ file; mọi GET cùng im theo `outage`, `sent` ghi lúc từng GET tới.
fn with_outage(body: Arc<Vec<u8>>, outage: Arc<Outage>, sent: Arc<Mutex<Vec<Instant>>>) -> Handler {
    Arc::new(move |request| {
        let mut reply = serve_file(request, &body, &FileOpts::default());
        if request.is_get() {
            sent.lock().expect("sent").push(Instant::now());
            reply.rate = Some(RATE);
            reply.outage = Some(outage.clone());
        }
        reply
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_global_outage_cuts_no_connection() {
    // Bốn kết nối cùng im 2,5 s giữa lúc tải; hạn treo 1 s, idle 5 s. Không kết nối nào nhận byte
    // trong đợt rớt nên không có kết nối nào bị coi là treo: không cắt, không mở lại, không mất lượt.
    // Mỗi đoạn 6 MiB: lúc mạng rớt còn dưới 8 MiB nên không bị tách thêm đoạn.
    let data = payload(24 * MIB);
    let outage = Outage::new(Duration::from_millis(500), Duration::from_millis(2500));
    let sent = Arc::new(Mutex::new(Vec::new()));
    let handler = with_outage(Arc::new(data.clone()), outage.clone(), sent.clone());
    let server = Server::start(handler).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let mut options = options(4, 4);
    options.idle_timeout = Duration::from_secs(5);
    options.stall_timeout = Duration::from_secs(1);
    options.max_attempts = 1;
    let (_journal, engine) = engine(directory.path(), options);

    let (outcome, _) = timed(&engine, request(&server, directory.path(), "outage.bin")).await;
    assert_saved(&outcome, &data);
    assert!(outage.held() > 0, "the outage never paused a reply");
    let (begin, end) = outage.window().expect("outage window");
    let settle = end + Duration::from_millis(500);
    let reopened = sent
        .lock()
        .expect("sent")
        .iter()
        .filter(|at| **at >= begin && **at < settle)
        .count();
    assert_eq!(
        reopened, 0,
        "connections were cut and reopened during the outage"
    );
}
