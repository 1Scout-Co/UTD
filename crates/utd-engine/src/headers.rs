//! Header riêng của một task (ví dụ `http_headers` yt-dlp đưa: User-Agent, Accept-Language…).
//!
//! Chỉ nhận header "vô hại": không CR/LF, không header điều khiển kết nối/khung HTTP, không
//! cookie/đăng nhập. Giá trị không bao giờ được in ra (Debug che giá trị).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const MAX_REQUEST_HEADERS: usize = 32;
const MAX_NAME_LEN: usize = 64;
const MAX_VALUE_LEN: usize = 4096;

/// Engine tự quản (Range, Accept-Encoding: identity…), header khung HTTP, hoặc bí mật.
const FORBIDDEN: &[&str] = &[
    "host",
    "content-length",
    "range",
    "if-range",
    "connection",
    "keep-alive",
    "transfer-encoding",
    "te",
    "trailer",
    "upgrade",
    "expect",
    "accept-encoding",
    "cookie",
    "cookie2",
    "authorization",
];

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestHeader {
    pub name: String,
    pub value: String,
}

impl RequestHeader {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Result<Self> {
        let header = Self {
            name: name.into(),
            value: value.into(),
        };
        header.validate()?;
        Ok(header)
    }

    pub fn validate(&self) -> Result<()> {
        let name = self.name.as_str();
        let token =
            !name.is_empty() && name.len() <= MAX_NAME_LEN && name.bytes().all(is_token_byte);
        if !token {
            return Err(Error::InvalidInput(
                "request header name must be a 1-64 character HTTP token".into(),
            ));
        }
        if is_forbidden(name) {
            return Err(Error::InvalidInput(format!(
                "request header {name} is managed by the engine or carries credentials"
            )));
        }
        let value_ok = self.value.len() <= MAX_VALUE_LEN
            && self.value.trim() == self.value
            && self
                .value
                .bytes()
                .all(|byte| byte == b'\t' || (b' '..=b'~').contains(&byte));
        if !value_ok {
            // Không đưa giá trị vào thông báo lỗi.
            return Err(Error::InvalidInput(format!(
                "request header {name} has an invalid value"
            )));
        }
        Ok(())
    }
}

impl fmt::Debug for RequestHeader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestHeader")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .finish()
    }
}

/// Kiểm cả danh sách: mỗi header hợp lệ, không trùng tên, không quá `MAX_REQUEST_HEADERS`.
pub fn validate_request_headers(headers: &[RequestHeader]) -> Result<()> {
    if headers.len() > MAX_REQUEST_HEADERS {
        return Err(Error::InvalidInput(format!(
            "at most {MAX_REQUEST_HEADERS} request headers are allowed"
        )));
    }
    for (position, header) in headers.iter().enumerate() {
        header.validate()?;
        if headers[..position]
            .iter()
            .any(|earlier| earlier.name.eq_ignore_ascii_case(&header.name))
        {
            return Err(Error::InvalidInput(format!(
                "request header {} is repeated",
                header.name
            )));
        }
    }
    Ok(())
}

/// Lọc header từ nguồn ngoài (vd. `http_headers` của yt-dlp): bỏ header cấm/không hợp lệ/trùng,
/// giữ thứ tự, cắt ở `MAX_REQUEST_HEADERS`. Không bao giờ lỗi.
pub fn allowed_request_headers<I, K, V>(pairs: I) -> Vec<RequestHeader>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    let mut kept: Vec<RequestHeader> = Vec::new();
    for (name, value) in pairs {
        if kept.len() >= MAX_REQUEST_HEADERS {
            break;
        }
        let header = RequestHeader {
            name: name.into().trim().to_string(),
            value: value.into().trim().to_string(),
        };
        let repeated = kept
            .iter()
            .any(|earlier| earlier.name.eq_ignore_ascii_case(&header.name));
        if !repeated && header.validate().is_ok() {
            kept.push(header);
        }
    }
    kept
}

fn is_forbidden(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("proxy-") || FORBIDDEN.contains(&lower.as_str())
}

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_browser_like_headers() {
        for (name, value) in [
            ("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)"),
            ("Accept", "text/html,application/xhtml+xml;q=0.9,*/*;q=0.8"),
            ("Accept-Language", "en-us,en;q=0.5"),
            ("Sec-Fetch-Mode", "navigate"),
            ("Referer", "https://www.youtube.com/"),
            ("X-Empty", ""),
        ] {
            RequestHeader::new(name, value).unwrap_or_else(|_| panic!("rejected {name}"));
        }
    }

    #[test]
    fn rejects_engine_framing_and_credential_headers() {
        for name in [
            "Host",
            "content-length",
            "Range",
            "Connection",
            "Transfer-Encoding",
            "Cookie",
            "Authorization",
            "Proxy-Authorization",
            "proxy-connection",
            "Accept-Encoding",
            "If-Range",
        ] {
            assert!(RequestHeader::new(name, "x").is_err(), "accepted {name}");
        }
    }

    #[test]
    fn rejects_injection_and_bad_tokens() {
        assert!(RequestHeader::new("User-Agent", "a\r\nHost: evil").is_err());
        assert!(RequestHeader::new("User-Agent", "a\nb").is_err());
        assert!(RequestHeader::new("User-Agent", "nul\0").is_err());
        assert!(RequestHeader::new("User-Agent", " padded").is_err());
        assert!(RequestHeader::new("Bad Name", "x").is_err());
        assert!(RequestHeader::new("Bad:Name", "x").is_err());
        assert!(RequestHeader::new("", "x").is_err());
        assert!(RequestHeader::new("X-Long", "a".repeat(MAX_VALUE_LEN + 1)).is_err());
    }

    #[test]
    fn error_messages_and_debug_never_show_values() {
        let secret = "s3cr3t-value";
        let error = RequestHeader::new("X-Token", format!("{secret}\r\n")).unwrap_err();
        assert!(!error.to_string().contains(secret));
        let header = RequestHeader::new("X-Token", secret).expect("valid");
        assert!(!format!("{header:?}").contains(secret));
    }

    #[test]
    fn list_validation_rejects_duplicates_and_overflow() {
        let headers = vec![
            RequestHeader::new("Accept", "a").unwrap(),
            RequestHeader::new("accept", "b").unwrap(),
        ];
        assert!(validate_request_headers(&headers).is_err());
        let many: Vec<RequestHeader> = (0..=MAX_REQUEST_HEADERS)
            .map(|index| RequestHeader::new(format!("X-{index}"), "v").unwrap())
            .collect();
        assert!(validate_request_headers(&many).is_err());
    }

    #[test]
    fn filters_untrusted_header_maps() {
        let kept = allowed_request_headers([
            ("User-Agent", "UA"),
            ("Cookie", "SID=1"),
            ("Accept", " text/html "),
            ("user-agent", "second"),
            ("Bad\nName", "x"),
            ("Sec-Fetch-Mode", "navigate"),
        ]);
        let names: Vec<&str> = kept.iter().map(|header| header.name.as_str()).collect();
        assert_eq!(names, ["User-Agent", "Accept", "Sec-Fetch-Mode"]);
        assert_eq!(kept[0].value, "UA");
        assert_eq!(kept[1].value, "text/html");
        validate_request_headers(&kept).expect("filtered list is valid");
    }
}
