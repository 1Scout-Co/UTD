//! So file ra với bản ffmpeg ghép cùng đầu vào (`-c copy -f mp4`): box thời gian và bảng mẫu trùng
//! từng byte, gói ffprobe và framemd5 trùng hết, thời lượng ffprobe bằng nhau, giải mã thử đầu,
//! giữa, cuối không lỗi. ffmpeg/ffprobe lấy trong thư mục `UTD_FFMPEG`, không có thì từ PATH.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::boxes::{FourCc, children, name};
use crate::{Input, Kind};

/// Box so từng byte (đi sâu qua trak/edts/mdia/minf/stbl). stsd, hdlr, udta để ngoài: ffmpeg tự
/// viết lại (btrt, tên handler, encoder), không đổi gói nào.
const COMPARED: [&FourCc; 10] = [
    b"mvhd", b"tkhd", b"elst", b"mdhd", b"stts", b"stss", b"ctts", b"stsz", b"sgpd", b"sbgp",
];
const CONTAINERS: [&FourCc; 5] = [b"trak", b"edts", b"mdia", b"minf", b"stbl"];

pub(super) fn tool(name: &str) -> PathBuf {
    match std::env::var_os("UTD_FFMPEG") {
        Some(directory) => PathBuf::from(directory).join(name),
        None => PathBuf::from(name),
    }
}

/// Chạy công cụ, trả (stdout, stderr); thoát lỗi thì dừng test kèm stderr.
fn output(name: &str, args: &[&str]) -> (String, String) {
    let output = Command::new(tool(name))
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("không chạy được {name} (đặt UTD_FFMPEG): {error}"));
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{name} {args:?}: {stderr}");
    (String::from_utf8(output.stdout).unwrap(), stderr)
}

pub(super) fn run(name: &str, args: &[&str]) -> String {
    output(name, args).0
}

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// Ghép bằng ffmpeg như yt-dlp (`-map 0:v:0 -map 1:a:0 -c copy -f mp4`); trả thời gian chạy.
pub(super) fn reference(inputs: &[Input], output: &Path) -> Duration {
    let maps: Vec<String> = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            let kind = if input.kind == Kind::Video { "v" } else { "a" };
            format!("{index}:{kind}:0")
        })
        .collect();
    let mut args = vec!["-hide_banner", "-loglevel", "error", "-y"];
    for input in inputs {
        args.extend(["-i", text(&input.path)]);
    }
    for map in &maps {
        args.extend(["-map", map.as_str()]);
    }
    args.extend(["-c", "copy", "-f", "mp4", text(output)]);
    let started = Instant::now();
    run("ffmpeg", &args);
    started.elapsed()
}

/// So `ours` với `theirs` (bản ffmpeg) đủ các mặt ở đầu file.
pub(super) fn compare(ours: &Path, theirs: &Path) {
    let (left, right) = (timing_boxes(ours), timing_boxes(theirs));
    assert_eq!(left.len(), right.len(), "số box so");
    for ((kind, mine), (other, reference)) in left.iter().zip(&right) {
        assert_eq!(kind, other, "thứ tự box");
        assert!(mine == reference, "{} khác bản ffmpeg", name(kind));
    }
    same_lines(&packets(ours), &packets(theirs), "gói ffprobe");
    let md5 = |path: &Path| {
        let args = ["-v", "error", "-i", text(path), "-map", "0", "-c", "copy"];
        run("ffmpeg", &[&args[..], &["-f", "framemd5", "-"]].concat())
    };
    same_lines(&md5(ours), &md5(theirs), "framemd5");
    let durations = |path: &Path| {
        let entries = "format=duration:stream=start_time,duration,nb_frames";
        run(
            "ffprobe",
            &[
                "-v",
                "error",
                "-show_entries",
                entries,
                "-of",
                "csv=p=0",
                text(path),
            ],
        )
    };
    same_lines(&durations(ours), &durations(theirs), "thời lượng ffprobe");
    decodes(ours);
}

/// Gói ffprobe xếp theo stream (thứ tự xen kẽ chunk của hai bên khác nhau, không tính).
fn packets(path: &Path) -> String {
    let entries = "packet=stream_index,pts,dts,duration,size,flags";
    let listing = run(
        "ffprobe",
        &[
            "-v",
            "error",
            "-show_entries",
            entries,
            "-of",
            "csv=p=0",
            text(path),
        ],
    );
    let mut lines: Vec<&str> = listing.lines().collect();
    lines.sort_by_key(|line| line.split(',').next().unwrap_or_default().to_owned());
    lines.join("\n")
}

fn same_lines(mine: &str, reference: &str, what: &str) {
    if mine == reference {
        return;
    }
    let line = mine
        .lines()
        .zip(reference.lines())
        .position(|(left, right)| left != right);
    panic!(
        "{what} khác bản ffmpeg ở dòng {line:?} ({} / {} dòng)",
        mine.lines().count(),
        reference.lines().count()
    );
}

/// Giải mã 1 giây ở đầu, giữa, cuối với `-xerror`: lỗi giải mã nào cũng làm ffmpeg dừng.
fn decodes(path: &Path) {
    let duration: f64 = run(
        "ffprobe",
        &[
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
            text(path),
        ],
    )
    .trim()
    .parse()
    .unwrap();
    for start in [0.0, duration / 2.0, (duration - 2.0).max(0.0)] {
        let start = format!("{start:.3}");
        let args = ["-v", "error", "-xerror", "-ss", &start, "-i", text(path)];
        let tail = ["-t", "1", "-map", "0", "-f", "null", "-"];
        let (_, stderr) = output("ffmpeg", &[&args[..], &tail[..]].concat());
        assert!(stderr.is_empty(), "giải mã ở {start}s: {stderr}");
    }
}

/// Các box trong COMPARED theo thứ tự gặp trong moov (ở đâu trong file cũng tìm được).
fn timing_boxes(path: &Path) -> Vec<(FourCc, Vec<u8>)> {
    let moov = read_moov(path);
    let mut found = Vec::new();
    collect(&moov, &mut found);
    found
}

fn collect(body: &[u8], found: &mut Vec<(FourCc, Vec<u8>)>) {
    for child in children(body).unwrap() {
        if COMPARED.contains(&&child.kind) {
            found.push((child.kind, child.whole.to_vec()));
        } else if CONTAINERS.contains(&&child.kind) {
            collect(child.body, found);
        }
    }
}

/// Thân moov: duyệt header box cấp cao, nhảy qua mdat (file ra của ffmpeg để moov cuối).
pub(super) fn read_moov(path: &Path) -> Vec<u8> {
    let mut file = BufReader::new(File::open(path).unwrap());
    let length = file.get_ref().metadata().unwrap().len();
    let mut pos = 0;
    while pos < length {
        let mut head = [0u8; 16];
        file.read_exact(&mut head[..8]).unwrap();
        let mut size = u64::from(u32::from_be_bytes(head[..4].try_into().unwrap()));
        let mut header = 8;
        if size == 1 {
            file.read_exact(&mut head[8..]).unwrap();
            size = u64::from_be_bytes(head[8..].try_into().unwrap());
            header = 16;
        }
        if &head[4..8] == b"moov" {
            let mut body = vec![0; (size - header) as usize];
            file.read_exact(&mut body).unwrap();
            return body;
        }
        pos += size;
        file.seek(SeekFrom::Start(pos)).unwrap();
    }
    panic!("{}: không có moov", path.display());
}
