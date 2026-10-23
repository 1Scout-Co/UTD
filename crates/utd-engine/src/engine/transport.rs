//! Cách gửi yêu cầu của một task: header riêng chỉ tới đúng origin của link gốc, và trần byte
//! mỗi yêu cầu (YouTube bóp yêu cầu Range lớn hơn `http_chunk_size`).

use std::time::Duration;

use reqwest::header::{ACCEPT_ENCODING, HeaderMap, HeaderName, HeaderValue, LOCATION, RANGE};
use reqwest::redirect::Policy;
use reqwest::{Client, Method, RequestBuilder, Response};
use url::{Origin, Url};

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

#[derive(Clone)]
pub(crate) struct Transport {
    plain: Client,
    scoped: Client,
    origin: Origin,
    headers: HeaderMap,
    max_request: Option<u64>,
}

impl Transport {
    pub fn new(
        plain: &Client,
        scoped: &Client,
        task_url: &Url,
        headers: &[RequestHeader],
        max_request: Option<u64>,
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
            plain: plain.clone(),
            scoped: scoped.clone(),
            origin: task_url.origin(),
            headers: map,
            max_request: max_request.filter(|bytes| *bytes > 0),
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

    /// Gửi một yêu cầu. Header riêng chỉ đi kèm khi `url` cùng origin với link gốc; server
    /// chuyển hướng sang origin khác thì gửi lại tới đó không kèm header riêng.
    pub async fn send(
        &self,
        method: Method,
        url: &Url,
        range: Option<&str>,
        timeout: Option<Duration>,
    ) -> reqwest::Result<Response> {
        let scoped = !self.headers.is_empty() && url.origin() == self.origin;
        let response = self
            .build(method.clone(), url, range, timeout, scoped)
            .send()
            .await?;
        if !scoped || !response.status().is_redirection() {
            return Ok(response);
        }
        let Some(next) = redirect_target(&response) else {
            return Ok(response);
        };
        drop(response);
        self.build(method, &next, range, timeout, false)
            .send()
            .await
    }

    fn build(
        &self,
        method: Method,
        url: &Url,
        range: Option<&str>,
        timeout: Option<Duration>,
        scoped: bool,
    ) -> RequestBuilder {
        let client = if scoped { &self.scoped } else { &self.plain };
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
        let url = Url::parse("https://example.com/a").unwrap();
        Transport::new(&client, &client, &url, &[], cap).unwrap()
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
