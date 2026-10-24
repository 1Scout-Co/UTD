mod bframes;
mod build;
mod cases;
mod ffmpeg;
mod generated;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crate::{Brand, Input, Kind, Report, merge};

/// Luồng hình thật thử lần lượt với f140.m4a: AV1 (399) và H.264 có B-frame (299, 137, 298).
const REAL_VIDEOS: [&str; 4] = ["f399.mp4", "f299.mp4", "f137.mp4", "f298.mp4"];

/// Ghép bằng Rust rồi bằng ffmpeg (file `ff-<tên>` cạnh `output`), in thời gian hai bên và so
/// (box thời gian, gói, framemd5, thời lượng, giải mã thử). `UTD_MP4_KEEP=1` thì giữ cả hai file.
fn against_ffmpeg(inputs: &[Input], brand: Brand, output: &Path) -> Report {
    let started = Instant::now();
    let report = merge(inputs, output, brand, &AtomicBool::new(false)).unwrap();
    let rust = started.elapsed();
    let name = output.file_name().unwrap().to_string_lossy();
    let reference = output.with_file_name(format!("ff-{name}"));
    let took = ffmpeg::reference(inputs, &reference);
    println!("{name}: Rust {rust:?}, ffmpeg {took:?}, {report:?}");
    ffmpeg::compare(output, &reference);
    if std::env::var_os("UTD_MP4_KEEP").is_none() {
        let _ = std::fs::remove_file(output);
        let _ = std::fs::remove_file(&reference);
    }
    report
}

fn env_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var(name).unwrap_or_else(|_| panic!("thiếu {name}")))
}

/// Ghép luồng DASH thật của YouTube rồi so với bản ffmpeg. Chạy:
/// `UTD_MP4_FIXTURES=<thư mục> UTD_FFMPEG=<thư mục bin ffmpeg> cargo test --release -p utd-mp4 --
/// --ignored --nocapture`. Thư mục cần f140.m4a và ít nhất một luồng trong REAL_VIDEOS
/// (`yt-dlp -f 299 --fixup never`); file ra để cạnh đầu vào.
#[test]
#[ignore = "cần UTD_MP4_FIXTURES (luồng YouTube) và ffmpeg"]
fn merges_real_dash_streams() {
    let directory = env_path("UTD_MP4_FIXTURES");
    let audio = Input {
        path: directory.join("f140.m4a"),
        kind: Kind::Audio,
    };
    let videos: Vec<Input> = REAL_VIDEOS
        .iter()
        .map(|name| Input {
            path: directory.join(name),
            kind: Kind::Video,
        })
        .filter(|video| video.path.exists())
        .collect();
    assert!(!videos.is_empty(), "thiếu luồng hình");
    for video in videos {
        let stem = video
            .path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let output = directory.join(format!("out-{stem}.mp4"));
        let inputs = [video, audio.clone()];
        let report = against_ffmpeg(&inputs, Brand::Mp4, &output);
        if stem == "f399" {
            assert_eq!(report.samples, 534_470 + 383_631);
        }
        phases(&inputs, &output);
        cancel_midway(&inputs, &output.with_extension("cancel.mp4"));
        let _ = std::fs::remove_file(&output);
    }
    against_ffmpeg(&[audio], Brand::M4a, &directory.join("out-f140.m4a"));
}

/// Ghép một cặp bất kỳ (file > 4 GiB…): `UTD_MP4_VIDEO=<hình> UTD_MP4_AUDIO=<tiếng>`, file ra
/// cạnh file hình. Ra quá 4 GiB thì phải dùng co64.
#[test]
#[ignore = "cần UTD_MP4_VIDEO + UTD_MP4_AUDIO và ffmpeg"]
fn merges_a_given_pair() {
    let video = Input {
        path: env_path("UTD_MP4_VIDEO"),
        kind: Kind::Video,
    };
    let audio = Input {
        path: env_path("UTD_MP4_AUDIO"),
        kind: Kind::Audio,
    };
    let output = video.path.with_file_name("out-pair.mp4");
    let inputs = [video, audio];
    let report = against_ffmpeg(&inputs, Brand::Mp4, &output);
    assert_eq!(report.co64, report.file_bytes > u64::from(u32::MAX));
}

/// Huỷ sau 100 ms (đang chép): phải trả Cancelled nhanh và xoá file dở.
fn cancel_midway(inputs: &[Input], output: &Path) {
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let flag = std::sync::Arc::clone(&cancel);
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
        Instant::now()
    });
    let result = merge(inputs, output, Brand::Mp4, &cancel);
    let stopped = Instant::now();
    let asked = stopper.join().unwrap();
    assert!(matches!(result, Err(crate::Error::Cancelled)), "{result:?}");
    assert!(!output.exists());
    println!("huỷ: dừng sau {:?} kể từ lúc bật cờ", stopped - asked);
}

/// Đo từng pha của `merge` (đọc, xếp chunk, dựng moov, chép, tự kiểm) trên cùng đầu vào.
fn phases(inputs: &[Input], output: &Path) {
    let cancel = AtomicBool::new(false);
    let started = Instant::now();
    let (tracks, sources) = crate::load(inputs, Brand::Mp4, &cancel).unwrap();
    let loaded = started.elapsed();
    let chunks = crate::table::plan(&tracks);
    let planned = started.elapsed();
    let head = crate::write::head(Brand::Mp4, &tracks, &chunks).unwrap();
    let headed = started.elapsed();
    let written = crate::write::write(&head, &tracks, &chunks, &sources, output, &cancel).unwrap();
    let copied = started.elapsed();
    crate::verify(output, &tracks, &chunks, &head, written).unwrap();
    println!(
        "pha: đọc {loaded:?}, chunk {:?}, moov {:?}, chép {:?}, tự kiểm {:?}",
        planned - loaded,
        headed - planned,
        copied - headed,
        started.elapsed() - copied
    );
}
