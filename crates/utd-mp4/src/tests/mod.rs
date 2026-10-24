mod build;
mod cases;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crate::{Brand, Input, Kind, merge};

/// Ghép file thật của YouTube (AV1 399 + AAC 140). Chạy:
/// `UTD_MP4_FIXTURES=<thư mục> cargo test --release -p utd-mp4 -- --ignored --nocapture`;
/// `UTD_MP4_OUT=<file.mp4>` để giữ file ra (thêm bản `.m4a` chỉ tiếng cạnh đó).
#[test]
#[ignore = "cần UTD_MP4_FIXTURES: thư mục có f399.mp4 + f140.m4a"]
fn merges_real_dash_streams() {
    let directory = PathBuf::from(std::env::var("UTD_MP4_FIXTURES").expect("UTD_MP4_FIXTURES"));
    let keep = std::env::var_os("UTD_MP4_OUT").map(PathBuf::from);
    let output = keep.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("utd-mp4-{}.mp4", std::process::id()))
    });
    let video = Input {
        path: directory.join("f399.mp4"),
        kind: Kind::Video,
    };
    let audio = Input {
        path: directory.join("f140.m4a"),
        kind: Kind::Audio,
    };
    let cancel = AtomicBool::new(false);
    let started = Instant::now();
    let report = merge(
        &[video.clone(), audio.clone()],
        &output,
        Brand::Mp4,
        &cancel,
    )
    .unwrap();
    println!("mp4: {:?} trong {:?}", report, started.elapsed());
    assert_eq!(report.samples, 534_470 + 383_631);
    phases(&[video.clone(), audio.clone()], &output);
    cancel_midway(
        &[video, audio.clone()],
        &output.with_extension("cancel.mp4"),
    );

    let m4a = output.with_extension("m4a");
    let started = Instant::now();
    let report = merge(&[audio], &m4a, Brand::M4a, &cancel).unwrap();
    println!("m4a: {:?} trong {:?}", report, started.elapsed());
    if keep.is_none() {
        let _ = std::fs::remove_file(&output);
        let _ = std::fs::remove_file(&m4a);
    }
}

/// Huỷ sau 100 ms (đang chép): phải trả Cancelled nhanh và xoá file dở.
fn cancel_midway(inputs: &[Input], output: &std::path::Path) {
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
fn phases(inputs: &[Input], output: &std::path::Path) {
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

/// Các biến thể ffmpeg tự sinh (`-movflags frag_keyframe+empty_moov`, `+default_base_moof`,
/// `+omit_tfhd_offset`, mỗi khung một fragment, file thường). Chạy với `UTD_MP4_GEN=<thư mục>`;
/// file ra `out-<tên>.mp4` để cạnh đầu vào cho ffprobe so.
#[test]
#[ignore = "cần UTD_MP4_GEN: thư mục fixture ffmpeg tự sinh"]
fn merges_ffmpeg_generated_variants() {
    let directory = PathBuf::from(std::env::var("UTD_MP4_GEN").expect("UTD_MP4_GEN"));
    let cancel = AtomicBool::new(false);
    let input = |name: &str, kind| Input {
        path: directory.join(name),
        kind,
    };
    let cases = [
        ("fk", "v-h264-fk.mp4", "a-aac-fk.m4a"),
        ("dbm", "v-h264-dbm.mp4", "a-aac-dbm.m4a"),
        ("omit", "v-h264-omit.mp4", "a-aac-omit.m4a"),
        ("omitdbm", "v-h264-omitdbm.mp4", "a-aac-omitdbm.m4a"),
        ("small", "v-h264-small.mp4", "a-aac-dbm.m4a"),
        ("hevc", "v-hevc-dbm.mp4", "a-aac-dbm.m4a"),
        ("plain", "src-h264.mp4", "a-aac-dbm.m4a"),
    ];
    for (name, video, audio) in cases {
        let output = directory.join(format!("out-{name}.mp4"));
        let inputs = [input(video, Kind::Video), input(audio, Kind::Audio)];
        let report = merge(&inputs, &output, Brand::Mp4, &cancel);
        println!("{name}: {report:?}");
        assert!(report.is_ok(), "{name}");
    }
    let refused = [
        ("B-frame", "v-h264b-dbm.mp4", Kind::Video, Brand::Mp4),
        ("elst dịch AAC", "src-aac.m4a", Kind::Audio, Brand::M4a),
    ];
    for (name, file, kind, brand) in refused {
        let output = directory.join("out-refused.mp4");
        let result = merge(&[input(file, kind)], &output, brand, &cancel);
        println!("{name}: {result:?}");
        assert!(
            matches!(result, Err(crate::Error::Unsupported(_))),
            "{name}"
        );
        assert!(!output.exists());
    }
}
