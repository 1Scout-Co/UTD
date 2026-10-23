use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep;

#[derive(Clone)]
pub struct ServerConfig {
    pub ranges: bool,
    pub head_ok: bool,
    pub etag: String,
    pub retry_after_first: usize,
    pub drops: usize,
    pub drop_bytes: usize,
    pub chunk_delay: Duration,
    pub chunk_size: usize,
    pub omit_length: bool,
    /// Có giá trị ⇒ mọi yêu cầu nhận 302 tới địa chỉ này.
    pub redirect_to: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            ranges: true,
            head_ok: true,
            etag: "\"v1\"".into(),
            retry_after_first: 0,
            drops: 0,
            drop_bytes: 0,
            chunk_delay: Duration::ZERO,
            chunk_size: 16 * 1024,
            omit_length: false,
            redirect_to: None,
        }
    }
}

pub struct TestServer {
    pub addr: SocketAddr,
    pub requests: Arc<AtomicUsize>,
    pub range_requests: Arc<AtomicUsize>,
}

impl TestServer {
    pub async fn start(body: Vec<u8>, config: ServerConfig) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let requests = Arc::new(AtomicUsize::new(0));
        let range_requests = Arc::new(AtomicUsize::new(0));
        let drops_left = AtomicUsize::new(config.drops);
        let retries_left = AtomicUsize::new(config.retry_after_first);
        let state = Arc::new(Inner {
            body,
            config,
            requests: requests.clone(),
            range_requests: range_requests.clone(),
            drops_left,
            retries_left,
        });
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let state = state.clone();
                tokio::spawn(async move {
                    handle(stream, state).await;
                });
            }
        });
        Self {
            addr,
            requests,
            range_requests,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }
}

struct Inner {
    body: Vec<u8>,
    config: ServerConfig,
    requests: Arc<AtomicUsize>,
    range_requests: Arc<AtomicUsize>,
    drops_left: AtomicUsize,
    retries_left: AtomicUsize,
}

struct Request {
    head: bool,
    range: Option<(u64, Option<u64>)>,
}

async fn handle(mut stream: TcpStream, state: Arc<Inner>) {
    let Some(request) = read_request(&mut stream).await else {
        return;
    };
    state.requests.fetch_add(1, Ordering::SeqCst);
    if let Some(location) = &state.config.redirect_to {
        let response = format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(response.as_bytes()).await;
        return;
    }
    let total = state.body.len();
    let is_probe = request.head || matches!(request.range, Some((0, Some(0))));

    if !is_probe
        && state
            .retries_left
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| value.checked_sub(1))
            .is_ok()
    {
        let _ = stream
            .write_all(
                b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await;
        return;
    }

    if request.head {
        if !state.config.head_ok {
            let _ = stream
                .write_all(b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
            return;
        }
        let accept = if state.config.ranges { "bytes" } else { "none" };
        let response = if state.config.omit_length {
            format!(
                "HTTP/1.1 200 OK\r\nAccept-Ranges: {accept}\r\nETag: {etag}\r\nConnection: close\r\n\r\n",
                etag = state.config.etag
            )
        } else {
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nAccept-Ranges: {accept}\r\nETag: {etag}\r\nConnection: close\r\n\r\n",
                etag = state.config.etag
            )
        };
        let _ = stream.write_all(response.as_bytes()).await;
        return;
    }

    if state.config.ranges
        && let Some((start, end)) = request.range
    {
        state.range_requests.fetch_add(1, Ordering::SeqCst);
        if start >= total as u64 {
            let response = format!(
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{total}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(response.as_bytes()).await;
            return;
        }
        let end = end.unwrap_or(total as u64 - 1).min(total as u64 - 1);
        let slice = &state.body[start as usize..=end as usize];
        let header = format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{total}\r\nAccept-Ranges: bytes\r\nETag: {}\r\nConnection: close\r\n\r\n",
            slice.len(),
            state.config.etag
        );
        if stream.write_all(header.as_bytes()).await.is_err() {
            return;
        }
        let drop = slice.len() > 2 && take(&state.drops_left);
        write_body(&mut stream, slice, &state.config, drop).await;
        return;
    }

    let header = if state.config.omit_length {
        format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nAccept-Ranges: none\r\nETag: {}\r\nConnection: close\r\n\r\n",
            state.config.etag
        )
    } else {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nAccept-Ranges: none\r\nETag: {}\r\nConnection: close\r\n\r\n",
            state.config.etag
        )
    };
    if stream.write_all(header.as_bytes()).await.is_err() {
        return;
    }
    if state.config.omit_length {
        write_chunked(&mut stream, &state.body, &state.config).await;
        return;
    }
    let drop = total > 2 && take(&state.drops_left);
    write_body(&mut stream, &state.body, &state.config, drop).await;
}

async fn write_body(stream: &mut TcpStream, body: &[u8], config: &ServerConfig, drop: bool) {
    let limit = if drop {
        config.drop_bytes.min(body.len())
    } else {
        body.len()
    };
    let chunk = config.chunk_size.max(1);
    let mut written = 0;
    while written < limit {
        let end = (written + chunk).min(limit);
        if stream.write_all(&body[written..end]).await.is_err() {
            return;
        }
        written = end;
        if !config.chunk_delay.is_zero() {
            sleep(config.chunk_delay).await;
        }
    }
    let _ = stream.flush().await;
    if drop {
        let _ = stream.shutdown().await;
    }
}

async fn write_chunked(stream: &mut TcpStream, body: &[u8], config: &ServerConfig) {
    let chunk = config.chunk_size.max(1);
    let mut written = 0;
    while written < body.len() {
        let end = (written + chunk).min(body.len());
        let header = format!("{:x}\r\n", end - written);
        if stream.write_all(header.as_bytes()).await.is_err() {
            return;
        }
        if stream.write_all(&body[written..end]).await.is_err() {
            return;
        }
        if stream.write_all(b"\r\n").await.is_err() {
            return;
        }
        written = end;
        if !config.chunk_delay.is_zero() {
            sleep(config.chunk_delay).await;
        }
    }
    let _ = stream.write_all(b"0\r\n\r\n").await;
}

fn take(counter: &AtomicUsize) -> bool {
    counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| value.checked_sub(1))
        .is_ok()
}

async fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer[..position]);
            let mut lines = head.lines();
            let head_request = lines
                .next()?
                .split_whitespace()
                .next()
                .is_some_and(|method| method.eq_ignore_ascii_case("HEAD"));
            let mut range = None;
            for line in lines {
                let Some((name, value)) = line.split_once(':') else {
                    continue;
                };
                if name.eq_ignore_ascii_case("range") {
                    range = parse_range(value.trim());
                }
            }
            return Some(Request {
                head: head_request,
                range,
            });
        }
        if buffer.len() > 32 * 1024 {
            return None;
        }
    }
}

fn parse_range(value: &str) -> Option<(u64, Option<u64>)> {
    let value = value.strip_prefix("bytes=")?;
    let (start, end) = value.split_once('-')?;
    let start = start.trim().parse().ok()?;
    let end = end.trim();
    let end = if end.is_empty() {
        None
    } else {
        Some(end.parse().ok()?)
    };
    Some((start, end))
}
