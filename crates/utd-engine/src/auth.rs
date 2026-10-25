//! Cookie đăng nhập của trình duyệt cho các lượt tải (docs/specs/2026-10-23-utd-v1/notes/auth-plan.md §5.3).
//!
//! Chỉ ở RAM: không `Serialize`, `Debug` không in giá trị, giá trị xoá khỏi bộ nhớ khi bỏ (`Zeroizing`).
//! Đi theo `DownloadControl` của từng lượt chạy, journal chỉ ghi `DownloadTask::auth_sites` (tên miền). Gửi
//! theo luật RFC 6265 ở từng bước chuyển hướng: đúng miền, đúng đường dẫn, `Secure` chỉ qua https, bỏ cookie
//! hết hạn. Lọc SameSite làm ở tiện ích lúc đọc cookie.

use std::fmt;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::HeaderValue;
use url::{Host, Url};
use zeroize::Zeroizing;

use crate::model::{ErrorClass, TaskError};

/// Một lần cho phép nhận tối đa chừng này cookie (tiện ích cũng cắt ở đây).
pub const MAX_COOKIES: usize = 300;
const MAX_NAME_LEN: usize = 256;
const MAX_VALUE_LEN: usize = 4096;
const MAX_PATH_LEN: usize = 1024;
const MAX_DOMAIN_LEN: usize = 253;

/// Task cần đăng nhập mà lượt này không có cookie (vd sau khi mở lại app).
pub const AUTH_REQUIRED: &str = "auth.required";
/// Có kèm cookie mà máy chủ vẫn từ chối hoặc đưa về trang đăng nhập.
pub const AUTH_EXPIRED: &str = "auth.expired";

pub fn auth_required() -> TaskError {
    TaskError {
        class: ErrorClass::User,
        code: AUTH_REQUIRED.into(),
        message: "This page requires a sign-in.".into(),
    }
}

pub fn auth_expired() -> TaskError {
    TaskError {
        class: ErrorClass::User,
        code: AUTH_EXPIRED.into(),
        message: "The sign-in has expired.".into(),
    }
}

/// Một cookie như trình duyệt đưa (`chrome.cookies.Cookie`).
pub struct BrowserCookie {
    pub name: String,
    pub value: Zeroizing<String>,
    /// `.google.com` hay `google.com` đều được; viết thường, bỏ dấu chấm đầu khi nhận.
    pub domain: String,
    /// Cookie chỉ của đúng host (không có thuộc tính `Domain=`).
    pub host_only: bool,
    pub path: String,
    pub secure: bool,
    /// Giây Unix; `None` = cookie phiên.
    pub expires: Option<i64>,
}

impl fmt::Debug for BrowserCookie {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BrowserCookie")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .field("domain", &self.domain)
            .finish()
    }
}

impl BrowserCookie {
    /// Chuẩn hoá miền/đường dẫn; `None` nếu cookie không gửi được an toàn trong header (ký tự lạ, quá dài).
    fn normalized(mut self) -> Option<Self> {
        self.domain = self
            .domain
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase();
        if self.path.is_empty() {
            self.path = "/".into();
        }
        let name_ok = !self.name.is_empty()
            && self.name.len() <= MAX_NAME_LEN
            && self
                .name
                .bytes()
                .all(|byte| (b'!'..=b'~').contains(&byte) && !matches!(byte, b';' | b',' | b'='));
        let value_ok = self.value.len() <= MAX_VALUE_LEN && self.value.bytes().all(is_cookie_byte);
        let domain_ok = !self.domain.is_empty()
            && self.domain.len() <= MAX_DOMAIN_LEN
            && self.domain.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'.' | b'-' | b'_' | b':' | b'[' | b']')
            });
        let path_ok = self.path.starts_with('/')
            && self.path.len() <= MAX_PATH_LEN
            && self.path.bytes().all(is_cookie_byte);
        (name_ok && value_ok && domain_ok && path_ok).then_some(self)
    }

    fn sends_to(&self, url: &Url, now: i64) -> bool {
        let Some(host) = host_of(url) else {
            return false;
        };
        let domain_ok = if self.host_only || is_ip(&host) {
            host == self.domain
        } else {
            host == self.domain || host.ends_with(&format!(".{}", self.domain))
        };
        domain_ok
            && path_matches(&self.path, request_path(url))
            && (!self.secure || url.scheme() == "https")
            && self.expires.is_none_or(|expires| expires > now)
    }

    fn same_slot(&self, other: &Self) -> bool {
        self.name == other.name && self.domain == other.domain && self.path == other.path
    }
}

/// Hũ cookie của một lần cho phép. Dùng chung (`Arc`) cho mọi task của cùng trang; gộp thêm cookie máy chủ
/// đặt trong lúc tải (vd cookie xác nhận tải của Drive), như trình duyệt.
pub struct CookieJar {
    cookies: Mutex<Vec<BrowserCookie>>,
}

impl fmt::Debug for CookieJar {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "CookieJar({} cookies)", self.len())
    }
}

impl CookieJar {
    /// Bỏ cookie không hợp lệ và phần vượt `MAX_COOKIES`, không bao giờ lỗi.
    pub fn new(cookies: impl IntoIterator<Item = BrowserCookie>) -> Self {
        let kept = cookies
            .into_iter()
            .filter_map(BrowserCookie::normalized)
            .take(MAX_COOKIES)
            .collect();
        Self {
            cookies: Mutex::new(kept),
        }
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Header `Cookie` cho một yêu cầu tới `url` (`None` nếu không cookie nào hợp). Giá trị đánh dấu nhạy
    /// cảm: không hiện trong `Debug` của `http`, HTTP/2 không đưa vào bảng nén.
    pub fn header_for(&self, url: &Url) -> Option<HeaderValue> {
        let now = unix_now();
        let cookies = self.lock();
        let mut matching: Vec<&BrowserCookie> = cookies
            .iter()
            .filter(|cookie| cookie.sends_to(url, now))
            .collect();
        if matching.is_empty() {
            return None;
        }
        // Đường dẫn dài trước (RFC 6265 §5.4); cùng độ dài giữ thứ tự nhận.
        matching.sort_by_key(|cookie| std::cmp::Reverse(cookie.path.len()));
        let mut line = Zeroizing::new(String::new());
        for cookie in matching {
            if !line.is_empty() {
                line.push_str("; ");
            }
            line.push_str(&cookie.name);
            line.push('=');
            line.push_str(&cookie.value);
        }
        let mut value = HeaderValue::from_str(&line).ok()?;
        value.set_sensitive(true);
        Some(value)
    }

    /// Gộp `Set-Cookie` máy chủ trả cho yêu cầu tới `url` (RFC 6265 §5.2–5.3, phần cần cho tải file).
    pub fn absorb<'a>(&self, url: &Url, set_cookies: impl IntoIterator<Item = &'a HeaderValue>) {
        let Some(host) = host_of(url) else {
            return;
        };
        let now = unix_now();
        for header in set_cookies {
            let Ok(text) = header.to_str() else {
                continue;
            };
            let Some(cookie) = parse_set_cookie(text, &host, url, now) else {
                continue;
            };
            let mut cookies = self.lock();
            cookies.retain(|old| !old.same_slot(&cookie));
            let expired = cookie.expires.is_some_and(|expires| expires <= now);
            if !expired && cookies.len() < MAX_COOKIES {
                cookies.push(cookie);
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<BrowserCookie>> {
        self.cookies
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn parse_set_cookie(text: &str, host: &str, url: &Url, now: i64) -> Option<BrowserCookie> {
    let mut parts = text.split(';');
    let (name, value) = parts.next()?.split_once('=')?;
    let mut cookie = BrowserCookie {
        name: name.trim().to_string(),
        value: Zeroizing::new(value.trim().to_string()),
        domain: host.to_string(),
        host_only: true,
        path: default_path(url),
        secure: false,
        expires: None,
    };
    let mut max_age = None;
    for attribute in parts {
        let (key, raw) = attribute.split_once('=').unwrap_or((attribute, ""));
        let raw = raw.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "domain" if !raw.is_empty() => {
                let domain = raw.trim_start_matches('.').to_ascii_lowercase();
                // Chỉ nhận miền cha của host (không phải miền một nhãn như `com`), như trình duyệt.
                let parent = host == domain
                    || (!is_ip(host)
                        && domain.contains('.')
                        && host.ends_with(&format!(".{domain}")));
                if !parent {
                    return None;
                }
                cookie.domain = domain;
                cookie.host_only = false;
            }
            "path" if raw.starts_with('/') => cookie.path = raw.to_string(),
            "secure" => cookie.secure = true,
            "max-age" => max_age = raw.parse::<i64>().ok(),
            "expires" => {
                cookie.expires = httpdate::parse_http_date(raw)
                    .ok()
                    .and_then(|date| date.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX));
            }
            _ => {}
        }
    }
    if let Some(seconds) = max_age {
        cookie.expires = Some(if seconds <= 0 {
            i64::MIN
        } else {
            now.saturating_add(seconds)
        });
    }
    // Máy chủ http không được đặt cookie `Secure`.
    if cookie.secure && url.scheme() != "https" {
        return None;
    }
    cookie.normalized()
}

/// Chuỗi chuyển hướng dừng ở trang đăng nhập mà link gốc không phải trang đó: máy chủ đòi đăng nhập
/// thay vì trả file (Google, Microsoft, Dropbox, Facebook… và đường dẫn `/login`, `/signin`…).
pub fn is_login_redirect(original: &Url, landed: &Url) -> bool {
    looks_like_login(landed) && !looks_like_login(original)
}

fn looks_like_login(url: &Url) -> bool {
    const HOSTS: &[&str] = &[
        "accounts.google.com",
        "login.microsoftonline.com",
        "login.live.com",
        "login.microsoft.com",
        "appleid.apple.com",
        "login.yahoo.com",
    ];
    const SEGMENTS: &[&str] = &[
        "login",
        "log-in",
        "logon",
        "signin",
        "sign-in",
        "sign_in",
        "servicelogin",
        "sso",
    ];
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    HOSTS.contains(&host.as_str())
        || url.path_segments().is_some_and(|mut segments| {
            segments.any(|segment| SEGMENTS.contains(&segment.to_ascii_lowercase().as_str()))
        })
}

/// Giá trị/đường dẫn nới như trình duyệt (rộng hơn cookie-octet của RFC 6265): ký tự ASCII thấy được và
/// dấu cách, trừ `;` (dấu ngăn cookie trong header).
fn is_cookie_byte(byte: u8) -> bool {
    (b' '..=b'~').contains(&byte) && byte != b';'
}

fn host_of(url: &Url) -> Option<String> {
    match url.host()? {
        Host::Domain(domain) => Some(domain.to_ascii_lowercase()),
        Host::Ipv4(address) => Some(address.to_string()),
        Host::Ipv6(address) => Some(format!("[{address}]")),
    }
}

fn is_ip(host: &str) -> bool {
    host.starts_with('[') || host.parse::<std::net::Ipv4Addr>().is_ok()
}

fn request_path(url: &Url) -> &str {
    match url.path() {
        "" => "/",
        path => path,
    }
}

/// RFC 6265 §5.1.4.
fn path_matches(cookie_path: &str, request_path: &str) -> bool {
    request_path == cookie_path
        || (request_path.starts_with(cookie_path)
            && (cookie_path.ends_with('/') || request_path[cookie_path.len()..].starts_with('/')))
}

/// RFC 6265 §5.1.4: thư mục của đường dẫn yêu cầu.
fn default_path(url: &Url) -> String {
    let path = request_path(url);
    match path.rfind('/') {
        Some(0) | None => "/".into(),
        Some(end) => path[..end].to_string(),
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(
        name: &str,
        domain: &str,
        host_only: bool,
        path: &str,
        secure: bool,
    ) -> BrowserCookie {
        BrowserCookie {
            name: name.into(),
            value: Zeroizing::new(format!("v-{name}")),
            domain: domain.into(),
            host_only,
            path: path.into(),
            secure,
            expires: None,
        }
    }

    fn header(jar: &CookieJar, url: &str) -> Option<String> {
        jar.header_for(&Url::parse(url).unwrap())
            .map(|value| value.to_str().unwrap().to_string())
    }

    #[test]
    fn sends_domain_cookies_to_subdomains_and_host_only_cookies_to_the_host_only() {
        let jar = CookieJar::new([
            cookie("SID", ".google.com", false, "/", true),
            cookie("drive", "drive.google.com", true, "/", true),
        ]);
        assert_eq!(
            header(&jar, "https://drive.google.com/file/d/1/view").as_deref(),
            Some("SID=v-SID; drive=v-drive")
        );
        assert_eq!(
            header(&jar, "https://drive.usercontent.google.com/download?id=1").as_deref(),
            Some("SID=v-SID")
        );
        assert_eq!(
            header(&jar, "https://google.com/").as_deref(),
            Some("SID=v-SID")
        );
        assert_eq!(header(&jar, "https://evilgoogle.com/").as_deref(), None);
        assert_eq!(
            header(&jar, "https://google.com.evil.example/").as_deref(),
            None
        );
        assert_eq!(
            header(&jar, "https://cdn.example.net/file.bin").as_deref(),
            None
        );
    }

    #[test]
    fn respects_secure_path_and_expiry() {
        let mut old = cookie("old", "example.com", true, "/", false);
        old.expires = Some(1);
        let mut later = cookie("later", "example.com", true, "/", false);
        later.expires = Some(unix_now() + 3600);
        let jar = CookieJar::new([
            cookie("secure", "example.com", true, "/", true),
            cookie("docs", "example.com", true, "/docs", false),
            cookie("root", "example.com", true, "/", false),
            old,
            later,
        ]);
        // http: không gửi cookie Secure; đường dẫn dài xếp trước.
        assert_eq!(
            header(&jar, "http://example.com/docs/a.pdf").as_deref(),
            Some("docs=v-docs; root=v-root; later=v-later")
        );
        assert_eq!(
            header(&jar, "https://example.com/docsx").as_deref(),
            Some("secure=v-secure; root=v-root; later=v-later")
        );
    }

    #[test]
    fn drops_cookies_that_cannot_travel_safely_in_a_header() {
        let mut injected = cookie("ok", "example.com", true, "/", false);
        injected.value = Zeroizing::new("a\r\nX-Evil: 1".into());
        let mut semicolon = cookie("semi", "example.com", true, "/", false);
        semicolon.value = Zeroizing::new("a; b=c".into());
        let jar = CookieJar::new([
            injected,
            semicolon,
            cookie("bad name", "example.com", true, "/", false),
            cookie("", "example.com", true, "/", false),
            cookie("nopath", "example.com", true, "relative", false),
            cookie("good", ".Example.COM", false, "", false),
        ]);
        assert_eq!(jar.len(), 1);
        assert_eq!(
            header(&jar, "https://a.example.com/").as_deref(),
            Some("good=v-good")
        );
        let many = (0..MAX_COOKIES + 5)
            .map(|index| cookie(&format!("c{index}"), "x.org", true, "/", false));
        assert_eq!(CookieJar::new(many).len(), MAX_COOKIES);
    }

    #[test]
    fn debug_never_shows_values() {
        let jar = CookieJar::new([cookie("SID", "google.com", false, "/", false)]);
        assert_eq!(format!("{jar:?}"), "CookieJar(1 cookies)");
        let single = cookie("SID", "google.com", false, "/", false);
        assert!(!format!("{single:?}").contains("v-SID"));
        let value = jar
            .header_for(&Url::parse("https://google.com/").unwrap())
            .unwrap();
        assert!(value.is_sensitive());
        assert!(!format!("{value:?}").contains("v-SID"));
    }

    #[test]
    fn absorbs_set_cookie_like_a_browser() {
        let jar = CookieJar::new([cookie("SID", "google.com", false, "/", true)]);
        let url = Url::parse("https://drive.usercontent.google.com/download/file?id=1").unwrap();
        let headers = [
            HeaderValue::from_static("confirm=t; Path=/; Secure; HttpOnly"),
            HeaderValue::from_static("scoped=1"),
            HeaderValue::from_static("parent=1; Domain=.google.com; Path=/"),
            HeaderValue::from_static("tld=1; Domain=com"),
            HeaderValue::from_static("other=1; Domain=example.com"),
            HeaderValue::from_static("SID=; Domain=google.com; Path=/; Max-Age=0"),
        ];
        jar.absorb(&url, headers.iter());
        assert_eq!(
            header(&jar, "https://drive.usercontent.google.com/download/file").as_deref(),
            Some("scoped=1; confirm=t; parent=1")
        );
        assert_eq!(
            header(&jar, "https://www.google.com/").as_deref(),
            Some("parent=1")
        );
        // Máy chủ http không đặt được cookie Secure.
        let plain = Url::parse("http://example.com/a").unwrap();
        jar.absorb(&plain, [HeaderValue::from_static("s=1; Secure")].iter());
        assert_eq!(header(&jar, "https://example.com/a"), None);
    }

    #[test]
    fn recognises_login_redirects() {
        let url = |text: &str| Url::parse(text).unwrap();
        let file = url("https://drive.usercontent.google.com/download?id=1");
        assert!(is_login_redirect(
            &file,
            &url("https://accounts.google.com/v3/signin/identifier?continue=x")
        ));
        assert!(is_login_redirect(
            &url("https://intranet.local/docs/a.pdf"),
            &url("https://intranet.local/login?next=/docs/a.pdf")
        ));
        assert!(is_login_redirect(
            &url("https://www.dropbox.com/s/x/a.zip"),
            &url("https://www.dropbox.com/login?cont=x")
        ));
        assert!(!is_login_redirect(
            &file,
            &url("https://cdn.example.com/files/a.zip")
        ));
        assert!(!is_login_redirect(
            &url("https://example.com/login/guide.pdf"),
            &url("https://example.com/login/guide.pdf")
        ));
        assert!(!is_login_redirect(
            &file,
            &url("https://example.com/blogin/a")
        ));
    }
}
