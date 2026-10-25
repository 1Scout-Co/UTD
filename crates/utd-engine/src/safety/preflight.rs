//! Cảnh báo trước khi tải (S-22): chỉ từ link, tên file và kiểu nội dung server báo, chưa tải byte nào.
//! Cùng bộ luật với Download Guard sau khi tải (`dangerous_extension`, nội dung chạy được mang tên
//! loại vô hại) và với hộp Đổi tên (tên giả đuôi, ký tự đảo chiều): không có luật thứ hai.

use std::path::Path;

use percent_encoding::percent_decode_str;
use serde::Serialize;
use url::Url;

use super::dangerous_extension;

/// Đuôi hay bị dùng làm mồi: `hoa-don.pdf.exe` trông như PDF khi Windows ẩn đuôi.
const DECOYS: [&str; 34] = [
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "csv", "odt", "jpg", "jpeg",
    "png", "gif", "bmp", "webp", "svg", "heic", "mp3", "mp4", "m4a", "mkv", "avi", "mov", "wmv",
    "wav", "flac", "zip", "rar", "7z", "iso", "html", "htm",
];

/// Kiểu nội dung (MIME) của chương trình/script chạy được.
const EXECUTABLE_TYPES: [&str; 16] = [
    "application/x-msdownload",
    "application/x-msdos-program",
    "application/x-dosexec",
    "application/x-executable",
    "application/x-elf",
    "application/x-mach-binary",
    "application/vnd.microsoft.portable-executable",
    "application/x-ms-installer",
    "application/x-msi",
    "application/x-ms-shortcut",
    "application/x-sh",
    "application/x-shellscript",
    "application/x-bat",
    "application/hta",
    "application/java-archive",
    "application/vnd.android.package-archive",
];

const ARCHIVE_TYPES: [&str; 11] = [
    "application/zip",
    "application/x-zip-compressed",
    "application/vnd.rar",
    "application/x-rar-compressed",
    "application/x-7z-compressed",
    "application/gzip",
    "application/x-gzip",
    "application/x-tar",
    "application/x-xz",
    "application/x-bzip2",
    "application/zstd",
];

/// Nhóm loại file đủ chắc để so đuôi với MIME (âm thanh và hình chung một nhóm: `.webm`, `.ogg`, `.mp4`
/// có thể là cả hai). Loại khác (văn bản, tài liệu Office, ISO…) server hay báo lung tung: không so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Program,
    Pdf,
    Image,
    Media,
    Archive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningKind {
    /// Tên có ký tự vô hình/đảo chiều chữ: tên hiện ra khác tên thật.
    HiddenChars,
    /// Tên giả đuôi (`hoa-don.pdf.exe`, nhiều khoảng trắng trước đuôi thật).
    SpoofedName,
    /// Loại chạy được mã: tải xong sẽ bị giữ trong khu cách ly.
    DangerousType,
    /// Server báo kiểu nội dung khác hẳn đuôi file (vd chương trình mang tên `.pdf`).
    MimeMismatch,
    /// Nội dung đi qua `http://`: có thể bị đọc hoặc sửa trên đường.
    InsecureHttp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Warning {
    pub kind: WarningKind,
    /// Chi tiết ngắn để hiện kèm: đuôi (`exe`), cặp đuôi giả (`pdf.exe`), mã ký tự (`U+202E`),
    /// kiểu nội dung, tên máy chủ.
    pub detail: String,
}

/// Những gì biết về một link trước khi tải. `file_name` rỗng ⇒ lấy từ đường dẫn của link.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinkFacts<'a> {
    pub url: &'a str,
    /// Link sau chuyển hướng (đã dò), nơi thật sự gửi nội dung.
    pub final_url: Option<&'a str>,
    pub file_name: &'a str,
    pub content_type: Option<&'a str>,
}

/// Mọi cảnh báo của một link, nặng trước. Không cảnh báo ⇒ tải luôn không hỏi.
pub fn warnings(facts: &LinkFacts<'_>) -> Vec<Warning> {
    let source = facts.final_url.unwrap_or(facts.url);
    let name = if facts.file_name.trim().is_empty() {
        name_from_url(source)
    } else {
        facts.file_name.to_string()
    };
    let extension = extension_of(&name);
    let mut found = Vec::new();
    let hidden: Vec<String> = name
        .chars()
        .filter(|&c| hidden_char(c))
        .map(|c| format!("U+{:04X}", u32::from(c)))
        .collect();
    if !hidden.is_empty() {
        push(&mut found, WarningKind::HiddenChars, hidden.join(" "));
    }
    if let Some(detail) = spoof_detail(&name) {
        push(&mut found, WarningKind::SpoofedName, detail);
    }
    if dangerous_extension(&extension) {
        push(&mut found, WarningKind::DangerousType, extension.clone());
    }
    if let Some(essence) = facts.content_type.map(essence_of)
        && mime_mismatch(&essence, &extension)
    {
        push(&mut found, WarningKind::MimeMismatch, essence);
    }
    if let Some(host) = insecure_host(source) {
        push(&mut found, WarningKind::InsecureHttp, host);
    }
    found
}

fn push(found: &mut Vec<Warning>, kind: WarningKind, detail: String) {
    found.push(Warning { kind, detail });
}

/// Ký tự vô hình hoặc đảo chiều chữ (bidi): `phim\u{202E}4pm.exe` hiện thành `phimexe.mp4`.
pub fn hidden_char(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
    )
}

/// Tên giả dạng loại khác: đuôi thật chạy được mã, đứng sau một đuôi "vô hại" hay dùng làm mồi, hoặc
/// bị nhiều khoảng trắng đẩy ra khỏi chỗ nhìn thấy.
pub fn spoofed_name(name: &str) -> bool {
    spoof_detail(name).is_some()
}

/// `pdf.exe` (cặp đuôi giả) hoặc `exe` (đuôi thật bị đẩy xa); None khi tên không giả.
fn spoof_detail(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split('.').collect();
    let [.., last] = parts.as_slice() else {
        return None;
    };
    if parts.len() < 2 || !dangerous_extension(&last.to_ascii_lowercase()) {
        return None;
    }
    let real = last.to_ascii_lowercase();
    if let [_, .., inner, _] = parts.as_slice() {
        let inner = inner.trim().to_lowercase();
        if DECOYS.contains(&inner.as_str()) {
            return Some(format!("{inner}.{real}"));
        }
    }
    name.contains("   ").then_some(real)
}

fn extension_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .trim()
        .to_lowercase()
}

/// Phần cuối đường dẫn của link (đã giải mã %), như engine đặt tên khi server không gợi ý.
fn name_from_url(raw: &str) -> String {
    Url::parse(raw)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back())
                .map(|segment| percent_decode_str(segment).decode_utf8_lossy().into_owned())
        })
        .unwrap_or_default()
}

fn essence_of(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

fn mime_family(essence: &str) -> Option<Family> {
    if EXECUTABLE_TYPES.contains(&essence) {
        Some(Family::Program)
    } else if essence == "application/pdf" {
        Some(Family::Pdf)
    } else if essence.starts_with("image/") {
        Some(Family::Image)
    } else if essence.starts_with("video/") || essence.starts_with("audio/") || essence == "application/mp4"
    {
        Some(Family::Media)
    } else if ARCHIVE_TYPES.contains(&essence) {
        Some(Family::Archive)
    } else {
        None
    }
}

fn extension_family(extension: &str) -> Option<Family> {
    match extension {
        "pdf" => Some(Family::Pdf),
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "svg" | "heic" | "avif" | "ico" | "tif"
        | "tiff" => Some(Family::Image),
        "mp4" | "m4v" | "mkv" | "webm" | "mov" | "avi" | "wmv" | "flv" | "ts" | "mp3" | "m4a"
        | "aac" | "wav" | "flac" | "ogg" | "opus" | "wma" => Some(Family::Media),
        "zip" | "rar" | "7z" | "gz" | "tgz" | "tar" | "xz" | "bz2" | "zst" => Some(Family::Archive),
        _ => None,
    }
}

/// Server báo chương trình mà tên không phải loại chạy được (như Guard thấy `MZ` trong `.pdf`), hoặc hai
/// nhóm chắc chắn khác nhau (video mang tên `.pdf`). `octet-stream`, HTML (engine tự chặn, AC-06) và kiểu
/// lạ: không đủ căn cứ.
fn mime_mismatch(essence: &str, extension: &str) -> bool {
    match (mime_family(essence), extension_family(extension)) {
        (Some(Family::Program), _) => !dangerous_extension(extension),
        (Some(served), Some(named)) => served != named,
        _ => false,
    }
}

/// Máy chủ khi nội dung đi qua `http://` (trừ chính máy này: không ai chen vào giữa được).
fn insecure_host(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    if url.scheme() != "http" {
        return None;
    }
    let host = url.host_str()?.to_string();
    let loopback = host == "localhost"
        || host.ends_with(".localhost")
        || url.host().is_some_and(|host| match host {
            url::Host::Ipv4(address) => address.is_loopback(),
            url::Host::Ipv6(address) => address.is_loopback(),
            url::Host::Domain(_) => false,
        });
    (!loopback).then_some(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(facts: LinkFacts<'_>) -> Vec<WarningKind> {
        warnings(&facts).into_iter().map(|warning| warning.kind).collect()
    }

    fn link(url: &'static str, file_name: &'static str) -> LinkFacts<'static> {
        LinkFacts {
            url,
            file_name,
            ..LinkFacts::default()
        }
    }

    #[test]
    fn ordinary_https_files_pass_quietly() {
        for name in ["ubuntu-24.04.iso", "báo cáo.pdf", "setup.v1.2.zip", "Phim hay.mp4", "data"] {
            assert!(kinds(link("https://example.com/x", name)).is_empty(), "{name}");
        }
        let video = LinkFacts {
            content_type: Some("audio/mp4"),
            ..link("https://cdn.example.com/a", "clip.mp4")
        };
        assert!(kinds(video).is_empty(), "hình/tiếng chung một nhóm");
        let generic = LinkFacts {
            content_type: Some("application/octet-stream"),
            ..link("https://example.com/a", "tai-lieu.pdf")
        };
        assert!(kinds(generic).is_empty());
    }

    #[test]
    fn executables_use_the_guard_list() {
        assert_eq!(kinds(link("https://example.com/a", "Setup.EXE")), [WarningKind::DangerousType]);
        let warning = &warnings(&link("https://example.com/a", "x.ps1"))[0];
        assert_eq!(warning.detail, "ps1");
        assert_eq!(
            kinds(link("https://example.com/tools/get.msi", "")),
            [WarningKind::DangerousType],
            "không có tên ⇒ lấy từ link"
        );
    }

    #[test]
    fn spoofed_and_hidden_names_are_flagged_with_the_rename_rules() {
        let spoofed = warnings(&link("https://example.com/a", "hoa-don.PDF.exe"));
        assert_eq!(spoofed[0].kind, WarningKind::SpoofedName);
        assert_eq!(spoofed[0].detail, "pdf.exe");
        assert_eq!(spoofed[1].kind, WarningKind::DangerousType);
        assert!(spoofed_name("tai lieu      .exe"));
        assert!(spoofed_name("anh.JPG .scr"));
        assert!(!spoofed_name("setup.v1.2.exe"));
        assert!(!spoofed_name("ghi chu.pdf.txt"), "đuôi thật vô hại");

        let flipped = warnings(&link("https://example.com/a", "phim\u{202E}4pm.exe"));
        assert_eq!(flipped[0].kind, WarningKind::HiddenChars);
        assert_eq!(flipped[0].detail, "U+202E");
        let encoded = kinds(link("https://example.com/hoa-don%E2%80%AEfdp.exe", ""));
        assert_eq!(encoded[0], WarningKind::HiddenChars, "%E2%80%AE trong link");
    }

    #[test]
    fn content_type_must_agree_with_the_name() {
        let disguised = LinkFacts {
            content_type: Some("application/x-msdownload; charset=binary"),
            ..link("https://example.com/a", "invoice.pdf")
        };
        let found = warnings(&disguised);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, WarningKind::MimeMismatch);
        assert_eq!(found[0].detail, "application/x-msdownload");

        let nameless = LinkFacts {
            content_type: Some("application/vnd.microsoft.portable-executable"),
            ..link("https://example.com/download", "download")
        };
        assert_eq!(kinds(nameless), [WarningKind::MimeMismatch]);

        let wrong_family = LinkFacts {
            content_type: Some("video/mp4"),
            ..link("https://example.com/a", "contract.pdf")
        };
        assert_eq!(kinds(wrong_family), [WarningKind::MimeMismatch]);

        let honest = LinkFacts {
            content_type: Some("application/x-msdownload"),
            ..link("https://example.com/a", "tool.exe")
        };
        assert_eq!(kinds(honest), [WarningKind::DangerousType], "đuôi đúng thì chỉ cảnh báo loại");
        let page = LinkFacts {
            content_type: Some("text/html"),
            ..link("https://example.com/a", "file.zip")
        };
        assert!(kinds(page).is_empty(), "trang HTML do engine chặn (AC-06)");
    }

    #[test]
    fn plain_http_is_judged_by_where_the_bytes_come_from() {
        let found = warnings(&link("http://mirror.example.org/pub/a.iso", ""));
        assert_eq!(found[0].kind, WarningKind::InsecureHttp);
        assert_eq!(found[0].detail, "mirror.example.org");
        let upgraded = LinkFacts {
            final_url: Some("https://mirror.example.org/pub/a.iso"),
            ..link("http://mirror.example.org/pub/a.iso", "a.iso")
        };
        assert!(kinds(upgraded).is_empty(), "chuyển hướng sang https");
        let downgraded = LinkFacts {
            final_url: Some("http://cdn.example.org/a.iso"),
            ..link("https://example.org/a.iso", "a.iso")
        };
        assert_eq!(kinds(downgraded), [WarningKind::InsecureHttp]);
        for local in ["http://localhost:8080/a.zip", "http://127.0.0.1/a.zip", "http://[::1]/a.zip"] {
            assert!(kinds(link(local, "a.zip")).is_empty(), "{local}");
        }
    }

    #[test]
    fn serializes_kinds_for_the_interface() {
        let json = serde_json::to_string(&warnings(&link("http://example.com/a.exe", ""))).unwrap();
        assert_eq!(
            json,
            r#"[{"kind":"dangerous_type","detail":"exe"},{"kind":"insecure_http","detail":"example.com"}]"#
        );
    }
}
