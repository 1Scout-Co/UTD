//! `DownloadEngine::inspect`: dò tên, kích thước và "có phải trang web" cho bảng xem trước của hộp
//! Thêm hàng loạt, không tạo task nào trong journal.

mod kit;

use std::sync::Arc;

use kit::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reports_name_and_size_without_creating_a_task() {
    let body = Arc::new(payload(300_000));
    let server = Server::start(file_handler(body, FileOpts::default())).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), options(2, 4));

    let file = engine
        .inspect(&server.url("/files/b%E1%BA%A3n%20%C4%91%E1%BB%93.zip"))
        .await
        .expect("inspect");
    assert_eq!(file.file_name, "bản đồ.zip");
    assert_eq!(file.total_bytes, Some(300_000));
    assert!(!file.page);
    assert!(
        journal.list_tasks().expect("list").is_empty(),
        "không tạo task"
    );
    assert!(
        server
            .gets()
            .iter()
            .all(|request| request.range_len().is_none_or(|len| len <= 1))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn flags_web_pages_but_trusts_attachments() {
    let server = Server::start(Arc::new(|request: &Req| {
        let reply = Reply::new(200).header("Content-Type", "text/html; charset=utf-8");
        if request.path.starts_with("/get") {
            reply
                .header(
                    "Content-Disposition",
                    "attachment; filename=\"report.html\"",
                )
                .body("<html></html>")
        } else {
            reply.body("<!DOCTYPE html><html></html>")
        }
    }))
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (_journal, engine) = engine(directory.path(), options(2, 4));

    let page = engine.inspect(&server.url("/about")).await.expect("page");
    assert!(page.page, "trang HTML");
    let attachment = engine
        .inspect(&server.url("/get?id=1"))
        .await
        .expect("attachment");
    assert!(!attachment.page);
    assert_eq!(attachment.file_name, "report.html");

    let error = engine.inspect("ftp://example.com/a.zip").await.unwrap_err();
    assert_eq!(error.code, "url.invalid");
}
