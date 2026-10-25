//! Tải cần đăng nhập (notes/auth-plan.md §5.3, §9.3 T-C2..T-C4): cookie chỉ ở RAM, đi kèm mọi yêu cầu tới
//! đúng host, không sang host khác khi chuyển hướng, không xuống journal; task cần đăng nhập mà thiếu
//! cookie thì dừng trước khi gửi gì; trang đăng nhập và 401/403 được gọi đúng tên.

mod kit;

use std::sync::Arc;

use kit::*;
use utd_engine::auth::{AUTH_EXPIRED, AUTH_REQUIRED, BrowserCookie, CookieJar};
use utd_engine::recovery::{self, Plan};
use utd_engine::{now_ms, DownloadControl, DownloadOutcome, DownloadRequest, RecoveryWait};
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sign_in_wait_takes_a_new_link_only_with_the_cookies() {
    let secret = canary();
    let data = payload(MIB + 77);
    let server = Server::start(gated(secret.clone(), Arc::new(data.clone()))).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), options(2, 4));

    // Chưa đăng nhập: về trang đăng nhập ⇒ chờ link mới như link hết hạn (một đường chờ, recover.rs).
    let mut request = DownloadRequest::new(server.url("/file.bin"), downloads(directory.path()));
    request.file_name = Some("private.bin".into());
    let outcome = engine
        .download(request, DownloadControl::new())
        .await
        .expect("download");
    assert_eq!(error_code(&outcome), AUTH_REQUIRED);
    let failed = outcome.task().clone();
    let Plan::Wait(wait) = recovery::plan(&failed, failed.error.as_ref().unwrap(), true, now_ms())
    else {
        panic!("sign-in waits for a new link");
    };
    assert_eq!(wait.wait, RecoveryWait::Link);

    // Link mới không kèm đăng nhập: vẫn là trang đăng nhập, không nhận.
    let bare = engine
        .replace_link(&failed.id, &server.url("/file.bin?v=2"), Vec::new(), None)
        .await;
    assert!(bare.is_err(), "{bare:?}");

    // Tải lại từ trình duyệt kèm đăng nhập: dò bằng cookie, ghi dấu trang cần đăng nhập.
    let jar = Arc::new(CookieJar::new(vec![session_cookie("sid", &secret, "127.0.0.1")]));
    let relinked = engine
        .replace_link(
            &failed.id,
            &server.url("/file.bin?v=2"),
            Vec::new(),
            Some(("127.0.0.1", jar.clone())),
        )
        .await
        .expect("signed link");
    assert_eq!(relinked.auth_sites, ["127.0.0.1"]);
    assert!(relinked.recovery.is_none() && relinked.error.is_none());

    // Lượt sau không cookie (vd mở lại app) dừng ngay; có cookie thì tải xong.
    let outcome = engine
        .resume(&failed.id, DownloadControl::new())
        .await
        .expect("resume");
    assert_eq!(error_code(&outcome), AUTH_REQUIRED);
    let control = DownloadControl {
        cookies: Some(jar),
        ..DownloadControl::new()
    };
    let outcome = engine.resume(&failed.id, control).await.expect("resume");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    assert_eq!(std::fs::read(final_path(outcome.task())).expect("file"), data);
    drop(engine);
    drop(journal);
    assert!(files_containing(directory.path(), secret.as_bytes()).is_empty());
}

/// Trang xác nhận quét virus của máy chủ giả kiểu Google Drive (rút gọn từ trang thật): form GET `download-form`.
const DRIVE_CONFIRM: &str = r#"<!DOCTYPE html><html><head><title>Google Drive - Virus scan warning</title></head>
<body><p>Google Drive can&#39;t scan this file for viruses.</p><form id="download-form" action="/download"
method="get"><input type="submit" id="uc-download-link" value="Download anyway"/><input type="hidden" name="id"
value="big"><input type="hidden" name="export" value="download"><input type="hidden" name="confirm" value="t">
<input type="hidden" name="uuid" value="u-1"></form></body></html>"#;

/// Như `drive.usercontent.google.com/download`: cần cookie phiên (thiếu thì về trang đăng nhập), file lớn ra
/// trang xác nhận trước, gửi form (có `uuid`) mới ra file; tên file nằm trong Content-Disposition.
fn drive_like(secret: String, body: Arc<Vec<u8>>) -> Handler {
    Arc::new(move |request: &Req| {
        if request.path.starts_with("/ServiceLogin") {
            return Reply::new(200)
                .header("Content-Type", "text/html; charset=utf-8")
                .body("<form><input type=password></form>");
        }
        let signed_in = request.header("cookie").is_some_and(|cookie| {
            cookie
                .split("; ")
                .any(|pair| pair == format!("SID={secret}"))
        });
        if !signed_in {
            return Reply::redirect("/ServiceLogin?continue=/download");
        }
        let query = request.path.split_once('?').map_or("", |(_, query)| query);
        let has = |pair: &str| query.split('&').any(|item| item == pair);
        if has("id=big") && !has("uuid=u-1") {
            return Reply::new(200)
                .header("Content-Type", "text/html; charset=utf-8")
                .body(DRIVE_CONFIRM);
        }
        serve_file(request, &body, &FileOpts::default()).header(
            "Content-Disposition",
            "attachment; filename=\"clip.mp4\"; filename*=UTF-8''Clip%20quay%20ng%C3%A0y%201.mp4",
        )
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_drive_virus_scan_page_is_followed_with_the_cookies_and_named_by_the_server() {
    let secret = canary();
    let data = payload(MIB + 999);
    let server = Server::start(drive_like(secret.clone(), Arc::new(data.clone()))).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));
    let link = server.url("/download?id=big&export=download&confirm=t");

    // Chưa đăng nhập: về trang đăng nhập, không tạo file "download".
    let outcome = engine
        .download(DownloadRequest::new(link.clone(), downloads(directory.path())), DownloadControl::new())
        .await
        .expect("download");
    assert_eq!(error_code(&outcome), AUTH_REQUIRED);

    // Kèm cookie: trang xác nhận ⇒ theo form (thêm `uuid`) ⇒ file thật, tên từ Content-Disposition.
    let mut request = DownloadRequest::new(link, downloads(directory.path()));
    request.auth_sites = vec!["127.0.0.1".into()];
    let outcome = engine
        .download(request, signed_in(vec![session_cookie("SID", &secret, "127.0.0.1")]))
        .await
        .expect("download");
    assert!(outcome.is_completed(), "unexpected outcome: {outcome:?}");
    let task = outcome.task();
    assert_eq!(task.file_name, "Clip quay ngày 1.mp4");
    assert!(task.final_url.as_deref().is_some_and(|url| url.contains("uuid=u-1")), "{:?}", task.final_url);
    assert_eq!(std::fs::read(final_path(task)).expect("file"), data);
    let pages = server
        .requests()
        .iter()
        .filter(|request| request.method == "GET" && request.path.contains("id=big") && !request.path.contains("uuid"))
        .count();
    assert!(pages <= 3, "đọc trang xác nhận có giới hạn: {pages}");
}
