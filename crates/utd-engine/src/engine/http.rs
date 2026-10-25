use std::time::{Duration, SystemTime};

use percent_encoding::percent_decode_str;
use reqwest::header::{
    HeaderMap, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG,
    LAST_MODIFIED, RETRY_AFTER,
};
use reqwest::{Method, Response, StatusCode};
use url::Url;

use super::transport::Transport;
use crate::model::{validate_file_name, DownloadTask, ErrorClass, TaskError};
use crate::recovery::MAX_SERVER_WAIT;

/// HEAD chỉ để dò nhanh: máy chủ nhận kết nối rồi im thì thôi sớm, hỏi bằng GET.
const HEAD_TIMEOUT: Duration = Duration::from_secs(8);

pub(crate) struct Probe {
    pub url: Url,
    pub total_bytes: Option<u64>,
    pub supports_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub file_name: Option<String>,
    pub content_type: Option<String>,
    /// Server gửi `Content-Disposition` đặt tên file / attachment: chắc chắn là file.
    pub attachment: bool,
}

pub(crate) struct RequestFailure {
    pub error: TaskError,
    pub retry_after: Option<Duration>,
}

impl RequestFailure {
    pub fn retryable(&self) -> bool {
        self.error.class == ErrorClass::Transient
    }
}

/// ETag / Last-Modified của file: biết file trên máy chủ đã đổi chưa (AC-04), link mới có cùng file không.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl Validators {
    pub fn of(task: &DownloadTask) -> Self {
        Self {
            etag: task.etag.clone(),
            last_modified: task.last_modified.clone(),
        }
    }

    pub fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            etag: header_text(headers, ETAG),
            last_modified: header_text(headers, LAST_MODIFIED),
        }
    }

    /// `Some(true)`: mọi validator so được (hai bên cùng có) đều giống; `Some(false)`: đều khác; `None`:
    /// không có gì để so, hoặc cái giống cái khác (ETag khác mà Last-Modified giống).
    pub fn agrees(&self, other: &Self) -> Option<bool> {
        let etag = match (&self.etag, &other.etag) {
            (Some(left), Some(right)) => Some(strong(left) == strong(right)),
            _ => None,
        };
        let modified = match (&self.last_modified, &other.last_modified) {
            (Some(left), Some(right)) => Some(left.trim() == right.trim()),
            _ => None,
        };
        match (etag, modified) {
            (None, None) => None,
            (Some(same), None) | (None, Some(same)) => Some(same),
            (Some(etag), Some(modified)) => (etag == modified).then_some(etag),
        }
    }

    /// File (cùng link) đã đổi: mọi validator so được đều khác. Một bên thiếu thì không kết luận; ETag khác
    /// mà Last-Modified giống vẫn là một file (cụm máy chủ Apache gắn inode vào ETag).
    pub fn differs(&self, other: &Self) -> bool {
        self.agrees(other) == Some(false)
    }
}

/// ETag yếu (`W/"…"`) do máy chủ nén gắn thêm: so phần giá trị.
fn strong(etag: &str) -> &str {
    let etag = etag.trim();
    etag.strip_prefix("W/").unwrap_or(etag)
}

pub(crate) struct ContentRange {
    pub start: u64,
    pub end: u64,
    pub total: Option<u64>,
}

pub(crate) async fn probe(
    transport: &Transport,
    url: &Url,
    timeout: Duration,
) -> Result<Probe, RequestFailure> {
    let head = transport
        .send(Method::HEAD, url, None, Some(timeout.min(HEAD_TIMEOUT)))
        .await;
    let mut head_probe = None;
    if let Ok(response) = head
        && response.status().is_success()
    {
        let headers = response.headers();
        let length = content_length(headers);
        let advertised = accepts_ranges(headers);
        let probe = probe_from(&response, advertised, length);
        // HEAD báo 0 byte không đáng tin (trang YouTube trả HTML kèm Content-Length: 0):
        // hỏi lại bằng GET để biết chắc file rỗng.
        if advertised && length.is_some_and(|length| length > 0) {
            return Ok(probe);
        }
        head_probe = Some(probe);
    }

    let response = transport
        .send(Method::GET, url, Some("bytes=0-0"), Some(timeout))
        .await
        .map_err(|error| classify_error(&error))?;
    let status = response.status();
    if status == StatusCode::RANGE_NOT_SATISFIABLE {
        let total = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_content_range_total);
        if total == Some(0) {
            return Ok(empty_file(&response, head_probe));
        }
        return Err(failure_from_status(status, response.headers()));
    }
    if !status.is_success() {
        return Err(failure_from_status(status, response.headers()));
    }
    let partial = status == StatusCode::PARTIAL_CONTENT;
    let headers = response.headers();
    let total = if partial {
        headers
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_content_range_total)
    } else {
        content_length(headers)
    };
    Ok(probe_from(&response, partial || accepts_ranges(headers), total))
}

fn probe_from(response: &Response, supports_ranges: bool, total_bytes: Option<u64>) -> Probe {
    let headers = response.headers();
    Probe {
        url: response.url().clone(),
        total_bytes,
        supports_ranges,
        etag: header_text(headers, ETAG),
        last_modified: header_text(headers, LAST_MODIFIED),
        file_name: file_name_from(headers, response.url()),
        content_type: header_text(headers, CONTENT_TYPE),
        attachment: is_attachment(headers),
    }
}

/// File rỗng thật: server trả 416 `bytes */0` cho Range. Thân 416 của nginx/Apache là trang lỗi
/// HTML ⇒ kiểu nội dung và tên lấy từ HEAD; không có HEAD thì để trống (xem đầu file lúc xong).
fn empty_file(response: &Response, head: Option<Probe>) -> Probe {
    let mut probe = probe_from(response, true, Some(0));
    probe.content_type = head.as_ref().and_then(|head| head.content_type.clone());
    probe.attachment = head.as_ref().is_some_and(|head| head.attachment);
    if let Some(name) = head.and_then(|head| head.file_name) {
        probe.file_name = Some(name);
    }
    probe
}

fn is_attachment(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.trim().to_ascii_lowercase().starts_with("attachment")
                || disposition_file_name(value).is_some()
        })
}

pub(crate) fn failure_from_status(status: StatusCode, headers: &HeaderMap) -> RequestFailure {
    let class = match status.as_u16() {
        401 | 403 | 407 => ErrorClass::User,
        404 | 410 | 451 => ErrorClass::Permanent,
        408 | 425 | 429 => ErrorClass::Transient,
        code if (500..600).contains(&code) => ErrorClass::Transient,
        _ if status.is_client_error() => ErrorClass::Permanent,
        _ => ErrorClass::Permanent,
    };
    let message = match status.as_u16() {
        401 | 403 | 407 => "The server refused access to this file.",
        404 | 410 => "The file was not found on the server.",
        429 => "The server is rate limiting this download.",
        503 => "The server is busy. Try again later.",
        _ => "The server returned an error.",
    };
    RequestFailure {
        error: TaskError {
            class,
            code: format!("http.{}", status.as_u16()),
            message: message.into(),
        },
        retry_after: parse_retry_after(headers),
    }
}

pub(crate) fn classify_error(error: &reqwest::Error) -> RequestFailure {
    let (code, message) = if error.is_timeout() {
        ("network.timeout", "The connection timed out.")
    } else if error.is_connect() {
        ("network.connect", "Could not connect to the server.")
    } else if error.is_body() {
        ("network.body", "The connection dropped while receiving data.")
    } else {
        ("network.request", "The network request failed.")
    };
    RequestFailure {
        error: TaskError {
            class: ErrorClass::Transient,
            code: code.into(),
            message: message.into(),
        },
        retry_after: None,
    }
}

pub(crate) fn parse_content_range(value: &str) -> Option<ContentRange> {
    let rest = value.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let total = match total.trim() {
        "*" => None,
        total => Some(total.parse().ok()?),
    };
    Some(ContentRange {
        start: start.trim().parse().ok()?,
        end: end.trim().parse().ok()?,
        total,
    })
}

fn parse_content_range_total(value: &str) -> Option<u64> {
    let (_, total) = value.trim().split_once('/')?;
    let total = total.trim();
    if total == "*" { None } else { total.parse().ok() }
}

pub(crate) fn host_key(url: &Url) -> String {
    let host = url.host_str().unwrap_or("unknown").to_ascii_lowercase();
    match url.port_or_known_default() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    }
}

pub(crate) fn sanitize_suggested_name(raw: &str) -> String {
    let decoded = percent_decode_str(raw).decode_utf8_lossy();
    let base = decoded.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(character, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            {
                '_'
            } else {
                character
            }
        })
        .collect();
    let mut name = cleaned.trim().trim_end_matches('.').to_string();
    if name.chars().count() > crate::model::MAX_FILE_NAME_CHARS {
        name = name.chars().take(crate::model::MAX_FILE_NAME_CHARS).collect();
    }
    if name.is_empty() || name == "." || name == ".." {
        name = "download.bin".into();
    }
    if validate_file_name(&name).is_ok() {
        name
    } else {
        "download.bin".into()
    }
}

fn file_name_from(headers: &HeaderMap, url: &Url) -> Option<String> {
    if let Some(value) = headers.get(CONTENT_DISPOSITION).and_then(|value| value.to_str().ok())
        && let Some(name) = disposition_file_name(value)
    {
        return Some(name);
    }
    url.path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
}

fn disposition_file_name(value: &str) -> Option<String> {
    let mut plain = None;
    let mut encoded = None;
    for part in value.split(';') {
        let Some((key, raw)) = part.trim().split_once('=') else {
            continue;
        };
        let raw = raw.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "filename" => plain = Some(unquote(raw).to_string()),
            "filename*" => {
                let payload = raw.rsplit_once("''").map(|(_, name)| name).unwrap_or(raw);
                encoded = Some(percent_decode_str(payload).decode_utf8_lossy().into_owned());
            }
            _ => {}
        }
    }
    encoded.or(plain).filter(|name| !name.is_empty())
}

fn unquote(value: &str) -> &str {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn header_text(headers: &HeaderMap, name: reqwest::header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok())
}

fn accepts_ranges(headers: &HeaderMap) -> bool {
    headers
        .get(reqwest::header::ACCEPT_RANGES)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("bytes"))
}

fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds).min(MAX_SERVER_WAIT));
    }
    let date = httpdate::parse_http_date(value).ok()?;
    date.duration_since(SystemTime::now())
        .ok()
        .map(|duration| duration.min(MAX_SERVER_WAIT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_hostile_and_encoded_names() {
        assert_eq!(sanitize_suggested_name(r"..\..\evil.exe"), "evil.exe");
        assert_eq!(sanitize_suggested_name("a%2Fb%2Fc.iso"), "c.iso");
        assert_eq!(sanitize_suggested_name("tải-về.mp4"), "tải-về.mp4");
        assert_eq!(sanitize_suggested_name(""), "download.bin");
        assert_eq!(sanitize_suggested_name("CON"), "download.bin");
    }

    #[test]
    fn validators_need_every_comparable_value_to_differ() {
        let of = |etag: Option<&str>, modified: Option<&str>| Validators {
            etag: etag.map(str::to_string),
            last_modified: modified.map(str::to_string),
        };
        let lm = Some("Tue, 01 Oct 2026 10:00:00 GMT");
        let later = Some("Wed, 02 Oct 2026 10:00:00 GMT");
        assert!(of(Some("\"a\""), None).differs(&of(Some("\"b\""), None)));
        assert!(!of(Some("\"a\""), None).differs(&of(Some("W/\"a\""), None)));
        assert!(of(None, lm).differs(&of(None, later)));
        assert!(!of(Some("\"a\""), lm).differs(&of(Some("\"b\""), lm)), "inode khác, cùng file");
        assert!(of(Some("\"a\""), lm).differs(&of(Some("\"b\""), later)));
        assert!(!of(Some("\"a\""), None).differs(&of(None, lm)), "không so được thì không kết luận");
        assert_eq!(of(None, None).agrees(&of(Some("\"a\""), lm)), None);
        assert_eq!(of(Some("\"a\""), lm).agrees(&of(Some("\"b\""), lm)), None, "cái giống cái khác");
        assert_eq!(of(Some("\"a\""), lm).agrees(&of(Some("W/\"a\""), lm)), Some(true));
    }

    #[test]
    fn long_retry_after_is_kept_for_the_scheduler() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, "3600".parse().unwrap());
        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(3600)));
        headers.insert(RETRY_AFTER, "999999".parse().unwrap());
        assert_eq!(parse_retry_after(&headers), Some(MAX_SERVER_WAIT));
    }

    #[test]
    fn parses_content_ranges() {
        let range = parse_content_range("bytes 0-1023/4096").expect("range");
        assert_eq!((range.start, range.end, range.total), (0, 1023, Some(4096)));
        let open = parse_content_range("bytes 10-20/*").expect("open range");
        assert_eq!(open.total, None);
        assert!(parse_content_range("nonsense").is_none());
    }
}
