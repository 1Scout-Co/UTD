//! Tải cần đăng nhập (notes/auth-plan.md §5.3, §9.3 T-C2..T-C4): cookie chỉ ở RAM, đi kèm mọi yêu cầu tới
//! đúng host, không sang host khác khi chuyển hướng, không xuống journal; task cần đăng nhập mà thiếu
//! cookie thì dừng trước khi gửi gì; trang đăng nhập và 401/403 được gọi đúng tên.

mod kit;

use std::sync::Arc;

use kit::*;
use utd_engine::auth::{AUTH_EXPIRED, AUTH_REQUIRED, BrowserCookie, CookieJar};
use utd_engine::{DownloadControl, DownloadOutcome, DownloadRequest};
use uuid::Uuid;
use zeroize::Zeroizing;

fn canary() -> String {
    format!("UTDCANARY{}", Uuid::new_v4().simple())
}

fn session_cookie(name: &str, value: &str, host: &str) -> BrowserCookie {
    BrowserCookie {
        name: name.into(),
        value: Zeroizing::new(value.into()),
        domain: host.into(),
        host_only: true,
        path: "/".into(),
        secure: false,
        expires: None,
    }
}

fn signed_in(cookies: Vec<BrowserCookie>) -> DownloadControl {
    DownloadControl {
        cookies: Some(Arc::new(CookieJar::new(cookies))),
        ..DownloadControl::new()
    }
}

fn error_code(outcome: &DownloadOutcome) -> &str {
    match outcome {
        DownloadOutcome::Failed(task) => {
            task.error.as_ref().map_or("", |error| error.code.as_str())
        }
        other => panic!("expected a failure, got {other:?}"),
    }
}

/// Chỉ trả file khi có `sid=<secret>`; không có thì đưa về trang đăng nhập (như trang thật).
fn gated(secret: String, body: Arc<Vec<u8>>) -> Handler {
    Arc::new(move |request: &Req| {
        if request.path.starts_with("/login") {
            return Reply::new(200)
                .header("Content-Type", "text/html; charset=utf-8")
                .body("<form method=post><input type=password></form>");
        }
        let signed_in = request.header("cookie").is_some_and(|cookie| {
            cookie
                .split("; ")
                .any(|pair| pair == format!("sid={secret}"))
        });
        if !signed_in {
            return Reply::redirect("/login?next=/file.bin");
        }
        serve_file(request, &body, &FileOpts::default())
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cookies_go_with_every_request_to_the_host_and_never_to_the_disk() {
    let secret = canary();
    let data = payload(MIB + 4321);
    let server = Server::start(gated(secret.clone(), Arc::new(data.clone()))).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), options(2, 4));
    let mut request = DownloadRequest::new(server.url("/file.bin"), downloads(directory.path()));
    request.file_name = Some("private.bin".into());
    request.max_request_bytes = Some(256 * KIB as u64);
    request.auth_sites = vec!["127.0.0.1".into()];

    let outcome = engine
        .download(
            request,
            signed_in(vec![session_cookie("sid", &secret, "127.0.0.1")]),
        )
        .await
        .expect("download");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    assert_eq!(
        std::fs::read(final_path(outcome.task())).expect("file"),
        data
    );
    let requests = server.requests();
    assert!(requests.len() > 3, "{requests:?}");
    assert!(
        requests
            .iter()
            .all(|request| request.header("cookie").is_some())
    );

    let stored = journal
        .load_task(&outcome.task().id)
        .expect("load")
        .expect("task");
    assert_eq!(stored.auth_sites, ["127.0.0.1"]);
    drop(engine);
    drop(journal);
    assert_eq!(
        files_containing(directory.path(), secret.as_bytes()),
        Vec::<std::path::PathBuf>::new(),
        "cookie không được xuống đĩa"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_redirect_to_another_host_carries_no_cookie() {
    let secret = canary();
    let data = payload(MIB);
    let target = Server::start(file_handler(Arc::new(data.clone()), FileOpts::default())).await;
    // Cùng máy, khác host: cookie riêng của 127.0.0.1 không hợp với localhost.
    let location = target
        .url("/cdn/file.bin")
        .replace("127.0.0.1", "localhost");
    let origin = Server::start(Arc::new(move |_request: &Req| Reply::redirect(&location))).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));
    let mut request = DownloadRequest::new(origin.url("/watch"), downloads(directory.path()));
    request.file_name = Some("moved.bin".into());
    request.auth_sites = vec!["127.0.0.1".into()];

    let outcome = engine
        .download(
            request,
            signed_in(vec![session_cookie("sid", &secret, "127.0.0.1")]),
        )
        .await
        .expect("download");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    assert_eq!(
        std::fs::read(final_path(outcome.task())).expect("file"),
        data
    );
    assert!(
        origin
            .requests()
            .iter()
            .all(|request| request.header("cookie").is_some())
    );
    let moved = target.requests();
    assert!(!moved.is_empty());
    for request in &moved {
        assert_eq!(request.header("cookie"), None, "{request:?}");
        assert_eq!(request.header("authorization"), None, "{request:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_signed_in_task_without_cookies_stops_before_any_request() {
    let server = Server::start(file_handler(Arc::new(payload(KIB)), FileOpts::default())).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));
    let mut request = DownloadRequest::new(server.url("/file.bin"), downloads(directory.path()));
    request.auth_sites = vec!["127.0.0.1".into()];

    let outcome = engine
        .download(request, DownloadControl::new())
        .await
        .expect("download");
    assert_eq!(error_code(&outcome), AUTH_REQUIRED);
    assert!(server.requests().is_empty(), "không được gửi yêu cầu nào");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_login_page_instead_of_the_file_means_sign_in_is_required() {
    let secret = canary();
    let server = Server::start(gated(secret.clone(), Arc::new(payload(KIB)))).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));

    let request = DownloadRequest::new(server.url("/file.bin"), downloads(directory.path()));
    let outcome = engine
        .download(request, DownloadControl::new())
        .await
        .expect("download");
    assert_eq!(error_code(&outcome), AUTH_REQUIRED);

    // Có cookie nhưng sai (phiên đã hết): vẫn về trang đăng nhập ⇒ đăng nhập hết hạn.
    let mut request = DownloadRequest::new(server.url("/file.bin"), downloads(directory.path()));
    request.auth_sites = vec!["127.0.0.1".into()];
    let outcome = engine
        .download(
            request,
            signed_in(vec![session_cookie("sid", "old", "127.0.0.1")]),
        )
        .await
        .expect("download");
    assert_eq!(error_code(&outcome), AUTH_EXPIRED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refused_cookies_stop_at_once_as_expired() {
    let server = Server::start(Arc::new(|_request: &Req| Reply::new(403))).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));
    let mut request = DownloadRequest::new(server.url("/file.bin"), downloads(directory.path()));
    request.auth_sites = vec!["127.0.0.1".into()];

    let outcome = engine
        .download(
            request,
            signed_in(vec![session_cookie("sid", "old", "127.0.0.1")]),
        )
        .await
        .expect("download");
    assert_eq!(error_code(&outcome), AUTH_EXPIRED);
    // HEAD rồi GET dò, không thử lại nhiều lượt.
    assert!(server.requests().len() <= 2, "{:?}", server.requests());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cookies_the_server_sets_are_used_for_later_requests() {
    let data = Arc::new(payload(MIB));
    let body = data.clone();
    // Như Drive: lần đầu đặt cookie xác nhận, các yêu cầu sau phải mang nó.
    let server = Server::start(Arc::new(move |request: &Req| {
        let confirmed = request
            .header("cookie")
            .is_some_and(|cookie| cookie.contains("confirm=t"));
        let reply = serve_file(request, &body, &FileOpts::default());
        if confirmed {
            reply
        } else {
            reply.header("Set-Cookie", "confirm=t; Path=/")
        }
    }))
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));
    let mut request = DownloadRequest::new(server.url("/file.bin"), downloads(directory.path()));
    request.file_name = Some("confirmed.bin".into());
    request.max_request_bytes = Some(256 * KIB as u64);
    request.auth_sites = vec!["127.0.0.1".into()];

    let outcome = engine
        .download(
            request,
            signed_in(vec![session_cookie("sid", "s", "127.0.0.1")]),
        )
        .await
        .expect("download");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    let gets = server.gets();
    let later = &gets[1..];
    assert!(!later.is_empty());
    assert!(
        later.iter().all(|request| request
            .header("cookie")
            .is_some_and(|cookie| cookie.contains("confirm=t"))),
        "{later:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_real_file_under_a_login_path_still_downloads() {
    let data = Arc::new(payload(MIB));
    let body = data.clone();
    // Đường dẫn có chữ `login` nhưng trả file thật (không phải trang HTML): không phải trang đăng nhập.
    let server = Server::start(Arc::new(move |request: &Req| {
        if request.path == "/dl" {
            return Reply::redirect("/files/login/app.zip");
        }
        serve_file(request, &body, &FileOpts::default())
    }))
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));
    let request = DownloadRequest::new(server.url("/dl"), downloads(directory.path()));
    let outcome = engine
        .download(request, DownloadControl::new())
        .await
        .expect("download");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
}
