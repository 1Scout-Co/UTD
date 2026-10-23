//! Trần byte mỗi yêu cầu (`max_request_bytes`): mỗi kết nối gửi nhiều yêu cầu Range nhỏ nối
//! nhau trên cùng kết nối keep-alive (YouTube bóp yêu cầu lớn hơn `http_chunk_size`).

mod kit;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use kit::*;
use tokio::sync::mpsc;
use utd_engine::{
    DownloadControl, DownloadOutcome, DownloadRequest, DownloadTask, EngineEvent, RequestHeader,
    TaskKind, TaskStatus,
};

fn capped(url: String, directory: &Path, name: &str, cap: Option<u64>) -> DownloadRequest {
    let mut request = DownloadRequest::new(url, downloads(directory));
    request.file_name = Some(name.into());
    request.max_request_bytes = cap;
    request
}

fn assert_capped(server: &Server, cap: u64) -> usize {
    let gets = server.gets();
    for get in &gets {
        let length = get
            .range_len()
            .unwrap_or_else(|| panic!("GET without a closed range: {get:?}"));
        assert!(
            length <= cap,
            "request of {length} bytes exceeds the {cap}-byte cap"
        );
    }
    gets.len()
}

fn assert_file(outcome: &DownloadOutcome, data: &[u8]) -> DownloadTask {
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    let task = outcome.task().clone();
    let saved = std::fs::read(final_path(&task)).expect("final file");
    assert_eq!(sha256(&saved), sha256(data), "content differs");
    task
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_request_stays_under_the_cap_and_reuses_connections() {
    let data = payload(6 * MIB);
    let server = Server::start(file_handler(Arc::new(data.clone()), FileOpts::default())).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), options(4, 4));
    let cap = 256 * KIB as u64;

    let outcome = engine
        .download(
            capped(
                server.url("/video.webm"),
                directory.path(),
                "video.webm",
                Some(cap),
            ),
            DownloadControl::new(),
        )
        .await
        .expect("download");
    let task = assert_file(&outcome, &data);

    let gets = assert_capped(&server, cap);
    assert!(gets as u64 >= 6 * MIB as u64 / cap, "only {gets} requests");
    assert!(server.peak_inflight() > 1, "requests never ran in parallel");
    assert!(
        server.connections() * 2 < gets,
        "keep-alive not reused: {} connections for {gets} requests",
        server.connections()
    );
    let stored = journal.load_task(&task.id).expect("load").expect("task");
    assert_eq!(stored.max_request_bytes, Some(cap));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_cap_dodges_a_server_that_throttles_big_ranges() {
    const SLOW: u64 = 128 * KIB as u64;
    let data = payload(8 * MIB);
    let cap = 512 * KIB as u64;
    let throttled = Arc::new(AtomicUsize::new(0));
    let body = Arc::new(data.clone());
    let handler: Handler = {
        let throttled = throttled.clone();
        Arc::new(move |request| {
            let mut reply = serve_file(request, &body, &FileOpts::default());
            let big = request.range_len().is_none_or(|length| length > cap);
            if request.is_get() && big {
                throttled.fetch_add(1, Ordering::SeqCst);
                reply.rate = Some(SLOW);
            }
            reply
        })
    };
    let server = Server::start(handler).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(4, 8));
    let engine = Arc::new(engine);

    // Có trần: mọi yêu cầu ≤ trần ⇒ hết tốc lực. Không trần cần ít nhất 8 MiB / (8 × 128 KiB/s) = 8 s.
    let started = Instant::now();
    let outcome = engine
        .download(
            capped(
                server.url("/fast.bin"),
                directory.path(),
                "fast.bin",
                Some(cap),
            ),
            DownloadControl::new(),
        )
        .await
        .expect("download");
    let elapsed = started.elapsed();
    assert_file(&outcome, &data);
    assert!(
        elapsed < Duration::from_secs(4),
        "capped download took {elapsed:?}"
    );
    assert_eq!(throttled.load(Ordering::SeqCst), 0);
    assert_capped(&server, cap);

    // Không trần: các yêu cầu lớn bị bóp, sau 1,5 s còn chưa được nửa file.
    let control = DownloadControl::new();
    let cancel = control.cancel.clone();
    let handle = tokio::spawn({
        let engine = engine.clone();
        let request = capped(server.url("/slow.bin"), directory.path(), "slow.bin", None);
        async move { engine.download(request, control).await }
    });
    tokio::time::sleep(Duration::from_millis(1500)).await;
    cancel.cancel();
    let outcome = handle.await.expect("join").expect("download");
    let DownloadOutcome::Paused(task) = outcome else {
        panic!("uncapped download should still be running: {outcome:?}");
    };
    assert!(throttled.load(Ordering::SeqCst) > 0);
    assert!(
        task.downloaded_bytes < data.len() as u64 / 2,
        "uncapped got {} bytes in 1.5 s",
        task.downloaded_bytes
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_and_resume_keep_the_cap() {
    let data = payload(4 * MIB);
    let body = Arc::new(data.clone());
    let handler: Handler = Arc::new(move |request| {
        let mut reply = serve_file(request, &body, &FileOpts::default());
        reply.rate = Some(2 * MIB as u64);
        reply
    });
    let server = Server::start(handler).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), options(2, 4));
    let engine = Arc::new(engine);
    let cap = 128 * KIB as u64;

    let (events, mut receiver) = mpsc::unbounded_channel();
    let control = DownloadControl::with_events(events);
    let cancel = control.cancel.clone();
    let handle = tokio::spawn({
        let engine = engine.clone();
        let request = capped(
            server.url("/paused.bin"),
            directory.path(),
            "paused.bin",
            Some(cap),
        );
        async move { engine.download(request, control).await }
    });
    let watched = async {
        while let Some(event) = receiver.recv().await {
            if let EngineEvent::Progress { progress } = event
                && progress.downloaded_bytes >= MIB as u64
            {
                cancel.cancel();
                break;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(15), watched)
        .await
        .expect("progress before timeout");
    let outcome = handle.await.expect("join").expect("download");
    let DownloadOutcome::Paused(task) = outcome else {
        panic!("expected pause: {outcome:?}");
    };
    assert!(task.downloaded_bytes > 0);
    let stored = journal.load_task(&task.id).expect("load").expect("task");
    assert_eq!(stored.max_request_bytes, Some(cap), "cap lost on pause");

    let resumed = tokio::time::timeout(
        Duration::from_secs(20),
        engine.resume(&task.id, DownloadControl::new()),
    )
    .await
    .expect("resume before timeout")
    .expect("resume");
    assert_file(&resumed, &data);
    assert_capped(&server, cap);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recovers_from_drops_mid_request_with_the_cap() {
    let data = payload(3 * MIB);
    let body = Arc::new(data.clone());
    let served = Arc::new(AtomicUsize::new(0));
    let cut = Arc::new(AtomicUsize::new(0));
    let handler: Handler = {
        let (served, cut) = (served.clone(), cut.clone());
        Arc::new(move |request| {
            let mut reply = serve_file(request, &body, &FileOpts::default());
            let large = request.range_len().is_some_and(|length| length > 1024);
            if request.is_get() && large && served.fetch_add(1, Ordering::SeqCst) % 4 == 3 {
                cut.fetch_add(1, Ordering::SeqCst);
                reply.cut_after = Some(10_000);
            }
            reply
        })
    };
    let server = Server::start(handler).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(3, 4));
    let cap = 192 * KIB as u64;

    let outcome = engine
        .download(
            capped(
                server.url("/flaky.bin"),
                directory.path(),
                "flaky.bin",
                Some(cap),
            ),
            DownloadControl::new(),
        )
        .await
        .expect("download");
    assert_file(&outcome, &data);
    assert!(cut.load(Ordering::SeqCst) > 0, "no drop happened");
    assert_capped(&server, cap);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn keeps_going_when_the_server_answers_shorter_ranges() {
    // CDN trả tối đa 100 KiB mỗi phản hồi dù xin 256 KiB: vẫn tải tiếp, không coi là rớt mạng.
    const SHORT: u64 = 100 * KIB as u64;
    let data = payload(2 * MIB + 77);
    let body = Arc::new(data.clone());
    let server = Server::start(Arc::new(move |request: &Req| {
        let mut shortened = request.clone();
        if let Some((start, Some(end))) = request.range {
            shortened.range = Some((start, Some(end.min(start + SHORT - 1))));
        }
        serve_file(&shortened, &body, &FileOpts::default())
    }))
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));
    let cap = 256 * KIB as u64;
    let outcome = engine
        .download(
            capped(server.url("/short.bin"), directory.path(), "short.bin", Some(cap)),
            DownloadControl::new(),
        )
        .await
        .expect("download");
    assert_file(&outcome, &data);
    assert_capped(&server, cap);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn streams_unknown_length_in_capped_ranges() {
    let cap = 256 * KIB as u64;
    // Không chia hết (đoạn cuối ngắn) và chia hết (server trả 416 ở mốc cuối).
    for size in [MIB + 12_345, MIB] {
        let data = payload(size);
        let opts = FileOpts {
            unknown_total: true,
            ..FileOpts::default()
        };
        let server = Server::start(file_handler(Arc::new(data.clone()), opts)).await;
        let directory = tempfile::tempdir().expect("tempdir");
        let (_journal, engine) = engine(directory.path(), options(2, 4));
        let outcome = engine
            .download(
                capped(
                    server.url("/live.bin"),
                    directory.path(),
                    "live.bin",
                    Some(cap),
                ),
                DownloadControl::new(),
            )
            .await
            .expect("download");
        let task = assert_file(&outcome, &data);
        assert_eq!(task.total_bytes, Some(size as u64));
        let gets = assert_capped(&server, cap);
        assert!(gets as u64 >= size as u64 / cap);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn streams_unknown_length_when_the_server_answers_shorter_ranges() {
    // Không báo tổng (`/*`) và trả tối đa 100 KiB mỗi lần: khúc ngắn không phải là hết file.
    const SHORT: u64 = 100 * KIB as u64;
    let data = payload(MIB);
    let body = Arc::new(data.clone());
    let opts = FileOpts {
        unknown_total: true,
        ..FileOpts::default()
    };
    let server = Server::start(Arc::new(move |request: &Req| {
        let mut shortened = request.clone();
        if let Some((start, Some(end))) = request.range {
            shortened.range = Some((start, Some(end.min(start + SHORT - 1))));
        }
        serve_file(&shortened, &body, &opts)
    }))
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));
    let cap = 256 * KIB as u64;
    let outcome = engine
        .download(
            capped(server.url("/cdn.bin"), directory.path(), "cdn.bin", Some(cap)),
            DownloadControl::new(),
        )
        .await
        .expect("download");
    let task = assert_file(&outcome, &data);
    assert_eq!(task.total_bytes, Some(MIB as u64));
    assert_capped(&server, cap);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_request_that_never_answers_is_retried_after_the_idle_timeout() {
    // Một yêu cầu có trần không bao giờ nhận được đầu phản hồi (kết nối chết lặng): đoạn đó phải
    // thử lại sau idle_timeout, không treo cả lượt tải.
    let data = payload(4 * MIB);
    let body = Arc::new(data.clone());
    let served = Arc::new(AtomicUsize::new(0));
    let stalled = Arc::new(AtomicUsize::new(0));
    let handler: Handler = {
        let (served, stalled) = (served.clone(), stalled.clone());
        Arc::new(move |request| {
            let mut reply = serve_file(request, &body, &FileOpts::default());
            if request.is_get() && served.fetch_add(1, Ordering::SeqCst) == 3 {
                stalled.fetch_add(1, Ordering::SeqCst);
                reply.delay = Some(Duration::from_secs(30));
            }
            reply
        })
    };
    let server = Server::start(handler).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let mut options = options(2, 4);
    options.idle_timeout = Duration::from_secs(1);
    let (_journal, engine) = engine(directory.path(), options);
    let cap = 256 * KIB as u64;

    let started = Instant::now();
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        engine.download(
            capped(
                server.url("/stall.bin"),
                directory.path(),
                "stall.bin",
                Some(cap),
            ),
            DownloadControl::new(),
        ),
    )
    .await
    .expect("download hung on a silent request")
    .expect("download");
    let elapsed = started.elapsed();
    assert_file(&outcome, &data);
    assert_eq!(stalled.load(Ordering::SeqCst), 1, "no request stalled");
    assert!(
        elapsed < Duration::from_secs(8),
        "a silent request held the download for {elapsed:?}"
    );
    assert_capped(&server, cap);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_a_hidden_media_part_with_headers_and_cap() {
    let data = payload(2 * MIB);
    let body = Arc::new(data.clone());
    let opts = FileOpts {
        content_type: Some("video/webm".into()),
        ..FileOpts::default()
    };
    let server = Server::start(Arc::new(move |request: &Req| {
        let mut reply = serve_file(request, &body, &opts);
        reply.rate = Some(2 * MIB as u64);
        reply
    }))
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), options(2, 4));
    let parent = DownloadTask::new(
        "https://www.youtube.com/watch?v=09OGSRptb0M",
        "Video.mp4",
        downloads(directory.path()),
        1,
    )
    .expect("parent")
    .with_kind(TaskKind::Media);
    journal.save_task(&parent).expect("save parent");

    let mut request = capped(
        server.url("/videoplayback?itag=399"),
        directory.path(),
        "Video.f399.webm",
        Some(256 * KIB as u64),
    );
    request.parent_id = Some(parent.id.clone());
    request.request_headers = vec![
        RequestHeader::new("User-Agent", "Mozilla/5.0 (UTD test)").expect("ua"),
        RequestHeader::new("Sec-Fetch-Mode", "navigate").expect("mode"),
    ];
    let (events, mut receiver) = mpsc::unbounded_channel();
    let outcome = engine
        .download(request, DownloadControl::with_events(events))
        .await
        .expect("download");
    let child = assert_file(&outcome, &data);
    assert_eq!(child.kind, TaskKind::MediaPart);
    assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));

    let listed = journal.list_tasks().expect("list");
    assert_eq!(
        listed.iter().map(|task| &task.id).collect::<Vec<_>>(),
        [&parent.id]
    );
    let children = journal.list_children(&parent.id).expect("children");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].status, TaskStatus::Completed);

    let mut peak = 0;
    while let Ok(event) = receiver.try_recv() {
        if let EngineEvent::Progress { progress } = event {
            assert_eq!(progress.task_id, child.id);
            peak = peak.max(progress.connections);
        }
    }
    assert!(peak >= 1, "no live connection reported");
    for request in server.requests() {
        assert_eq!(request.header("user-agent"), Some("Mozilla/5.0 (UTD test)"));
        assert_eq!(request.header("sec-fetch-mode"), Some("navigate"));
    }
}
