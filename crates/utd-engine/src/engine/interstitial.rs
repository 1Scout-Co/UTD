//! Trang xác nhận trước khi tải. Google Drive trả trang HTML "không quét virus được" cho file lớn thay vì file,
//! kèm form GET `id="download-form"` trỏ về chính máy chủ tải (`id`, `export`, `confirm`, `uuid`…); bấm "Vẫn tải
//! xuống" là gửi form đó. Engine đọc trang (có giới hạn), dựng link từ form rồi dò lại một lần. Chỉ theo form cùng
//! host với trang, không chạy script, không gửi gì khác ngoài các ô ẩn của form.

use std::time::Duration;

use reqwest::Method;
use url::Url;

use super::transport::Transport;

/// Đọc tối đa chừng này byte của trang xác nhận (trang của Google ~2 KiB).
const MAX_PAGE_BYTES: usize = 256 * 1024;
/// Form tải của trang xác nhận.
const FORM_ID: &str = "download-form";

/// Link tải thật từ trang xác nhận ở `page` (đã dò thấy là trang HTML); `None` khi trang không có form đó.
pub(crate) async fn follow(transport: &Transport, page: &Url, timeout: Duration) -> Option<Url> {
    let mut response = transport.send(Method::GET, page, None, Some(timeout)).await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let base = response.url().clone();
    let mut body = Vec::new();
    while body.len() < MAX_PAGE_BYTES {
        match response.chunk().await.ok()? {
            Some(chunk) => body.extend_from_slice(&chunk),
            None => break,
        }
    }
    body.truncate(MAX_PAGE_BYTES);
    form_target(&String::from_utf8_lossy(&body), &base)
}

/// Link mà form `download-form` (GET, cùng host với trang `base`) gửi tới, kèm các ô có tên của form.
pub(crate) fn form_target(html: &str, base: &Url) -> Option<Url> {
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    let (start, form) = loop {
        let start = from + lower[from..].find("<form")?;
        let tag = tag_at(html, start)?;
        let attributes = attributes(tag);
        if value_of(&attributes, "id") == Some(FORM_ID) {
            break (start, attributes);
        }
        from = start + 5;
    };
    if value_of(&form, "method").is_some_and(|method| !method.eq_ignore_ascii_case("get")) {
        return None;
    }
    let mut target = base.join(value_of(&form, "action").unwrap_or("")).ok()?;
    let same_host = target.scheme() == base.scheme()
        && target.host_str() == base.host_str()
        && target.port_or_known_default() == base.port_or_known_default();
    if !same_host {
        return None;
    }
    let end = lower[start..].find("</form").map_or(html.len(), |end| start + end);
    target.set_query(None);
    target.set_fragment(None);
    let mut inside = start;
    while let Some(offset) = lower[inside..end].find("<input") {
        let at = inside + offset;
        let Some(tag) = tag_at(html, at) else { break };
        inside = at + tag.len();
        let input = attributes(tag);
        let kind = value_of(&input, "type").unwrap_or("text").to_ascii_lowercase();
        if matches!(kind.as_str(), "submit" | "button" | "image" | "reset" | "file") {
            continue;
        }
        if let Some(name) = value_of(&input, "name").filter(|name| !name.is_empty()) {
            let value = value_of(&input, "value").unwrap_or("");
            target.query_pairs_mut().append_pair(name, value);
        }
    }
    Some(target)
}

/// Thẻ mở bắt đầu ở `start` (tới `>` đầu tiên ngoài ngoặc kép/đơn).
fn tag_at(html: &str, start: usize) -> Option<&str> {
    let mut quote = None;
    for (offset, character) in html[start..].char_indices() {
        match (quote, character) {
            (Some(open), _) if character == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(character),
            (None, '>') => return Some(&html[start..=start + offset]),
            _ => {}
        }
    }
    None
}

/// Thuộc tính của một thẻ mở (tên viết thường, giá trị đã giải `&amp;`…).
fn attributes(tag: &str) -> Vec<(String, String)> {
    let inner = tag.trim_start_matches('<').trim_end_matches('>').trim_end_matches('/');
    let mut rest = inner
        .split_once(|character: char| character.is_whitespace())
        .map_or("", |(_, rest)| rest);
    let mut found = Vec::new();
    loop {
        rest = rest.trim_start_matches(|character: char| character.is_whitespace() || character == '/');
        if rest.is_empty() {
            return found;
        }
        let name_end = rest
            .find(|character: char| character.is_whitespace() || character == '=' || character == '/')
            .unwrap_or(rest.len());
        let name = rest[..name_end].to_ascii_lowercase();
        rest = rest[name_end..].trim_start();
        let mut value = String::new();
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let (raw, remainder) = match after.chars().next() {
                Some(quote @ ('"' | '\'')) => {
                    let body = &after[1..];
                    match body.find(quote) {
                        Some(close) => (&body[..close], &body[close + 1..]),
                        None => (body, ""),
                    }
                }
                _ => {
                    let close = after.find(char::is_whitespace).unwrap_or(after.len());
                    (&after[..close], &after[close..])
                }
            };
            value = decode_entities(raw);
            rest = remainder;
        }
        if !name.is_empty() {
            found.push((name, value));
        }
    }
}

fn value_of<'a>(attributes: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// Giải các thực thể HTML hay gặp trong giá trị thuộc tính.
fn decode_entities(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|end| *end <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "lt" => Some('<'),
            "gt" => Some('>'),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|number| number.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(character) => {
                out.push(character);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://drive.usercontent.google.com/download?id=1AbC&export=download&confirm=t";

    /// Rút gọn từ trang thật của Google Drive (2025): file lớn, không quét virus được.
    const GOOGLE: &str = r#"<!DOCTYPE html><html><head><title>Google Drive - Virus scan warning</title></head>
<body><div class="uc-main"><div id="uc-text"><p class="uc-warning-caption">Google Drive can&#39;t scan this file
for viruses.</p></div><form id="download-form" action="https://drive.usercontent.google.com/download"
method="get"><input type="submit" id="uc-download-link" class="goog-inline-block jfk-button jfk-button-action"
value="Download anyway"/><input type="hidden" name="id" value="1AbC"><input type="hidden" name="export"
value="download"><input type="hidden" name="confirm" value="t"><input type="hidden" name="uuid"
value="5a1f&amp;x=1"></form></div></body></html>"#;

    fn base() -> Url {
        Url::parse(BASE).unwrap()
    }

    #[test]
    fn builds_the_link_of_the_download_form() {
        let target = form_target(GOOGLE, &base()).expect("form");
        assert_eq!(
            target.as_str(),
            "https://drive.usercontent.google.com/download?id=1AbC&export=download&confirm=t&uuid=5a1f%26x%3D1"
        );
    }

    #[test]
    fn relative_actions_and_single_quotes_work() {
        let html = "<form method='GET' id='download-form' action='/download'>\
                    <input name=id value=1AbC><input type=hidden name='confirm' value='t'></form>";
        let target = form_target(html, &base()).expect("form");
        assert_eq!(
            target.as_str(),
            "https://drive.usercontent.google.com/download?id=1AbC&confirm=t"
        );
    }

    #[test]
    fn ignores_other_forms_other_hosts_and_post() {
        let login = "<form id=\"gaia_loginform\" action=\"https://accounts.google.com/signin\"><input name=a></form>";
        assert_eq!(form_target(login, &base()), None);
        let elsewhere = GOOGLE.replace(
            "action=\"https://drive.usercontent.google.com/download\"",
            "action=\"https://evil.example/download\"",
        );
        assert_eq!(form_target(&elsewhere, &base()), None);
        let post = GOOGLE.replace("method=\"get\"", "method=\"post\"");
        assert_eq!(form_target(&post, &base()), None);
        assert_eq!(form_target("<p>Hello</p>", &base()), None);
        assert_eq!(form_target("<form id=\"download-form\"", &base()), None, "thẻ chưa đóng");
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(decode_entities("a&amp;b&#39;c&#x2F;d&bogus;e&"), "a&b'c/d&bogus;e&");
    }
}
