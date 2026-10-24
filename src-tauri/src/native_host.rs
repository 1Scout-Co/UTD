use std::io::{BufRead, Read, Write};

use serde::{Deserialize, Serialize};

const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostRequest {
    version: u32,
    #[serde(rename = "type")]
    kind: String,
    url: String,
    #[serde(rename = "pageUrl")]
    page_url: Option<String>,
    suggested_name: Option<String>,
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
    if request.version != 1 || request.kind != "download" {
        return Err("Yêu cầu từ trình duyệt không hợp lệ.".into());
    }
    let url = request.url.trim();
    if url.len() > 8192
        || !(url.starts_with("http://") || url.starts_with("https://"))
    {
        return Err("Chỉ hỗ trợ URL HTTP/HTTPS.".into());
    }
    let suggested_name = request
        .suggested_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty() && name.chars().count() <= 180);
    // Trang đang mở (để UTD mở đúng luồng media); bỏ qua nếu lạ thay vì từ chối cả yêu cầu.
    let page_url = request
        .page_url
        .as_deref()
        .map(str::trim)
        .filter(|page| page.len() <= 8192 && (page.starts_with("http://") || page.starts_with("https://")));

    let (config, path) = crate::ipc::read_config()?;
    let payload = serde_json::json!({
        "token": config.token,
        "url": url,
        "suggestedName": suggested_name,
        "pageUrl": page_url,
    });
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
}
