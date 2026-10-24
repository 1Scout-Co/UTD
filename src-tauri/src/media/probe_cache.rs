//! Luồng của lần "Phân tích" (`yt-dlp -J` lúc mở hộp chọn định dạng). Lượt tải đầu ngay sau đó
//! dựng kế hoạch từ đây, khỏi chạy yt-dlp lần hai (~1,5 s); thời gian chờ quảng cáo
//! (`available_at`) cũng trôi đi trong lúc người dùng chọn. Chỉ giữ `Stream` của luồng http(s)
//! không DRM, không giữ nguyên JSON (vài trăm KB mỗi video).

use std::sync::{Mutex, OnceLock};

use serde_json::Value;

use crate::media::resolve::{self, Resolved, Stream};

/// Số trang nhớ cùng lúc; đầy thì bỏ trang lâu chưa dùng nhất.
const CAPACITY: usize = 16;
/// Nhớ tối đa chừng này giây sau lần phân tích.
const MAX_AGE: i64 = 60 * 60;
/// Thôi dùng sớm chừng này giây trước khi link đầu tiên hết hạn (`expire=`).
const EXPIRY_MARGIN: i64 = 600;
/// Từ khoá selector của yt-dlp, không phải mã định dạng.
const KEYWORDS: &[&str] = &[
    "b",
    "w",
    "bv",
    "ba",
    "wv",
    "wa",
    "best",
    "worst",
    "bestvideo",
    "bestaudio",
    "worstvideo",
    "worstaudio",
    "all",
    "mergeall",
];

struct Entry {
    /// URL đã hỏi yt-dlp và `webpage_url` của trang.
    keys: Vec<String>,
    streams: Vec<Stream>,
    /// Giây Unix; từ lúc này không dùng nữa.
    until: i64,
}

impl Entry {
    fn matches(&self, urls: &[&str]) -> bool {
        self.keys.iter().any(|key| urls.contains(&key.as_str()))
    }
}

/// LRU nhỏ: đầu danh sách là trang dùng gần nhất.
#[derive(Default)]
struct Cache {
    entries: Vec<Entry>,
}

impl Cache {
    /// Thay trang trùng URL, bỏ trang hết hạn và trang cũ nhất khi đầy.
    fn insert(&mut self, entry: Entry, now: i64) {
        self.entries
            .retain(|old| old.until > now && !old.keys.iter().any(|key| entry.keys.contains(key)));
        self.entries.insert(0, entry);
        self.entries.truncate(CAPACITY);
    }

    /// Luồng của đủ các mã `ids` theo đúng thứ tự; thiếu một mã là `None`.
    fn find(&mut self, urls: &[&str], ids: &[&str], now: i64) -> Option<Vec<Stream>> {
        self.entries.retain(|entry| entry.until > now);
        let position = self.entries.iter().position(|entry| entry.matches(urls))?;
        let entry = self.entries.remove(position);
        let streams = ids
            .iter()
            .map(|id| {
                entry
                    .streams
                    .iter()
                    .find(|stream| stream.format_id == *id)
                    .cloned()
            })
            .collect();
        self.entries.insert(0, entry);
        streams
    }

    fn forget(&mut self, urls: &[&str]) {
        self.entries.retain(|entry| !entry.matches(urls));
    }
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

/// Nhớ luồng sau một lần phân tích thành công; `original_url` = URL đã hỏi yt-dlp.
pub(super) fn remember(value: &Value, original_url: &str, now: i64) {
    if resolve::flagged(value.get("_has_drm")) {
        return;
    }
    let mut keys = vec![original_url.to_string()];
    if let Some(page) = value.get("webpage_url").and_then(Value::as_str)
        && page != original_url
    {
        keys.push(page.to_string());
    }
    store(keys, direct_streams(value), now);
}

/// Lưu `streams` cho các URL `keys`; không còn kịp dùng (link sắp hết hạn) thì thôi.
pub(super) fn store(keys: Vec<String>, streams: Vec<Stream>, now: i64) {
    let Some(until) = lifetime(&streams, now) else {
        return;
    };
    if let Ok(mut cache) = cache().lock() {
        cache.insert(
            Entry {
                keys,
                streams,
                until,
            },
            now,
        );
    }
}

/// Luồng của đúng các mã ở đầu `selector` (`399+140/…`) cho một trong các URL `urls`, còn hạn.
/// `None` = selector không ghi mã cụ thể, trang chưa phân tích, đã hết hạn hoặc thiếu mã.
pub(super) fn lookup(urls: &[&str], selector: &str, now: i64) -> Option<Resolved> {
    let ids = selector_ids(selector)?;
    let streams = cache().lock().ok()?.find(urls, &ids, now)?;
    Some(Resolved {
        streams,
        drm: false,
        live: false,
    })
}

/// Link đã lưu bị máy chủ từ chối (đổi mạng, hết hạn sớm): bỏ để lượt sau hỏi yt-dlp.
pub(super) fn forget(urls: &[&str]) {
    if let Ok(mut cache) = cache().lock() {
        cache.forget(urls);
    }
}

/// Mã định dạng ghi thẳng được vào selector (`399`, `140-drc`, `hls-720p`), không phải từ khoá.
pub(super) fn plain_id(id: &str) -> bool {
    let allowed = |character: char| character.is_ascii_alphanumeric() || "-_.".contains(character);
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(allowed)
        && !KEYWORDS.contains(&id.to_ascii_lowercase().as_str())
}

/// Mã ngôn ngữ ghi được vào bộ lọc `[language=…]` không cần ngoặc kép (`en-US`, `zh-Hans`).
pub(super) fn plain_language(language: &str) -> bool {
    let allowed = |character: char| character.is_ascii_alphanumeric() || "-_.".contains(character);
    !language.is_empty() && language.len() <= 32 && language.chars().all(allowed)
}

/// Các mã trước dấu `/` đầu tiên: `399+140/bv*+ba/b` → `[399, 140]`; `bv*+ba/b` → `None`.
pub(super) fn selector_ids(selector: &str) -> Option<Vec<&str>> {
    let head = selector.split('/').next()?;
    head.split('+')
        .map(|token| pinned_id(token.trim()))
        .collect()
}

/// `399` hoặc `140-23[language=en-US]` (probe ghim mã yt-dlp tự đánh số kèm ngôn ngữ) → mã. Bộ lọc
/// khác thì `None`: luồng trong cache không chắc khớp, để yt-dlp tự chọn.
fn pinned_id(token: &str) -> Option<&str> {
    let id = match token.split_once('[') {
        Some((id, filter)) => {
            let language = filter.strip_suffix(']')?.strip_prefix("language=")?;
            plain_language(language).then_some(id)?
        }
        None => token,
    };
    plain_id(id).then_some(id)
}

/// Luồng engine tải thẳng được (http/https, không DRM) trong `formats`.
fn direct_streams(value: &Value) -> Vec<Stream> {
    value
        .get("formats")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(resolve::parse_stream)
                .filter(|stream| matches!(stream.protocol.as_str(), "http" | "https"))
                .filter(|stream| !stream.drm)
                .collect()
        })
        .unwrap_or_default()
}

/// Hạn dùng: min(60 phút, link hết hạn sớm nhất − 10 phút).
fn lifetime(streams: &[Stream], now: i64) -> Option<i64> {
    if streams.is_empty() {
        return None;
    }
    let until = streams
        .iter()
        .filter_map(|stream| resolve::expires_at(&stream.url))
        .map(|at| at - EXPIRY_MARGIN)
        .fold(now + MAX_AGE, i64::min);
    (until > now).then_some(until)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: i64 = 1_800_000_000;

    fn format(id: &str, protocol: &str, ext: &str, expire: i64) -> Value {
        json!({
            "format_id": id,
            "url": format!("https://rr1.googlevideo.com/videoplayback?itag={id}&expire={expire}"),
            "protocol": protocol,
            "ext": ext,
            "vcodec": if ext == "m4a" { "none" } else { "av01.0.09M.08" },
            "acodec": if ext == "m4a" { "mp4a.40.2" } else { "none" },
            "http_headers": { "User-Agent": "Mozilla/5.0" }
        })
    }

    fn info(page: &str, expire: i64) -> Value {
        let mut drm = format("999", "https", "mp4", expire);
        drm["has_drm"] = json!(true);
        json!({
            "webpage_url": page,
            "formats": [
                format("sb0", "mhtml", "mhtml", expire),
                format("140-drc", "https", "m4a", expire),
                format("140", "https", "m4a", expire),
                format("616", "m3u8_native", "mp4", expire),
                format("399", "https", "mp4", expire),
                drm
            ]
        })
    }

    fn entry(key: &str, until: i64) -> Entry {
        Entry {
            keys: vec![key.to_string()],
            streams: direct_streams(&info(key, NOW + 6 * 3600)),
            until,
        }
    }

    fn unique(prefix: &str) -> String {
        format!("{prefix}{}", uuid::Uuid::new_v4())
    }

    #[test]
    fn reads_explicit_ids_only_from_the_selector_head() {
        assert_eq!(selector_ids("399+140/bv*+ba/b"), Some(vec!["399", "140"]));
        assert_eq!(selector_ids("140/ba[ext=m4a]/ba"), Some(vec!["140"]));
        assert_eq!(
            selector_ids("140-drc+hls-720p.1"),
            Some(vec!["140-drc", "hls-720p.1"])
        );
        assert_eq!(
            selector_ids("399+140-23[language=en-US]/bv*+ba/b"),
            Some(vec!["399", "140-23"]),
            "mã yt-dlp đánh số, ghim kèm ngôn ngữ"
        );
        assert_eq!(
            selector_ids("251-4[language=zh-Hans]/ba/b"),
            Some(vec!["251-4"])
        );
        for selector in [
            "bv*+ba/b",
            "ba/b",
            "best",
            "b",
            "",
            "399+",
            "bv[height<=720]+140",
            "399+140[ext=m4a]/bv*+ba/b",
            "140-1[language=]/ba",
            "140-1[language=en/ba",
            "140-1[language=en US]/ba",
            "140-1[language=en][ext=m4a]/ba",
            "[language=en]/ba",
            "ba[language=en]/b",
        ] {
            assert_eq!(selector_ids(selector), None, "{selector}");
        }
    }

    #[test]
    fn remembers_direct_streams_by_page_and_original_url() {
        let short = unique("https://youtu.be/");
        let page = unique("https://www.youtube.com/watch?v=");
        let now = resolve::now_secs();
        remember(&info(&page, now + 6 * 3600), &short, now);
        let merged = lookup(&[short.as_str()], "399+140/bv*+ba/b", now).expect("theo URL đã dán");
        let ids: Vec<&str> = merged
            .streams
            .iter()
            .map(|stream| stream.format_id.as_str())
            .collect();
        assert_eq!(ids, ["399", "140"], "đúng thứ tự trong selector");
        assert_eq!(
            merged.streams[0].headers.len(),
            1,
            "giữ header riêng của format"
        );
        assert!(!merged.drm && !merged.live);
        let both = ["https://other", page.as_str()];
        assert!(lookup(&both, "140/ba", now).is_some(), "theo webpage_url");
        assert!(
            lookup(&both, "399+140[language=en]/bv*+ba/b", now).is_some(),
            "mã ghim kèm ngôn ngữ"
        );
        assert!(
            lookup(&[page.as_str()], "616+140/bv*+ba/b", now).is_none(),
            "HLS không lưu"
        );
        assert!(
            lookup(&[page.as_str()], "999/b", now).is_none(),
            "format DRM không lưu"
        );
        assert!(
            lookup(&[page.as_str()], "bv*+ba/b", now).is_none(),
            "không ghi mã cụ thể"
        );
        assert!(
            lookup(&[page.as_str()], "137+140/bv*+ba/b", now).is_none(),
            "thiếu một mã"
        );

        forget(&[short.as_str()]);
        assert!(
            lookup(&[page.as_str()], "140/ba", now).is_none(),
            "quên cả trang"
        );

        let maybe = unique("https://www.youtube.com/watch?v=");
        let mut drm = info(&maybe, now + 6 * 3600);
        drm["_has_drm"] = json!("maybe");
        remember(&drm, &maybe, now);
        assert!(
            lookup(&[maybe.as_str()], "140/ba", now).is_none(),
            "nghi DRM thì không lưu"
        );
    }

    #[test]
    fn expires_ten_minutes_before_the_first_link_and_after_an_hour_at_most() {
        let streams = direct_streams(&info("p", NOW + 1000));
        assert_eq!(lifetime(&streams, NOW), Some(NOW + 400));
        assert_eq!(
            lifetime(&direct_streams(&info("p", NOW + 7 * 3600)), NOW),
            Some(NOW + 3600)
        );
        assert_eq!(
            lifetime(&direct_streams(&info("p", NOW + 600)), NOW),
            None,
            "sắp hết hạn"
        );
        assert_eq!(lifetime(&[], NOW), None);

        let mut cache = Cache::default();
        cache.insert(entry("p", NOW + 400), NOW);
        assert!(cache.find(&["p"], &["399", "140"], NOW + 399).is_some());
        assert!(
            cache.find(&["p"], &["399", "140"], NOW + 400).is_none(),
            "hết hạn"
        );
        assert!(cache.entries.is_empty(), "bỏ trang hết hạn");
    }

    #[test]
    fn keeps_the_sixteen_most_recently_used_pages() {
        let mut cache = Cache::default();
        for index in 0..CAPACITY {
            cache.insert(entry(&format!("p{index}"), NOW + 3600), NOW);
        }
        assert!(
            cache.find(&["p0"], &["140"], NOW).is_some(),
            "dùng lại ⇒ lên đầu"
        );
        cache.insert(entry("new", NOW + 3600), NOW);
        assert_eq!(cache.entries.len(), CAPACITY);
        assert!(cache.find(&["p0"], &["140"], NOW).is_some());
        assert!(
            cache.find(&["p1"], &["140"], NOW).is_none(),
            "trang lâu chưa dùng bị bỏ"
        );

        let mut fresh = entry("p0", NOW + 3600);
        fresh.streams.truncate(1);
        cache.insert(fresh, NOW);
        assert_eq!(cache.entries.len(), CAPACITY, "cùng URL ⇒ thay, không thêm");
        assert!(
            cache.find(&["p0"], &["399"], NOW).is_none(),
            "dùng bản phân tích mới"
        );
        assert!(cache.find(&["p0"], &["140-drc"], NOW).is_some());
        cache.forget(&["p0"]);
        assert!(cache.find(&["p0"], &["140-drc"], NOW).is_none());
    }
}
