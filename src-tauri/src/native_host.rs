use std::io::{BufRead, Read, Write};

use serde::{Deserialize, Serialize};

const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_URL_BYTES: usize = 8192;

/// `type`: `download` (một link) hoặc `batch` (lô link "Tải tất cả link bằng UTD", trong `urls`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostRequest {
    version: u32,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    url: String,
    #[serde(rename = "pageUrl")]
    page_url: Option<String>,
    suggested_name: Option<String>,
    #[serde(default)]
    urls: Vec<String>,
}

fn is_web_url(value: &str) -> bool {
    value.len() <= MAX_URL_BYTES && (value.starts_with("http://") || value.starts_with("https://"))
}

/// Tin nhắn từ trình duyệt ⇒ dòng JSON gửi qua pipe (chưa kèm token). App kiểm lại lần nữa (`ipc.rs`).
fn pipe_payload(request: &HostRequest) -> Result<serde_json::Value, String> {
    if request.version != 1 {
        return Err("Yêu cầu từ trình duyệt không hợp lệ.".into());
    }
    // Trang đang mở (để UTD mở đúng luồng media); bỏ qua nếu lạ thay vì từ chối cả yêu cầu.
    let page_url = request.page_url.as_deref().map(str::trim).filter(|page| is_web_url(page));
    match request.kind.as_str() {
        "download" => {
            let url = request.url.trim();
            if !is_web_url(url) {
                return Err("Chỉ hỗ trợ URL HTTP/HTTPS.".into());
            }
            let suggested_name = request
                .suggested_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty() && name.chars().count() <= 180);
            Ok(serde_json::json!({
                "url": url,
                "suggestedName": suggested_name,
                "pageUrl": page_url,
            }))
        }
        "batch" => {
            if request.urls.len() > crate::batch::MAX_LINKS {
                return Err("Quá nhiều link trong một lần gửi.".into());
            }
            let urls: Vec<&str> = request
                .urls
                .iter()
                .map(|url| url.trim())
                .filter(|url| is_web_url(url))
                .collect();
            if urls.is_empty() {
                return Err("Trang này không có link tải được.".into());
            }
            Ok(serde_json::json!({
                "url": page_url.unwrap_or(""),
                "pageUrl": page_url,
                "batch": urls,
            }))
        }
        _ => Err("Yêu cầu từ trình duyệt không hợp lệ.".into()),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HostResponse {
    ok: bool,
    task_id: Option<String>,
    message: Option<String>,
}

pub fn invoked() -> bool {
    if std::env::var("UTD_NATIVE_HOST").is_ok_and(|value| value == "1") {
        return true;
    }
    if std::env::args().any(|argument| argument == "--native-host") {
        return true;
    }
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .is_some_and(|stem| stem.eq_ignore_ascii_case("utd-native-host"))
}

pub fn run() -> i32 {
    match forward() {
        Ok(response) => {
            let ok = response.ok;
            write_response(&response);
            if ok { 0 } else { 1 }
        }
        Err(message) => {
            log_line(&format!("error: {message}"));
            write_response(&HostResponse {
                ok: false,
                task_id: None,
                message: Some(message),
            });
            1
        }
    }
}

fn forward() -> Result<HostResponse, String> {
    let request = read_request()?;
    let mut payload = pipe_payload(&request)?;

    // Windows (named pipe) và macOS (Unix socket) gửi cùng một dòng JSON: mọi loại tin nhắn đi chung đường.
    let (config, path) = crate::ipc::read_config()?;
    payload["token"] = serde_json::Value::String(config.token.clone());
    let payload = serde_json::to_string(&payload).map_err(|error| error.to_string())?;
    let response = exchange(connect(&config, &path)?, &payload)?;
    if response.ok {
        log_line("forwarded");
        Ok(response)
    } else {
        Err(response
            .message
            .unwrap_or_else(|| "UTD từ chối yêu cầu.".into()))
    }
}

const NOT_RUNNING: &str = "UTD chưa chạy. Mở UTD rồi thử lại.";

/// Windows: named pipe ghi trong `ipc.json`.
#[cfg(windows)]
fn connect(config: &crate::ipc::IpcConfig, _: &std::path::Path) -> Result<std::fs::File, String> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(crate::ipc::pipe_path(&config.pipe))
        .map_err(|_| NOT_RUNNING.to_string())
}

/// macOS: Unix socket nằm cạnh `ipc.json` (`config_path`).
#[cfg(unix)]
fn connect(
    _: &crate::ipc::IpcConfig,
    config_path: &std::path::Path,
) -> Result<std::os::unix::net::UnixStream, String> {
    let directory = config_path.parent().ok_or_else(|| NOT_RUNNING.to_string())?;
    let stream = std::os::unix::net::UnixStream::connect(crate::ipc::socket_path(directory))
        .map_err(|_| NOT_RUNNING.to_string())?;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(15)));
    Ok(stream)
}

/// Gửi một dòng JSON, đọc một dòng trả lời.
fn exchange<S: Read + Write>(mut stream: S, payload: &str) -> Result<HostResponse, String> {
    stream
        .write_all(format!("{payload}\n").as_bytes())
        .map_err(|_| "Không gửi được yêu cầu sang UTD.".to_string())?;
    stream
        .flush()
        .map_err(|_| "Không gửi được yêu cầu sang UTD.".to_string())?;

    let mut reader = std::io::BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|_| "UTD không phản hồi.".to_string())?;
    if line.trim().is_empty() {
        return Err("UTD không phản hồi.".into());
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Response {
        ok: bool,
        task_id: Option<String>,
        message: Option<String>,
    }
    let response: Response =
        serde_json::from_str(line.trim()).map_err(|_| "UTD trả về dữ liệu lạ.".to_string())?;
    Ok(HostResponse {
        ok: response.ok,
        task_id: response.task_id,
        message: response.message,
    })
}

fn read_request() -> Result<HostRequest, String> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut length = [0u8; 4];
    input
        .read_exact(&mut length)
        .map_err(|_| "Trình duyệt không gửi dữ liệu.".to_string())?;
    let size = u32::from_le_bytes(length) as usize;
    if size == 0 || size > MAX_MESSAGE_BYTES {
        return Err("Kích thước tin nhắn không hợp lệ.".into());
    }
    let mut payload = vec![0u8; size];
    input
        .read_exact(&mut payload)
        .map_err(|_| "Không đọc được tin nhắn từ trình duyệt.".to_string())?;
    serde_json::from_slice(&payload).map_err(|_| "Tin nhắn từ trình duyệt không hợp lệ.".into())
}

fn write_response(response: &HostResponse) {
    let Ok(payload) = serde_json::to_vec(response) else {
        return;
    };
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    let _ = output.write_all(&(payload.len() as u32).to_le_bytes());
    let _ = output.write_all(&payload);
    let _ = output.flush();
}

fn log_line(message: &str) {
    let Ok(directory) = crate::ipc::data_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&directory);
    let path = directory.join("native-host.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Đầu kia giả: trả sẵn một dòng, giữ lại những gì host gửi.
    struct Peer {
        reply: std::io::Cursor<Vec<u8>>,
        sent: Vec<u8>,
    }

    impl Read for Peer {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.reply.read(buffer)
        }
    }

    impl Write for Peer {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.sent.extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn peer(reply: &str) -> Peer {
        Peer {
            reply: std::io::Cursor::new(reply.as_bytes().to_vec()),
            sent: Vec::new(),
        }
    }

    #[test]
    fn exchange_sends_one_line_and_reads_one_line() {
        let mut app = peer("{\"ok\":true,\"taskId\":\"t1\",\"message\":null}\n");
        let response = exchange(&mut app, "{\"url\":\"https://x\"}").unwrap();
        assert!(response.ok);
        assert_eq!(response.task_id.as_deref(), Some("t1"));
        assert_eq!(app.sent, b"{\"url\":\"https://x\"}\n");
        assert_eq!(exchange(&mut peer(""), "{}").unwrap_err(), "UTD không phản hồi.");
        assert_eq!(exchange(&mut peer("rac\n"), "{}").unwrap_err(), "UTD trả về dữ liệu lạ.");
    }

    fn message(value: serde_json::Value) -> HostRequest {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn single_links_keep_the_old_message_shape() {
        let payload = pipe_payload(&message(serde_json::json!({
            "version": 1, "type": "download", "url": " https://example.com/a.zip ",
            "pageUrl": "javascript:x", "suggestedName": "  ",
        })))
        .unwrap();
        assert_eq!(
            payload,
            serde_json::json!({ "url": "https://example.com/a.zip", "suggestedName": null, "pageUrl": null })
        );
        let bad = message(serde_json::json!({ "version": 1, "type": "download", "url": "file:///c:/x" }));
        assert!(pipe_payload(&bad).is_err());
        let old = message(serde_json::json!({ "version": 2, "type": "download", "url": "https://a.b/c" }));
        assert!(pipe_payload(&old).is_err());
    }

    #[test]
    fn batches_are_checked_and_capped() {
        let payload = pipe_payload(&message(serde_json::json!({
            "version": 1, "type": "batch", "pageUrl": "https://example.com/gallery",
            "urls": ["https://example.com/1.jpg", "data:image/png;base64,AA", " https://example.com/2.zip "],
        })))
        .unwrap();
        assert_eq!(payload["batch"], serde_json::json!(["https://example.com/1.jpg", "https://example.com/2.zip"]));
        assert_eq!(payload["url"], "https://example.com/gallery");

        let junk = message(serde_json::json!({ "version": 1, "type": "batch", "urls": ["blob:x"] }));
        assert!(pipe_payload(&junk).is_err());
        let many: Vec<String> = (0..=crate::batch::MAX_LINKS)
            .map(|index| format!("https://example.com/{index}.jpg"))
            .collect();
        let flood = message(serde_json::json!({ "version": 1, "type": "batch", "urls": many }));
        assert!(pipe_payload(&flood).is_err());
        let unknown = message(serde_json::json!({ "version": 1, "type": "exec", "url": "https://a.b" }));
        assert!(pipe_payload(&unknown).is_err());
    }

    /// `run` ghi câu lỗi vào `native-host.log` (P-04): câu lỗi không bao giờ lặp lại nội dung tin nhắn, kể
    /// cả link có token hay trường lạ.
    #[test]
    fn errors_that_reach_the_host_log_never_echo_the_request() {
        let secret = "UTDCANARY7f3a9c";
        for request in [
            serde_json::json!({ "version": 1, "type": "download", "url": format!("file:///c:/{secret}") }),
            serde_json::json!({ "version": 1, "type": "download", "url": format!("https://a.b/?t={secret}{}", "x".repeat(MAX_URL_BYTES)) }),
            serde_json::json!({ "version": 2, "type": "download", "url": format!("https://a.b/?t={secret}") }),
            serde_json::json!({ "version": 1, "type": secret, "url": "https://a.b/" }),
            serde_json::json!({ "version": 1, "type": "batch", "urls": [format!("javascript:{secret}")] }),
        ] {
            let error = pipe_payload(&message(request)).unwrap_err();
            assert!(!error.contains(secret), "{error}");
        }
        let reply = "{\"ok\":false,\"taskId\":null,\"message\":\"Token IPC không hợp lệ.\"}\n";
        let response = exchange(&mut peer(reply), &format!("{{\"token\":\"{secret}\"}}")).unwrap();
        assert!(!response.message.unwrap_or_default().contains(secret));
    }
}
