//! Bài thử đường tải nhanh với server cục bộ (127.0.0.1) và hai file media thật do ffmpeg tạo.
//! Không có ffmpeg thì bỏ qua (in rõ lý do).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;
use utd_engine::{
    now_ms, DownloadEngine, DownloadTask, EngineOptions, HashState, Journal, MediaJob,
    ProgressSnapshot, TaskKind, TaskStatus,
};

use super::*;
use crate::media::finish::Sink;
use crate::media::resolve::Resolved;

const CAP: u64 = 64 * 1024;

#[derive(Debug, Clone)]
struct Hit {
    path: String,
    method: String,
    range_len: Option<u64>,
    user_agent: Option<String>,
    referer: Option<String>,
    cookie: bool,
    cookie_text: Option<String>,
}

enum Route {
    File(Arc<Vec<u8>>, &'static str),
    /// 403 khi header `Cookie` không có đúng chuỗi này (CDN TikTok đòi `tt_chain_token`).
    Gated(Arc<Vec<u8>>, &'static str, &'static str),
    Forbidden,
}

struct Server {
    port: u16,
    hits: Arc<Mutex<Vec<Hit>>>,
    delay_ms: Arc<AtomicU64>,
}

impl Server {
    fn start(routes: HashMap<String, Route>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(Mutex::new(Vec::new()));
        let delay_ms = Arc::new(AtomicU64::new(0));
        let routes = Arc::new(routes);
        let (log, delay) = (hits.clone(), delay_ms.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (routes, log, delay) = (routes.clone(), log.clone(), delay.clone());
                std::thread::spawn(move || answer(stream, &routes, &log, &delay));
            }
        });
        Self {
            port,
            hits,
            delay_ms,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    fn hits(&self, path: &str) -> Vec<Hit> {
        self.hits
            .lock()
            .unwrap()
            .iter()
            .filter(|hit| hit.path == path)
            .cloned()
            .collect()
    }
}

/// Một yêu cầu mỗi kết nối (`Connection: close`), có Range, HEAD và 403.
fn answer(
    mut stream: TcpStream,
    routes: &HashMap<String, Route>,
    log: &Mutex<Vec<Hit>>,
    delay: &AtomicU64,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    while !buffer.windows(4).any(|window| window == b"\r\n\r\n") && buffer.len() < 65536 {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(count) => buffer.extend_from_slice(&chunk[..count]),
        }
    }
    let request = String::from_utf8_lossy(&buffer).into_owned();
    let mut lines = request.lines();
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let method = first.next().unwrap_or("").to_string();
    let target = first.next().unwrap_or("");
    let path = target.split('?').next().unwrap_or("").to_string();
    let header = |name: &str| {
        request.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
    };
    let range = header("range").and_then(|value| {
        let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
        Some((start.parse::<u64>().ok()?, end.parse::<u64>().ok()))
    });
    let mut hit = Hit {
        path: path.clone(),
        method: method.clone(),
        range_len: None,
        user_agent: header("user-agent"),
        referer: header("referer"),
        cookie: header("cookie").is_some(),
        cookie_text: header("cookie"),
    };
    let opened = match routes.get(&path) {
        Some(Route::Gated(body, kind, needed)) => header("cookie")
            .is_some_and(|cookie| cookie.split("; ").any(|pair| pair == *needed))
            .then_some((body, kind)),
        Some(Route::File(body, kind)) => Some((body, kind)),
        _ => None,
    };
    let reply = match (opened, routes.get(&path)) {
        (Some((body, kind)), _) => {
            let total = body.len() as u64;
            if method == "HEAD" {
                head(200, total, kind, None, &[])
            } else if let Some((start, end)) = range {
                let end = end.unwrap_or(total - 1).min(total - 1);
                hit.range_len = Some(end - start + 1);
                let range = format!("bytes {start}-{end}/{total}");
                head(
                    206,
                    end - start + 1,
                    kind,
                    Some(&range),
                    &body[start as usize..=end as usize],
                )
            } else {
                head(200, total, kind, None, body)
            }
        }
        (None, Some(Route::Forbidden | Route::Gated(..))) => head(403, 0, "text/plain", None, &[]),
        (None, _) => head(404, 0, "text/plain", None, &[]),
    };
    log.lock().unwrap().push(hit);
    let wait = delay.load(Ordering::Relaxed);
    if wait > 0 {
        std::thread::sleep(Duration::from_millis(wait));
    }
    let _ = stream.write_all(&reply);
    let _ = stream.flush();
}

fn head(status: u16, length: u64, kind: &str, range: Option<&str>, body: &[u8]) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        206 => "Partial Content",
        403 => "Forbidden",
        _ => "Not Found",
    };
    let mut text = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {length}\r\nContent-Type: {kind}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n"
    );
    if let Some(range) = range {
        text.push_str(&format!("Content-Range: {range}\r\n"));
    }
    text.push_str("\r\n");
    let mut bytes = text.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

#[derive(Default)]
struct Recorder {
    tasks: Mutex<Vec<DownloadTask>>,
    progress: Mutex<Vec<ProgressSnapshot>>,
    /// Tạm dừng khi thấy byte đầu tiên (bài thử dừng/tiếp tục).
    pause_on_bytes: Option<CancellationToken>,
}

impl Sink for Recorder {
    fn task(&self, task: &DownloadTask) {
        self.tasks.lock().unwrap().push(task.clone());
    }

    fn progress(&self, progress: ProgressSnapshot) {
        if let Some(token) = &self.pause_on_bytes
            && progress.downloaded_bytes > 0
        {
            token.cancel();
        }
        self.progress.lock().unwrap().push(progress);
    }
}

struct Fixed {
    resolved: Resolved,
    calls: AtomicUsize,
}

impl Resolver for Fixed {
    fn resolve<'a>(&'a self, _url: &'a str, _selector: &'a str) -> ResolveFuture<'a> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let resolved = self.resolved.clone();
        Box::pin(async move { Ok(resolved) })
    }
}

/// Thư mục thử, xoá khi bỏ. Đặt cuối struct: journal (SQLite) phải đóng trước, nếu không
/// Windows không cho xoá `utd.db`.
struct TempRoot(PathBuf);

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Lab {
    downloads: PathBuf,
    ffmpeg: PathBuf,
    video: Arc<Vec<u8>>,
    audio: Arc<Vec<u8>>,
    /// Một file có cả hình lẫn tiếng, như format gộp của TikTok.
    combined: Arc<Vec<u8>>,
    journal: Arc<Journal>,
    engine: Arc<DownloadEngine>,
    _root: TempRoot,
}

fn find_ffmpeg() -> Option<PathBuf> {
    crate::media::tools::detect()
        .ffmpeg
        .map(|info| PathBuf::from(info.path))
        .filter(|path| path.is_file())
}

fn ffmpeg(binary: &Path, args: &[&str]) {
    let status = crate::media::std_command(binary)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .status()
        .expect("run ffmpeg");
    assert!(status.success(), "ffmpeg {args:?}");
}

/// Hai file thật: hình MPEG-4 (không tiếng) và tiếng AAC, đủ lớn để cần nhiều yêu cầu 64 KiB.
fn lab() -> Option<Lab> {
    let Some(binary) = find_ffmpeg() else {
        eprintln!("SKIP fast-path integration: không tìm thấy ffmpeg (Cài đặt → Công cụ media)");
        return None;
    };
    let root = std::env::temp_dir().join(format!("utd-fast-{}", uuid::Uuid::new_v4()));
    let downloads = root.join("Downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let video = root.join("video.mp4");
    let audio = root.join("audio.m4a");
    let (video_out, audio_out) = (video.to_string_lossy(), audio.to_string_lossy());
    let picture = "testsrc2=duration=4:size=640x360:rate=25";
    let video_args = [
        "-f", "lavfi", "-i", picture, "-c:v", "mpeg4", "-q:v", "2", "-an",
    ];
    ffmpeg(
        &binary,
        &[&video_args[..], &["-f", "mp4", &video_out]].concat(),
    );
    let tone = "sine=frequency=440:duration=4";
    let audio_args = [
        "-f", "lavfi", "-i", tone, "-c:a", "aac", "-b:a", "128k", "-vn",
    ];
    ffmpeg(
        &binary,
        &[&audio_args[..], &["-f", "ipod", &audio_out]].concat(),
    );
    let combined = root.join("combined.mp4");
    let combined_out = combined.to_string_lossy();
    let combined_args = [
        "-f", "lavfi", "-i", picture, "-f", "lavfi", "-i", tone, "-c:v", "mpeg4", "-q:v", "2",
        "-c:a", "aac", "-b:a", "128k", "-shortest",
    ];
    ffmpeg(
        &binary,
        &[&combined_args[..], &["-f", "mp4", &combined_out]].concat(),
    );
    let journal = Arc::new(Journal::open(root.join("utd.db")).unwrap());
    let engine = Arc::new(DownloadEngine::new(journal.clone(), EngineOptions::default()).unwrap());
    Some(Lab {
        video: Arc::new(std::fs::read(&video).unwrap()),
        audio: Arc::new(std::fs::read(&audio).unwrap()),
        combined: Arc::new(std::fs::read(&combined).unwrap()),
        downloads,
        ffmpeg: binary,
        journal,
        engine,
        _root: TempRoot(root),
    })
}

impl Lab {
    fn server(&self) -> Server {
        let mut routes = HashMap::new();
        routes.insert(
            "/v.mp4".to_string(),
            Route::File(self.video.clone(), "video/mp4"),
        );
        routes.insert(
            "/a.m4a".to_string(),
            Route::File(self.audio.clone(), "audio/mp4"),
        );
        routes.insert("/gone".to_string(), Route::Forbidden);
        routes.insert(
            "/tt-v.mp4".to_string(),
            Route::Gated(self.video.clone(), "video/mp4", "tt_chain_token=abc"),
        );
        routes.insert(
            "/tt-a.m4a".to_string(),
            Route::Gated(self.audio.clone(), "audio/mp4", "tt_chain_token=abc"),
        );
        routes.insert(
            "/tt-c.mp4".to_string(),
            Route::Gated(self.combined.clone(), "video/mp4", "tt_chain_token=abc"),
        );
        Server::start(routes)
    }

    fn parent(&self) -> (DownloadTask, MediaJob) {
        self.parent_for(
            "https://www.youtube.com/watch?v=09OGSRptb0M",
            None,
            "bv*+ba/b",
        )
    }

    fn parent_for(
        &self,
        url: &str,
        page_url: Option<&str>,
        selector: &str,
    ) -> (DownloadTask, MediaJob) {
        let mut task = DownloadTask::new(
            url,
            "Clip.mp4",
            self.downloads.to_string_lossy().into_owned(),
            1,
        )
        .unwrap()
        .with_kind(TaskKind::Media);
        task.hash_state = HashState::Unavailable;
        self.journal.save_task(&task).unwrap();
        let job = MediaJob {
            task_id: task.id.clone(),
            url: task.url.clone(),
            page_url: page_url.map(str::to_string),
            selector: selector.into(),
            audio_format: None,
            title: "Clip".into(),
            created_at: now_ms(),
        };
        self.journal.save_media_job(&job).unwrap();
        (task, job)
    }

    fn streams(&self, server: &Server, video_path: &str, audio_path: &str) -> Resolved {
        let expire = resolve::now_secs() + 6 * 3600;
        let stream = |path: &str, ext: &str, vcodec: &str, acodec: &str, size: usize| Stream {
            format_id: if ext == "mp4" {
                "399".into()
            } else {
                "140".into()
            },
            url: server.url(&format!("{path}?expire={expire}&itag=1")),
            protocol: "http".into(),
            ext: ext.into(),
            container: Some(format!("{ext}_dash")),
            vcodec: vcodec.into(),
            acodec: acodec.into(),
            size: Some(size as u64),
            exact_size: Some(size as u64),
            headers: vec![
                ("User-Agent".into(), "UTD-test-agent".into()),
                ("Cookie".into(), "SID=secret".into()),
            ],
            chunk: Some(CAP),
            available_at: None,
            drm: false,
            cookies: Default::default(),
        };
        Resolved {
            streams: vec![
                stream(video_path, "mp4", "mp4v.20.9", "none", self.video.len()),
                stream(audio_path, "m4a", "none", "mp4a.40.2", self.audio.len()),
            ],
            drm: false,
            live: false,
        }
    }

    /// Trang vừa "Phân tích": luồng nằm trong cache theo URL trang (mỗi bài thử một trang, cache
    /// dùng chung cả tiến trình); job tải theo link rút gọn như khi dán `youtu.be/…`.
    fn analysed(&self, resolved: &Resolved) -> (DownloadTask, MediaJob) {
        let id = uuid::Uuid::new_v4();
        let page = format!("https://www.youtube.com/watch?v={id}");
        probe_cache::store(
            vec![page.clone()],
            resolved.streams.clone(),
            resolve::now_secs(),
        );
        self.parent_for(
            &format!("https://youtu.be/{id}"),
            Some(&page),
            "399+140/bv*+ba/b",
        )
    }

    fn fast<'a>(
        &self,
        sink: &'a dyn Sink,
        resolver: &'a dyn Resolver,
        cancel: CancellationToken,
    ) -> Fast<'a> {
        Fast {
            engine: self.engine.clone(),
            journal: self.journal.clone(),
            sink,
            resolver,
            ffmpeg: Some(self.ffmpeg.clone()),
            cancel,
            started: Instant::now(),
            cookies: None,
        }
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.downloads)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Số luồng theo loại (`video`, `audio`) của file kết quả.
    fn stream_kinds(&self, file: &Path) -> Vec<String> {
        let probe = self.ffmpeg.with_file_name(if cfg!(windows) {
            "ffprobe.exe"
        } else {
            "ffprobe"
        });
        if probe.is_file() {
            let output = crate::media::std_command(&probe)
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "stream=codec_type",
                    "-of",
                    "csv=p=0",
                ])
                .arg(file)
                .output()
                .expect("ffprobe");
            return String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect();
        }
        let output = crate::media::std_command(&self.ffmpeg)
            .args(["-hide_banner", "-i"])
            .arg(file)
            .output()
            .expect("ffmpeg -i");
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                line.starts_with("Stream #").then(|| {
                    if line.contains("Video:") {
                        "video"
                    } else if line.contains("Audio:") {
                        "audio"
                    } else {
                        "other"
                    }
                    .to_string()
                })
            })
            .collect()
    }
}

fn assert_finished(lab: &Lab, task: &DownloadTask) {
    assert_eq!(task.status, TaskStatus::Completed, "{:?}", task.error);
    assert_eq!(task.file_name, "Clip.mp4");
    let output = lab.downloads.join("Clip.mp4");
    let mut kinds = lab.stream_kinds(&output);
    kinds.sort();
    assert_eq!(kinds, ["audio", "video"], "1 luồng hình + 1 luồng tiếng");
    assert_eq!(lab.files(), ["Clip.mp4"], "không còn file luồng hay .part");
    assert!(lab.journal.list_children(&task.id).unwrap().is_empty());
    let saved = lab.journal.load_task(&task.id).unwrap().unwrap();
    assert_eq!(saved.status, TaskStatus::Completed);
    assert_eq!(
        saved.total_bytes,
        Some(std::fs::metadata(&output).unwrap().len())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_both_streams_with_capped_requests_then_muxes() {
    let Some(lab) = lab() else { return };
    let server = lab.server();
    let (mut task, job) = lab.parent();
    let resolver = Fixed {
        resolved: lab.streams(&server, "/v.mp4", "/a.m4a"),
        calls: AtomicUsize::new(0),
    };
    let sink = Recorder::default();
    let fast = lab.fast(&sink, &resolver, CancellationToken::new());
    assert_eq!(run(&fast, &mut task, &job).await.unwrap(), FastEnd::Handled);
    assert_finished(&lab, &task);

    let video = server.hits("/v.mp4");
    let gets: Vec<&Hit> = video.iter().filter(|hit| hit.method == "GET").collect();
    let ranged: u64 = gets.iter().filter_map(|hit| hit.range_len).sum();
    assert!(gets.len() >= 2, "nhiều yêu cầu nhỏ, không một yêu cầu lớn");
    assert!(
        gets.iter()
            .all(|hit| hit.range_len.is_some_and(|len| len <= CAP)),
        "mọi yêu cầu xin ≤ http_chunk_size: {gets:?}"
    );
    assert!(ranged >= lab.video.len() as u64);
    assert!(
        video.iter().all(|hit| !hit.cookie),
        "không bao giờ gửi cookie"
    );
    assert!(video
        .iter()
        .all(|hit| hit.user_agent.as_deref() == Some("UTD-test-agent")));
    assert!(!server.hits("/a.m4a").is_empty());

    let progress = sink.progress.lock().unwrap();
    assert!(
        progress.iter().all(|snapshot| snapshot.task_id == task.id),
        "chỉ một dòng"
    );
    let tasks = sink.tasks.lock().unwrap();
    assert!(
        tasks.iter().all(|event| event.id == task.id),
        "task con không lên giao diện"
    );
}

/// TikTok: CDN trả 403 nếu thiếu cookie yt-dlp đặt cho luồng (`tt_chain_token`, giá trị có ngoặc kép).
/// Engine gửi kèm cookie đó (bỏ ngoặc) ngay lượt đầu, không đứng 0 B/s rồi quay về yt-dlp.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sends_the_cookies_yt_dlp_set_for_the_stream() {
    let Some(lab) = lab() else { return };
    let server = lab.server();
    let (mut task, job) = lab.parent();
    let mut resolved = lab.streams(&server, "/tt-v.mp4", "/tt-a.m4a");
    for stream in &mut resolved.streams {
        stream.cookies = resolve::StreamCookies::parse(
            "ttwid=1%7Cx; Domain=127.0.0.1; Path=/; Expires=1824135169; tt_chain_token=\"abc\"; Domain=127.0.0.1; Path=/",
        );
    }
    let resolver = Fixed {
        resolved,
        calls: AtomicUsize::new(0),
    };
    let sink = Recorder::default();
    let fast = lab.fast(&sink, &resolver, CancellationToken::new());
    assert_eq!(run(&fast, &mut task, &job).await.unwrap(), FastEnd::Handled);
    assert_finished(&lab, &task);
    assert_eq!(resolver.calls.load(Ordering::Relaxed), 1, "không hỏi yt-dlp lại");
    let video = server.hits("/tt-v.mp4");
    assert!(
        video
            .iter()
            .all(|hit| hit.cookie_text.as_deref() == Some("ttwid=1%7Cx; tt_chain_token=abc")),
        "{video:?}"
    );
}

/// `yt-dlp -J` của một video TikTok: mọi format gộp hình + tiếng, mỗi mức hai mirror (`-0`, `-1`), cookie
/// `tt_chain_token` ở từng format. Mirror `-0` trỏ vào `/gone` (403): chỉ mirror `-1` tải được.
fn tiktok_json(server: &Server, page: &str) -> serde_json::Value {
    let expire = resolve::now_secs() + 6 * 3600;
    let format = |id: &str, path: &str, width: u64, height: u64| {
        serde_json::json!({
            "format_id": id,
            "url": server.url(&format!("{path}?expire={expire}")),
            "protocol": "http",
            "ext": "mp4",
            "vcodec": "h265",
            "acodec": "aac",
            "width": width,
            "height": height,
            "filesize_approx": 3200,
            "http_headers": { "User-Agent": "UTD-test-agent" },
            "cookies": "ttwid=1%7Cx; Domain=127.0.0.1; Path=/; Expires=1824135169; tt_chain_token=\"abc\"; Domain=127.0.0.1; Path=/"
        })
    };
    serde_json::json!({
        "title": "Clip",
        "webpage_url": page,
        "formats": [
            format("bytevc1_720p_509915-0", "/gone", 720, 1280),
            format("bytevc1_720p_509915-1", "/gone", 720, 1280),
            format("bytevc1_1080p_1146746-0", "/gone", 1080, 1920),
            format("bytevc1_1080p_1146746-1", "/tt-c.mp4", 1080, 1920),
        ]
    })
}

/// Trang vừa "Phân tích" bằng JSON kiểu TikTok: job tải mang selector của dòng `id` mà hộp chọn định dạng trả.
fn analysed_tiktok(lab: &Lab, server: &Server, id: &str) -> (DownloadTask, MediaJob, String) {
    let page = format!(
        "https://www.tiktok.com/@a/video/{}",
        uuid::Uuid::new_v4().as_u128()
    );
    let value = tiktok_json(server, &page);
    let result = crate::media::probe::build_result(&page, &value, true).expect("probe result");
    probe_cache::remember(&value, &page, resolve::now_secs());
    let selector = result
        .formats
        .iter()
        .find(|format| format.id == id)
        .map(|format| format.selector.clone())
        .expect("dòng đã chọn");
    let (task, job) = lab.parent_for(&page, None, &selector);
    (task, job, selector)
}

/// TikTok: chọn "Tốt nhất" (hoặc một mức) là tải ngay, không chạy yt-dlp lần hai (trước đây `fresh`, đứng
/// 0 B/s ~12 s): selector ghim một mã gộp ⇒ cache trả đúng một luồng ⇒ kế hoạch Cached, một file, có cookie luồng.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn plans_one_cached_stream_with_its_cookies_for_a_combined_only_page() {
    let Some(lab) = lab() else { return };
    let server = lab.server();
    for id in ["best", "1080"] {
        let (mut task, job, selector) = analysed_tiktok(&lab, &server, id);
        assert!(
            selector.starts_with("bytevc1_1080p_1146746-1/"),
            "{selector}"
        );
        let resolver = Fixed {
            resolved: lab.streams(&server, "/gone", "/gone"),
            calls: AtomicUsize::new(0),
        };
        let cached = resolver.cached(&job).expect("luồng của lần phân tích");
        let ids: Vec<&str> = cached
            .streams
            .iter()
            .map(|stream| stream.format_id.as_str())
            .collect();
        assert_eq!(ids, ["bytevc1_1080p_1146746-1"], "đúng một luồng");

        let sink = Recorder::default();
        let fast = lab.fast(&sink, &resolver, CancellationToken::new());
        let Step::Go(plan) = start(&fast, &mut task, &job).await.unwrap() else {
            panic!("phải tải bằng đường nhanh");
        };
        assert_eq!(plan.source, Source::Cached);
        assert_eq!(
            plan.layout,
            Layout::Single,
            "một file có cả hình lẫn tiếng, không ghép"
        );
        assert_eq!(plan.children.len(), 1);
        assert_eq!(
            resolver.calls.load(Ordering::Relaxed),
            0,
            "không hỏi yt-dlp"
        );
        let url = utd_engine::validate_url(&plan.children[0].url).unwrap();
        let sent = plan.cookies.as_ref().and_then(|jar| jar.header_for(&url));
        assert_eq!(
            sent.as_ref().and_then(|value| value.to_str().ok()),
            Some("ttwid=1%7Cx; tt_chain_token=abc"),
            "cookie yt-dlp đặt cho luồng"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_a_combined_only_page_from_the_analysed_stream_without_asking_yt_dlp() {
    let Some(lab) = lab() else { return };
    let server = lab.server();
    let (mut task, job, _) = analysed_tiktok(&lab, &server, "best");
    let resolver = Fixed {
        resolved: lab.streams(&server, "/gone", "/gone"),
        calls: AtomicUsize::new(0),
    };
    let sink = Recorder::default();
    let fast = lab.fast(&sink, &resolver, CancellationToken::new());
    assert_eq!(run(&fast, &mut task, &job).await.unwrap(), FastEnd::Handled);
    assert_finished(&lab, &task);
    assert_eq!(
        resolver.calls.load(Ordering::Relaxed),
        0,
        "không chạy yt-dlp lần hai"
    );
    assert!(
        server.hits("/gone").is_empty(),
        "mirror -0 không được đụng tới"
    );
    let stream = server.hits("/tt-c.mp4");
    assert!(!stream.is_empty());
    assert!(
        stream
            .iter()
            .all(|hit| hit.cookie_text.as_deref() == Some("ttwid=1%7Cx; tt_chain_token=abc")),
        "{stream:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn falls_back_to_yt_dlp_when_the_server_refuses_before_any_byte() {
    let Some(lab) = lab() else { return };
    let server = lab.server();
    let (mut task, job) = lab.parent();
    let resolver = Fixed {
        resolved: lab.streams(&server, "/gone", "/gone"),
        calls: AtomicUsize::new(0),
    };
    let sink = Recorder::default();
    let fast = lab.fast(&sink, &resolver, CancellationToken::new());
    assert_eq!(
        run(&fast, &mut task, &job).await.unwrap(),
        FastEnd::Fallback
    );
    assert!(lab.journal.list_children(&task.id).unwrap().is_empty());
    assert!(
        lab.files().is_empty(),
        "không để lại .part: {:?}",
        lab.files()
    );
    assert_eq!(task.downloaded_bytes, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_gets_new_links_when_the_saved_ones_expired() {
    let Some(lab) = lab() else { return };
    let server = lab.server();
    let (mut task, job) = lab.parent();
    // Lượt trước: link đã hết hạn (`expire=1`) và máy chủ từ chối.
    let mut old = lab.streams(&server, "/gone", "/gone");
    for stream in &mut old.streams {
        stream.url = stream.url.replace("expire=", "old=") + "&expire=1";
    }
    parts::create(&lab.journal, &task, &old.streams, 16).unwrap();
    let resolver = Fixed {
        resolved: lab.streams(&server, "/v.mp4", "/a.m4a"),
        calls: AtomicUsize::new(0),
    };
    let sink = Recorder::default();
    let fast = lab.fast(&sink, &resolver, CancellationToken::new());
    assert_eq!(run(&fast, &mut task, &job).await.unwrap(), FastEnd::Handled);
    assert_eq!(
        resolver.calls.load(Ordering::Relaxed),
        1,
        "hỏi yt-dlp link mới một lần"
    );
    assert!(
        server.hits("/gone").is_empty(),
        "không gửi yêu cầu tới link hết hạn"
    );
    assert_finished(&lab, &task);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_with_finished_streams_only_muxes() {
    let Some(lab) = lab() else { return };
    let server = lab.server();
    let (mut task, job) = lab.parent();
    // Lượt trước tải xong cả hai luồng rồi bị dừng trước bước ghép.
    let streams = lab.streams(&server, "/v.mp4", "/a.m4a");
    let children = parts::create(&lab.journal, &task, &streams.streams, 16).unwrap();
    for child in &children {
        let outcome = lab
            .engine
            .resume(&child.id, utd_engine::DownloadControl::new())
            .await
            .unwrap();
        assert!(outcome.is_completed(), "{:?}", outcome.task().error);
    }
    task.status = TaskStatus::Paused;
    lab.journal.save_task(&task).unwrap();
    let before = server.hits.lock().unwrap().len();

    let resolver = Fixed {
        resolved: streams,
        calls: AtomicUsize::new(0),
    };
    let sink = Recorder::default();
    let fast = lab.fast(&sink, &resolver, CancellationToken::new());
    assert_eq!(run(&fast, &mut task, &job).await.unwrap(), FastEnd::Handled);
    assert_eq!(
        resolver.calls.load(Ordering::Relaxed),
        0,
        "không cần hỏi yt-dlp"
    );
    assert_eq!(
        server.hits.lock().unwrap().len(),
        before,
        "không tải lại byte nào"
    );
    assert!(
        sink.tasks
            .lock()
            .unwrap()
            .iter()
            .any(|event| event.status == TaskStatus::Verifying),
        "bước ghép phải báo trạng thái riêng"
    );
    assert_finished(&lab, &task);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_keeps_the_downloaded_ranges_and_resume_finishes() {
    let Some(lab) = lab() else { return };
    let server = lab.server();
    server.delay_ms.store(150, Ordering::Relaxed);
    let (mut task, job) = lab.parent();
    let resolver = Fixed {
        resolved: lab.streams(&server, "/v.mp4", "/a.m4a"),
        calls: AtomicUsize::new(0),
    };
    let cancel = CancellationToken::new();
    let pausing = Recorder {
        pause_on_bytes: Some(cancel.clone()),
        ..Recorder::default()
    };
    let fast = lab.fast(&pausing, &resolver, cancel);
    assert_eq!(run(&fast, &mut task, &job).await.unwrap(), FastEnd::Handled);
    assert_eq!(task.status, TaskStatus::Paused);
    {
        let progress = pausing.progress.lock().unwrap();
        assert!(progress.iter().all(|snapshot| snapshot.task_id == task.id));
        let busy = progress
            .iter()
            .find(|snapshot| snapshot.downloaded_bytes > 0)
            .expect("có tiến độ");
        assert!(busy.connections > 0, "số kết nối đang mở của cả hai luồng");
        assert!(busy.segments_total >= 2, "đoạn của cả hai luồng");
    }
    let children = lab.journal.list_children(&task.id).unwrap();
    assert_eq!(children.len(), 2);
    assert!(children
        .iter()
        .all(|child| child.status != TaskStatus::Failed));
    let video_part = children
        .iter()
        .find(|child| child.file_name == "Clip.f399.mp4")
        .expect("video part");
    let kept = video_part.downloaded_bytes;
    assert!(kept > 0, "đã giữ byte video tải được");
    assert!(task.downloaded_bytes > 0, "task cha lưu tiến độ gộp");
    // Yêu cầu của lượt trước có thể còn đang tới server: chờ rồi mới đếm.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let ranged = |server: &Server| -> u64 {
        server
            .hits("/v.mp4")
            .iter()
            .filter_map(|hit| hit.range_len)
            .sum()
    };
    let before = ranged(&server);

    server.delay_ms.store(0, Ordering::Relaxed);
    let sink = Recorder::default();
    let fast = lab.fast(&sink, &resolver, CancellationToken::new());
    assert_eq!(run(&fast, &mut task, &job).await.unwrap(), FastEnd::Handled);
    assert_eq!(
        resolver.calls.load(Ordering::Relaxed),
        1,
        "link còn hạn ⇒ không hỏi lại"
    );
    assert_finished(&lab, &task);
    let again = ranged(&server) - before;
    let video = lab.video.len() as u64;
    assert!(
        again <= video - kept,
        "chỉ tải phần còn thiếu: lượt sau xin {again} byte, còn thiếu {} byte",
        video - kept
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn starts_from_the_analysed_streams_without_asking_yt_dlp() {
    let Some(lab) = lab() else { return };
    let server = lab.server();
    let (mut task, job) = lab.analysed(&lab.streams(&server, "/v.mp4", "/a.m4a"));
    let resolver = Fixed {
        resolved: lab.streams(&server, "/gone", "/gone"),
        calls: AtomicUsize::new(0),
    };
    let sink = Recorder::default();
    let fast = lab.fast(&sink, &resolver, CancellationToken::new());
    assert_eq!(run(&fast, &mut task, &job).await.unwrap(), FastEnd::Handled);
    assert_eq!(
        resolver.calls.load(Ordering::Relaxed),
        0,
        "không chạy yt-dlp lần hai"
    );
    assert!(server.hits("/gone").is_empty());
    assert_finished(&lab, &task);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn analysed_links_refused_before_any_byte_get_new_links_from_yt_dlp() {
    let Some(lab) = lab() else { return };
    let server = lab.server();
    // Link của lần "Phân tích" đã chết (đổi mạng…): 403 ngay yêu cầu đầu.
    let (mut task, job) = lab.analysed(&lab.streams(&server, "/gone", "/gone"));
    let resolver = Fixed {
        resolved: lab.streams(&server, "/v.mp4", "/a.m4a"),
        calls: AtomicUsize::new(0),
    };
    let sink = Recorder::default();
    let fast = lab.fast(&sink, &resolver, CancellationToken::new());
    assert_eq!(
        run(&fast, &mut task, &job).await.unwrap(),
        FastEnd::Handled,
        "không rơi về yt-dlp tải một kết nối"
    );
    assert_eq!(
        resolver.calls.load(Ordering::Relaxed),
        1,
        "hỏi yt-dlp link mới một lần"
    );
    assert!(!server.hits("/gone").is_empty());
    assert!(resolver.cached(&job).is_none(), "quên link hỏng");
    assert_finished(&lab, &task);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_asks_for_new_links_when_saved_ones_fail_before_any_byte() {
    let Some(lab) = lab() else { return };
    let server = lab.server();
    let (mut task, job) = lab.parent();
    // Lượt trước dừng trước byte đầu; link còn hạn nhưng máy chủ đã từ chối.
    let saved = lab.streams(&server, "/gone", "/gone");
    parts::create(&lab.journal, &task, &saved.streams, 16).unwrap();
    let resolver = Fixed {
        resolved: lab.streams(&server, "/v.mp4", "/a.m4a"),
        calls: AtomicUsize::new(0),
    };
    let sink = Recorder::default();
    let fast = lab.fast(&sink, &resolver, CancellationToken::new());
    assert_eq!(run(&fast, &mut task, &job).await.unwrap(), FastEnd::Handled);
    assert_eq!(resolver.calls.load(Ordering::Relaxed), 1);
    assert_finished(&lab, &task);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fresh_links_refused_twice_still_fall_back_to_yt_dlp() {
    let Some(lab) = lab() else { return };
    let server = lab.server();
    // Link vừa phân tích hỏng, link mới của yt-dlp cũng hỏng: để yt-dlp tự tải như cũ.
    let (mut task, job) = lab.analysed(&lab.streams(&server, "/gone", "/gone"));
    let resolver = Fixed {
        resolved: lab.streams(&server, "/gone", "/gone"),
        calls: AtomicUsize::new(0),
    };
    let sink = Recorder::default();
    let fast = lab.fast(&sink, &resolver, CancellationToken::new());
    assert_eq!(
        run(&fast, &mut task, &job).await.unwrap(),
        FastEnd::Fallback
    );
    assert_eq!(resolver.calls.load(Ordering::Relaxed), 1);
    assert!(lab.journal.list_children(&task.id).unwrap().is_empty());
    assert!(
        lab.files().is_empty(),
        "không để lại .part: {:?}",
        lab.files()
    );
}

/// Video đọc thẳng từ trang (Tin Facebook, `page_streams`): luồng hình + tiếng tiện ích gửi, tải song song rồi ghép,
/// không hỏi yt-dlp. Link hỏng trước byte nào ⇒ `Fallback` (runner báo "mở lại trang", không chạy yt-dlp).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn page_streams_download_and_merge_without_yt_dlp() {
    use crate::media::page_streams::{self, Kind, PageStream};
    let Some(lab) = lab() else { return };
    let server = lab.server();
    let page = |video: &str, audio: &str| {
        let streams = [
            PageStream { url: server.url(video), kind: Kind::Video, codecs: Some("avc1.64001f".into()), size_bytes: None },
            PageStream { url: server.url(audio), kind: Kind::Audio, codecs: Some("mp4a.40.2".into()), size_bytes: None },
        ];
        page_streams::Fixed(page_streams::resolved(&streams, Some("https://www.facebook.com/stories/1/2/")))
    };
    let (mut task, job) = lab.parent_for(
        "https://www.facebook.com/stories/1/2/",
        Some("https://www.facebook.com/stories/1/2/"),
        page_streams::SELECTOR,
    );
    let resolver = page("/v.mp4", "/a.m4a");
    let sink = Recorder::default();
    let fast = lab.fast(&sink, &resolver, CancellationToken::new());
    assert_eq!(run(&fast, &mut task, &job).await.unwrap(), FastEnd::Handled);
    assert_finished(&lab, &task);
    assert!(
        server
            .hits("/v.mp4")
            .iter()
            .all(|hit| hit.referer.as_deref() == Some("https://www.facebook.com/")),
        "gửi gốc trang như trình phát của trang"
    );

    let (mut gone, job) = lab.parent_for("https://www.facebook.com/stories/1/3/", None, page_streams::SELECTOR);
    let resolver = page("/gone", "/gone");
    let fast = lab.fast(&sink, &resolver, CancellationToken::new());
    assert_eq!(run(&fast, &mut gone, &job).await.unwrap(), FastEnd::Fallback);
}

#[test]
fn shares_the_host_connection_budget_by_remaining_bytes() {
    // Hình 403 MB + tiếng 144 MB của 09OGSRptb0M: 12 + 4 kết nối, xong gần cùng lúc.
    assert_eq!(
        connection_shares(&[Some(402_957_281), Some(144_165_177)], 16),
        [Some(12), Some(4)]
    );
    assert_eq!(
        connection_shares(&[Some(10), Some(10_000)], 16),
        [Some(2), Some(14)],
        "luồng nhỏ vẫn có ít nhất 2 kết nối"
    );
    assert_eq!(
        connection_shares(&[Some(500), None], 16),
        [None, None],
        "không rõ kích thước ⇒ engine tự chọn"
    );
    assert_eq!(connection_shares(&[Some(500)], 16), [None]);
    assert_eq!(connection_shares(&[Some(0), Some(0)], 16), [None, None]);
}
