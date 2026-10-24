//! Tải có kiểm soát cho công cụ phụ trợ (yt-dlp, ffmpeg…): chỉ đi tới host trong danh sách
//! cho phép (kể cả từng bước chuyển hướng), giới hạn dung lượng, và chỉ đặt file vào chỗ
//! sau khi SHA-256 khớp.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::ACCEPT_ENCODING;
use reqwest::{Client, Response};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
pub use url::Url;

use crate::engine::USER_AGENT;

/// Luật mạng cho một lần tải: host được phép, có bắt buộc HTTPS không, tối đa bao nhiêu
/// bước chuyển hướng.
#[derive(Debug, Clone)]
pub struct FetchPolicy {
    /// Tên host khớp chính xác (không phân biệt hoa thường), không kèm cổng.
    pub allowed_hosts: Vec<String>,
    pub require_https: bool,
    pub max_redirects: usize,
    pub connect_timeout: Duration,
    /// Thời gian chờ tối đa giữa hai lần nhận dữ liệu.
    pub idle_timeout: Duration,
}

impl FetchPolicy {
    /// HTTPS bắt buộc, chỉ các host đã nêu.
    pub fn https_only(hosts: &[&str]) -> Self {
        Self {
            allowed_hosts: hosts.iter().map(|host| host.to_ascii_lowercase()).collect(),
            require_https: true,
            max_redirects: 5,
            connect_timeout: Duration::from_secs(15),
            idle_timeout: Duration::from_secs(30),
        }
    }

    /// Kiểm một URL (ban đầu hoặc một bước chuyển hướng).
    pub fn check(&self, url: &Url) -> Result<(), FetchError> {
        let scheme_ok = match url.scheme() {
            "https" => true,
            "http" => !self.require_https,
            _ => false,
        };
        let host = url.host_str().unwrap_or("").to_ascii_lowercase();
        if !scheme_ok {
            return Err(FetchError::InsecureUrl(host));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(FetchError::InvalidUrl);
        }
        if host.is_empty()
            || !self
                .allowed_hosts
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(&host))
        {
            return Err(FetchError::HostNotAllowed(host));
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("invalid URL")]
    InvalidUrl,
    #[error("insecure URL for host {0}")]
    InsecureUrl(String),
    #[error("host not allowed: {0}")]
    HostNotAllowed(String),
    #[error("too many redirects")]
    TooManyRedirects,
    #[error("HTTP status {0}")]
    Status(u16),
    #[error("response is larger than {0} bytes")]
    TooLarge(u64),
    #[error("response ended early ({received} of {expected} bytes)")]
    Truncated { received: u64, expected: u64 },
    #[error("response is not UTF-8 text")]
    NotText,
    #[error("invalid expected SHA-256")]
    InvalidHash,
    #[error("SHA-256 mismatch (expected {expected}, got {actual})")]
    HashMismatch { expected: String, actual: String },
    #[error("timed out")]
    Timeout,
    #[error("cancelled")]
    Cancelled,
    #[error("network: {0}")]
    Network(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Nội dung chữ cùng các URL đã đi qua khi chuyển hướng (không gồm URL ban đầu).
#[derive(Debug, Clone)]
pub struct FetchedText {
    pub text: String,
    pub redirects: Vec<Url>,
}

/// File đã tải xong và đã khớp SHA-256.
#[derive(Debug, Clone)]
pub struct VerifiedFile {
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}

/// Lỗi gắn vào bước chuyển hướng bị chặn, để đọc lại từ chuỗi `source()` của reqwest.
#[derive(Debug, thiserror::Error)]
#[error("redirect blocked")]
struct BlockedRedirect(Box<FetchError>);

struct Session {
    client: Client,
    redirects: Arc<Mutex<Vec<Url>>>,
}

fn session(policy: &FetchPolicy) -> Result<Session, FetchError> {
    let redirects = Arc::new(Mutex::new(Vec::new()));
    let seen = redirects.clone();
    let rules = policy.clone();
    let redirect = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() > rules.max_redirects {
            return attempt.error(BlockedRedirect(Box::new(FetchError::TooManyRedirects)));
        }
        if let Err(error) = rules.check(attempt.url()) {
            return attempt.error(BlockedRedirect(Box::new(error)));
        }
        if let Ok(mut list) = seen.lock() {
            list.push(attempt.url().clone());
        }
        attempt.follow()
    });
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(policy.connect_timeout)
        .redirect(redirect)
        .build()
        .map_err(|error| FetchError::Network(error.without_url().to_string()))?;
    Ok(Session { client, redirects })
}

fn classify(error: reqwest::Error) -> FetchError {
    let mut source = std::error::Error::source(&error);
    while let Some(inner) = source {
        if let Some(blocked) = inner.downcast_ref::<BlockedRedirect>() {
            return match blocked.0.as_ref() {
                FetchError::HostNotAllowed(host) => FetchError::HostNotAllowed(host.clone()),
                FetchError::InsecureUrl(host) => FetchError::InsecureUrl(host.clone()),
                FetchError::TooManyRedirects => FetchError::TooManyRedirects,
                _ => FetchError::InvalidUrl,
            };
        }
        source = inner.source();
    }
    if error.is_timeout() {
        return FetchError::Timeout;
    }
    // Bỏ URL khỏi thông báo: URL CDN đã ký có token tạm thời.
    FetchError::Network(error.without_url().to_string())
}

async fn open(
    session: &Session,
    url: &Url,
    policy: &FetchPolicy,
    max_bytes: u64,
) -> Result<Response, FetchError> {
    let request = session
        .client
        .get(url.clone())
        .header(ACCEPT_ENCODING, "identity")
        .send();
    let response = tokio::time::timeout(policy.connect_timeout + policy.idle_timeout, request)
        .await
        .map_err(|_| FetchError::Timeout)?
        .map_err(classify)?;
    policy.check(response.url())?;
    let status = response.status();
    if status.as_u16() != 200 {
        return Err(FetchError::Status(status.as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes)
    {
        return Err(FetchError::TooLarge(max_bytes));
    }
    Ok(response)
}

async fn next_chunk(
    response: &mut Response,
    policy: &FetchPolicy,
    cancel: Option<&CancellationToken>,
) -> Result<Option<Vec<u8>>, FetchError> {
    let read = tokio::time::timeout(policy.idle_timeout, response.chunk());
    let result = match cancel {
        Some(cancel) => tokio::select! {
            _ = cancel.cancelled() => return Err(FetchError::Cancelled),
            result = read => result,
        },
        None => read.await,
    };
    let chunk = result.map_err(|_| FetchError::Timeout)?.map_err(classify)?;
    Ok(chunk.map(|bytes| bytes.to_vec()))
}

fn parse_url(url: &str, policy: &FetchPolicy) -> Result<Url, FetchError> {
    let url = Url::parse(url).map_err(|_| FetchError::InvalidUrl)?;
    policy.check(&url)?;
    Ok(url)
}

/// Tải một file chữ nhỏ (vd danh sách checksum). Vượt `max_bytes` là lỗi.
pub async fn fetch_text(
    url: &str,
    max_bytes: u64,
    policy: &FetchPolicy,
) -> Result<FetchedText, FetchError> {
    let url = parse_url(url, policy)?;
    let session = session(policy)?;
    let mut response = open(&session, &url, policy, max_bytes).await?;
    let mut body = Vec::new();
    while let Some(chunk) = next_chunk(&mut response, policy, None).await? {
        if body.len() as u64 + chunk.len() as u64 > max_bytes {
            return Err(FetchError::TooLarge(max_bytes));
        }
        body.extend_from_slice(&chunk);
    }
    let text = String::from_utf8(body).map_err(|_| FetchError::NotText)?;
    let redirects = session
        .redirects
        .lock()
        .map(|list| list.clone())
        .unwrap_or_default();
    Ok(FetchedText { text, redirects })
}

/// Link "mới nhất" chuyển hướng tới file của một bản cụ thể: trả URL đích (đã qua luật host),
/// không tải nội dung. Dùng khi file checksum nằm cạnh file thật (`<đích>.sha256`).
pub async fn resolve_redirect(url: &str, policy: &FetchPolicy) -> Result<Url, FetchError> {
    let mut current = parse_url(url, policy)?;
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(policy.connect_timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| FetchError::Network(error.without_url().to_string()))?;
    for _ in 0..policy.max_redirects {
        let request = client
            .get(current.clone())
            .header(ACCEPT_ENCODING, "identity")
            .send();
        let response = tokio::time::timeout(policy.connect_timeout + policy.idle_timeout, request)
            .await
            .map_err(|_| FetchError::Timeout)?
            .map_err(classify)?;
        let status = response.status();
        if !status.is_redirection() {
            return if status.as_u16() == 200 {
                Ok(current)
            } else {
                Err(FetchError::Status(status.as_u16()))
            };
        }
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(FetchError::InvalidUrl)?;
        let next = current.join(location).map_err(|_| FetchError::InvalidUrl)?;
        policy.check(&next)?;
        current = next;
    }
    Err(FetchError::TooManyRedirects)
}

fn normalize_hash(expected: &str) -> Result<String, FetchError> {
    let expected = expected.trim().to_ascii_lowercase();
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(FetchError::InvalidHash);
    }
    Ok(expected)
}

fn part_path(dest: &Path) -> Result<PathBuf, FetchError> {
    let name = dest.file_name().ok_or(FetchError::InvalidUrl)?;
    let mut part = name.to_os_string();
    part.push(".part");
    Ok(dest.with_file_name(part))
}

/// Xoá file `.part` khi lần tải hỏng, bị huỷ hoặc future bị bỏ giữa chừng.
struct PartGuard {
    path: PathBuf,
    armed: bool,
}

impl Drop for PartGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn hex(digest: &[u8]) -> String {
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut text, "{byte:02x}").expect("writing to a string cannot fail");
    }
    text
}

/// Tải `url` vào `dest`: ghi ra `<dest>.part` cạnh đích, băm trong lúc ghi, và chỉ đổi tên
/// thành `dest` khi SHA-256 khớp `expected_sha256`. Lỗi bất kỳ ⇒ không còn file nào.
/// `on_progress(đã_tải, tổng)` được gọi sau mỗi khối dữ liệu.
pub async fn download_verified<F>(
    url: &str,
    expected_sha256: &str,
    dest: &Path,
    max_bytes: u64,
    policy: &FetchPolicy,
    cancel: &CancellationToken,
    mut on_progress: F,
) -> Result<VerifiedFile, FetchError>
where
    F: FnMut(u64, Option<u64>),
{
    let expected = normalize_hash(expected_sha256)?;
    let url = parse_url(url, policy)?;
    let part = part_path(dest)?;
    let mut guard = PartGuard {
        path: part.clone(),
        armed: true,
    };
    let session = session(policy)?;
    let mut response = tokio::select! {
        _ = cancel.cancelled() => return Err(FetchError::Cancelled),
        response = open(&session, &url, policy, max_bytes) => response?,
    };
    let total = response.content_length();
    let mut file = tokio::fs::File::create(&part).await?;
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    on_progress(0, total);
    while let Some(chunk) = next_chunk(&mut response, policy, Some(cancel)).await? {
        received += chunk.len() as u64;
        if received > max_bytes || total.is_some_and(|total| received > total) {
            return Err(FetchError::TooLarge(max_bytes));
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        on_progress(received, total);
    }
    file.flush().await?;
    file.sync_all().await?;
    drop(file);
    if let Some(expected_total) = total
        && received != expected_total
    {
        return Err(FetchError::Truncated {
            received,
            expected: expected_total,
        });
    }
    let actual = hex(&hasher.finalize());
    if actual != expected {
        return Err(FetchError::HashMismatch { expected, actual });
    }
    tokio::fs::rename(&part, dest).await?;
    guard.armed = false;
    Ok(VerifiedFile {
        path: dest.to_path_buf(),
        bytes: received,
        sha256: actual,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> FetchPolicy {
        FetchPolicy::https_only(&["github.com", "Release-Assets.GitHubUserContent.com"])
    }

    #[test]
    fn checks_scheme_host_and_credentials() {
        let policy = policy();
        let ok = Url::parse("https://GitHub.com/a").expect("url");
        assert!(policy.check(&ok).is_ok());
        let cdn = Url::parse("https://release-assets.githubusercontent.com/x").expect("url");
        assert!(policy.check(&cdn).is_ok());
        let http = Url::parse("http://github.com/a").expect("url");
        assert!(matches!(
            policy.check(&http),
            Err(FetchError::InsecureUrl(_))
        ));
        let other = Url::parse("https://github.com.evil.example/a").expect("url");
        assert!(matches!(
            policy.check(&other),
            Err(FetchError::HostNotAllowed(_))
        ));
        let creds = Url::parse("https://user:pass@github.com/a").expect("url");
        assert!(matches!(policy.check(&creds), Err(FetchError::InvalidUrl)));
        let file = Url::parse("file:///C:/x").expect("url");
        assert!(policy.check(&file).is_err());
    }

    #[test]
    fn normalizes_expected_hash() {
        let upper = "A".repeat(64);
        assert_eq!(normalize_hash(&upper).expect("hash"), "a".repeat(64));
        assert!(normalize_hash("abc").is_err());
        assert!(normalize_hash(&"g".repeat(64)).is_err());
    }
}
