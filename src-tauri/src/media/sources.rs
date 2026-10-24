//! Nguồn tải công cụ media theo nền tảng, dùng chung cho Windows và macOS. Chỉ nguồn chính chủ có
//! SHA-256 công bố: GitHub yt-dlp/yt-dlp, yt-dlp/FFmpeg-Builds, denoland/deno; ffmpeg cho macOS lấy
//! từ ffmpeg.martin-riedl.de (bản build có ký, SHA-256 nằm cạnh từng file). Chi tiết và lý do chọn:
//! `docs/specs/2026-10-23-utd-v1/notes/auto-tools.md`.

use serde::{Deserialize, Serialize};
use utd_engine::fetch::FetchPolicy;

use crate::media::verify;

const MIB: u64 = 1024 * 1024;
/// GitHub chuyển `releases/.../download` sang CDN đã ký; `objects.` là CDN cũ.
const GITHUB_HOSTS: &[&str] = &[
    "github.com",
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
];
const RIEDL_HOST: &str = "ffmpeg.martin-riedl.de";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tool {
    #[serde(rename = "yt-dlp")]
    YtDlp,
    #[serde(rename = "deno")]
    Deno,
    #[serde(rename = "ffmpeg")]
    Ffmpeg,
}

impl Tool {
    /// Thứ tự tải lần đầu: yt-dlp nhỏ và cần trước nhất, deno cho YouTube, ffmpeg lớn nhất để cuối.
    pub const ALL: [Tool; 3] = [Tool::YtDlp, Tool::Deno, Tool::Ffmpeg];

    pub fn name(self) -> &'static str {
        match self {
            Self::YtDlp => "yt-dlp",
            Self::Deno => "deno",
            Self::Ffmpeg => "ffmpeg",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    WindowsX64,
    WindowsArm64,
    MacArm64,
    MacX64,
}

impl Platform {
    /// Nền tảng của bản UTD đang chạy; `None` = chưa có bảng nguồn (Linux…), không tự tải.
    pub fn current() -> Option<Self> {
        if cfg!(all(windows, target_arch = "x86_64")) {
            Some(Self::WindowsX64)
        } else if cfg!(all(windows, target_arch = "aarch64")) {
            Some(Self::WindowsArm64)
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Some(Self::MacArm64)
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            Some(Self::MacX64)
        } else {
            None
        }
    }

    fn windows(self) -> bool {
        matches!(self, Self::WindowsX64 | Self::WindowsArm64)
    }

    /// Tên file chạy được: `ffmpeg.exe` trên Windows, `ffmpeg` trên macOS.
    pub fn exe(self, stem: &str) -> String {
        if self.windows() {
            format!("{stem}.exe")
        } else {
            stem.to_string()
        }
    }
}

/// Cách lấy SHA-256 của một file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sums {
    /// Danh sách `<sha256>  <tên>` của cả bản phát hành (SHA2-256SUMS, checksums.sha256).
    List { url: String, name: String },
    /// File SHA-256 riêng của một file (deno: dạng sha256sum hoặc Get-FileHash của PowerShell).
    Sidecar { url: String, name: String },
    /// Link của file là link "mới nhất" chuyển hướng; SHA-256 ở `<link đích>.sha256`.
    Beside,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pack {
    /// Tải về là chạy được luôn (đổi tên thành `binaries[0]`).
    Binary,
    Zip,
}

#[derive(Debug, Clone)]
pub struct Asset {
    /// Link bản mới nhất.
    pub url: String,
    /// Link cùng bản với file checksum: `{tag}` thay bằng tag đọc từ chuyển hướng của link checksum,
    /// nên `latest` vừa đổi giữa hai lần tải cũng không lệch.
    pub pinned: Option<String>,
    pub sums: Sums,
    pub pack: Pack,
}

/// Một file chạy được trong gói và cách chạy thử trước khi dùng.
#[derive(Debug, Clone)]
pub struct Binary {
    pub name: String,
    pub flag: &'static str,
    /// Dòng đầu của `<binary> <flag>` hợp lệ không.
    pub accept: fn(&str) -> bool,
}

#[derive(Debug, Clone)]
pub struct Release {
    pub tool: Tool,
    pub assets: Vec<Asset>,
    pub binaries: Vec<Binary>,
    /// File giấy phép trong gói, chép cạnh binary thành `<tool>-LICENSE.txt`.
    pub license: Option<&'static str>,
    pub hosts: Vec<String>,
    pub max_bytes: u64,
    pub need_free: u64,
}

impl Release {
    pub fn policy(&self) -> FetchPolicy {
        let hosts: Vec<&str> = self.hosts.iter().map(String::as_str).collect();
        FetchPolicy::https_only(&hosts)
    }
}

pub fn release(tool: Tool, platform: Platform) -> Release {
    match tool {
        Tool::YtDlp => yt_dlp(platform),
        Tool::Deno => deno(platform),
        Tool::Ffmpeg if platform.windows() => ffmpeg_windows(platform),
        Tool::Ffmpeg => ffmpeg_macos(platform),
    }
}

fn github_hosts() -> Vec<String> {
    GITHUB_HOSTS.iter().map(|host| host.to_string()).collect()
}

fn yt_dlp(platform: Platform) -> Release {
    let asset = match platform {
        Platform::WindowsX64 => "yt-dlp.exe",
        Platform::WindowsArm64 => "yt-dlp_arm64.exe",
        // Bản universal2: chạy được cả Apple Silicon lẫn Intel.
        Platform::MacArm64 | Platform::MacX64 => "yt-dlp_macos",
    };
    let base = "https://github.com/yt-dlp/yt-dlp/releases";
    Release {
        tool: Tool::YtDlp,
        assets: vec![Asset {
            url: format!("{base}/latest/download/{asset}"),
            pinned: Some(format!("{base}/download/{{tag}}/{asset}")),
            sums: Sums::List {
                url: format!("{base}/latest/download/SHA2-256SUMS"),
                name: asset.into(),
            },
            pack: Pack::Binary,
        }],
        binaries: vec![Binary {
            name: platform.exe("yt-dlp"),
            flag: "--version",
            accept: verify::is_date_version,
        }],
        license: None,
        hosts: github_hosts(),
        max_bytes: 96 * MIB,
        need_free: 300 * MIB,
    }
}

fn deno(platform: Platform) -> Release {
    let triple = match platform {
        Platform::WindowsX64 => "x86_64-pc-windows-msvc",
        Platform::WindowsArm64 => "aarch64-pc-windows-msvc",
        Platform::MacArm64 => "aarch64-apple-darwin",
        Platform::MacX64 => "x86_64-apple-darwin",
    };
    let zip = format!("deno-{triple}.zip");
    let base = "https://github.com/denoland/deno/releases";
    Release {
        tool: Tool::Deno,
        assets: vec![Asset {
            url: format!("{base}/latest/download/{zip}"),
            pinned: Some(format!("{base}/download/{{tag}}/{zip}")),
            sums: Sums::Sidecar {
                url: format!("{base}/latest/download/{zip}.sha256sum"),
                name: zip,
            },
            pack: Pack::Zip,
        }],
        binaries: vec![Binary {
            name: platform.exe("deno"),
            flag: "--version",
            accept: |line| line.starts_with("deno "),
        }],
        license: None,
        hosts: github_hosts(),
        max_bytes: 160 * MIB,
        need_free: 700 * MIB,
    }
}

fn ffmpeg_binaries(platform: Platform) -> Vec<Binary> {
    vec![
        Binary {
            name: platform.exe("ffmpeg"),
            flag: "-version",
            accept: |line| line.starts_with("ffmpeg version"),
        },
        Binary {
            name: platform.exe("ffprobe"),
            flag: "-version",
            accept: |line| line.starts_with("ffprobe version"),
        },
    ]
}

/// Bản `latest` của FFmpeg-Builds dựng lại mỗi ngày, cùng tag `latest` (không ghim được).
fn ffmpeg_windows(platform: Platform) -> Release {
    let arch = if platform == Platform::WindowsArm64 {
        "winarm64"
    } else {
        "win64"
    };
    let zip = format!("ffmpeg-master-latest-{arch}-gpl.zip");
    let base = "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest";
    Release {
        tool: Tool::Ffmpeg,
        assets: vec![Asset {
            url: format!("{base}/{zip}"),
            pinned: None,
            sums: Sums::List {
                url: format!("{base}/checksums.sha256"),
                name: zip,
            },
            pack: Pack::Zip,
        }],
        binaries: ffmpeg_binaries(platform),
        license: Some("LICENSE.txt"),
        hosts: github_hosts(),
        max_bytes: 400 * MIB,
        need_free: 1200 * MIB,
    }
}

/// yt-dlp/FFmpeg-Builds không có macOS: dùng bản release (không phải snapshot) của Martin Riedl,
/// ffmpeg và ffprobe là hai gói riêng.
fn ffmpeg_macos(platform: Platform) -> Release {
    let arch = if platform == Platform::MacArm64 {
        "arm64"
    } else {
        "amd64"
    };
    let asset = |name: &str| Asset {
        url: format!("https://{RIEDL_HOST}/redirect/latest/macos/{arch}/release/{name}.zip"),
        pinned: None,
        sums: Sums::Beside,
        pack: Pack::Zip,
    };
    Release {
        tool: Tool::Ffmpeg,
        assets: vec![asset("ffmpeg"), asset("ffprobe")],
        binaries: ffmpeg_binaries(platform),
        license: None,
        hosts: vec![RIEDL_HOST.to_string()],
        max_bytes: 200 * MIB,
        need_free: 800 * MIB,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLATFORMS: [Platform; 4] = [
        Platform::WindowsX64,
        Platform::WindowsArm64,
        Platform::MacArm64,
        Platform::MacX64,
    ];

    #[test]
    fn every_platform_has_every_tool_from_allowed_https_hosts() {
        for platform in PLATFORMS {
            for tool in Tool::ALL {
                let release = release(tool, platform);
                assert_eq!(release.tool, tool);
                assert!(!release.assets.is_empty() && !release.binaries.is_empty());
                let policy = release.policy();
                for asset in &release.assets {
                    let mut links = vec![asset.url.clone()];
                    links.extend(asset.pinned.as_ref().map(|pin| pin.replace("{tag}", "v1")));
                    if let Sums::List { url, .. } | Sums::Sidecar { url, .. } = &asset.sums {
                        links.push(url.clone());
                    }
                    for link in links {
                        let url = utd_engine::fetch::Url::parse(&link).expect("url");
                        assert!(policy.check(&url).is_ok(), "{link}");
                    }
                }
            }
        }
    }

    #[test]
    fn names_follow_the_platform() {
        let windows = release(Tool::YtDlp, Platform::WindowsArm64);
        assert!(windows.assets[0].url.ends_with("/yt-dlp_arm64.exe"));
        assert_eq!(windows.binaries[0].name, "yt-dlp.exe");
        let mac = release(Tool::YtDlp, Platform::MacX64);
        assert!(mac.assets[0].url.ends_with("/yt-dlp_macos"));
        assert_eq!(mac.binaries[0].name, "yt-dlp");

        let deno = release(Tool::Deno, Platform::MacArm64);
        assert_eq!(
            deno.assets[0].sums,
            Sums::Sidecar {
                url: "https://github.com/denoland/deno/releases/latest/download/deno-aarch64-apple-darwin.zip.sha256sum".into(),
                name: "deno-aarch64-apple-darwin.zip".into(),
            }
        );
        assert_eq!(
            deno.assets[0].pinned.as_deref(),
            Some(
                "https://github.com/denoland/deno/releases/download/{tag}/deno-aarch64-apple-darwin.zip"
            )
        );

        let ffmpeg = release(Tool::Ffmpeg, Platform::WindowsArm64);
        assert!(
            ffmpeg.assets[0]
                .url
                .ends_with("ffmpeg-master-latest-winarm64-gpl.zip")
        );
        let names: Vec<_> = ffmpeg
            .binaries
            .iter()
            .map(|binary| binary.name.as_str())
            .collect();
        assert_eq!(names, ["ffmpeg.exe", "ffprobe.exe"]);

        let mac = release(Tool::Ffmpeg, Platform::MacArm64);
        assert_eq!(mac.assets.len(), 2);
        assert_eq!(
            mac.assets[1].url,
            "https://ffmpeg.martin-riedl.de/redirect/latest/macos/arm64/release/ffprobe.zip"
        );
        assert_eq!(mac.assets[0].sums, Sums::Beside);
        assert_eq!(mac.binaries[0].name, "ffmpeg");
    }

    #[test]
    fn binary_checks_accept_only_real_version_lines() {
        let deno = release(Tool::Deno, Platform::WindowsX64);
        assert!((deno.binaries[0].accept)(
            "deno 2.9.8 (stable, release, x86_64-pc-windows-msvc)"
        ));
        assert!(!(deno.binaries[0].accept)("node v22"));
        let ffmpeg = release(Tool::Ffmpeg, Platform::MacX64);
        assert!((ffmpeg.binaries[1].accept)(
            "ffprobe version 9.0.2-https://www.martin-riedl.de"
        ));
        assert!(!(ffmpeg.binaries[1].accept)("ffmpeg version 9.0.2"));
        assert_eq!(Tool::parse("deno"), Some(Tool::Deno));
        assert_eq!(Tool::parse("node"), None);
        assert_eq!(
            Platform::current().is_some(),
            cfg!(any(windows, target_os = "macos")),
            "Windows/macOS phải có bảng nguồn"
        );
    }
}
