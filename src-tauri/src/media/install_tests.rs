//! Tải công cụ thật từ đầu đến cuối với server giả trên 127.0.0.1: chọn đúng bản theo tag, bỏ qua khi
//! đã mới nhất, giữ bản cũ khi SHA-256 sai, đọc checksum kiểu deno/martin-riedl, mất mạng thì vòng tự
//! tải thử lại. "File chạy được" là script in ra dòng phiên bản (`.cmd` trên Windows, `sh` nơi khác).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use utd_engine::Journal;
use utd_engine::fetch::FetchPolicy;

use crate::events::ToolProgress;
use crate::media::auto::{Auto, BoxFuture, Toolbox, Wait};
use crate::media::install::{Emit, InstallError, Job, Outcome, install_job};
use crate::media::sources::{Asset, Binary, Pack, Release, Sums, Tool};
use crate::media::unpack::tests::make_zip;
use crate::media::verify;

/// `ACTIVE` (một lần cài mỗi lúc) là biến chung của cả app: bài thử chạm tới nó chạy lần lượt.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    SERIAL.lock().await
}

pub(crate) fn serial_blocking() -> tokio::sync::MutexGuard<'static, ()> {
    SERIAL.blocking_lock()
}

#[derive(Clone)]
enum Reply {
    Body(Arc<Vec<u8>>),
    Redirect(String),
}

struct Server {
    port: u16,
    routes: Arc<Mutex<HashMap<String, Reply>>>,
    hits: Arc<Mutex<Vec<String>>>,
    /// Số yêu cầu tới đầu tiên bị cắt ngang (giả mất mạng).
    drop_first: Arc<AtomicUsize>,
}

impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let routes: Arc<Mutex<HashMap<String, Reply>>> = Arc::default();
        let hits: Arc<Mutex<Vec<String>>> = Arc::default();
        let drop_first = Arc::new(AtomicUsize::new(0));
        let (table, log, drops) = (routes.clone(), hits.clone(), drop_first.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (table, log, drops) = (table.clone(), log.clone(), drops.clone());
                std::thread::spawn(move || answer(stream, &table, &log, &drops));
            }
        });
        Self {
            port,
            routes,
            hits,
            drop_first,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    fn body(&self, path: &str, bytes: &[u8]) {
        let reply = Reply::Body(Arc::new(bytes.to_vec()));
        self.routes
            .lock()
            .expect("routes")
            .insert(path.into(), reply);
    }

    fn redirect(&self, path: &str, to: &str) {
        let reply = Reply::Redirect(to.into());
        self.routes
            .lock()
            .expect("routes")
            .insert(path.into(), reply);
    }

    fn hits(&self, path: &str) -> usize {
        let hits = self.hits.lock().expect("hits");
        hits.iter().filter(|hit| *hit == path).count()
    }
}

/// Bớt 1 nếu còn lớn hơn 0 (như `fetch_update`, mà Rust mới đổi tên thành `try_update` và báo
/// deprecated): viết tay để dựng được trên mọi bản stable, giống `engine/adaptive.rs::update_if`.
fn take_one(left: &AtomicUsize) -> bool {
    let mut current = left.load(Ordering::SeqCst);
    while current > 0 {
        match left.compare_exchange_weak(current, current - 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return true,
            Err(actual) => current = actual,
        }
    }
    false
}

/// Một yêu cầu mỗi kết nối (`Connection: close`).
fn answer(
    mut stream: TcpStream,
    routes: &Mutex<HashMap<String, Reply>>,
    log: &Mutex<Vec<String>>,
    drops: &AtomicUsize,
) {
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    let text = String::from_utf8_lossy(&request);
    let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
    log.lock().expect("log").push(path.clone());
    if take_one(drops) {
        return;
    }
    let reply = routes.lock().expect("routes").get(&path).cloned();
    let head = match &reply {
        Some(Reply::Body(body)) => format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n", body.len()),
        Some(Reply::Redirect(to)) => {
            format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\n")
        }
        None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n".into(),
    };
    let _ = stream.write_all(format!("{head}Connection: close\r\n\r\n").as_bytes());
    if let Some(Reply::Body(body)) = reply {
        let _ = stream.write_all(&body);
    }
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Script in ra `line` khi chạy với bất kỳ tham số nào.
fn script(line: &str) -> Vec<u8> {
    if cfg!(windows) {
        format!("@echo {line}\r\n").into_bytes()
    } else {
        format!("#!/bin/sh\necho '{line}'\n").into_bytes()
    }
}

fn exe(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.cmd")
    } else {
        stem.to_string()
    }
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("utd-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&path).expect("dir");
    path
}

fn local_policy() -> FetchPolicy {
    let mut policy = FetchPolicy::https_only(&["127.0.0.1"]);
    policy.require_https = false;
    policy.idle_timeout = Duration::from_secs(5);
    policy
}

fn recorder() -> (Emit, Arc<Mutex<Vec<ToolProgress>>>) {
    let seen: Arc<Mutex<Vec<ToolProgress>>> = Arc::default();
    let sink = seen.clone();
    let emit: Emit = Arc::new(move |progress| sink.lock().expect("seen").push(progress));
    (emit, seen)
}

fn release(tool: Tool, assets: Vec<Asset>, binaries: Vec<Binary>) -> Release {
    Release {
        tool,
        assets,
        binaries,
        license: Some("LICENSE.txt"),
        hosts: vec!["127.0.0.1".into()],
        max_bytes: 1024 * 1024,
        need_free: 0,
    }
}

/// yt-dlp như GitHub thật: `latest/download/SHA2-256SUMS` chuyển hướng sang `download/<tag>/…`.
fn yt_dlp_release(server: &Server, tag: &str, payload: &[u8]) -> Release {
    let base = "/yt-dlp/yt-dlp/releases";
    let sums = format!("{}  yt-dlp_fake\n", sha256(payload));
    server.redirect(
        &format!("{base}/latest/download/SHA2-256SUMS"),
        &format!("{base}/download/{tag}/SHA2-256SUMS"),
    );
    server.body(
        &format!("{base}/download/{tag}/SHA2-256SUMS"),
        sums.as_bytes(),
    );
    server.body(&format!("{base}/download/{tag}/yt-dlp_fake"), payload);
    release(
        Tool::YtDlp,
        vec![Asset {
            url: server.url(&format!("{base}/latest/download/yt-dlp_fake")),
            pinned: Some(server.url(&format!("{base}/download/{{tag}}/yt-dlp_fake"))),
            sums: Sums::List {
                url: server.url(&format!("{base}/latest/download/SHA2-256SUMS")),
                name: "yt-dlp_fake".into(),
            },
            pack: Pack::Binary,
        }],
        vec![Binary {
            name: exe("yt-dlp"),
            flag: "--version",
            accept: verify::is_date_version,
        }],
    )
}

async fn run(
    release: &Release,
    bin: &Path,
    current: Option<&str>,
    emit: Emit,
) -> Result<Outcome, InstallError> {
    let job = Job {
        release,
        bin,
        policy: local_policy(),
        current: current.map(str::to_string),
        emit,
        retry_delay: Duration::from_millis(10),
    };
    install_job(&job).await
}

fn leftovers(bin: &Path) -> Vec<String> {
    std::fs::read_dir(bin)
        .expect("bin")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("utd-tmp-"))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn installs_yt_dlp_pinned_to_the_tag_and_skips_when_current() {
    let _serial = serial().await;
    let server = Server::start();
    let bin = scratch("bin");
    let payload = script("2026.10.22");
    let release = yt_dlp_release(&server, "2026.10.22", &payload);
    let (emit, seen) = recorder();

    let outcome = run(&release, &bin, None, emit.clone()).await;
    assert_eq!(outcome, Ok(Outcome::Installed));
    assert_eq!(
        std::fs::read(bin.join(exe("yt-dlp"))).expect("tool"),
        payload
    );
    // Tải đúng link của tag đọc từ chuyển hướng, không phải link `latest`.
    assert_eq!(
        server.hits("/yt-dlp/yt-dlp/releases/download/2026.10.22/yt-dlp_fake"),
        1
    );
    assert_eq!(
        server.hits("/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_fake"),
        0
    );
    let stages: Vec<&str> = seen
        .lock()
        .expect("seen")
        .iter()
        .map(|item| item.stage)
        .collect();
    for stage in ["checksum", "download", "check", "install"] {
        assert!(stages.contains(&stage), "{stage} in {stages:?}");
    }

    // Bản đang dùng bằng/mới hơn tag ⇒ không tải gì.
    let outcome = run(&release, &bin, Some("2026.10.22"), emit.clone()).await;
    assert_eq!(outcome, Ok(Outcome::Current));
    assert_eq!(
        server.hits("/yt-dlp/yt-dlp/releases/download/2026.10.22/yt-dlp_fake"),
        1
    );

    // Bản cũ hơn ⇒ tải, bản cũ giữ thành .bak.
    let outcome = run(&release, &bin, Some("2026.08.19"), emit).await;
    assert_eq!(outcome, Ok(Outcome::Installed));
    assert!(bin.join(format!("{}.bak", exe("yt-dlp"))).is_file());
    assert!(leftovers(&bin).is_empty());
    let _ = std::fs::remove_dir_all(&bin);
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_checksum_or_broken_binary_keeps_the_old_tool() {
    let _serial = serial().await;
    let server = Server::start();
    let bin = scratch("bin");
    let old = script("2026.01.01");
    std::fs::write(bin.join(exe("yt-dlp")), &old).expect("old");
    let release = yt_dlp_release(&server, "2026.10.22", &script("2026.10.22"));
    // File trên server bị tráo sau khi công bố SHA-256.
    server.body(
        "/yt-dlp/yt-dlp/releases/download/2026.10.22/yt-dlp_fake",
        b"tampered",
    );
    let (emit, _) = recorder();
    let outcome = run(&release, &bin, None, emit.clone()).await;
    assert!(
        matches!(outcome, Err(InstallError::Failed(ref message)) if message.contains("SHA-256")),
        "{outcome:?}"
    );
    // Lệch hash được thử lại đúng một lần (bản `latest` có thể vừa dựng lại).
    assert_eq!(
        server.hits("/yt-dlp/yt-dlp/releases/download/2026.10.22/yt-dlp_fake"),
        2
    );
    assert_eq!(std::fs::read(bin.join(exe("yt-dlp"))).expect("old"), old);

    // Hash khớp nhưng chạy không ra phiên bản ⇒ không thay.
    let broken = script("hello");
    let release = yt_dlp_release(&server, "2026.10.23", &broken);
    let outcome = run(&release, &bin, None, emit).await;
    assert!(
        matches!(outcome, Err(InstallError::Failed(_))),
        "{outcome:?}"
    );
    assert_eq!(std::fs::read(bin.join(exe("yt-dlp"))).expect("old"), old);
    assert!(leftovers(&bin).is_empty());
    let _ = std::fs::remove_dir_all(&bin);
}

#[tokio::test(flavor = "multi_thread")]
async fn installs_deno_from_zip_with_a_powershell_sidecar() {
    let _serial = serial().await;
    let server = Server::start();
    let bin = scratch("bin");
    let work = scratch("zip");
    let zip_name = "deno-x86_64-pc-windows-msvc.zip";
    let (name, body) = (exe("deno"), script("deno 2.9.8 (stable, release)"));
    make_zip(&work, zip_name, &[(name.as_str(), body.as_slice())]);
    let zip = std::fs::read(work.join(zip_name)).expect("zip");
    let base = "/denoland/deno/releases";
    let sidecar = format!(
        "\r\nAlgorithm : SHA256\r\nHash      : {}\r\nPath      : C:\\a\\deno\\target\\release\\{zip_name}\r\n\r\n",
        sha256(&zip).to_ascii_uppercase()
    );
    server.redirect(
        &format!("{base}/latest/download/{zip_name}.sha256sum"),
        &format!("{base}/download/v2.9.8/{zip_name}.sha256sum"),
    );
    server.body(
        &format!("{base}/download/v2.9.8/{zip_name}.sha256sum"),
        sidecar.as_bytes(),
    );
    server.body(&format!("{base}/download/v2.9.8/{zip_name}"), &zip);
    let release = release(
        Tool::Deno,
        vec![Asset {
            url: server.url(&format!("{base}/latest/download/{zip_name}")),
            pinned: Some(server.url(&format!("{base}/download/{{tag}}/{zip_name}"))),
            sums: Sums::Sidecar {
                url: server.url(&format!("{base}/latest/download/{zip_name}.sha256sum")),
                name: zip_name.into(),
            },
            pack: Pack::Zip,
        }],
        vec![Binary {
            name: exe("deno"),
            flag: "--version",
            accept: |line| line.starts_with("deno "),
        }],
    );
    let (emit, _) = recorder();
    assert_eq!(
        run(&release, &bin, Some("2.9.7"), emit.clone()).await,
        Ok(Outcome::Installed)
    );
    assert!(bin.join(exe("deno")).is_file());
    // deno 2.10 mới hơn v2.9.8 (so theo số, không theo chữ).
    assert_eq!(
        run(&release, &bin, Some("2.10.0"), emit).await,
        Ok(Outcome::Current)
    );
    let _ = std::fs::remove_dir_all(&bin);
    let _ = std::fs::remove_dir_all(&work);
}

#[tokio::test(flavor = "multi_thread")]
async fn installs_two_packages_whose_checksums_sit_beside_the_redirect_target() {
    let _serial = serial().await;
    let server = Server::start();
    let bin = scratch("bin");
    let work = scratch("zip");
    let mut assets = Vec::new();
    for name in ["ffmpeg", "ffprobe"] {
        let zip_name = format!("{name}.zip");
        let (file, body) = (exe(name), script(&format!("{name} version 9.0.2")));
        make_zip(&work, &zip_name, &[(file.as_str(), body.as_slice())]);
        let zip = std::fs::read(work.join(&zip_name)).expect("zip");
        let target = format!("/download/macos/arm64/1789931890_9.0.2/{zip_name}");
        server.redirect(
            &format!("/redirect/latest/macos/arm64/release/{zip_name}"),
            &target,
        );
        server.body(&target, &zip);
        server.body(
            &format!("{target}.sha256"),
            format!("{}  {zip_name}\n", sha256(&zip)).as_bytes(),
        );
        assets.push(Asset {
            url: server.url(&format!("/redirect/latest/macos/arm64/release/{zip_name}")),
            pinned: None,
            sums: Sums::Beside,
            pack: Pack::Zip,
        });
    }
    let binaries = vec![
        Binary {
            name: exe("ffmpeg"),
            flag: "-version",
            accept: |line| line.starts_with("ffmpeg version"),
        },
        Binary {
            name: exe("ffprobe"),
            flag: "-version",
            accept: |line| line.starts_with("ffprobe version"),
        },
    ];
    let release = release(Tool::Ffmpeg, assets, binaries);
    let (emit, seen) = recorder();
    assert_eq!(
        run(&release, &bin, None, emit).await,
        Ok(Outcome::Installed)
    );
    assert!(bin.join(exe("ffmpeg")).is_file() && bin.join(exe("ffprobe")).is_file());
    let extracted = seen
        .lock()
        .expect("seen")
        .iter()
        .filter(|item| item.stage == "extract")
        .count();
    assert!(extracted >= 2, "mỗi gói giải nén một lần");
    let _ = std::fs::remove_dir_all(&bin);
    let _ = std::fs::remove_dir_all(&work);
}

/// Máy giả cho vòng tự tải: "có" công cụ khi file nằm trong `bin`, tải bằng đường cài thật.
struct Local {
    bin: PathBuf,
    releases: HashMap<Tool, Release>,
}

impl Toolbox for Local {
    fn missing(&self) -> BoxFuture<'_, Vec<Tool>> {
        Box::pin(async move {
            Tool::ALL
                .into_iter()
                .filter(|tool| self.releases.contains_key(tool))
                .filter(|tool| {
                    !self
                        .bin
                        .join(&self.releases[tool].binaries[0].name)
                        .is_file()
                })
                .collect()
        })
    }

    fn active(&self, _tool: Tool) -> BoxFuture<'_, Option<(String, bool)>> {
        Box::pin(async { None })
    }

    fn install(
        &self,
        tool: Tool,
        current: Option<String>,
    ) -> BoxFuture<'_, Result<Outcome, InstallError>> {
        Box::pin(async move {
            let (emit, _) = recorder();
            run(&self.releases[&tool], &self.bin, current.as_deref(), emit).await
        })
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_retries_after_the_network_drops_and_waiting_tasks_continue() {
    let _serial = serial().await;
    let server = Server::start();
    let bin = scratch("bin");
    let release = yt_dlp_release(&server, "2026.10.22", &script("2026.10.22"));
    // Ba yêu cầu đầu bị cắt ngang: lần tải đầu (và có thể lần thử lại) hỏng như mất mạng.
    server.drop_first.store(3, Ordering::SeqCst);
    let journal = Arc::new(Journal::open(bin.join("utd.db")).expect("journal"));
    let toolbox = Local {
        bin: bin.clone(),
        releases: HashMap::from([(Tool::YtDlp, release)]),
    };
    let auto = Auto::new(Box::new(toolbox), journal, None)
        .with_backoff(vec![Duration::from_millis(50), Duration::from_millis(100)]);
    let runner = auto.clone();
    let loop_task = tokio::spawn(async move { runner.bootstrap().await });

    // Phân tích (không kiên nhẫn) gặp lần lỗi đầu thì trả lỗi để hộp thoại báo.
    let first = auto
        .wait_for(&[Tool::YtDlp], &[], 1, &CancellationToken::new())
        .await;
    assert!(
        matches!(first, Wait::Failed(ref message) if message.contains("kết nối")),
        "{first:?}"
    );
    // Task tải (kiên nhẫn) chờ qua các lần thử lại tới khi có yt-dlp.
    let waited = tokio::time::timeout(
        Duration::from_secs(20),
        auto.wait_for(&[Tool::YtDlp], &[Tool::Deno], 10, &CancellationToken::new()),
    )
    .await
    .expect("không chờ mãi");
    assert_eq!(waited, Wait::Ready);
    assert!(bin.join(exe("yt-dlp")).is_file());
    let state = auto.state();
    assert!(
        state.pending.is_empty() && state.error.is_none() && state.active.is_none(),
        "{state:?}"
    );
    loop_task.abort();
    let _ = std::fs::remove_dir_all(&bin);
}
