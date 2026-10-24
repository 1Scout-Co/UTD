use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::build::{
    Built, Frag, Init, Plain, Scratch, Smp, Trun, edit, full, plain, sample_bytes, smp,
};
use crate::boxes::children;
use crate::table::{self, CHUNK_BYTES, Track};
use crate::{Brand, Error, Input, Kind, Report, merge, read, write};

const SYNC: u32 = 0x0200_0000;
const NON_SYNC: u32 = 0x0001_0000;

fn trun(flags: u32, samples: Vec<Smp>) -> Trun {
    Trun {
        flags,
        first_flags: SYNC,
        samples,
    }
}

fn sizes(sizes: &[u32]) -> Vec<Smp> {
    sizes.iter().map(|size| smp(256, *size, NON_SYNC)).collect()
}

fn audio() -> Built {
    let mut built = Built::fragmented(Init::audio());
    let mut first = Frag::new(0x02_002A, vec![trun(0x201, sizes(&[11, 12, 13, 14, 15]))]);
    first.defaults = [1024, 0, 0];
    first.tfdt = Some(0);
    built.fragment(&first);
    built.fragment(&Frag::new(
        0x02_0000,
        vec![trun(0x201, sizes(&[16, 17, 18]))],
    ));
    built
}

fn input(path: PathBuf, kind: Kind) -> Input {
    Input { path, kind }
}

/// Ghép các file dựng sẵn, đọc lại file ra.
fn merged(
    scratch: &Scratch,
    parts: &[(&Built, Kind)],
    brand: Brand,
) -> (PathBuf, Report, Vec<Track>) {
    let inputs: Vec<Input> = parts
        .iter()
        .enumerate()
        .map(|(index, (built, kind))| input(built.save(scratch, &format!("in{index}")), *kind))
        .collect();
    let output = scratch.0.join("out.mp4");
    let report = merge(&inputs, &output, brand, &AtomicBool::new(false)).unwrap();
    let tracks = read::read_file(&output).unwrap();
    (output, report, tracks)
}

/// Bảng mẫu và byte từng mẫu của file ra trùng với nguồn.
fn assert_same(built: &Built, track: &Track, output: &Path) {
    assert_eq!(track.samples, built.samples);
    assert_eq!(sample_bytes(output, track), built.payloads);
}

fn refusal(result: Result<Report, Error>) -> String {
    match result {
        Err(Error::Unsupported(reason)) => reason,
        other => panic!("cần Unsupported, được {other:?}"),
    }
}

fn top_level(path: &Path) -> Vec<[u8; 4]> {
    let bytes = std::fs::read(path).unwrap();
    children(&bytes)
        .unwrap()
        .iter()
        .map(|child| child.kind)
        .collect()
}

/// Phần ftyp + moov (trước mdat), để đếm box mà không lẫn byte mẫu.
fn moov(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap();
    let boxes = children(&bytes).unwrap();
    boxes
        .iter()
        .take_while(|child| &child.kind != b"mdat")
        .flat_map(|child| child.whole.to_vec())
        .collect()
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

#[test]
fn merges_fragments_into_one_file_with_moov_first() {
    let scratch = Scratch::new("basic");
    let mut video = Built::fragmented(Init::video());
    let mut first = Frag::new(0x02_0000, vec![trun(0x205, sizes(&[100, 20, 30, 40, 50]))]);
    first.tfdt = Some(0);
    video.fragment(&first);
    let mut second = Frag::new(
        0x02_0000,
        vec![trun(
            0x305,
            vec![smp(256, 90, 0), smp(512, 9, 0), smp(256, 8, 0)],
        )],
    );
    second.tfdt = Some(1280);
    video.fragment(&second);
    let flags = vec![
        smp(0, 70, 0),
        smp(0, 7, NON_SYNC),
        smp(0, 6, 0x0100_0000),
        smp(0, 5, NON_SYNC),
    ];
    video.fragment(&Frag::new(0x02_0000, vec![trun(0x601, flags)]));
    let audio = audio();

    let (output, report, tracks) = merged(
        &scratch,
        &[(&audio, Kind::Audio), (&video, Kind::Video)],
        Brand::Mp4,
    );
    assert_eq!(top_level(&output), [*b"ftyp", *b"moov", *b"mdat"]);
    assert_eq!(report.samples, 20);
    assert_eq!(tracks.len(), 2, "hình trước, tiếng sau");
    assert_same(&video, &tracks[0], &output);
    assert_same(&audio, &tracks[1], &output);
    let sync: Vec<bool> = tracks[0].samples.iter().map(|sample| sample.sync).collect();
    assert_eq!(
        sync,
        [
            true, false, false, false, false, true, false, false, true, false, false, false
        ]
    );
    let bytes = moov(&output);
    assert_eq!(&std::fs::read(&output).unwrap()[8..12], b"isom");
    assert_eq!(count(&bytes, b"avc1"), 2, "ftyp + stsd");
    assert_eq!(
        count(&bytes, b"stss"),
        1,
        "tiếng toàn keyframe: không ghi stss"
    );
    assert_eq!(count(&bytes, b"sgpd"), 1, "AAC có nhóm roll");
    assert_eq!(count(&bytes, b"mvex"), 0);
}

#[test]
fn fills_a_zero_tkhd_size_from_the_sample_entry() {
    let scratch = Scratch::new("tkhd");
    let mut video = one_fragment(Init::video(), |_| {});
    let tkhd = video
        .bytes
        .windows(4)
        .position(|window| window == b"tkhd")
        .unwrap();
    // Từ chữ "tkhd": 4 + version/flags 4 + 5 trường 4 B + reserved 8 + layer…volume 8 + matrix 36.
    video.bytes[tkhd + 80..tkhd + 88].fill(0);
    let (_, _, tracks) = merged(&scratch, &[(&video, Kind::Video)], Brand::Mp4);
    assert_eq!((tracks[0].width, tracks[0].height), (320 << 16, 240 << 16));
}

#[test]
fn follows_base_data_offset_and_runs_without_data_offset() {
    let scratch = Scratch::new("base");
    let mut video = Built::fragmented(Init::video());
    // Trun đầu không có data-offset bắt đầu ở base; trun sau nối tiếp trun trước.
    let mut first = Frag::new(
        0x01 | 0x08 | 0x10,
        vec![trun(0, sizes(&[0; 3])), trun(0x200, sizes(&[5, 6]))],
    );
    first.defaults = [256, 33, 0];
    video.fragment(&first);
    video.fragment(&Frag::new(0x01, vec![trun(0x201, sizes(&[44, 45]))]));
    let audio = audio();
    let (output, _, tracks) = merged(
        &scratch,
        &[(&video, Kind::Video), (&audio, Kind::Audio)],
        Brand::Mp4,
    );
    assert_same(&video, &tracks[0], &output);
    assert_same(&audio, &tracks[1], &output);
    assert_eq!(video.samples[0].size, 33);
}

#[test]
fn takes_sample_defaults_from_tfhd_then_trex() {
    let scratch = Scratch::new("defaults");
    let mut init = Init::video();
    init.trex = [1, 512, 64, 0];
    let mut video = Built::fragmented(init);
    let mut first = Frag::new(0x02_0038, vec![trun(0x005, vec![Smp::default(); 3])]);
    first.defaults = [300, 77, NON_SYNC];
    video.fragment(&first);
    video.fragment(&Frag::new(
        0x02_0000,
        vec![trun(0x001, vec![Smp::default(); 2])],
    ));
    let (output, _, tracks) = merged(&scratch, &[(&video, Kind::Video)], Brand::Mp4);
    assert_same(&video, &tracks[0], &output);
    let table: Vec<(u32, u32, bool)> = tracks[0]
        .samples
        .iter()
        .map(|sample| (sample.size, sample.duration, sample.sync))
        .collect();
    assert_eq!(
        table,
        [
            (77, 300, true),
            (77, 300, false),
            (77, 300, false),
            (64, 512, true),
            (64, 512, true)
        ]
    );
}

#[test]
fn stretches_the_previous_sample_over_a_tfdt_gap() {
    let scratch = Scratch::new("gap");
    let mut video = Built::fragmented(Init::video());
    video.fragment(&Frag::new(0x02_0000, vec![trun(0x205, sizes(&[10, 11]))]));
    let mut later = Frag::new(0x02_0000, vec![trun(0x205, sizes(&[12]))]);
    later.tfdt = Some(512 + 100);
    video.fragment(&later);
    video.samples[1].duration += 100;
    let (output, _, tracks) = merged(&scratch, &[(&video, Kind::Video)], Brand::Mp4);
    assert_same(&video, &tracks[0], &output);

    let mut back = Built::fragmented(Init::video());
    back.fragment(&Frag::new(0x02_0000, vec![trun(0x205, sizes(&[10, 11]))]));
    let mut early = Frag::new(0x02_0000, vec![trun(0x205, sizes(&[12]))]);
    early.tfdt = Some(256);
    back.fragment(&early);
    let path = back.save(&scratch, "back");
    let result = merge(
        &[input(path, Kind::Video)],
        &scratch.0.join("x"),
        Brand::Mp4,
        &AtomicBool::new(false),
    );
    assert!(refusal(result).contains("tfdt lùi"));
}

#[test]
fn accepts_zero_composition_offsets() {
    let scratch = Scratch::new("cts0");
    let mut video = Built::fragmented(Init::video());
    video.fragment(&Frag::new(
        0x02_0000,
        vec![trun(0xA05, sizes(&[10, 11, 12]))],
    ));
    let (output, _, tracks) = merged(&scratch, &[(&video, Kind::Video)], Brand::Mp4);
    assert_same(&video, &tracks[0], &output);
}

/// Một file dựng hỏng hoặc lạ, kèm đoạn lý do phải có.
fn refused(name: &str, built: &Built, kind: Kind, brand: Brand, reason: &str) {
    let scratch = Scratch::new(&format!("refuse-{name}"));
    let path = built.save(&scratch, "in");
    let output = scratch.0.join("out.mp4");
    std::fs::write(&output, b"old").unwrap();
    let result = merge(
        &[input(path, kind)],
        &output,
        brand,
        &AtomicBool::new(false),
    );
    let got = refusal(result);
    assert!(got.contains(reason), "{name}: {got}");
    assert!(!output.exists(), "{name}: không để lại file dở");
}

fn one_fragment(init: Init, edit: impl FnOnce(&mut Frag)) -> Built {
    let mut built = Built::fragmented(init);
    let mut frag = Frag::new(0x02_0000, vec![trun(0x205, sizes(&[10, 11, 12]))]);
    edit(&mut frag);
    built.fragment(&frag);
    built
}

#[test]
fn refuses_structures_it_has_not_checked() {
    let (video, mp4) = (Kind::Video, Brand::Mp4);
    let senc = one_fragment(Init::video(), |frag| {
        frag.extra = full(b"senc", 0, 0, &[0; 4])
    });
    refused("senc", &senc, video, mp4, "mã hoá");
    let mut encv = Init::video();
    encv.codec = *b"encv";
    refused("encv", &one_fragment(encv, |_| {}), video, mp4, "mã hoá");
    refused(
        "traf",
        &one_fragment(Init::video(), |frag| frag.trafs = 2),
        video,
        mp4,
        "nhiều traf",
    );
    let cts = one_fragment(Init::video(), |frag| {
        frag.truns[0].flags |= 0x800;
        frag.truns[0].samples[1].cts = 512;
    });
    refused("cts", &cts, video, mp4, "cts");
    let sdi = one_fragment(Init::video(), |frag| {
        frag.tfhd_flags |= 0x02;
        frag.sdi = 2;
    });
    refused("sdi", &sdi, video, mp4, "sample_description_index = 2");
    refused(
        "outside",
        &one_fragment(Init::video(), |frag| frag.shift = 4),
        video,
        mp4,
        "ngoài mdat",
    );
    let both = one_fragment(Init::video(), |frag| frag.truns[0].flags |= 0x400);
    refused(
        "flags",
        &both,
        video,
        mp4,
        "first-sample-flags lẫn sample-flags",
    );
    refused(
        "tfdt",
        &one_fragment(Init::video(), |frag| frag.tfdt = Some(256)),
        video,
        mp4,
        "tfdt đầu tiên",
    );
    let mut edts = Init::video();
    edts.edts = true;
    refused(
        "edts",
        &one_fragment(edts, |_| {}),
        video,
        mp4,
        "init có edts",
    );
    let mut mp3 = Init::audio();
    mp3.object_type = 0x6B;
    refused(
        "mp3",
        &one_fragment(mp3, |_| {}),
        Kind::Audio,
        Brand::M4a,
        "không phải AAC",
    );
    refused("kind", &audio(), video, mp4, "không phải track");
    let mut cut = one_fragment(Init::video(), |_| {});
    cut.bytes.truncate(cut.bytes.len() - 5);
    refused("cut", &cut, video, mp4, "vượt cuối file");
    let mut webm = one_fragment(Init::video(), |_| {});
    webm.bytes
        .splice(0..8, [0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x86, 0x81]);
    refused("webm", &webm, video, mp4, "không phải file MP4");
}

#[test]
fn checks_brand_against_tracks() {
    let audio = audio();
    let video = one_fragment(Init::video(), |_| {});
    refused(
        "m4a-video",
        &video,
        Kind::Video,
        Brand::M4a,
        "M4a không nhận 1 hình",
    );
    refused(
        "mp4-audio",
        &audio,
        Kind::Audio,
        Brand::Mp4,
        "Mp4 không nhận 0 hình",
    );
}

#[test]
fn writes_audio_only_m4a_with_roll_group() {
    let scratch = Scratch::new("m4a");
    let audio = audio();
    let (output, _, tracks) = merged(&scratch, &[(&audio, Kind::Audio)], Brand::M4a);
    assert_same(&audio, &tracks[0], &output);
    let bytes = moov(&output);
    assert_eq!(&bytes[8..12], b"M4A ");
    assert_eq!(count(&bytes, b"roll"), 2, "sgpd + sbgp");
    assert_eq!(count(&bytes, b"vmhd"), 0);
}

#[test]
fn reads_plain_mp4_tables() {
    let scratch = Scratch::new("plain");
    let samples: Vec<Smp> = (0..7)
        .map(|index| {
            smp(
                256 * (1 + index % 2),
                20 + index,
                if index % 3 == 0 { 0 } else { NON_SYNC },
            )
        })
        .collect();
    let options = Plain {
        per_chunk: 3,
        stz2: true,
        cts: 0,
        edts: Some(edit(1_000_000, 0)),
    };
    let video = plain(&Init::video(), &samples, &options);
    let audio = audio();
    let (output, _, tracks) = merged(
        &scratch,
        &[(&video, Kind::Video), (&audio, Kind::Audio)],
        Brand::Mp4,
    );
    assert_same(&video, &tracks[0], &output);
    assert_same(&audio, &tracks[1], &output);

    let ctts = plain(
        &Init::video(),
        &samples,
        &Plain {
            cts: 512,
            ..options.clone()
        },
    );
    refused("ctts", &ctts, Kind::Video, Brand::Mp4, "ctts");
    let shifted = plain(
        &Init::video(),
        &samples,
        &Plain {
            edts: Some(edit(0, 1024)),
            ..options.clone()
        },
    );
    refused("elst", &shifted, Kind::Video, Brand::Mp4, "edit list");
    let short = plain(
        &Init::video(),
        &samples,
        &Plain {
            edts: Some(edit(10, 0)),
            ..options
        },
    );
    refused("elst-short", &short, Kind::Video, Brand::Mp4, "edit list");
}

#[test]
fn switches_to_co64_and_largesize_past_the_32_bit_limit() {
    let scratch = Scratch::new("co64");
    let video = one_fragment(Init::video(), |_| {});
    let audio = audio();
    let inputs = [
        input(video.save(&scratch, "v"), Kind::Video),
        input(audio.save(&scratch, "a"), Kind::Audio),
    ];
    let cancel = AtomicBool::new(false);
    let (tracks, sources) = crate::load(&inputs, Brand::Mp4, &cancel).unwrap();
    let chunks = table::plan(&tracks);
    let head = write::head_with_limit(Brand::Mp4, &tracks, &chunks, 64).unwrap();
    assert!(head.co64);
    let output = scratch.0.join("out.mp4");
    let written = write::write(&head, &tracks, &chunks, &sources, &output, &cancel).unwrap();
    crate::verify(&output, &tracks, &chunks, &head, written).unwrap();
    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(count(&moov(&output), b"co64"), 2);
    assert_eq!(count(&moov(&output), b"stco"), 0);
    let mdat = head.bytes.len() - 16;
    assert_eq!(&bytes[mdat..mdat + 8], [0, 0, 0, 1, b'm', b'd', b'a', b't']);
    let again = read::read_file(&output).unwrap();
    assert_same(&video, &again[0], &output);
    assert_same(&audio, &again[1], &output);
}

#[test]
fn cancelling_removes_the_output() {
    let scratch = Scratch::new("cancel");
    let video = one_fragment(Init::video(), |_| {});
    let inputs = [input(video.save(&scratch, "v"), Kind::Video)];
    let output = scratch.0.join("out.mp4");
    std::fs::write(&output, b"old").unwrap();
    let result = merge(&inputs, &output, Brand::Mp4, &AtomicBool::new(true));
    assert!(matches!(result, Err(Error::Cancelled)));
    assert!(!output.exists());

    let (tracks, sources) = crate::load(&inputs, Brand::Mp4, &AtomicBool::new(false)).unwrap();
    let chunks = table::plan(&tracks);
    let head = write::head(Brand::Mp4, &tracks, &chunks).unwrap();
    let copy = write::write(
        &head,
        &tracks,
        &chunks,
        &sources,
        &output,
        &AtomicBool::new(true),
    );
    assert!(
        matches!(copy, Err(Error::Cancelled)),
        "huỷ được cả lúc đang chép"
    );
}

#[test]
fn strips_av1_temporal_delimiters_like_ffmpeg() {
    let scratch = Scratch::new("av1");
    let mut init = Init::video();
    init.codec = *b"av01";
    let mut video = Built::fragmented(init.clone());
    video.fragment(&Frag::new(
        0x02_0000,
        vec![trun(0x205, sizes(&[30, 5, 6, 40]))],
    ));
    video.fragment(&Frag::new(0x02_0000, vec![trun(0x205, sizes(&[31, 5]))]));
    assert_eq!(video.samples[1].size, 3, "mẫu mong đợi đã bỏ 2 byte");
    let (output, _, tracks) = merged(&scratch, &[(&video, Kind::Video)], Brand::Mp4);
    assert_same(&video, &tracks[0], &output);
    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(count(&bytes[..32], b"av01"), 1, "ftyp có brand av01");

    let mut broken = Built::fragmented(init);
    broken.fragment(&Frag::new(
        0x02_0000,
        vec![trun(0x205, sizes(&[30, 5, 6, 40]))],
    ));
    let third = broken.offsets[2] as usize;
    broken.bytes[third] = 0x0A;
    refused(
        "av1",
        &broken,
        Kind::Video,
        Brand::Mp4,
        "temporal delimiter",
    );
}

#[test]
fn plans_chunks_within_one_second_and_one_mebibyte_in_time_order() {
    let scratch = Scratch::new("plan");
    let mut video = Built::fragmented(Init::video());
    let mut big = sizes(&[700_000, 600_000, 10]);
    big.extend(sizes(&[100; 130]));
    video.fragment(&Frag::new(0x02_0000, vec![trun(0x205, big)]));
    video.fragment(&Frag::new(0x02_0000, vec![trun(0x205, sizes(&[50; 10]))]));
    let mut audio = Built::fragmented(Init::audio());
    audio.fragment(&Frag::new(0x02_0000, vec![trun(0x201, sizes(&[8; 200]))]));
    let tracks = vec![
        read::read_file(&video.save(&scratch, "v"))
            .unwrap()
            .remove(0),
        read::read_file(&audio.save(&scratch, "a"))
            .unwrap()
            .remove(0),
    ];
    let chunks = table::plan(&tracks);
    let mut previous = 0.0;
    for chunk in &chunks {
        let track = &tracks[chunk.track];
        let samples = &track.samples[chunk.first..chunk.first + chunk.count as usize];
        let duration: u64 = samples
            .iter()
            .map(|sample| u64::from(sample.duration))
            .sum();
        assert!(duration <= u64::from(track.timescale));
        assert!(chunk.size <= CHUNK_BYTES || chunk.count == 1);
        let start = chunk.dts as f64 / f64::from(track.timescale);
        assert!(start >= previous, "xếp theo DTS");
        previous = start;
    }
    let video_chunks: Vec<u32> = chunks
        .iter()
        .filter(|chunk| chunk.track == 0)
        .map(|chunk| chunk.count)
        .collect();
    assert_eq!(
        video_chunks,
        [1, 60, 60, 12, 10],
        "1 MiB, 1 giây, rồi run mới"
    );
    assert_eq!(
        chunks
            .iter()
            .map(|chunk| chunk.count as usize)
            .sum::<usize>(),
        143 + 200
    );
}
