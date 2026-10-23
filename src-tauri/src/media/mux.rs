//! Ghép/chuyển các luồng đã tải bằng ffmpeg. Luôn ghi ra file tạm của UTD rồi đổi tên không ghi
//! đè, nên không bao giờ đè file có sẵn của người dùng.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use crate::media::resolve::{AudioFormat, Layout};
use crate::media::tokio_command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Container {
    Mp4,
    Mkv,
    Webm,
    Mov,
    M4a,
    Mp3,
}

impl Container {
    pub fn ext(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
            Self::Mov => "mov",
            Self::M4a => "m4a",
            Self::Mp3 => "mp3",
        }
    }

    /// Tên muxer của ffmpeg: file tạm không có đuôi media nên phải nói rõ.
    fn muxer(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "matroska",
            Self::Webm => "webm",
            Self::Mov => "mov",
            Self::M4a => "ipod",
            Self::Mp3 => "mp3",
        }
    }

    fn from_ext(ext: &str) -> Option<Self> {
        match ext {
            "mp4" | "m4v" => Some(Self::Mp4),
            "mkv" => Some(Self::Mkv),
            "webm" => Some(Self::Webm),
            "mov" => Some(Self::Mov),
            "m4a" => Some(Self::M4a),
            "mp3" => Some(Self::Mp3),
            _ => None,
        }
    }
}

/// Một cách chạy ffmpeg; thử lần lượt, cách sau là đường lui của cách trước.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Recipe {
    /// Hình đầu vào 0 + tiếng đầu vào 1, chép nguyên luồng.
    Merge(Container),
    /// Một file: sửa vỏ (DASH) bằng cách chép nguyên luồng.
    Remux(Container),
    /// Lấy tiếng, chép nguyên (đã là AAC/MP3).
    AudioCopy(Container),
    AacEncode,
    Mp3Encode,
}

impl Recipe {
    pub fn output(self) -> Container {
        match self {
            Self::Merge(container) | Self::Remux(container) | Self::AudioCopy(container) => {
                container
            }
            Self::AacEncode => Container::M4a,
            Self::Mp3Encode => Container::Mp3,
        }
    }
}

/// Codec của một đầu vào (chữ thường; rỗng = không rõ) và đuôi file đã tải.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Hint {
    pub vcodec: String,
    pub acodec: String,
    pub ext: String,
    /// Vỏ cần ffmpeg sửa: luồng DASH, hoặc không rõ (lượt tiếp tục). File thường đã phát được,
    /// chép lại cả file chỉ tốn thời gian.
    pub remux: bool,
}

/// Các cách thử theo thứ tự. Rỗng ⇒ không cần ffmpeg, đổi tên luồng là xong.
pub(super) fn recipes(layout: Layout, hints: &[Hint]) -> Vec<Recipe> {
    let first = hints.first().cloned().unwrap_or_default();
    match layout {
        Layout::Merge => {
            let audio = hints.get(1).cloned().unwrap_or_default();
            if mp4_video(&first.vcodec) && mp4_audio(&audio.acodec) {
                vec![Recipe::Merge(Container::Mp4), Recipe::Merge(Container::Mkv)]
            } else {
                vec![Recipe::Merge(Container::Mkv)]
            }
        }
        Layout::Single => Container::from_ext(&first.ext)
            .filter(|_| first.remux)
            .map(|container| vec![Recipe::Remux(container)])
            .unwrap_or_default(),
        Layout::Audio(AudioFormat::M4a) => {
            if is_aac(&first) {
                vec![Recipe::AudioCopy(Container::M4a), Recipe::AacEncode]
            } else {
                vec![Recipe::AacEncode]
            }
        }
        Layout::Audio(AudioFormat::Mp3) => {
            if first.acodec.starts_with("mp3") || (first.acodec.is_empty() && first.ext == "mp3") {
                vec![Recipe::AudioCopy(Container::Mp3), Recipe::Mp3Encode]
            } else {
                vec![Recipe::Mp3Encode]
            }
        }
    }
}

fn is_aac(hint: &Hint) -> bool {
    if hint.acodec.is_empty() {
        return matches!(hint.ext.as_str(), "m4a" | "mp4" | "aac");
    }
    hint.acodec.starts_with("mp4a") || hint.acodec.starts_with("aac")
}

/// Codec hình MP4 chứa được; không rõ thì cứ thử MP4 (hỏng thì lui về MKV).
fn mp4_video(codec: &str) -> bool {
    codec.is_empty()
        || codec == "none"
        || [
            "avc", "h264", "hev", "hvc", "h265", "av01", "vp09", "vp9", "mp4v", "dvh", "dva",
        ]
        .iter()
        .any(|prefix| codec.starts_with(prefix))
}

fn mp4_audio(codec: &str) -> bool {
    codec.is_empty()
        || codec == "none"
        || [
            "mp4a", "aac", "opus", "mp3", "ac-3", "ac3", "ec-3", "eac3", "flac", "alac",
        ]
        .iter()
        .any(|prefix| codec.starts_with(prefix))
}

/// Tham số ffmpeg: `inputs` theo thứ tự của `Layout`, ghi ra `output` (file tạm của UTD).
pub(super) fn ffmpeg_args(recipe: Recipe, inputs: &[PathBuf], output: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-loglevel", "error", "-y"]
        .into_iter()
        .map(OsString::from)
        .collect();
    for input in inputs {
        args.push("-i".into());
        args.push(input.as_os_str().to_os_string());
    }
    let tail: &[&str] = match recipe {
        Recipe::Merge(_) => &["-map", "0:v:0", "-map", "1:a:0", "-c", "copy"],
        Recipe::Remux(_) => &["-map", "0:v?", "-map", "0:a?", "-c", "copy"],
        Recipe::AudioCopy(_) => &["-vn", "-map", "0:a:0", "-c:a", "copy"],
        Recipe::AacEncode => &["-vn", "-map", "0:a:0", "-c:a", "aac", "-b:a", "192k"],
        Recipe::Mp3Encode => &["-vn", "-map", "0:a:0", "-c:a", "libmp3lame", "-q:a", "2"],
    };
    args.extend(tail.iter().map(OsString::from));
    // Không `-movflags +faststart`: ffmpeg phải ghi lại cả file lần nữa (chậm với file lớn),
    // phát trên máy không cần; yt-dlp cũng không bật khi ghép.
    args.push("-f".into());
    args.push(recipe.output().muxer().into());
    args.push(output.as_os_str().to_os_string());
    args
}

pub(super) enum MuxRun {
    Done,
    Cancelled,
    /// Đuôi lỗi của ffmpeg (chỉ để ghi log).
    Failed(String),
}

pub(super) async fn run_ffmpeg(
    ffmpeg: &Path,
    args: &[OsString],
    cancel: &CancellationToken,
) -> MuxRun {
    let mut command = tokio_command(ffmpeg);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return MuxRun::Failed(error.to_string()),
    };
    let mut stderr = child.stderr.take();
    let reader = tokio::spawn(async move {
        let mut text = Vec::new();
        if let Some(stream) = stderr.as_mut() {
            let _ = stream.take(64 * 1024).read_to_end(&mut text).await;
        }
        String::from_utf8_lossy(&text).into_owned()
    });
    let status = tokio::select! {
        status = child.wait() => status,
        _ = cancel.cancelled() => {
            // Chờ ffmpeg thoát hẳn để còn xoá được file tạm.
            let _ = child.kill().await;
            reader.abort();
            return MuxRun::Cancelled;
        }
    };
    let tail = reader.await.unwrap_or_default();
    match status {
        Ok(status) if status.success() => MuxRun::Done,
        Ok(status) => MuxRun::Failed(format!("{status}: {}", last_lines(&tail))),
        Err(error) => MuxRun::Failed(error.to_string()),
    }
}

fn last_lines(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    lines[lines.len().saturating_sub(3)..].join(" | ")
}

/// Đổi tên không ghi đè. Ổ không có hard link (FAT32/exFAT, vài ổ mạng): kiểm rồi đổi tên.
pub(super) fn move_no_clobber(from: &Path, to: &Path) -> io::Result<()> {
    match std::fs::hard_link(from, to) {
        Ok(()) => {
            if let Err(error) = std::fs::remove_file(from) {
                let _ = std::fs::remove_file(to);
                return Err(error);
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(error),
        Err(_) if std::fs::symlink_metadata(to).is_ok() => {
            Err(io::Error::from(io::ErrorKind::AlreadyExists))
        }
        Err(_) => std::fs::rename(from, to),
    }
}

/// `<base>.<ext>`, đã có thì `<base> (1).<ext>`… Không bao giờ trả tên đang có file.
pub(super) fn free_target(directory: &Path, base: &str, ext: &str) -> Option<PathBuf> {
    (0..100).find_map(|counter| {
        let name = if counter == 0 {
            format!("{base}.{ext}")
        } else {
            format!("{base} ({counter}).{ext}")
        };
        let candidate = directory.join(&name);
        (utd_engine::validate_file_name(&name).is_ok()
            && std::fs::symlink_metadata(&candidate).is_err())
        .then_some(candidate)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint(vcodec: &str, acodec: &str, ext: &str) -> Hint {
        Hint {
            vcodec: vcodec.into(),
            acodec: acodec.into(),
            ext: ext.into(),
            remux: true,
        }
    }

    fn text(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn merges_video_and_audio_into_mp4_by_copying() {
        let hints = [
            hint("av01.0.09m.08", "none", "mp4"),
            hint("none", "opus", "webm"),
        ];
        let plan = recipes(Layout::Merge, &hints);
        assert_eq!(
            plan,
            vec![Recipe::Merge(Container::Mp4), Recipe::Merge(Container::Mkv)]
        );
        let inputs = [PathBuf::from("v.f399.mp4"), PathBuf::from("a.f251.webm")];
        let args = text(&ffmpeg_args(plan[0], &inputs, Path::new("out.part")));
        assert_eq!(
            &args[..5],
            ["-hide_banner", "-nostdin", "-loglevel", "error", "-y"]
        );
        let joined = args.join(" ");
        assert!(joined.contains("-i v.f399.mp4 -i a.f251.webm"));
        assert!(joined.contains("-map 0:v:0 -map 1:a:0 -c copy"));
        assert!(joined.ends_with("-map 0:v:0 -map 1:a:0 -c copy -f mp4 out.part"));
    }

    #[test]
    fn falls_back_to_mkv_when_mp4_cannot_hold_the_codecs() {
        let vp8 = [hint("vp8", "none", "webm"), hint("none", "vorbis", "webm")];
        assert_eq!(
            recipes(Layout::Merge, &vp8),
            vec![Recipe::Merge(Container::Mkv)]
        );
        let args = text(&ffmpeg_args(
            Recipe::Merge(Container::Mkv),
            &[PathBuf::from("v"), PathBuf::from("a")],
            Path::new("out.part"),
        ));
        let joined = args.join(" ");
        assert!(joined.ends_with("-f matroska out.part"));
        assert!(!joined.contains("faststart"));
        let unknown = [hint("", "", "mp4"), hint("", "", "webm")];
        assert_eq!(
            recipes(Layout::Merge, &unknown)[0],
            Recipe::Merge(Container::Mp4)
        );
        assert_eq!(Recipe::Merge(Container::Mkv).output().ext(), "mkv");
    }

    #[test]
    fn copies_aac_into_m4a_and_encodes_anything_else() {
        let aac = [hint("none", "mp4a.40.2", "m4a")];
        let plan = recipes(Layout::Audio(AudioFormat::M4a), &aac);
        assert_eq!(
            plan,
            vec![Recipe::AudioCopy(Container::M4a), Recipe::AacEncode]
        );
        let copy = text(&ffmpeg_args(plan[0], &[PathBuf::from("a")], Path::new("o"))).join(" ");
        assert!(copy.ends_with("-vn -map 0:a:0 -c:a copy -f ipod o"));
        let encode = text(&ffmpeg_args(plan[1], &[PathBuf::from("a")], Path::new("o"))).join(" ");
        assert!(encode.contains("-c:a aac -b:a 192k"));
        assert!(encode.ends_with("-f ipod o"));

        let opus = [hint("none", "opus", "webm")];
        assert_eq!(
            recipes(Layout::Audio(AudioFormat::M4a), &opus),
            vec![Recipe::AacEncode]
        );
        let saved = [hint("", "", "m4a")];
        assert_eq!(
            recipes(Layout::Audio(AudioFormat::M4a), &saved)[0],
            Recipe::AudioCopy(Container::M4a),
            "không rõ codec mà đuôi m4a ⇒ thử chép trước"
        );
    }

    #[test]
    fn encodes_mp3_with_lame_quality_two() {
        let opus = [hint("none", "opus", "webm")];
        let plan = recipes(Layout::Audio(AudioFormat::Mp3), &opus);
        assert_eq!(plan, vec![Recipe::Mp3Encode]);
        let args = text(&ffmpeg_args(plan[0], &[PathBuf::from("a")], Path::new("o"))).join(" ");
        assert!(args.contains("-vn -map 0:a:0 -c:a libmp3lame -q:a 2 -f mp3 o"));
        let mp3 = [hint("none", "mp3", "mp3")];
        assert_eq!(
            recipes(Layout::Audio(AudioFormat::Mp3), &mp3)[0],
            Recipe::AudioCopy(Container::Mp3)
        );
    }

    #[test]
    fn remuxes_a_single_stream_in_its_own_container() {
        assert_eq!(
            recipes(Layout::Single, &[hint("avc1", "mp4a", "mp4")]),
            vec![Recipe::Remux(Container::Mp4)]
        );
        assert_eq!(
            recipes(Layout::Single, &[hint("vp9", "opus", "webm")]),
            vec![Recipe::Remux(Container::Webm)]
        );
        assert!(recipes(Layout::Single, &[hint("", "", "3gp")]).is_empty());
        let progressive = Hint {
            remux: false,
            ..hint("avc1", "mp4a", "mp4")
        };
        assert!(
            recipes(Layout::Single, &[progressive]).is_empty(),
            "file thường (không DASH) chỉ đổi tên, không chép lại"
        );
        let args = text(&ffmpeg_args(
            Recipe::Remux(Container::Webm),
            &[PathBuf::from("s")],
            Path::new("o"),
        ))
        .join(" ");
        assert!(args.contains("-map 0:v? -map 0:a? -c copy -f webm o"));
    }

    #[test]
    fn never_moves_over_an_existing_file() {
        let directory = std::env::temp_dir().join(format!("utd-mux-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let from = directory.join("tmp.part");
        let taken = directory.join("Clip.mp4");
        std::fs::write(&from, b"new").unwrap();
        std::fs::write(&taken, b"user").unwrap();
        assert!(move_no_clobber(&from, &taken).is_err());
        assert_eq!(std::fs::read(&taken).unwrap(), b"user");

        let free = free_target(&directory, "Clip", "mp4").unwrap();
        assert_eq!(free, directory.join("Clip (1).mp4"));
        move_no_clobber(&from, &free).unwrap();
        assert!(!from.exists());
        assert_eq!(std::fs::read(&free).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(&directory);
    }
}
