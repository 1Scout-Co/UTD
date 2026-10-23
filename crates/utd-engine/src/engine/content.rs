//! AC-06: link trả về trang web (HTML) hoặc không có dữ liệu thay vì file ⇒ báo lỗi, không
//! lưu file. Trang `.html` thật và file 0 byte được server báo rõ vẫn tải bình thường.

use std::path::Path;
use std::time::Duration;

use reqwest::{Method, StatusCode};
use tokio::io::AsyncReadExt;
use url::Url;

use super::transport::Transport;
use crate::model::{ErrorClass, TaskError};

const SNIFF_BYTES: usize = 1024;
const PAGE_NAMES: [&str; 7] = [
    ".htm", ".html", ".xhtml", ".xht", ".shtml", ".mht", ".mhtml",
];
const HTML_STARTS: [&str; 4] = ["<!doctype html", "<html", "<head", "<body"];

/// Cần kiểm nội dung lúc nào.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageCheck {
    /// Tên là trang web, server đặt tên file, hoặc kiểu không phải HTML: tin server.
    Skip,
    /// Server nói HTML: đọc thử 1 KB đầu trước khi tải.
    Probe,
    /// Server không nói kiểu: xem đầu file sau khi tải xong.
    Finish,
}

pub(crate) fn page_check(
    file_name: &str,
    content_type: Option<&str>,
    attachment: bool,
) -> PageCheck {
    if attachment || is_page_name(file_name) {
        return PageCheck::Skip;
    }
    match content_type {
        Some(kind) if is_html_type(kind) => PageCheck::Probe,
        Some(_) => PageCheck::Skip,
        None => PageCheck::Finish,
    }
}

pub(crate) fn is_html_type(content_type: &str) -> bool {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(essence.as_str(), "text/html" | "application/xhtml+xml")
}

pub(crate) fn is_page_name(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    PAGE_NAMES
        .iter()
        .any(|extension| lower.ends_with(extension))
}

/// Đầu nội dung giống tài liệu HTML (bỏ BOM, khoảng trắng, chú thích, khai báo XML).
pub(crate) fn looks_like_html(prefix: &[u8]) -> bool {
    let mut rest = prefix.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(prefix);
    for _ in 0..8 {
        rest = trim_start(rest);
        let skip = if rest.starts_with(b"<!--") {
            find(rest, b"-->").map(|end| end + 3)
        } else if rest.starts_with(b"<?xml") {
            find(rest, b"?>").map(|end| end + 2)
        } else {
            None
        };
        match skip {
            Some(skip) => rest = &rest[skip..],
            None => break,
        }
    }
    let head: Vec<u8> = rest.iter().take(16).map(u8::to_ascii_lowercase).collect();
    HTML_STARTS
        .iter()
        .any(|marker| head.starts_with(marker.as_bytes()))
}

/// Đọc tối đa 1 KB đầu của link. `None` khi không đọc được; rỗng khi server không có dữ liệu.
pub(crate) async fn sniff_remote(
    transport: &Transport,
    url: &Url,
    timeout: Duration,
) -> Option<Vec<u8>> {
    let range = format!("bytes=0-{}", SNIFF_BYTES - 1);
    let mut response = transport
        .send(Method::GET, url, Some(&range), Some(timeout))
        .await
        .ok()?;
    if response.status() == StatusCode::RANGE_NOT_SATISFIABLE {
        return Some(Vec::new());
    }
    if !response.status().is_success() {
        return None;
    }
    let mut prefix = Vec::with_capacity(SNIFF_BYTES);
    while prefix.len() < SNIFF_BYTES {
        match response.chunk().await.ok()? {
            Some(chunk) => prefix.extend_from_slice(&chunk),
            None => break,
        }
    }
    prefix.truncate(SNIFF_BYTES);
    Some(prefix)
}

/// File tải xong là một trang HTML.
pub(crate) async fn file_is_page(path: &Path) -> bool {
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return false;
    };
    let mut prefix = vec![0u8; SNIFF_BYTES];
    let mut filled = 0;
    while filled < SNIFF_BYTES {
        match file.read(&mut prefix[filled..]).await {
            Ok(0) | Err(_) => break,
            Ok(read) => filled += read,
        }
    }
    looks_like_html(&prefix[..filled])
}

pub(crate) fn not_a_file() -> TaskError {
    TaskError {
        class: ErrorClass::User,
        code: "http.not_a_file".into(),
        message: "Link này là một trang web, không phải file.".into(),
    }
}

pub(crate) fn empty_response() -> TaskError {
    TaskError {
        class: ErrorClass::Permanent,
        code: "http.empty".into(),
        message: "Máy chủ không gửi dữ liệu nào.".into(),
    }
}

fn trim_start(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    &bytes[start..]
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_html_documents() {
        assert!(looks_like_html(b"<!DOCTYPE html><html lang=\"vi\">"));
        assert!(looks_like_html(b"\xEF\xBB\xBF\r\n  <html><head>"));
        assert!(looks_like_html(b"<!-- served by x --><!doctype HTML>"));
        assert!(looks_like_html(
            b"<?xml version=\"1.0\"?>\n<html xmlns=\"x\">"
        ));
        assert!(!looks_like_html(b"PK\x03\x04binary"));
        assert!(!looks_like_html(b"<?xml version=\"1.0\"?><svg/>"));
        assert!(!looks_like_html(b"\x1aE\xdf\xa3webm"));
        assert!(!looks_like_html(b""));
    }

    #[test]
    fn decides_when_to_check_content() {
        assert_eq!(
            page_check("watch", Some("text/html; charset=utf-8"), false),
            PageCheck::Probe
        );
        assert_eq!(
            page_check("page.HTML", Some("text/html"), false),
            PageCheck::Skip
        );
        assert_eq!(
            page_check("setup.zip", Some("text/html"), true),
            PageCheck::Skip
        );
        assert_eq!(
            page_check("video.webm", Some("video/webm"), false),
            PageCheck::Skip
        );
        assert_eq!(page_check("file.bin", None, false), PageCheck::Finish);
        assert!(is_html_type("application/xhtml+xml"));
        assert!(!is_html_type("text/plain"));
    }
}
