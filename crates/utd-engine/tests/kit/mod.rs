#![allow(dead_code)]
//! Server thử giữ kết nối (keep-alive) cho các bài thử trần byte, header riêng và AC-06:
//! ghi lại từng yêu cầu, trả lời theo kịch bản, phục vụ Range, giới hạn tốc độ từng phản hồi.

use std::fmt::Write as _;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep;
use utd_engine::{DownloadEngine, DownloadTask, EngineOptions, Journal};

pub const KIB: usize = 1024;
pub const MIB: usize = 1024 * 1024;
const PIECE: usize = 16 * KIB;

#[derive(Debug, Clone)]
pub struct Req {
    pub method: String,
    pub path: String,
    /// Tên đã viết thường.
    pub headers: Vec<(String, String)>,
    pub range: Option<(u64, Option<u64>)>,
    pub connection: usize,
}

impl Req {
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn is_get(&self) -> bool {
        self.method == "GET"
    }

    /// Số byte đã xin; `None` khi không có Range hoặc Range mở.
    pub fn range_len(&self) -> Option<u64> {
        match self.range {
            Some((start, Some(end))) if end >= start => Some(end - start + 1),
            _ => None,
        }
    }
}

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub chunked: bool,
    /// Không tự thêm Content-Length (HEAD không báo độ dài).
    pub no_length: bool,
    /// B/s cho phản hồi này; `None` = hết tốc lực.
    pub rate: Option<u64>,
    /// Gửi chừng này byte thân rồi đóng kết nối.
    pub cut_after: Option<usize>,
    /// Gửi chừng này byte thân rồi im, giữ kết nối mở tới khi phía tải đóng (kết nối treo).
    pub stall_after: Option<usize>,
    /// Chờ chừng này rồi mới gửi đầu phản hồi (server treo, kết nối chết lặng).
    pub delay: Option<Duration>,
    /// Đợt rớt mạng dùng chung: mọi phản hồi mang nó cùng im trong cùng một khoảng.
    pub outage: Option<Arc<Outage>>,
    /// Đóng kết nối ngay, không trả lời gì (mạng đang rớt: kết nối bị reset).
    pub reset: bool,
    /// Công tắc mạng: `Switch::cut` (sau lúc tạo phản hồi) ⇒ đóng kết nối giữa chừng.
    pub switch: Option<(Arc<Switch>, u64)>,
}

/// Công tắc mạng dùng chung cho bài thử chaos: `cut` cắt mọi phản hồi đang gửi (rớt Wi-Fi, đổi mạng),
/// `set_down(true)` thì yêu cầu mới bị đóng ngay không trả lời tới khi bật lại.
#[derive(Default)]
pub struct Switch {
    generation: AtomicU64,
    down: AtomicBool,
}

impl Switch {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn cut(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn set_down(&self, down: bool) {
        self.down.store(down, Ordering::SeqCst);
        if down {
            self.cut();
        }
    }

    pub fn is_down(&self) -> bool {
        self.down.load(Ordering::SeqCst)
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    fn cut_since(&self, generation: u64) -> bool {
        self.generation() != generation
    }
}

/// Mạng rớt cả loạt (Wi-Fi chuyển điểm phát, modem khởi động lại): mọi phản hồi mang cùng một
/// `Outage` cùng im trong `[from, from + length)`, tính từ lúc phản hồi đầu tiên bắt đầu gửi.
pub struct Outage {
    epoch: OnceLock<Instant>,
    from: Duration,
    length: Duration,
    held: AtomicUsize,
}

impl Outage {
    pub fn new(from: Duration, length: Duration) -> Arc<Self> {
        Arc::new(Self {
            epoch: OnceLock::new(),
            from,
            length,
            held: AtomicUsize::new(0),
        })
    }

    /// Khoảng im `[đầu, cuối)`; `None` khi chưa có phản hồi nào.
    pub fn window(&self) -> Option<(Instant, Instant)> {
        let begin = *self.epoch.get()? + self.from;
        Some((begin, begin + self.length))
    }

    /// Số lần một phản hồi phải im chờ hết đợt rớt.
    pub fn held(&self) -> usize {
        self.held.load(Ordering::SeqCst)
    }

    async fn hold(&self) {
        self.epoch.get_or_init(Instant::now);
        let Some((begin, end)) = self.window() else {
            return;
        };
        let now = Instant::now();
        if now >= begin && now < end {
            self.held.fetch_add(1, Ordering::SeqCst);
            sleep(end - now).await;
        }
    }
}

impl Reply {
    pub fn new(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
            chunked: false,
            no_length: false,
            rate: None,
            cut_after: None,
            stall_after: None,
            delay: None,
            outage: None,
            reset: false,
            switch: None,
        }
    }

    /// Đóng kết nối ngay, không trả lời.
    pub fn reset() -> Self {
        Self {
            reset: true,
            ..Self::new(0)
        }
    }

    /// Phản hồi bị cắt khi công tắc mạng `cut` sau lúc này.
    pub fn on(mut self, switch: &Arc<Switch>) -> Self {
        self.switch = Some((switch.clone(), switch.generation()));
        self
    }

    pub fn rate(mut self, bytes_per_second: u64) -> Self {
        self.rate = Some(bytes_per_second);
        self
    }

    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_string(), value.into()));
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    pub fn chunked(mut self) -> Self {
        self.chunked = true;
        self
    }

    pub fn no_length(mut self) -> Self {
        self.no_length = true;
        self
    }

    pub fn redirect(location: &str) -> Self {
        Self::new(302).header("Location", location)
    }
}

pub type Handler = Arc<dyn Fn(&Req) -> Reply + Send + Sync>;

pub struct Server {
    pub addr: SocketAddr,
    log: Arc<Mutex<Vec<Req>>>,
    connections: Arc<AtomicUsize>,
    inflight: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct Shared {
    handler: Handler,
    log: Arc<Mutex<Vec<Req>>>,
    connections: Arc<AtomicUsize>,
    inflight: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

impl Server {
    pub async fn start(handler: Handler) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let shared = Shared {
            handler,
            log: Arc::new(Mutex::new(Vec::new())),
            connections: Arc::new(AtomicUsize::new(0)),
            inflight: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
        };
        let server = Self {
            addr,
            log: shared.log.clone(),
            connections: shared.connections.clone(),
            inflight: shared.inflight.clone(),
            peak: shared.peak.clone(),
        };
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let shared = shared.clone();
                let id = shared.connections.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move { serve_connection(stream, shared, id).await });
            }
        });
        server
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    pub fn requests(&self) -> Vec<Req> {
        self.log.lock().expect("log").clone()
    }

    pub fn gets(&self) -> Vec<Req> {
        self.requests().into_iter().filter(Req::is_get).collect()
    }

    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    pub fn peak_inflight(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }
}

async fn serve_connection(mut stream: TcpStream, shared: Shared, id: usize) {
    let mut buffer = Vec::new();
    loop {
        let Some(mut request) = read_request(&mut stream, &mut buffer).await else {
            return;
        };
        request.connection = id;
        shared.log.lock().expect("log").push(request.clone());
        let reply = (shared.handler)(&request);
        let head = request.method == "HEAD";
        let counted = !head;
        if counted {
            let now = shared.inflight.fetch_add(1, Ordering::SeqCst) + 1;
            shared.peak.fetch_max(now, Ordering::SeqCst);
        }
        let kept = write_reply(&mut stream, &reply, head).await;
        if counted {
            shared.inflight.fetch_sub(1, Ordering::SeqCst);
        }
        if !kept {
            let _ = stream.shutdown().await;
            return;
        }
    }
}

/// Trả `false` khi kết nối phải đóng.
async fn write_reply(stream: &mut TcpStream, reply: &Reply, head: bool) -> bool {
    if reply.reset {
        return false;
    }
    if let Some(delay) = reply.delay {
        sleep(delay).await;
    }
    if let Some(outage) = &reply.outage {
        outage.hold().await;
    }
    let mut text = format!("HTTP/1.1 {} {}\r\n", reply.status, reason(reply.status));
    for (name, value) in &reply.headers {
        let _ = write!(text, "{name}: {value}\r\n");
    }
    let has_length = reply
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-length"));
    if reply.chunked {
        text.push_str("Transfer-Encoding: chunked\r\n");
    } else if !has_length && !reply.no_length {
        let _ = write!(text, "Content-Length: {}\r\n", reply.body.len());
    }
    text.push_str("\r\n");
    if stream.write_all(text.as_bytes()).await.is_err() {
        return false;
    }
    if head {
        return stream.flush().await.is_ok();
    }
    let limit = reply
        .cut_after
        .unwrap_or(usize::MAX)
        .min(reply.stall_after.unwrap_or(usize::MAX))
        .min(reply.body.len());
    let mut written = 0;
    while written < limit {
        if let Some(outage) = &reply.outage {
            outage.hold().await;
        }
        if reply
            .switch
            .as_ref()
            .is_some_and(|(switch, generation)| switch.cut_since(*generation))
        {
            return false;
        }
        let end = (written + PIECE).min(limit);
        let piece = &reply.body[written..end];
        let sent = if reply.chunked {
            let header = format!("{:x}\r\n", piece.len());
            stream.write_all(header.as_bytes()).await.is_ok()
                && stream.write_all(piece).await.is_ok()
                && stream.write_all(b"\r\n").await.is_ok()
        } else {
            stream.write_all(piece).await.is_ok()
        };
        if !sent {
            return false;
        }
        written = end;
        if let Some(rate) = reply.rate {
            sleep(Duration::from_secs_f64(
                piece.len() as f64 / rate.max(1) as f64,
            ))
            .await;
        }
    }
    if reply
        .stall_after
        .is_some_and(|stall| stall < reply.body.len())
    {
        // Treo: không gửi thêm, chờ phía tải tự đóng (đọc được EOF) hoặc hết giờ thử.
        let _ = stream.flush().await;
        let mut byte = [0u8; 1];
        let _ = tokio::time::timeout(Duration::from_secs(60), stream.read(&mut byte)).await;
        return false;
    }
    if reply.cut_after.is_some_and(|cut| cut < reply.body.len()) {
        let _ = stream.flush().await;
        return false;
    }
    if reply.chunked && stream.write_all(b"0\r\n\r\n").await.is_err() {
        return false;
    }
    stream.flush().await.is_ok()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        206 => "Partial Content",
        302 => "Found",
        404 => "Not Found",
        405 => "Method Not Allowed",
        416 => "Range Not Satisfiable",
        429 => "Too Many Requests",
        _ => "Status",
    }
}

async fn read_request(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> Option<Req> {
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer[..position]).into_owned();
            buffer.drain(..position + 4);
            return parse_request(&head);
        }
        if buffer.len() > 64 * KIB {
            return None;
        }
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

fn parse_request(head: &str) -> Option<Req> {
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_ascii_uppercase();
    let path = first.next()?.to_string();
    let mut headers = Vec::new();
    let mut range = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        if name == "range" {
            range = parse_range(&value);
        }
        headers.push((name, value));
    }
    Some(Req {
        method,
        path,
        headers,
        range,
        connection: 0,
    })
}

fn parse_range(value: &str) -> Option<(u64, Option<u64>)> {
    let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
    let start = start.trim().parse().ok()?;
    let end = end.trim();
    let end = if end.is_empty() {
        None
    } else {
        Some(end.parse().ok()?)
    };
    Some((start, end))
}

/// Cách phục vụ một file có Range.
#[derive(Clone)]
pub struct FileOpts {
    pub content_type: Option<String>,
    /// Không báo tổng (HEAD không Content-Length, Content-Range `/*`).
    pub unknown_total: bool,
}

impl Default for FileOpts {
    fn default() -> Self {
        Self {
            content_type: Some("application/octet-stream".into()),
            unknown_total: false,
        }
    }
}

/// HEAD / GET có Range / GET thường cho `body`.
pub fn serve_file(request: &Req, body: &[u8], opts: &FileOpts) -> Reply {
    let total = body.len() as u64;
    let with_type = |reply: Reply| match &opts.content_type {
        Some(kind) => reply.header("Content-Type", kind.clone()),
        None => reply,
    };
    if request.method == "HEAD" {
        let reply = with_type(Reply::new(200).header("Accept-Ranges", "bytes"));
        return if opts.unknown_total {
            reply.no_length()
        } else {
            reply.header("Content-Length", total.to_string())
        };
    }
    let Some((start, end)) = request.range else {
        return with_type(Reply::new(200).body(body.to_vec()));
    };
    let total_text = if opts.unknown_total {
        "*".to_string()
    } else {
        total.to_string()
    };
    if start >= total {
        return Reply::new(416).header("Content-Range", format!("bytes */{total_text}"));
    }
    let end = end.unwrap_or(total - 1).min(total - 1);
    let slice = body[start as usize..=end as usize].to_vec();
    with_type(
        Reply::new(206)
            .header("Accept-Ranges", "bytes")
            .header("Content-Range", format!("bytes {start}-{end}/{total_text}"))
            .body(slice),
    )
}

pub fn file_handler(body: Arc<Vec<u8>>, opts: FileOpts) -> Handler {
    Arc::new(move |request| serve_file(request, &body, &opts))
}

pub fn payload(size: usize) -> Vec<u8> {
    (0..size)
        .map(|index| ((index.wrapping_mul(31).wrapping_add(7)) % 251) as u8)
        .collect()
}

pub fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        write!(&mut hex, "{byte:02x}").expect("hex");
    }
    hex
}

pub fn options(initial: u16, max: u16) -> EngineOptions {
    EngineOptions {
        initial_connections: initial,
        large_initial_connections: initial,
        max_connections: max,
        host_connections: 16,
        global_connections: 32,
        max_attempts: 5,
        connect_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(5),
        // Nhỏ hơn idle_timeout như cấu hình thật: luật "kết nối treo" chạy trong mọi bài.
        stall_timeout: Duration::from_secs(2),
        sample_interval: Duration::from_millis(50),
        checkpoint_interval: Duration::from_millis(100),
        balance_interval: Duration::from_millis(200),
        user_agent: "utd-test/0.1".into(),
        quarantine_dir: None,
    }
}

pub fn engine(directory: &Path, options: EngineOptions) -> (Arc<Journal>, DownloadEngine) {
    let journal = Arc::new(Journal::open(directory.join("utd.db")).expect("journal"));
    let engine = DownloadEngine::new(journal.clone(), options).expect("engine");
    (journal, engine)
}

pub fn downloads(directory: &Path) -> String {
    directory.join("downloads").to_string_lossy().into_owned()
}

pub fn final_path(task: &DownloadTask) -> std::path::PathBuf {
    Path::new(&task.dest_dir).join(&task.file_name)
}

/// Mọi file trong `directory` (kể cả thư mục con) có chứa `needle`: journal (`utd.db`, `-wal`, `-shm`), file
/// tạm, file tải. Bài thử "cookie mồi" dùng để chứng minh bí mật không bao giờ xuống đĩa (P-02, P-04, AC-30).
pub fn files_containing(directory: &Path, needle: &[u8]) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).expect("read_dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if std::fs::read(&path)
                .is_ok_and(|bytes| bytes.windows(needle.len()).any(|window| window == needle))
            {
                found.push(path);
            }
        }
    }
    found
}
