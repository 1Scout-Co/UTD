//! Dựng fMP4/MP4 nhỏ trong RAM cho test: helper ghi box, init (moov + mvex), từng moof + mdat với
//! các biến thể cờ tfhd/trun, và file thường với stbl đầy đủ. Tự tính mẫu mong đợi (giải mặc định
//! trun → tfhd → trex độc lập với bộ đọc) và byte dữ liệu từng mẫu.

use std::path::{Path, PathBuf};

use crate::Kind;
use crate::table::Sample;

pub(super) fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

pub(super) fn full(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut inner = ((u32::from(version) << 24) | flags).to_be_bytes().to_vec();
    inner.extend_from_slice(body);
    bx(kind, &inner)
}

const IDENTITY: [u8; 36] = [
    0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0x40, 0, 0, 0,
];

fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect()
}

/// Thư mục tạm riêng mỗi test, tự xoá.
pub(super) struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("utd-mp4-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
pub(super) struct Init {
    pub kind: Kind,
    pub codec: [u8; 4],
    pub timescale: u32,
    /// sdi, duration, size, flags của trex.
    pub trex: [u32; 4],
    /// objectTypeIndication của esds (0x40 = AAC).
    pub object_type: u8,
    /// edts đặt trong trak của init (xem `edit`).
    pub edts: Option<Vec<u8>>,
    /// Box thêm vào cuối moov (udta…).
    pub extra: Vec<u8>,
}

impl Init {
    pub fn video() -> Self {
        Self {
            kind: Kind::Video,
            codec: *b"avc1",
            timescale: 15360,
            trex: [1, 256, 0, 0x0101_0000],
            object_type: 0x40,
            edts: None,
            extra: Vec::new(),
        }
    }

    pub fn audio() -> Self {
        Self {
            kind: Kind::Audio,
            codec: *b"mp4a",
            timescale: 48000,
            trex: [1, 1024, 0, 0],
            object_type: 0x40,
            edts: None,
            extra: Vec::new(),
        }
    }

    fn entry(&self) -> Vec<u8> {
        let mut body = vec![0; 6];
        body.extend_from_slice(&1u16.to_be_bytes());
        if self.kind == Kind::Audio {
            body.extend_from_slice(&[0; 8]);
            body.extend_from_slice(&[0, 2, 0, 16, 0, 0, 0, 0]);
            body.extend_from_slice(&(48000u32 << 16).to_be_bytes());
            let config = [0x05, 0x02, 0x11, 0x90, 0x06, 0x01, 0x02];
            let mut decoder = vec![0x04, 13 + 4, self.object_type, 0x15, 0, 0, 0];
            decoder.extend_from_slice(&words(&[128_000, 128_000]));
            decoder.extend_from_slice(&config[..4]);
            let mut es = vec![0x03, (3 + decoder.len() + 3) as u8, 0, 1, 0];
            es.extend_from_slice(&decoder);
            es.extend_from_slice(&config[4..]);
            body.extend_from_slice(&full(b"esds", 0, 0, &es));
        } else {
            body.extend_from_slice(&[0; 16]);
            body.extend_from_slice(&[
                1, 0x40, 0, 0xF0, 0, 0x48, 0, 0, 0, 0x48, 0, 0, 0, 0, 0, 0, 0, 1,
            ]);
            body.extend_from_slice(&[0; 32]);
            body.extend_from_slice(&[0, 0x18, 0xFF, 0xFF]);
        }
        bx(&self.codec, &body)
    }

    /// trak với `tables` là phần stbl sau stsd.
    pub fn trak(&self, tables: &[u8], edts: Option<Vec<u8>>) -> Vec<u8> {
        let video = self.kind == Kind::Video;
        let mut tkhd = words(&[0, 0, 1, 0, 0, 0, 0]);
        tkhd.extend_from_slice(&[0, 0, 0, 0, u8::from(!video), 0, 0, 0]);
        tkhd.extend_from_slice(&IDENTITY);
        tkhd.extend_from_slice(&words(if video {
            &[320 << 16, 240 << 16]
        } else {
            &[0, 0]
        }));
        let handler = if video { b"vide" } else { b"soun" };
        let mut hdlr = vec![0; 4];
        hdlr.extend_from_slice(handler);
        hdlr.extend_from_slice(&[0; 12]);
        hdlr.extend_from_slice(b"Test\0");
        let media_header = if video {
            full(b"vmhd", 0, 1, &[0; 8])
        } else {
            full(b"smhd", 0, 0, &[0; 4])
        };
        let dref = full(
            b"dref",
            0,
            0,
            &[&words(&[1])[..], &full(b"url ", 0, 1, &[])].concat(),
        );
        let stsd = full(b"stsd", 0, 0, &[&words(&[1])[..], &self.entry()].concat());
        let stbl = bx(b"stbl", &[stsd, tables.to_vec()].concat());
        let minf = bx(b"minf", &[media_header, bx(b"dinf", &dref), stbl].concat());
        let mdhd = full(
            b"mdhd",
            0,
            0,
            &[&words(&[0, 0, self.timescale, 0])[..], &[0x55, 0xC4, 0, 0]].concat(),
        );
        let mdia = bx(b"mdia", &[mdhd, full(b"hdlr", 0, 0, &hdlr), minf].concat());
        let edts = edts.or_else(|| self.edts.clone());
        bx(
            b"trak",
            &[full(b"tkhd", 0, 3, &tkhd), edts.unwrap_or_default(), mdia].concat(),
        )
    }
}

/// edts một mục (segment, media_time, tốc độ 1).
pub(super) fn edit(segment: u32, media_time: u32) -> Vec<u8> {
    edits(&[(segment, media_time, 0x0001_0000)])
}

/// edts với các mục (segment, media_time, tốc độ 16.16) version 0.
pub(super) fn edits(entries: &[(u32, u32, u32)]) -> Vec<u8> {
    let mut body = words(&[entries.len() as u32]);
    for (segment, media_time, rate) in entries {
        body.extend_from_slice(&words(&[*segment, *media_time, *rate]));
    }
    bx(b"edts", &full(b"elst", 0, 0, &body))
}

fn mvhd() -> Vec<u8> {
    let mut body = words(&[0, 0, 1000, 0, 0x0001_0000]);
    body.extend_from_slice(&[1, 0]);
    body.extend_from_slice(&[0; 10]);
    body.extend_from_slice(&IDENTITY);
    body.extend_from_slice(&[0; 24]);
    body.extend_from_slice(&words(&[2]));
    full(b"mvhd", 0, 0, &body)
}

#[derive(Clone, Copy, Default)]
pub(super) struct Smp {
    pub duration: u32,
    pub size: u32,
    pub flags: u32,
    pub cts: u32,
}

pub(super) fn smp(duration: u32, size: u32, flags: u32) -> Smp {
    Smp {
        duration,
        size,
        flags,
        cts: 0,
    }
}

#[derive(Clone, Default)]
pub(super) struct Trun {
    /// 1: lệch cts có dấu.
    pub version: u8,
    pub flags: u32,
    pub first_flags: u32,
    pub samples: Vec<Smp>,
}

#[derive(Clone)]
pub(super) struct Frag {
    pub tfhd_flags: u32,
    pub sdi: u32,
    /// duration, size, flags mặc định của tfhd (ghi khi có cờ tương ứng).
    pub defaults: [u32; 3],
    pub tfdt: Option<u64>,
    pub truns: Vec<Trun>,
    pub trafs: usize,
    /// Box thêm vào traf (senc…).
    pub extra: Vec<u8>,
    /// Lệch data-offset (thử dữ liệu nằm ngoài mdat).
    pub shift: i64,
}

impl Frag {
    pub fn new(tfhd_flags: u32, truns: Vec<Trun>) -> Self {
        Self {
            tfhd_flags,
            sdi: 1,
            defaults: [0; 3],
            tfdt: None,
            truns,
            trafs: 1,
            extra: Vec::new(),
            shift: 0,
        }
    }
}

/// File đang dựng, kèm bảng mẫu mong đợi và byte từng mẫu (đã bỏ delimiter nếu là AV1).
pub(super) struct Built {
    pub init: Init,
    pub bytes: Vec<u8>,
    pub samples: Vec<Sample>,
    pub payloads: Vec<Vec<u8>>,
    /// Vị trí từng mẫu trong `bytes` (để làm hỏng một mẫu).
    pub offsets: Vec<u64>,
    pub decode_end: u64,
}

/// Byte mẫu thứ `number`: AV1 mở bằng temporal delimiter như YouTube.
fn payload(init: &Init, number: usize, size: u32) -> (Vec<u8>, Vec<u8>) {
    let data: Vec<u8> = (0..size as usize)
        .map(|index| (number * 7 + index * 13 + 1) as u8)
        .collect();
    if &init.codec == b"av01" && size >= 2 {
        let mut source = data.clone();
        source[..2].copy_from_slice(&crate::av1::DELIMITER);
        return (source.clone(), source[2..].to_vec());
    }
    (data.clone(), data)
}

impl Built {
    pub fn fragmented(init: Init) -> Self {
        let trex = full(
            b"trex",
            0,
            0,
            &words(&[1, init.trex[0], init.trex[1], init.trex[2], init.trex[3]]),
        );
        let moov = bx(
            b"moov",
            &[
                mvhd(),
                init.trak(&empty_tables(), None),
                bx(b"mvex", &trex),
                init.extra.clone(),
            ]
            .concat(),
        );
        let bytes = [bx(b"ftyp", b"iso6\0\0\0\0iso6mp41"), moov].concat();
        Self {
            init,
            bytes,
            samples: Vec::new(),
            payloads: Vec::new(),
            offsets: Vec::new(),
            decode_end: 0,
        }
    }

    pub fn save(&self, scratch: &Scratch, name: &str) -> PathBuf {
        scratch.file(name, &self.bytes)
    }

    /// Thêm một moof + mdat theo `frag`.
    pub fn fragment(&mut self, frag: &Frag) -> &mut Self {
        let resolved = self.resolve(frag);
        let moof_start = self.bytes.len() as u64;
        let draft = moof(frag, &resolved, 0, &[]);
        let payload_start = moof_start + draft.len() as u64 + 8;
        let base = if frag.tfhd_flags & 0x01 != 0 {
            payload_start
        } else {
            moof_start
        };
        let mut offsets = Vec::new();
        let mut cursor = payload_start;
        for run in &resolved {
            offsets.push((cursor as i64 - base as i64 + frag.shift) as i32);
            cursor += run.iter().map(|sample| u64::from(sample.size)).sum::<u64>();
        }
        let moof = moof(frag, &resolved, base, &offsets);
        let mut data = Vec::new();
        for sample in resolved.iter().flatten() {
            let number = self.samples.len();
            let (source, output) = payload(&self.init, number, sample.size);
            self.offsets.push(payload_start + data.len() as u64);
            data.extend_from_slice(&source);
            let trim = (source.len() - output.len()) as u32;
            self.samples.push(Sample {
                size: sample.size - trim,
                duration: sample.duration,
                sync: sample.flags & 0x0101_0000 == 0,
                cts: sample.cts,
            });
            self.payloads.push(output);
            self.decode_end += u64::from(sample.duration);
        }
        self.bytes.extend_from_slice(&moof);
        self.bytes.extend_from_slice(&bx(b"mdat", &data));
        self
    }

    /// Giải mặc định trun → tfhd → trex cho từng mẫu (cờ đầy đủ để tính keyframe).
    fn resolve(&self, frag: &Frag) -> Vec<Vec<Smp>> {
        let flags = frag.tfhd_flags;
        let pick = |bit: u32, tfhd: u32, trex: u32| if flags & bit != 0 { tfhd } else { trex };
        let duration = pick(0x08, frag.defaults[0], self.init.trex[1]);
        let size = pick(0x10, frag.defaults[1], self.init.trex[2]);
        let sample_flags = pick(0x20, frag.defaults[2], self.init.trex[3]);
        let run = |trun: &Trun| -> Vec<Smp> {
            trun.samples
                .iter()
                .enumerate()
                .map(|(index, sample)| Smp {
                    duration: if trun.flags & 0x100 != 0 {
                        sample.duration
                    } else {
                        duration
                    },
                    size: if trun.flags & 0x200 != 0 {
                        sample.size
                    } else {
                        size
                    },
                    flags: match (
                        trun.flags & 0x400 != 0,
                        index == 0 && trun.flags & 0x004 != 0,
                    ) {
                        (true, _) => sample.flags,
                        (false, true) => trun.first_flags,
                        (false, false) => sample_flags,
                    },
                    cts: sample.cts,
                })
                .collect()
        };
        frag.truns.iter().map(run).collect()
    }
}

fn moof(frag: &Frag, resolved: &[Vec<Smp>], base: u64, offsets: &[i32]) -> Vec<u8> {
    let mut tfhd = words(&[1]);
    if frag.tfhd_flags & 0x01 != 0 {
        tfhd.extend_from_slice(&base.to_be_bytes());
    }
    for (bit, value) in [
        (0x02, frag.sdi),
        (0x08, frag.defaults[0]),
        (0x10, frag.defaults[1]),
        (0x20, frag.defaults[2]),
    ] {
        if frag.tfhd_flags & bit != 0 {
            tfhd.extend_from_slice(&value.to_be_bytes());
        }
    }
    let mut traf = full(b"tfhd", 0, frag.tfhd_flags, &tfhd);
    if let Some(time) = frag.tfdt {
        traf.extend_from_slice(&full(b"tfdt", 1, 0, &time.to_be_bytes()));
    }
    for (index, trun) in frag.truns.iter().enumerate() {
        let mut body = words(&[trun.samples.len() as u32]);
        if trun.flags & 0x001 != 0 {
            body.extend_from_slice(&offsets.get(index).copied().unwrap_or(0).to_be_bytes());
        }
        if trun.flags & 0x004 != 0 {
            body.extend_from_slice(&trun.first_flags.to_be_bytes());
        }
        for sample in &resolved[index] {
            for (bit, value) in [
                (0x100, sample.duration),
                (0x200, sample.size),
                (0x400, sample.flags),
                (0x800, sample.cts),
            ] {
                if trun.flags & bit != 0 {
                    body.extend_from_slice(&value.to_be_bytes());
                }
            }
        }
        traf.extend_from_slice(&full(b"trun", trun.version, trun.flags, &body));
    }
    traf.extend_from_slice(&frag.extra);
    let traf = bx(b"traf", &traf);
    let mut moof = full(b"mfhd", 0, 0, &words(&[1]));
    for _ in 0..frag.trafs {
        moof.extend_from_slice(&traf);
    }
    bx(b"moof", &moof)
}

fn empty_tables() -> Vec<u8> {
    [
        full(b"stts", 0, 0, &words(&[0])),
        full(b"stsc", 0, 0, &words(&[0])),
        full(b"stsz", 0, 0, &words(&[0, 0])),
        full(b"stco", 0, 0, &words(&[0])),
    ]
    .concat()
}

/// Tuỳ chọn file thường.
#[derive(Clone, Default)]
pub(super) struct Plain {
    pub per_chunk: u32,
    pub stz2: bool,
    /// Lệch cts của mẫu thứ hai (0 = không có ctts).
    pub cts: u32,
    pub edts: Option<Vec<u8>>,
    /// Một byte sdtp mỗi mẫu (rỗng = không có sdtp).
    pub sdtp: Vec<u8>,
}

/// File thường: ftyp, mdat, moov với stbl đầy đủ (mdat trước moov như ffmpeg).
pub(super) fn plain(init: &Init, samples: &[Smp], options: &Plain) -> Built {
    let mut built = Built::fragmented(init.clone());
    built.bytes = bx(b"ftyp", b"isom\0\0\x02\0isommp41");
    let payload_start = built.bytes.len() as u64 + 8;
    let mut data = Vec::new();
    let mut offsets = Vec::new();
    for (number, sample) in samples.iter().enumerate() {
        if number % options.per_chunk as usize == 0 {
            offsets.push((payload_start + data.len() as u64) as u32);
        }
        let (source, output) = payload(init, number, sample.size);
        data.extend_from_slice(&source);
        built.samples.push(Sample {
            size: output.len() as u32,
            duration: sample.duration,
            sync: sample.flags & 0x0101_0000 == 0,
            cts: if number == 1 { options.cts } else { 0 },
        });
        built.payloads.push(output);
    }
    built.bytes.extend_from_slice(&bx(b"mdat", &data));
    let tables = plain_tables(samples, options, &offsets);
    let moov = bx(
        b"moov",
        &[mvhd(), init.trak(&tables, options.edts.clone())].concat(),
    );
    built.bytes.extend_from_slice(&moov);
    built
}

fn plain_tables(samples: &[Smp], options: &Plain, offsets: &[u32]) -> Vec<u8> {
    let count = samples.len() as u32;
    let mut stts = words(&[count]);
    samples
        .iter()
        .for_each(|sample| stts.extend_from_slice(&words(&[1, sample.duration])));
    let sync: Vec<u32> = (1..=count)
        .filter(|number| samples[*number as usize - 1].flags & 0x0101_0000 == 0)
        .collect();
    let sizes = if options.stz2 {
        let mut body = vec![0, 0, 0, 8];
        body.extend_from_slice(&words(&[count]));
        body.extend(samples.iter().map(|sample| sample.size as u8));
        full(b"stz2", 0, 0, &body)
    } else {
        let mut body = words(&[0, count]);
        samples
            .iter()
            .for_each(|sample| body.extend_from_slice(&words(&[sample.size])));
        full(b"stsz", 0, 0, &body)
    };
    let mut tables = full(b"stts", 0, 0, &stts);
    if options.cts != 0 {
        let rest = count - 2;
        let entries = words(&[3, 1, 0, 1, options.cts, rest, 0]);
        tables.extend_from_slice(&full(b"ctts", 0, 0, &entries));
    }
    tables.extend_from_slice(&full(
        b"stss",
        0,
        0,
        &[&words(&[sync.len() as u32])[..], &words(&sync)].concat(),
    ));
    if !options.sdtp.is_empty() {
        tables.extend_from_slice(&full(b"sdtp", 0, 0, &options.sdtp));
    }
    let (whole, rest) = (count / options.per_chunk, count % options.per_chunk);
    let mut stsc = vec![(1, options.per_chunk)];
    if rest != 0 {
        stsc.push((whole + 1, rest));
    }
    let mut body = words(&[stsc.len() as u32]);
    stsc.iter()
        .for_each(|(first, per)| body.extend_from_slice(&words(&[*first, *per, 1])));
    tables.extend_from_slice(&full(b"stsc", 0, 0, &body));
    tables.extend_from_slice(&sizes);
    tables.extend_from_slice(&full(
        b"stco",
        0,
        0,
        &[&words(&[offsets.len() as u32])[..], &words(offsets)].concat(),
    ));
    tables
}

/// Byte dữ liệu từng mẫu của một track đọc lại từ file ra (theo run + kích thước).
pub(super) fn sample_bytes(path: &Path, track: &crate::table::Track) -> Vec<Vec<u8>> {
    let file = std::fs::read(path).unwrap();
    let mut samples = track.samples.iter();
    let mut out = Vec::new();
    for run in &track.runs {
        let mut offset = run.offset as usize;
        for sample in samples.by_ref().take(run.count as usize) {
            out.push(file[offset..offset + sample.size as usize].to_vec());
            offset += sample.size as usize;
        }
    }
    out
}
