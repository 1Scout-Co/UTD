mod support;

use std::fmt::Write as _;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use sha2::{Digest, Sha256};
use support::{ServerConfig, TestServer};
use tokio_util::sync::CancellationToken;
use utd_engine::fetch::{download_verified, fetch_text, FetchError, FetchPolicy};

fn payload(size: usize) -> Vec<u8> {
    (0..size).map(|index| (index % 239) as u8).collect()
}

fn sha256(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(&mut hex, "{byte:02x}").expect("hex");
    }
    hex
}

/// Máy chủ thử chạy HTTP trên 127.0.0.1 nên luật thử cho phép http và chỉ host đó.
fn local_policy() -> FetchPolicy {
    FetchPolicy {
        allowed_hosts: vec!["127.0.0.1".into()],
        require_https: false,
        max_redirects: 3,
        connect_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(5),
    }
}

fn leftovers(directory: &Path) -> Vec<String> {
    std::fs::read_dir(directory)
        .expect("read dir")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

#[tokio::test]
async fn downloads_and_verifies_into_place() {
    let data = payload(300 * 1024);
    let server = TestServer::start(data.clone(), ServerConfig::default()).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let dest = directory.path().join("tool.exe");
    let mut calls = 0;
    let mut last = (0, None);

    let verified = download_verified(
        &server.url("/tool.exe"),
        &sha256(&data).to_ascii_uppercase(),
        &dest,
        1024 * 1024,
        &local_policy(),
        &CancellationToken::new(),
        |done, total| {
            calls += 1;
            last = (done, total);
        },
    )
    .await
    .expect("download");

    assert_eq!(verified.bytes, data.len() as u64);
    assert_eq!(verified.sha256, sha256(&data));
    assert_eq!(std::fs::read(&dest).expect("file"), data);
    assert_eq!(leftovers(directory.path()), vec!["tool.exe".to_string()]);
    assert!(calls >= 2);
    assert_eq!(last, (data.len() as u64, Some(data.len() as u64)));
    assert_eq!(server.range_requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn hash_mismatch_leaves_no_file() {
    let data = payload(200 * 1024);
    let server = TestServer::start(data, ServerConfig::default()).await;
    let directory = tempfile::tempdir().expect("tempdir");
    let dest = directory.path().join("tool.exe");

    let error = download_verified(
        &server.url("/tool.exe"),
        &"0".repeat(64),
        &dest,
        1024 * 1024,
        &local_policy(),
        &CancellationToken::new(),
        |_, _| {},
    )
    .await
    .expect_err("mismatch");

    assert!(
        matches!(error, FetchError::HashMismatch { .. }),
        "{error:?}"
    );
    assert!(leftovers(directory.path()).is_empty());
}

#[tokio::test]
async fn oversized_download_is_rejected_and_cleaned() {
    let data = payload(64 * 1024);
    let server = TestServer::start(
        data.clone(),
        ServerConfig {
            omit_length: true,
            ..ServerConfig::default()
        },
    )
    .await;
    let directory = tempfile::tempdir().expect("tempdir");

    let error = download_verified(
        &server.url("/tool.exe"),
        &sha256(&data),
        &directory.path().join("tool.exe"),
        16 * 1024,
        &local_policy(),
        &CancellationToken::new(),
        |_, _| {},
    )
    .await
    .expect_err("too large");

    assert!(matches!(error, FetchError::TooLarge(_)), "{error:?}");
    assert!(leftovers(directory.path()).is_empty());
}

#[tokio::test]
async fn cancel_stops_and_cleans() {
    let data = payload(512 * 1024);
    let server = TestServer::start(
        data.clone(),
        ServerConfig {
            chunk_size: 4 * 1024,
            chunk_delay: Duration::from_millis(20),
            ..ServerConfig::default()
        },
    )
    .await;
    let directory = tempfile::tempdir().expect("tempdir");
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();

    let error = download_verified(
        &server.url("/tool.exe"),
        &sha256(&data),
        &directory.path().join("tool.exe"),
        1024 * 1024,
        &local_policy(),
        &cancel,
        move |done, _| {
            if done > 32 * 1024 {
                trigger.cancel();
            }
        },
    )
    .await
    .expect_err("cancelled");

    assert!(matches!(error, FetchError::Cancelled), "{error:?}");
    assert!(leftovers(directory.path()).is_empty());
}

#[tokio::test]
async fn follows_allowed_redirect_and_reports_hops() {
    let target = TestServer::start(b"abc  tool.exe\n".to_vec(), ServerConfig::default()).await;
    let hop = TestServer::start(
        Vec::new(),
        ServerConfig {
            redirect_to: Some(target.url("/releases/download/v1/SUMS")),
            ..ServerConfig::default()
        },
    )
    .await;

    let fetched = fetch_text(&hop.url("/latest/SUMS"), 1024, &local_policy())
        .await
        .expect("text");

    assert_eq!(fetched.text, "abc  tool.exe\n");
    assert_eq!(fetched.redirects.len(), 1);
    assert_eq!(fetched.redirects[0].path(), "/releases/download/v1/SUMS");
}

#[tokio::test]
async fn redirect_to_disallowed_host_is_blocked() {
    let target = TestServer::start(payload(1024), ServerConfig::default()).await;
    // "localhost" trỏ cùng máy nhưng không nằm trong danh sách host được phép.
    let hop = TestServer::start(
        Vec::new(),
        ServerConfig {
            redirect_to: Some(format!("http://localhost:{}/evil", target.addr.port())),
            ..ServerConfig::default()
        },
    )
    .await;
    let directory = tempfile::tempdir().expect("tempdir");

    let error = download_verified(
        &hop.url("/tool.exe"),
        &sha256(&payload(1024)),
        &directory.path().join("tool.exe"),
        1024 * 1024,
        &local_policy(),
        &CancellationToken::new(),
        |_, _| {},
    )
    .await
    .expect_err("blocked");

    assert!(
        matches!(&error, FetchError::HostNotAllowed(host) if host == "localhost"),
        "{error:?}"
    );
    assert_eq!(target.requests.load(Ordering::SeqCst), 0);
    assert!(leftovers(directory.path()).is_empty());

    let text = fetch_text(&hop.url("/SUMS"), 1024, &local_policy()).await;
    assert!(
        matches!(text, Err(FetchError::HostNotAllowed(_))),
        "{text:?}"
    );
}

#[tokio::test]
async fn https_only_policy_refuses_plain_http_without_a_request() {
    let server = TestServer::start(payload(1024), ServerConfig::default()).await;
    let policy = FetchPolicy::https_only(&["127.0.0.1"]);

    let result = fetch_text(&server.url("/SUMS"), 1024, &policy).await;

    assert!(
        matches!(result, Err(FetchError::InsecureUrl(_))),
        "{result:?}"
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn redirect_loops_are_capped() {
    // Chuỗi hai bước chuyển hướng: first → second → end, vượt giới hạn một bước.
    let end = TestServer::start(b"ok".to_vec(), ServerConfig::default()).await;
    let second = TestServer::start(
        Vec::new(),
        ServerConfig {
            redirect_to: Some(end.url("/x")),
            ..ServerConfig::default()
        },
    )
    .await;
    let first = TestServer::start(
        Vec::new(),
        ServerConfig {
            redirect_to: Some(second.url("/y")),
            ..ServerConfig::default()
        },
    )
    .await;
    let mut policy = local_policy();
    policy.max_redirects = 1;

    let result = fetch_text(&first.url("/z"), 1024, &policy).await;

    assert!(
        matches!(result, Err(FetchError::TooManyRedirects)),
        "{result:?}"
    );
    assert_eq!(end.requests.load(Ordering::SeqCst), 0);
    policy.max_redirects = 2;
    let fetched = fetch_text(&first.url("/z"), 1024, &policy)
        .await
        .expect("two hops");
    assert_eq!(fetched.text, "ok");
    assert_eq!(fetched.redirects.len(), 2);
}

#[tokio::test]
async fn oversized_text_is_rejected() {
    let big = vec![b'a'; 8 * 1024];
    let sized = TestServer::start(big.clone(), ServerConfig::default()).await;
    let chunked = TestServer::start(
        big,
        ServerConfig {
            omit_length: true,
            chunk_size: 1024,
            ..ServerConfig::default()
        },
    )
    .await;

    for server in [&sized, &chunked] {
        let result = fetch_text(&server.url("/SUMS"), 4 * 1024, &local_policy()).await;
        assert!(matches!(result, Err(FetchError::TooLarge(_))), "{result:?}");
    }
    let fits = fetch_text(&sized.url("/SUMS"), 8 * 1024, &local_policy())
        .await
        .expect("fits");
    assert_eq!(fits.text.len(), 8 * 1024);
    assert!(fits.redirects.is_empty());
}
