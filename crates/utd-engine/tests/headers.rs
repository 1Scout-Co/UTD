//! Header riêng của task (`request_headers`): gửi kèm mọi yêu cầu tới đúng origin của link,
//! bỏ khi bị chuyển hướng sang origin khác, lưu bền trong journal, chặn header nguy hiểm.

mod kit;

use std::sync::Arc;

use kit::*;
use utd_engine::{
    DownloadControl, DownloadRequest, Error, Journal, RequestHeader, allowed_request_headers,
};

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) UTD-Test";

fn task_headers() -> Vec<RequestHeader> {
    vec![
        RequestHeader::new("User-Agent", UA).expect("ua"),
        RequestHeader::new("Accept-Language", "vi-VN,vi;q=0.9").expect("lang"),
        RequestHeader::new("Sec-Fetch-Mode", "navigate").expect("mode"),
    ]
}

fn assert_has_task_headers(request: &Req) {
    assert_eq!(request.header("user-agent"), Some(UA), "{request:?}");
    assert_eq!(request.header("accept-language"), Some("vi-VN,vi;q=0.9"));
    assert_eq!(request.header("sec-fetch-mode"), Some("navigate"));
    assert_eq!(request.header("accept-encoding"), Some("identity"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sends_task_headers_on_every_request_to_the_same_origin() {
    let data = payload(2 * MIB);
    let body = Arc::new(data.clone());
    // Chuyển hướng cùng origin vẫn giữ header.
    let server = Server::start(Arc::new(move |request: &Req| {
        if request.path == "/old" {
            return Reply::redirect("/file.bin");
        }
        serve_file(request, &body, &FileOpts::default())
    }))
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), options(2, 4));
    let mut request = DownloadRequest::new(server.url("/old"), downloads(directory.path()));
    request.file_name = Some("same.bin".into());
    request.max_request_bytes = Some(256 * KIB as u64);
    request.request_headers = task_headers();

    let outcome = engine
        .download(request, DownloadControl::new())
        .await
        .expect("download");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    assert_eq!(
        std::fs::read(final_path(outcome.task())).expect("file"),
        data
    );
    let requests = server.requests();
    assert!(requests.len() > 3);
    requests.iter().for_each(assert_has_task_headers);

    let stored = journal
        .load_task(&outcome.task().id)
        .expect("load")
        .expect("task");
    assert_eq!(stored.request_headers, task_headers());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drops_task_headers_after_a_cross_origin_redirect() {
    let data = payload(MIB + 4321);
    let target = Server::start(file_handler(Arc::new(data.clone()), FileOpts::default())).await;
    let location = target.url("/cdn/file.bin");
    let origin = Server::start(Arc::new(move |_request: &Req| Reply::redirect(&location))).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));
    let mut request = DownloadRequest::new(origin.url("/watch"), downloads(directory.path()));
    request.file_name = Some("moved.bin".into());
    request.request_headers = task_headers();

    let outcome = engine
        .download(request, DownloadControl::new())
        .await
        .expect("download");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    assert_eq!(
        std::fs::read(final_path(outcome.task())).expect("file"),
        data
    );

    let first = origin.requests();
    assert!(!first.is_empty());
    first.iter().for_each(assert_has_task_headers);
    let moved = target.requests();
    assert!(moved.len() >= 2, "{moved:?}");
    for request in &moved {
        assert_eq!(
            request.header("user-agent"),
            Some("utd-test/0.1"),
            "{request:?}"
        );
        assert_eq!(request.header("accept-language"), None, "{request:?}");
        assert_eq!(request.header("sec-fetch-mode"), None, "{request:?}");
    }
}

#[test]
fn persists_headers_and_cap_across_reopen() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("utd.db");
    let mut task = utd_engine::DownloadTask::new(
        "https://rr1.example.com/videoplayback",
        "part.webm",
        downloads(directory.path()),
        16,
    )
    .expect("task");
    task.max_request_bytes = Some(10 * MIB as u64);
    task.request_headers = task_headers();
    {
        let journal = Journal::open(&path).expect("journal");
        journal.save_task(&task).expect("save");
    }
    let journal = Journal::open(&path).expect("reopen");
    let loaded = journal.load_task(&task.id).expect("load").expect("task");
    assert_eq!(loaded, task);
    let names: Vec<&str> = loaded
        .request_headers
        .iter()
        .map(|header| header.name.as_str())
        .collect();
    assert_eq!(names, ["User-Agent", "Accept-Language", "Sec-Fetch-Mode"]);
}

#[test]
fn rejects_dangerous_headers_before_anything_is_saved() {
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), options(2, 4));
    for header in [
        RequestHeader {
            name: "Cookie".into(),
            value: "SID=secret".into(),
        },
        RequestHeader {
            name: "Host".into(),
            value: "evil.example".into(),
        },
        RequestHeader {
            name: "User-Agent".into(),
            value: "ua\r\nX-Injected: 1".into(),
        },
    ] {
        let mut request =
            DownloadRequest::new("https://example.com/a.bin", downloads(directory.path()));
        request.request_headers = vec![header];
        match engine.enqueue(request) {
            Err(Error::InvalidInput(message)) => {
                assert!(!message.contains("secret") && !message.contains("X-Injected"));
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }
    assert!(journal.list_tasks().expect("list").is_empty());

    let kept = allowed_request_headers([
        ("User-Agent", UA),
        ("Cookie", "SID=secret"),
        ("Accept", "*/*"),
    ]);
    assert_eq!(kept.len(), 2);
}
