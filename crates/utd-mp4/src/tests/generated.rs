//! Fixture ffmpeg tự sinh (`UTD_MP4_GEN=<thư mục>`): thiếu file nào thì sinh bằng ffmpeg từ
//! testsrc2/sine 10 giây, ghép từng cặp rồi so với bản ffmpeg ghép cùng đầu vào; các file chưa
//! ghép được phải trả Unsupported và không để lại file ra.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::{against_ffmpeg, ffmpeg};
use crate::{Brand, Error, Input, Kind, merge};

const PICTURE: &str = "-f lavfi -i testsrc2=size=320x240:rate=30 -t 10 -pix_fmt yuv420p";
const FRAGMENTS: &str = "-movflags frag_keyframe+empty_moov";

/// (tên file, đối số ffmpeg). `{p}` là nguồn hình, `{f}` là cờ fragment; tên fixture khác trong
/// đối số đổi thành đường dẫn trong thư mục.
const FIXTURES: [(&str, &str); 21] = [
    ("src-h264.mp4", "{p} -c:v libx264 -bf 0 -g 60"),
    ("src-h264b.mp4", "{p} -c:v libx264 -bf 3 -g 60"),
    (
        "src-aac.m4a",
        "-f lavfi -i sine=frequency=440:sample_rate=48000 -t 10 -c:a aac -b:a 96k",
    ),
    ("v-h264-fk.mp4", "-i src-h264.mp4 -c copy {f}"),
    (
        "v-h264-dbm.mp4",
        "-i src-h264.mp4 -c copy {f}+default_base_moof",
    ),
    (
        "v-h264-omit.mp4",
        "-i src-h264.mp4 -c copy {f}+omit_tfhd_offset",
    ),
    (
        "v-h264-omitdbm.mp4",
        "-i src-h264.mp4 -c copy {f}+omit_tfhd_offset+default_base_moof",
    ),
    (
        "v-h264-small.mp4",
        "-i src-h264.mp4 -c copy -movflags frag_every_frame+empty_moov+default_base_moof",
    ),
    ("a-aac-fk.m4a", "-i src-aac.m4a -c copy {f}"),
    (
        "a-aac-dbm.m4a",
        "-i src-aac.m4a -c copy {f}+default_base_moof",
    ),
    (
        "a-aac-omit.m4a",
        "-i src-aac.m4a -c copy {f}+omit_tfhd_offset",
    ),
    (
        "a-aac-omitdbm.m4a",
        "-i src-aac.m4a -c copy {f}+omit_tfhd_offset+default_base_moof",
    ),
    // delay_moov giữ edit list trong init: priming 1024 của AAC, media_time = cts đầu của hình.
    (
        "a-aac-delay.m4a",
        "-i src-aac.m4a -c copy {f}+delay_moov+default_base_moof",
    ),
    (
        "v-hevc-dbm.mp4",
        "{p} -c:v libx265 -tag:v hvc1 -x265-params bframes=0:keyint=60:log-level=error \
         {f}+default_base_moof",
    ),
    (
        "v-hevcb-dbm.mp4",
        "{p} -c:v libx265 -tag:v hvc1 -x265-params bframes=3:keyint=60:log-level=error \
         {f}+default_base_moof",
    ),
    ("v-h264b-fk.mp4", "-i src-h264b.mp4 -c copy {f}"),
    (
        "v-h264b-dbm.mp4",
        "-i src-h264b.mp4 -c copy {f}+default_base_moof",
    ),
    (
        "v-h264b-delay.mp4",
        "-i src-h264b.mp4 -c copy {f}+delay_moov+default_base_moof",
    ),
    (
        "v-h264b-neg.mp4",
        "-i src-h264b.mp4 -c copy {f}+default_base_moof+negative_cts_offsets",
    ),
    // 1 giờ, timescale 600 kHz: mdhd của hình ≥ INT32_MAX nên ffmpeg ghi version 1 (dù < 2³²).
    (
        "v-h264b-long.mp4",
        "-f lavfi -i testsrc2=size=64x64:rate=1 -t 3600 -pix_fmt yuv420p -c:v libx264 -bf 3 \
         -g 60 -video_track_timescale 600000 {f}+default_base_moof",
    ),
    (
        "a-aac-long.m4a",
        "-f lavfi -i sine=frequency=440:sample_rate=8000 -t 3600 -c:a aac -b:a 16k \
         {f}+delay_moov+default_base_moof",
    ),
];

/// Ghép được và phải trùng bản ffmpeg: (tên, hình, tiếng).
const MERGED: [(&str, &str, &str); 12] = [
    ("fk", "v-h264-fk.mp4", "a-aac-fk.m4a"),
    ("dbm", "v-h264-dbm.mp4", "a-aac-dbm.m4a"),
    ("omit", "v-h264-omit.mp4", "a-aac-omit.m4a"),
    ("omitdbm", "v-h264-omitdbm.mp4", "a-aac-omitdbm.m4a"),
    ("small", "v-h264-small.mp4", "a-aac-dbm.m4a"),
    ("hevc", "v-hevc-dbm.mp4", "a-aac-dbm.m4a"),
    ("plain", "src-h264.mp4", "a-aac-dbm.m4a"),
    ("h264b-fk", "v-h264b-fk.mp4", "a-aac-dbm.m4a"),
    ("h264b-dbm", "v-h264b-dbm.mp4", "a-aac-delay.m4a"),
    ("h264b-delay", "v-h264b-delay.mp4", "a-aac-fk.m4a"),
    ("hevcb", "v-hevcb-dbm.mp4", "a-aac-delay.m4a"),
    ("long", "v-h264b-long.mp4", "a-aac-long.m4a"),
];

/// Chưa ghép (ffmpeg làm): (tên, file, loại, vỏ).
const REFUSED: [(&str, &str, Kind, Brand); 3] = [
    ("cts âm", "v-h264b-neg.mp4", Kind::Video, Brand::Mp4),
    (
        "B-frame file thường",
        "src-h264b.mp4",
        Kind::Video,
        Brand::Mp4,
    ),
    (
        "elst dịch AAC file thường",
        "src-aac.m4a",
        Kind::Audio,
        Brand::M4a,
    ),
];

/// Sinh các fixture còn thiếu theo thứ tự (nguồn trước, bản fragment sau).
fn generate(directory: &Path) {
    for (name, recipe) in FIXTURES {
        let target = directory.join(name);
        if target.exists() {
            continue;
        }
        let recipe = recipe.replace("{p}", PICTURE).replace("{f}", FRAGMENTS);
        let paths: Vec<String> = recipe
            .split_whitespace()
            .map(
                |word| match FIXTURES.iter().any(|(other, _)| *other == word) {
                    true => directory.join(word).display().to_string(),
                    false => word.to_owned(),
                },
            )
            .collect();
        let mut args = vec!["-hide_banner", "-loglevel", "error", "-y"];
        args.extend(paths.iter().map(String::as_str));
        let target = target.display().to_string();
        args.push(&target);
        ffmpeg::run("ffmpeg", &args);
    }
}

fn input(directory: &Path, name: &str, kind: Kind) -> Input {
    Input {
        path: directory.join(name),
        kind,
    }
}

#[test]
#[ignore = "cần UTD_MP4_GEN (thư mục fixture, tự sinh) và ffmpeg"]
fn merges_ffmpeg_generated_variants() {
    let directory = PathBuf::from(std::env::var("UTD_MP4_GEN").expect("UTD_MP4_GEN"));
    std::fs::create_dir_all(&directory).unwrap();
    generate(&directory);
    for (name, video, audio) in MERGED {
        let inputs = [
            input(&directory, video, Kind::Video),
            input(&directory, audio, Kind::Audio),
        ];
        against_ffmpeg(
            &inputs,
            Brand::Mp4,
            &directory.join(format!("out-{name}.mp4")),
        );
    }
    let primed = [input(&directory, "a-aac-delay.m4a", Kind::Audio)];
    against_ffmpeg(&primed, Brand::M4a, &directory.join("out-aac-delay.m4a"));
    let cancel = AtomicBool::new(false);
    for (name, file, kind, brand) in REFUSED {
        let output = directory.join("out-refused.mp4");
        let result = merge(&[input(&directory, file, kind)], &output, brand, &cancel);
        println!("{name}: {result:?}");
        assert!(matches!(result, Err(Error::Unsupported(_))), "{name}");
        assert!(!output.exists());
    }
}
