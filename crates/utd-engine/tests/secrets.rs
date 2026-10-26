//! Cookie mồi (P-02, P-04, AC-30; notes/auth-plan.md §9.3): bí mật lọt vào header từ nguồn ngoài (vd
//! `http_headers` của yt-dlp) không tới máy chủ, không xuống journal hay file nào. Khung `files_containing`
//! dùng lại khi engine nhận cookie đăng nhập chỉ ở RAM.

mod kit;

use std::sync::Arc;

use kit::*;
use utd_engine::{
    DownloadControl, DownloadRequest, DownloadTask, Journal, allowed_request_headers,
};
use uuid::Uuid;

fn canary() -> String {
    format!("UTDCANARY{}", Uuid::new_v4().simple())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn credential_headers_never_reach_the_server_or_the_disk() {
    let secret = canary();
    let data = payload(MIB + 777);
    let server = Server::start(file_handler(Arc::new(data.clone()), FileOpts::default())).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let (journal, engine) = engine(directory.path(), options(2, 4));
    let mut request = DownloadRequest::new(server.url("/file.bin"), downloads(directory.path()));
    request.file_name = Some("file.bin".into());
    request.max_request_bytes = Some(256 * KIB as u64);
    request.request_headers = allowed_request_headers([
        ("User-Agent", "UTD-Test".to_string()),
        ("Cookie", format!("SID={secret}")),
        ("cookie2", format!("x={secret}")),
        ("Authorization", format!("Bearer {secret}")),
        ("Proxy-Authorization", format!("Basic {secret}")),
        ("X-Note", "ok".to_string()),
    ]);

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
    assert!(requests.len() > 2, "{requests:?}");
    for request in &requests {
        assert_eq!(request.header("user-agent"), Some("UTD-Test"));
        assert!(
            request
                .headers
                .iter()
                .all(|(_, value)| !value.contains(&secret)),
            "máy chủ nhận bí mật: {request:?}"
        );
    }
    drop(engine);
    drop(journal);
    assert_eq!(
        files_containing(directory.path(), secret.as_bytes()),
        Vec::<std::path::PathBuf>::new()
    );
}

/// Bộ quét phải thấy được bí mật nằm trong WAL của SQLite, nếu không bài trên đúng một cách vô nghĩa.
/// (URL có token trong query vẫn được journal giữ để tải tiếp: câu Q7 của kế hoạch xử lý sau.)
#[test]
fn the_scanner_finds_a_secret_written_to_the_journal() {
    let secret = canary();
    let directory = tempfile::tempdir().expect("tempdir");
    let journal = Journal::open(directory.path().join("utd.db")).expect("journal");
    let task = DownloadTask::new(
        format!("https://example.com/file.bin?token={secret}"),
        "file.bin",
        downloads(directory.path()),
        4,
    )
    .expect("task");
    journal.save_task(&task).expect("save");
    let found = files_containing(directory.path(), secret.as_bytes());
    assert!(!found.is_empty(), "không thấy bí mật trong journal");
    assert!(
        found.iter().all(|path| path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("utd.db"))),
        "{found:?}"
    );
}
