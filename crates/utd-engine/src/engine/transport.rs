//! Cách gửi yêu cầu của một task: header riêng chỉ tới đúng origin của link gốc, cookie đăng nhập theo
//! luật miền/đường dẫn ở từng bước chuyển hướng, và trần byte mỗi yêu cầu (YouTube bóp yêu cầu Range lớn
//! hơn `http_chunk_size`).

use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{
    ACCEPT_ENCODING, COOKIE, HeaderMap, HeaderName, HeaderValue, LOCATION, RANGE, SET_COOKIE,
};
use reqwest::redirect::Policy;
use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use url::{Origin, Url};

use super::http::{RequestFailure, failure_from_status};
use crate::auth::{CookieJar, auth_expired};
use crate::headers::RequestHeader;
use crate::{Error, Result};

const MAX_REDIRECTS: usize = 10;

pub(crate) fn plain_policy() -> Policy {
    Policy::limited(MAX_REDIRECTS)
}

/// Client cho task có header riêng: theo chuyển hướng cùng origin; sang origin khác thì dừng
/// để `Transport::send` gửi lại bằng client thường, không kèm header riêng.
pub(crate) fn scoped_policy() -> Policy {
    Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            return attempt.error("too many redirects");
        }
        let same_origin = attempt
            .previous()
            .first()
            .is_some_and(|first| first.origin() == attempt.url().origin());
        if same_origin {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

/// Ba client dùng chung của engine (mỗi client giữ pool kết nối riêng).
#[derive(Clone)]
pub(crate) struct Clients {
    pub plain: Client,
    /// Dừng ở chuyển hướng sang origin khác (`scoped_policy`).
    pub scoped: Client,
    /// Không tự theo chuyển hướng: task có cookie tự theo để tính lại `Cookie` ở từng bước.
    pub manual: Client,
}

#[derive(Clone)]
pub(crate) struct Transport {
    clients: Clients,
    origin: Origin,
    headers: HeaderMap,
    max_request: Option<u64>,
    cookies: Option<Arc<CookieJar>>,
}

impl Transport {
    pub fn new(
        clients: &Clients,
        task_url: &Url,
        headers: &[RequestHeader],
        max_request: Option<u64>,
        cookies: Option<Arc<CookieJar>>,
    ) -> Result<Self> {
        let mut map = HeaderMap::new();
        for header in headers {
            let name = HeaderName::from_bytes(header.name.as_bytes())
                .map_err(|_| Error::InvalidInput("invalid request header name".into()))?;
            let value = HeaderValue::from_str(&header.value).map_err(|_| {
                Error::InvalidInput(format!(
                    "request header {} has an invalid value",
                    header.name
                ))
            })?;
            map.insert(name, value);
        }
        Ok(Self {
            clients: clients.clone(),
            origin: task_url.origin(),
            headers: map,
            max_request: max_request.filter(|bytes| *bytes > 0),
            cookies,
        })
    }

    pub fn max_request(&self) -> Option<u64> {
        self.max_request
    }

    /// Byte cuối (gồm) cho yêu cầu bắt đầu ở `next` trong đoạn kết thúc ở `end`.
    pub fn request_end(&self, next: u64, end: u64) -> u64 {
        match self.max_request {
            Some(cap) => end.min(next.saturating_add(cap - 1)),
            None => end,
        }
    }

    /// Lỗi cho một mã HTTP không thành công. Đã kèm cookie mà máy chủ vẫn 401/403 ⇒ `auth.expired`
    /// (không thử lại: đăng nhập phải làm mới từ trình duyệt).
    pub fn status_failure(&self, status: StatusCode, headers: &HeaderMap) -> RequestFailure {
        if self.cookies.is_some() && matches!(status.as_u16(), 401 | 403) {
            return RequestFailure {
                error: auth_expired(),
                retry_after: None,
            };
        }
        failure_from_status(status, headers)
    }

    pub fn has_cookies(&self) -> bool {
        self.cookies.is_some()
    }

    /// Gửi một yêu cầu. Header riêng chỉ đi kèm khi `url` cùng origin với link gốc; server
    /// chuyển hướng sang origin khác thì gửi lại tới đó không kèm header riêng.
    pub async fn send(
        &self,
        method: Method,
        url: &Url,
        range: Option<&str>,
        timeout: Option<Duration>,
    ) -> reqwest::Result<Response> {
        if let Some(cookies) = &self.cookies {
            return self
                .send_with_cookies(cookies, method, url, range, timeout)
                .await;
        }
        let scoped = self.scoped_to(url);
        let client = if scoped {
            &self.clients.scoped
        } else {
            &self.clients.plain
        };
        let response = self
            .build(client, method.clone(), url, range, timeout, scoped)
            .send()
            .await?;
        if !scoped || !response.status().is_redirection() {
            return Ok(response);
        }
        let Some(next) = redirect_target(&response) else {
            return Ok(response);
        };
        drop(response);
        self.build(&self.clients.plain, method, &next, range, timeout, false)
            .send()
            .await
    }

    /// Tự theo chuyển hướng (tối đa `MAX_REDIRECTS`): mỗi bước gửi đúng cookie hợp miền/đường dẫn của bước
    /// đó (trang lạ không nhận cookie nào) và gộp `Set-Cookie` vào hũ. Hết lượt thì trả phản hồi 3xx cuối,
    /// người gọi coi như lỗi máy chủ.
    async fn send_with_cookies(
        &self,
        cookies: &CookieJar,
        method: Method,
        url: &Url,
        range: Option<&str>,
        timeout: Option<Duration>,
    ) -> reqwest::Result<Response> {
        let mut current = url.clone();
        let mut hops = 0;
        loop {
            let mut request = self.build(
                &self.clients.manual,
                method.clone(),
                &current,
                range,
                timeout,
                self.scoped_to(&current),
            );
            if let Some(cookie) = cookies.header_for(&current) {
                request = request.header(COOKIE, cookie);
            }
            let response = request.send().await?;
            cookies.absorb(&current, response.headers().get_all(SET_COOKIE));
            if !response.status().is_redirection() || hops >= MAX_REDIRECTS {
                return Ok(response);
            }
            let Some(next) = redirect_target(&response) else {
                return Ok(response);
            };
            drop(response);
            current = next;
            hops += 1;
        }
    }

    fn scoped_to(&self, url: &Url) -> bool {
        !self.headers.is_empty() && url.origin() == self.origin
    }

    fn build(
        &self,
        client: &Client,
        method: Method,
        url: &Url,
        range: Option<&str>,
        timeout: Option<Duration>,
        scoped: bool,
    ) -> RequestBuilder {
        let mut request = client
            .request(method, url.clone())
            .header(ACCEPT_ENCODING, "identity");
        if let Some(range) = range {
            request = request.header(RANGE, range);
        }
        if scoped {
            request = request.headers(self.headers.clone());
        }
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        request
    }
}

fn redirect_target(response: &Response) -> Option<Url> {
    let location = response.headers().get(LOCATION)?.to_str().ok()?;
    let next = response.url().join(location).ok()?;
    matches!(next.scheme(), "http" | "https").then_some(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transport(cap: Option<u64>) -> Transport {
        let client = Client::new();
        let clients = Clients {
            plain: client.clone(),
            scoped: client.clone(),
            manual: client,
        };
        let url = Url::parse("https://example.com/a").unwrap();
        Transport::new(&clients, &url, &[], cap, None).unwrap()
    }

    #[test]
    fn caps_each_request_inside_the_segment() {
        let capped = transport(Some(10));
        assert_eq!(capped.request_end(0, 99), 9);
        assert_eq!(capped.request_end(95, 99), 99);
        assert_eq!(capped.request_end(u64::MAX - 1, u64::MAX), u64::MAX);
        assert_eq!(transport(None).request_end(0, 99), 99);
    }
}
