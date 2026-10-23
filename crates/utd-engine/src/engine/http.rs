use std::time::{Duration, SystemTime};

use percent_encoding::percent_decode_str;
use reqwest::header::{
    HeaderMap, ACCEPT_ENCODING, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, ETAG,
    LAST_MODIFIED, RANGE, RETRY_AFTER,
};
use reqwest::{Client, Response, StatusCode};
use url::Url;

use crate::model::{validate_file_name, ErrorClass, TaskError};

pub(crate) struct Probe {
    pub url: Url,
    pub total_bytes: Option<u64>,
    pub supports_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub file_name: Option<String>,
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

pub(crate) struct ContentRange {
    pub start: u64,
    pub end: u64,
    pub total: Option<u64>,
}

pub(crate) async fn probe(
    client: &Client,
    url: &Url,
    timeout: Duration,
) -> Result<Probe, RequestFailure> {
    let head = client
        .head(url.clone())
        .header(ACCEPT_ENCODING, "identity")
        .timeout(timeout)
        .send()
        .await;
    if let Ok(response) = head
        && response.status().is_success()
    {
        let headers = response.headers();
        let length = content_length(headers);
        let advertised = accepts_ranges(headers);
        if length.is_some() && (advertised || length == Some(0)) {
            return Ok(probe_from(&response, advertised, length));
        }
    }

    let response = client
        .get(url.clone())
        .header(ACCEPT_ENCODING, "identity")
        .header(RANGE, "bytes=0-0")
        .timeout(timeout)
        .send()
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
            return Ok(probe_from(&response, true, total));
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
    }
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
        return Some(Duration::from_secs(seconds.min(300)));
    }
    let date = httpdate::parse_http_date(value).ok()?;
    date.duration_since(SystemTime::now())
        .ok()
        .map(|duration| duration.min(Duration::from_secs(300)))
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
    fn parses_content_ranges() {
        let range = parse_content_range("bytes 0-1023/4096").expect("range");
        assert_eq!((range.start, range.end, range.total), (0, 1023, Some(4096)));
        let open = parse_content_range("bytes 10-20/*").expect("open range");
        assert_eq!(open.total, None);
        assert!(parse_content_range("nonsense").is_none());
    }
}
