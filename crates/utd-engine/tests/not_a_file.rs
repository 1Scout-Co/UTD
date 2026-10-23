//! AC-06: link là trang web (HTML) hoặc không có dữ liệu thay vì file ⇒ báo lỗi dễ hiểu, không
//! để lại file. Lỗi "watch" 0 byte: trang YouTube trả HEAD `text/html` kèm `Content-Length: 0`.

mod kit;

use std::path::Path;
use std::sync::Arc;

use kit::*;
use utd_engine::{DownloadControl, DownloadOutcome, DownloadRequest, DownloadTask, TaskStatus};

const PAGE: &str =
    "<!DOCTYPE html><html lang=\"vi-VN\"><head><title>Video</title></head><body>…</body></html>";
const HTML: &str = "text/html; charset=utf-8";

async fn fetch(
    server: &Server,
    path: &str,
    name: Option<&str>,
) -> (tempfile::TempDir, DownloadOutcome) {
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));
    let mut request = DownloadRequest::new(server.url(path), downloads(directory.path()));
    request.file_name = name.map(str::to_string);
    let outcome = engine
        .download(request, DownloadControl::new())
        .await
        .expect("download");
    (directory, outcome)
}

fn assert_rejected(outcome: &DownloadOutcome, code: &str) -> DownloadTask {
    let DownloadOutcome::Failed(task) = outcome else {
        panic!("expected a failure, got {outcome:?}");
    };
    assert_eq!(task.status, TaskStatus::Failed);
    let error = task.error.as_ref().expect("error");
    assert_eq!(error.code, code);
    assert!(!error.message.is_empty());
    assert!(!final_path(task).exists(), "a final file was saved");
    assert!(
        !Path::new(&task.temp_path).exists(),
        "the part file was left behind"
    );
    task.clone()
}

fn assert_saved(outcome: &DownloadOutcome, body: &[u8]) {
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    assert_eq!(
        std::fs::read(final_path(outcome.task())).expect("file"),
        body
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_a_youtube_watch_page() {
    // Như www.youtube.com/watch: HEAD 200 HTML với Content-Length: 0, GET bỏ qua Range, trả trang.
    let server = Server::start(Arc::new(|request: &Req| {
        if request.method == "HEAD" {
            return Reply::new(200)
                .header("Content-Type", HTML)
                .header("Content-Length", "0");
        }
        Reply::new(200)
            .header("Content-Type", HTML)
            .body(PAGE)
            .chunked()
    }))
    .await;
    let (_directory, outcome) = fetch(&server, "/watch?v=09OGSRptb0M", None).await;
    let task = assert_rejected(&outcome, "http.not_a_file");
    assert_eq!(task.file_name, "watch");
    assert_eq!(
        task.error.expect("error").message,
        "Link này là một trang web, không phải file."
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_an_html_page_without_a_length() {
    let server = Server::start(Arc::new(|request: &Req| {
        let reply = Reply::new(200).header("Content-Type", "text/html");
        if request.method == "HEAD" {
            reply.no_length()
        } else {
            reply.body(PAGE)
        }
    }))
    .await;
    let (_directory, outcome) = fetch(&server, "/download?id=7", Some("setup.exe")).await;
    assert_rejected(&outcome, "http.not_a_file");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_an_empty_html_response() {
    let server = Server::start(Arc::new(|_request: &Req| {
        Reply::new(200).header("Content-Type", HTML)
    }))
    .await;
    let (_directory, outcome) = fetch(&server, "/watch", None).await;
    assert_rejected(&outcome, "http.not_a_file");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_a_response_that_sends_nothing() {
    // Không báo độ dài, không gửi byte nào: không phải "file 0 byte".
    let server = Server::start(Arc::new(|request: &Req| {
        if request.method == "HEAD" {
            return Reply::new(405);
        }
        Reply::new(200).chunked()
    }))
    .await;
    let (_directory, outcome) = fetch(&server, "/file.bin", None).await;
    let task = assert_rejected(&outcome, "http.empty");
    assert_eq!(task.downloaded_bytes, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_untyped_html_after_download() {
    // Server không nói kiểu nội dung: xem đầu file lúc xong.
    let body = Arc::new(PAGE.repeat(400).into_bytes());
    let opts = FileOpts {
        content_type: None,
        ..FileOpts::default()
    };
    let server = Server::start(file_handler(body, opts)).await;
    let (_directory, outcome) = fetch(&server, "/report.pdf", None).await;
    let task = assert_rejected(&outcome, "http.not_a_file");
    assert_eq!(task.downloaded_bytes, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn keeps_a_real_html_file() {
    let body = Arc::new(PAGE.as_bytes().to_vec());
    let opts = FileOpts {
        content_type: Some(HTML.into()),
        ..FileOpts::default()
    };
    let server = Server::start(file_handler(body, opts)).await;
    let (_directory, outcome) = fetch(&server, "/docs/page.html", None).await;
    assert_saved(&outcome, PAGE.as_bytes());
    assert_eq!(outcome.task().file_name, "page.html");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn keeps_an_explicit_zero_byte_file() {
    let server = Server::start(Arc::new(|request: &Req| {
        let reply = Reply::new(200)
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", "0");
        if request.method == "HEAD" {
            reply.header("Accept-Ranges", "bytes")
        } else {
            reply
        }
    }))
    .await;
    let (_directory, outcome) = fetch(&server, "/empty.lock", None).await;
    assert_saved(&outcome, b"");
    assert_eq!(outcome.task().total_bytes, Some(0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn keeps_an_empty_file_served_by_nginx() {
    // nginx/Apache: Range trên file rỗng ⇒ 416 `bytes */0` kèm trang lỗi HTML. Kiểu nội dung là
    // của HEAD (hoặc không rõ khi HEAD hỏng), không phải của trang lỗi.
    const ERROR_PAGE: &str = "<html>\r\n<head><title>416 Requested Range Not Satisfiable</title></head>\r\n<body><center><h1>416 Requested Range Not Satisfiable</h1></center><hr><center>nginx</center></body>\r\n</html>\r\n";
    for head_works in [true, false] {
        let server = Server::start(Arc::new(move |request: &Req| {
            if request.method == "HEAD" {
                if !head_works {
                    return Reply::new(405);
                }
                return Reply::new(200)
                    .header("Content-Type", "application/octet-stream")
                    .header("Content-Length", "0")
                    .header("Accept-Ranges", "bytes");
            }
            Reply::new(416)
                .header("Content-Type", "text/html")
                .header("Content-Range", "bytes */0")
                .body(ERROR_PAGE)
        }))
        .await;
        let (_directory, outcome) = fetch(&server, "/empty.lock", None).await;
        assert_saved(&outcome, b"");
        assert_eq!(outcome.task().total_bytes, Some(0));
        assert_eq!(outcome.task().file_name, "empty.lock");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn keeps_a_binary_mislabelled_as_html() {
    // Script tải file quên đặt Content-Type (PHP mặc định text/html) nhưng nội dung là file thật.
    let mut zip = b"PK\x03\x04".to_vec();
    zip.extend(payload(300 * KIB));
    let body = Arc::new(zip.clone());
    let opts = FileOpts {
        content_type: Some("text/html".into()),
        ..FileOpts::default()
    };
    let server = Server::start(file_handler(body, opts)).await;
    let (_directory, outcome) = fetch(&server, "/files/tool.zip", None).await;
    assert_saved(&outcome, &zip);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn keeps_html_typed_attachments() {
    let body = Arc::new(payload(64 * KIB));
    let served = body.clone();
    let server = Server::start(Arc::new(move |request: &Req| {
        serve_file(
            request,
            &served,
            &FileOpts {
                content_type: Some("text/html".into()),
                ..FileOpts::default()
            },
        )
        .header("Content-Disposition", "attachment; filename=\"report.zip\"")
    }))
    .await;
    let (_directory, outcome) = fetch(&server, "/get.php?id=1", None).await;
    assert_saved(&outcome, &body);
    assert_eq!(outcome.task().file_name, "report.zip");
}
