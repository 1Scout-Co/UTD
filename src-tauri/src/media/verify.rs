//! Đọc file SHA-256, tag phát hành và số phiên bản công cụ: hàm thuần, không chạm mạng hay đĩa.

use utd_engine::fetch::Url;

use crate::media::sources::Tool;

/// Dòng `<sha256>  <tên>` hoặc `<sha256> *<tên>`; tên phải khớp đúng (không khớp tiền tố).
/// Có hai dòng cùng tên mà khác hash ⇒ không tin.
pub(crate) fn checksum_for(text: &str, asset: &str) -> Option<String> {
    let mut found: Option<String> = None;
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        let Some((hash, name)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let name = name.trim_start();
        let name = name.strip_prefix('*').unwrap_or(name);
        if name != asset {
            continue;
        }
        let hash = hex64(hash)?;
        if found.as_ref().is_some_and(|previous| *previous != hash) {
            return None;
        }
        found = Some(hash);
    }
    found
}

fn hex64(text: &str) -> Option<String> {
    (text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| text.to_ascii_lowercase())
}

/// File SHA-256 riêng của một file: dạng sha256sum (`<hash>  <tên>`) hoặc kết quả `Get-FileHash` của
/// PowerShell (deno cho Windows: dòng `Algorithm : SHA256`, `Hash : <HEX>`, `Path : …\<tên>`).
pub(crate) fn sidecar_hash(text: &str, asset: &str) -> Option<String> {
    if let Some(hash) = checksum_for(text, asset) {
        return Some(hash);
    }
    let (mut hash, mut sha256, mut named) = (None, false, false);
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Algorithm" => sha256 = value.eq_ignore_ascii_case("SHA256"),
            "Hash" => hash = hex64(value),
            "Path" => named = value.rsplit(['\\', '/']).next() == Some(asset),
            _ => {}
        }
    }
    hash.filter(|_| sha256 && named)
}

/// Tag thật của bản `latest`, đọc từ bước chuyển hướng `<máy chủ>/<chủ>/<repo>/releases/download/<tag>/<tên>`
/// (luật host của lần tải đã chặn máy chủ lạ).
pub(crate) fn release_tag(redirects: &[Url]) -> Option<String> {
    redirects.iter().find_map(|url| {
        let segments: Vec<&str> = url.path_segments()?.collect();
        match segments.as_slice() {
            [_, _, "releases", "download", tag, _] if *tag != "latest" && !tag.is_empty() => {
                Some(tag.to_string())
            }
            _ => None,
        }
    })
}

/// Phiên bản yt-dlp dạng `2026.10.21` (có thể thêm `.123456` ở bản nightly).
pub(crate) fn is_date_version(version: &str) -> bool {
    let bytes = version.as_bytes();
    bytes.len() >= 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'.'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'.'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
}

/// `2.9.8`, `v2.9.8`, `deno 2.9.8 (stable, …)` → (2, 9, 8).
pub(crate) fn semver(text: &str) -> Option<(u64, u64, u64)> {
    let text = text.trim();
    let text = text.strip_prefix("deno ").unwrap_or(text);
    let text = text.strip_prefix('v').unwrap_or(text);
    let core = text.split(|character: char| !(character.is_ascii_digit() || character == '.'));
    let mut parts = core.into_iter().next()?.split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    Some((next()?, next()?, next().unwrap_or(0)))
}

/// Bản đang dùng đã bằng hoặc mới hơn tag mới nhất ⇒ khỏi tải. Không đọc được số nào ⇒ tải.
pub(crate) fn is_current(tool: Tool, current: &str, tag: &str) -> bool {
    match tool {
        Tool::YtDlp => is_date_version(current) && is_date_version(tag) && current >= tag,
        Tool::Deno => {
            matches!((semver(current), semver(tag)), (Some(have), Some(latest)) if have >= latest)
        }
        Tool::Ffmpeg => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "33121d192d706e62f684a06f80918e3f3a8ba52ac3a65213b6951c39c7d9f555";
    const B: &str = "7ce38c77db13db50c64e656b1056a0448e6c38ec0f0ccf28e30dd5271c008850";

    #[test]
    fn checksum_matches_exact_names_only() {
        let sums =
            format!("{B}  yt-dlp_x86.exe\n{A}  yt-dlp.exe\n{B}  yt-dlp.exe.sig\n{B}  yt-dlp\n");
        assert_eq!(checksum_for(&sums, "yt-dlp.exe").as_deref(), Some(A));
        assert_eq!(checksum_for(&sums, "yt-dlp.ex"), None);
        assert_eq!(checksum_for(&sums, "yt-dlp").as_deref(), Some(B));

        let ffmpeg = format!(
            "{B}  ffmpeg-master-latest-win64-gpl-shared.zip\r\n{}  ffmpeg-master-latest-win64-gpl.zip\r\n",
            A.to_ascii_uppercase()
        );
        assert_eq!(
            checksum_for(&ffmpeg, "ffmpeg-master-latest-win64-gpl.zip").as_deref(),
            Some(A)
        );
    }

    #[test]
    fn checksum_accepts_binary_marker_and_rejects_bad_lines() {
        assert_eq!(
            checksum_for(&format!("\u{feff}{A} *yt-dlp.exe\n"), "yt-dlp.exe").as_deref(),
            Some(A)
        );
        assert_eq!(
            checksum_for(&format!("{A}\tyt-dlp.exe"), "yt-dlp.exe").as_deref(),
            Some(A)
        );
        assert_eq!(checksum_for("abc123  yt-dlp.exe\n", "yt-dlp.exe"), None);
        assert_eq!(
            checksum_for(&format!("{}  yt-dlp.exe", "z".repeat(64)), "yt-dlp.exe"),
            None
        );
        assert_eq!(checksum_for("", "yt-dlp.exe"), None);
        // Hai hash khác nhau cho cùng một tên: không tin.
        assert_eq!(
            checksum_for(&format!("{A}  yt-dlp.exe\n{B}  yt-dlp.exe\n"), "yt-dlp.exe"),
            None
        );
        assert_eq!(
            checksum_for(&format!("{A}  yt-dlp.exe\n{A}  yt-dlp.exe\n"), "yt-dlp.exe").as_deref(),
            Some(A)
        );
    }

    #[test]
    fn reads_both_deno_sidecar_formats() {
        // Đúng như file thật của deno v2.9.8 (Windows: Get-FileHash, CRLF; macOS: sha256sum).
        let windows = format!(
            "\r\nAlgorithm : SHA256\r\nHash      : {}\r\nPath      : C:\\a\\deno\\deno\\target\\release\\deno-x86_64-pc-windows-msvc.zip\r\n\r\n",
            A.to_ascii_uppercase()
        );
        let name = "deno-x86_64-pc-windows-msvc.zip";
        assert_eq!(sidecar_hash(&windows, name).as_deref(), Some(A));
        assert_eq!(
            sidecar_hash(&windows, "deno-aarch64-pc-windows-msvc.zip"),
            None
        );
        let md5 = windows.replace("SHA256", "MD5");
        assert_eq!(sidecar_hash(&md5, name), None);
        let mac = format!("{B}  deno-aarch64-apple-darwin.zip\n");
        assert_eq!(
            sidecar_hash(&mac, "deno-aarch64-apple-darwin.zip").as_deref(),
            Some(B)
        );
        assert_eq!(
            sidecar_hash("Hash : xyz\nPath : x\\deno.zip", "deno.zip"),
            None
        );
    }

    #[test]
    fn reads_release_tag_from_redirects() {
        let hops = [
            Url::parse(
                "https://github.com/yt-dlp/yt-dlp/releases/download/2026.10.21/SHA2-256SUMS",
            )
            .expect("url"),
            Url::parse(
                "https://release-assets.githubusercontent.com/github-production-release-asset/1/x",
            )
            .expect("url"),
        ];
        assert_eq!(release_tag(&hops).as_deref(), Some("2026.10.21"));
        let latest = [Url::parse(
            "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/checksums.sha256",
        )
        .expect("url")];
        assert_eq!(release_tag(&latest), None);
        assert_eq!(release_tag(&[]), None);
    }

    #[test]
    fn compares_versions_per_tool() {
        assert!(is_date_version("2026.10.21"));
        assert!(is_date_version("2026.10.22.123456"));
        assert!(!is_date_version("không rõ"));
        assert!(!is_date_version("8.1.2"));
        assert!(is_current(Tool::YtDlp, "2026.10.21", "2026.10.21"));
        assert!(is_current(Tool::YtDlp, "2026.10.21.123456", "2026.10.21"));
        assert!(!is_current(Tool::YtDlp, "2026.08.19", "2026.10.21"));
        assert!(!is_current(Tool::YtDlp, "không rõ", "2026.10.21"));

        assert_eq!(semver("v2.9.8"), Some((2, 9, 8)));
        assert_eq!(semver("deno 2.10.0 (stable, release)"), Some((2, 10, 0)));
        assert_eq!(semver("2.1"), Some((2, 1, 0)));
        assert_eq!(semver("abc"), None);
        assert!(
            is_current(Tool::Deno, "2.10.0", "v2.9.8"),
            "2.10 > 2.9 theo số"
        );
        assert!(!is_current(Tool::Deno, "2.9.7", "v2.9.8"));
        assert!(!is_current(Tool::Ffmpeg, "8.1", "latest"));
    }
}
