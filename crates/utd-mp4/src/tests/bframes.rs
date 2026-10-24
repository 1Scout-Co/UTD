//! B-frame và edit list: ctts, elst, thời lượng tkhd/mdhd/mvhd tính như ffmpeg (số mong đợi tính
//! tay theo movenc.c; bản tiếng dựng ở đây đã cho ffmpeg 8.1 ghép và ra đúng các số này, bản hình
//! cần dữ liệu H.264 thật nên đối chiếu qua fixture ở `UTD_MP4_GEN`/`UTD_MP4_FIXTURES`), và các
//! cấu trúc ffmpeg sẽ đổi mốc khác đi nên phải trả Unsupported.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::build::{Built, Frag, Init, Scratch, Smp, Trun, bx, edit, edits, full};
use super::cases::{NON_SYNC, SYNC, refused, sizes, trun};
use super::ffmpeg::read_moov;
use crate::boxes::{Cursor, FourCc, children};
use crate::{Brand, Input, Kind, merge, read};

/// trun: data-offset, first-sample-flags, duration, size, cts.
const WITH_CTS: u32 = 0xB05;

/// Tám khung kiểu x264 `-bf 3` (thứ tự giải mã I P B B P B B P), 30 khung/giây ở timescale
/// 15360: hiển thị trễ 2 khung nên cts mẫu đầu 1024.
fn bframes(init: Init, edit_trun: impl FnOnce(&mut Trun)) -> Built {
    let cts = [1024, 2048, 512, 512, 2048, 512, 512, 1024];
    let samples = cts
        .iter()
        .enumerate()
        .map(|(index, cts)| Smp {
            duration: 512,
            size: 40 + index as u32,
            flags: NON_SYNC,
            cts: *cts,
        })
        .collect();
    let mut trun = Trun {
        version: 0,
        flags: WITH_CTS,
        first_flags: SYNC,
        samples,
    };
    edit_trun(&mut trun);
    let mut built = Built::fragmented(init);
    built.fragment(&Frag::new(0x02_0000, vec![trun]));
    built
}

fn merge_one(scratch: &Scratch, built: &Built, kind: Kind, brand: Brand) -> PathBuf {
    let input = Input {
        path: built.save(scratch, "in"),
        kind,
    };
    let output = scratch.0.join("out.mp4");
    merge(&[input], &output, brand, &AtomicBool::new(false)).unwrap();
    output
}

/// Thân box `kind` đầu tiên trong moov (đi sâu qua trak/edts/mdia/minf/stbl).
fn body(path: &Path, kind: &FourCc) -> Vec<u8> {
    fn find(data: &[u8], kind: &FourCc) -> Option<Vec<u8>> {
        children(data).unwrap().iter().find_map(|child| {
            if &child.kind == kind {
                Some(child.body.to_vec())
            } else if [b"trak", b"edts", b"mdia", b"minf", b"stbl"].contains(&&child.kind) {
                find(child.body, kind)
            } else {
                None
            }
        })
    }
    find(&read_moov(path), kind).unwrap_or_else(|| panic!("thiếu {kind:?}"))
}

/// (version, duration) của mvhd, tkhd hoặc mdhd.
fn duration(path: &Path, kind: &FourCc) -> (u8, u64) {
    let data = body(path, kind);
    let mut cursor = Cursor::new(&data);
    let (version, _) = cursor.full().unwrap();
    cursor.skip(if version == 1 { 16 } else { 8 }).unwrap();
    // timescale (mvhd, mdhd) hoặc track_ID + reserved (tkhd).
    cursor.skip(if kind == b"tkhd" { 8 } else { 4 }).unwrap();
    (version, cursor.sized(version).unwrap())
}

/// Các mục elst: (segment, media_time).
fn elst(path: &Path) -> Vec<(u64, i64)> {
    let data = body(path, b"elst");
    let mut cursor = Cursor::new(&data);
    let (version, _) = cursor.full().unwrap();
    let count = cursor.u32().unwrap();
    (0..count)
        .map(|_| {
            let segment = cursor.sized(version).unwrap();
            let media_time = cursor.sized(version).unwrap() as i64;
            assert_eq!(cursor.u32().unwrap(), 0x0001_0000);
            (segment, media_time)
        })
        .collect()
}

/// Các mục (số mẫu, giá trị) của stts hoặc ctts.
fn table(path: &Path, kind: &FourCc) -> Vec<(u32, u32)> {
    let data = body(path, kind);
    let mut cursor = Cursor::new(&data);
    cursor.full().unwrap();
    let count = cursor.u32().unwrap();
    (0..count)
        .map(|_| (cursor.u32().unwrap(), cursor.u32().unwrap()))
        .collect()
}

#[test]
fn writes_ctts_and_an_edit_list_like_ffmpeg() {
    let scratch = Scratch::new("bframes");
    let video = bframes(Init::video(), |_| {});
    let output = merge_one(&scratch, &video, Kind::Video, Brand::Mp4);
    let tracks = read::read_output(&output).unwrap();
    assert_eq!(tracks[0].samples, video.samples, "giữ cts từng mẫu");
    let ctts = [
        (1, 1024),
        (1, 2048),
        (2, 512),
        (1, 2048),
        (2, 512),
        (1, 1024),
    ];
    assert_eq!(table(&output, b"ctts"), ctts);
    assert_eq!(table(&output, b"stts"), [(8, 512)]);
    // ffmpeg dịch PTS đầu về 0 (DTS đầu −1024); start_cts = 512 (gói có DTS 0); PTS cuối 4096.
    assert_eq!(duration(&output, b"mdhd"), (0, 4096 + 1024 - 512));
    assert_eq!(duration(&output, b"tkhd"), (0, 267), "⌈4096 / 15,36⌉");
    assert_eq!(duration(&output, b"mvhd"), (0, 267));
    assert_eq!(
        elst(&output),
        [(300 - 34, 1024)],
        "⌈4608 / 15,36⌉ + ⌊−512 / 15,36⌋"
    );
}

#[test]
fn takes_any_single_init_edit_like_ffmpeg() {
    // fftools dịch mỗi đầu vào cho PTS đầu về 0 nên media_time của init không đổi file ra.
    for media_time in [0, 512, 1024, 4096] {
        let scratch = Scratch::new(&format!("edit-{media_time}"));
        let mut init = Init::video();
        let segment = if media_time == 1024 { 270 } else { 0 };
        init.edts = Some(edit(segment, media_time));
        let output = merge_one(&scratch, &bframes(init, |_| {}), Kind::Video, Brand::Mp4);
        assert_eq!(elst(&output), [(266, 1024)], "media_time {media_time}");
        assert_eq!(duration(&output, b"mdhd"), (0, 4608));
        assert_eq!(duration(&output, b"tkhd"), (0, 267));
    }
}

fn aac(edts: Option<Vec<u8>>, timescale: u32) -> Built {
    let mut init = Init::audio();
    init.edts = edts;
    init.timescale = timescale;
    let mut built = Built::fragmented(init);
    built.fragment(&Frag::new(
        0x02_0000,
        vec![trun(0x201, sizes(&[30, 31, 32, 33, 34]))],
    ));
    built
}

#[test]
fn keeps_aac_priming_from_the_init_edit_like_ffmpeg() {
    let scratch = Scratch::new("priming");
    // 5 khung 1024 mẫu ở 48 kHz, priming 1024 (AudioSpecificConfig AAC-LC 48 kHz).
    let primed = aac(Some(edit(0, 1024)), 48000);
    let output = merge_one(&scratch, &primed, Kind::Audio, Brand::M4a);
    assert_eq!(duration(&output, b"mdhd"), (0, 5120));
    assert_eq!(duration(&output, b"tkhd"), (0, 86), "⌈4096 / 48⌉");
    assert_eq!(
        elst(&output),
        [(107 - 22, 1024)],
        "⌈5120 / 48⌉ + ⌊−1024 / 48⌋"
    );
    let output = merge_one(&scratch, &aac(None, 48000), Kind::Audio, Brand::M4a);
    assert_eq!(elst(&output), [(107, 0)]);
    assert_eq!(duration(&output, b"tkhd"), (0, 107));
    // Tần số trong AudioSpecificConfig khác timescale: start_time của ffmpeg lệch, không đoán.
    let other = aac(Some(edit(0, 1024)), 44100);
    refused("rate", &other, Kind::Audio, Brand::M4a, "priming AAC");
}

#[test]
fn refuses_timings_ffmpeg_would_change() {
    let (video, mp4) = (Kind::Video, Brand::Mp4);
    let negative = bframes(Init::video(), |trun| {
        trun.version = 1;
        trun.samples[2].cts = (-512i32) as u32;
    });
    refused("cts-âm", &negative, video, mp4, "cts âm");
    let huge = bframes(Init::video(), |trun| trun.samples[2].cts = 0x8000_0000);
    refused("cts-lớn", &huge, video, mp4, "31 bit");
    let mut sdtp = Built::fragmented(Init::video());
    let mut frag = Frag::new(0x02_0000, vec![trun(0x205, sizes(&[10, 11]))]);
    frag.extra = full(b"sdtp", 0, 0, &[0x10, 0x18]);
    sdtp.fragment(&frag);
    refused("sdtp", &sdtp, video, mp4, "traf có sdtp");
    let cases = [
        (
            "hai-mục",
            edits(&[(0, 0, 0x1_0000), (0, 512, 0x1_0000)]),
            "nhiều mục",
        ),
        ("trống", edits(&[(100, u32::MAX, 0x1_0000)]), "đoạn trống"),
        ("tốc-độ", edits(&[(0, 0, 0x2_0000)]), "đổi tốc độ"),
    ];
    for (name, edts, reason) in cases {
        let mut init = Init::video();
        init.edts = Some(edts);
        refused(name, &bframes(init, |_| {}), video, mp4, reason);
    }
    let mut init = Init::video();
    init.extra = bx(b"udta", &bx(b"meta", b"----iTunSMPB 00000840"));
    refused("smpb", &bframes(init, |_| {}), video, mp4, "iTunSMPB");
    let late = bframes(Init::video(), |trun| trun.first_flags = NON_SYNC);
    refused("keyframe", &late, video, mp4, "keyframe");
    let zero = bframes(Init::video(), |trun| trun.samples[3].duration = 0);
    refused("duration", &zero, video, mp4, "duration 0");
    let mut audio = Built::fragmented(Init::audio());
    let mut samples = sizes(&[10, 11]);
    samples[1].cts = 1024;
    audio.fragment(&Frag::new(0x02_0000, vec![trun(0xA01, samples)]));
    refused(
        "tiếng",
        &audio,
        Kind::Audio,
        Brand::M4a,
        "tiếng có lệch cts",
    );
}

#[test]
fn switches_to_version_1_at_the_ffmpeg_threshold() {
    // ffmpeg ghi version 1 khi thời lượng ≥ INT32_MAX (không đợi vượt 32 bit không dấu).
    for (total, version) in [(i32::MAX as u32 - 1, 0), (i32::MAX as u32, 1)] {
        let scratch = Scratch::new(&format!("v{version}"));
        let mut init = Init::video();
        init.timescale = 1000;
        let half = total / 2;
        let samples = vec![
            Smp {
                duration: half,
                size: 10,
                flags: NON_SYNC,
                cts: 0,
            },
            Smp {
                duration: total - half,
                size: 11,
                flags: NON_SYNC,
                cts: 0,
            },
        ];
        let mut built = Built::fragmented(init);
        built.fragment(&Frag::new(0x02_0000, vec![trun(0x305, samples)]));
        let output = merge_one(&scratch, &built, Kind::Video, Brand::Mp4);
        let expected = (version, u64::from(total));
        assert_eq!(duration(&output, b"mvhd"), expected);
        assert_eq!(duration(&output, b"tkhd"), expected);
        assert_eq!(duration(&output, b"mdhd"), expected);
        assert_eq!(elst(&output), [(u64::from(total), 0)]);
        let elst_version = body(&output, b"elst")[0];
        assert_eq!(elst_version, version);
    }
}
