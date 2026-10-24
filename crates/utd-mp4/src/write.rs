//! Ghi file ra trong một lượt: ftyp, moov (đã tính sẵn offset từng chunk), rồi mdat chép tuần tự
//! từ các nguồn. moov ở đầu nên mở và tua nhanh, không phải ghi lại cả file như faststart.

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::av1::DELIMITER;
use crate::boxes::{FourCc, Out};
use crate::table::{Chunk, Run, Sample, Track};
use crate::timing::{self, Timing};
use crate::{Brand, Error, Kind, unsupported};

const BUFFER: usize = 8 << 20;
const MOVIE_TIMESCALE: u32 = 1000;
const IDENTITY: [u8; 36] = [
    0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0x40, 0, 0, 0,
];

/// Phần đầu file ra: ftyp + moov + header mdat.
pub(crate) struct Head {
    pub bytes: Vec<u8>,
    pub moov_bytes: u64,
    /// Tổng byte mẫu trong mdat.
    pub payload: u64,
    pub co64: bool,
}

pub(crate) fn head(brand: Brand, tracks: &[Track], chunks: &[Chunk]) -> Result<Head, Error> {
    head_with_limit(brand, tracks, chunks, u64::from(u32::MAX))
}

/// `limit` là trần của offset/kích thước 32 bit (test hạ xuống để thử co64 và largesize).
pub(crate) fn head_with_limit(
    brand: Brand,
    tracks: &[Track],
    chunks: &[Chunk],
    limit: u64,
) -> Result<Head, Error> {
    let ftyp = ftyp(brand, tracks);
    let timings = tracks
        .iter()
        .map(|track| timing::timing(track, MOVIE_TIMESCALE))
        .collect::<Result<Vec<_>, _>>()?;
    let movie = Movie {
        tracks,
        timings: &timings,
        chunks,
    };
    let payload: u64 = chunks.iter().map(|chunk| chunk.size).sum();
    let large = payload + 8 > limit;
    let mdat_header = if large { 16 } else { 8 };
    let last = chunks.last().map_or(0, |chunk| chunk.size);
    // Kích thước moov không phụ thuộc giá trị offset, chỉ phụ thuộc stco hay co64.
    let mut co64 = false;
    let start = loop {
        let moov = movie.moov(0, co64).len() as u64;
        let start = ftyp.len() as u64 + moov + mdat_header;
        if co64 || start + payload - last <= limit {
            break start;
        }
        co64 = true;
    };
    let moov = movie.moov(start, co64);
    if moov.len() as u64 > u64::from(u32::MAX) {
        return Err(unsupported("moov vượt 4 GiB"));
    }
    let mut out = Out { buf: ftyp };
    out.bytes(&moov);
    if large {
        out.u32(1);
        out.bytes(b"mdat");
        out.u64(payload + 16);
    } else {
        out.u32((payload + 8) as u32);
        out.bytes(b"mdat");
    }
    Ok(Head {
        bytes: out.buf,
        moov_bytes: moov.len() as u64,
        payload,
        co64,
    })
}

fn ftyp(brand: Brand, tracks: &[Track]) -> Vec<u8> {
    let mut list: Vec<&FourCc> = match brand {
        Brand::M4a => vec![b"M4A ", b"M4A ", b"isom", b"iso2"],
        Brand::Mp4 => vec![b"isom", b"isom", b"iso2"],
    };
    let video = tracks.iter().find(|track| track.kind == Kind::Video);
    // Như ffmpeg: AV1/H.264 có brand riêng, HEVC thì không.
    match video.map(|track| &track.codec) {
        Some(b"av01") => list.push(b"av01"),
        Some(b"avc1" | b"avc3") => list.push(b"avc1"),
        _ => {}
    }
    list.push(b"mp41");
    let mut out = Out::default();
    let start = out.open(b"ftyp");
    out.bytes(list[0]);
    out.u32(0x200);
    for brand in &list[1..] {
        out.bytes(*brand);
    }
    out.close(start);
    out.buf
}

/// Vị trí (offset ở file ra) và số mẫu từng chunk, tách theo track; `start` là đầu payload mdat.
pub(crate) fn chunk_runs(tracks: usize, chunks: &[Chunk], start: u64) -> Vec<Vec<Run>> {
    let mut runs = vec![Vec::new(); tracks];
    let mut offset = start;
    for chunk in chunks {
        runs[chunk.track].push(Run {
            offset,
            count: chunk.count,
        });
        offset += chunk.size;
    }
    runs
}

/// Những gì moov cần: track, mốc thời gian đã tính, chunk theo thứ tự ghi.
struct Movie<'a> {
    tracks: &'a [Track],
    timings: &'a [Timing],
    chunks: &'a [Chunk],
}

impl Movie<'_> {
    fn moov(&self, start: u64, co64: bool) -> Vec<u8> {
        let mut out = Out::default();
        let moov = out.open(b"moov");
        let longest = self.timings.iter().map(|timing| timing.track).max();
        mvhd(&mut out, longest.unwrap_or(0), self.tracks.len() as u32 + 1);
        let runs = chunk_runs(self.tracks.len(), self.chunks, start);
        for (index, track) in self.tracks.iter().enumerate() {
            let id = index as u32 + 1;
            trak(
                &mut out,
                track,
                id,
                &self.timings[index],
                &runs[index],
                co64,
            );
        }
        out.close(moov);
        out.buf
    }
}

/// version 1 của mvhd/tkhd/mdhd khi giá trị ≥ INT32_MAX (ngưỡng của ffmpeg).
fn version(value: u64) -> u8 {
    u8::from(value >= i32::MAX as u64)
}

fn mvhd(out: &mut Out, duration: u64, next_track: u32) {
    let version = version(duration);
    let start = out.open_full(b"mvhd", version, 0);
    out.sized(version, 0);
    out.sized(version, 0);
    out.u32(MOVIE_TIMESCALE);
    out.sized(version, duration);
    out.u32(0x0001_0000);
    out.u16(0x0100);
    out.bytes(&[0; 10]);
    out.bytes(&IDENTITY);
    out.bytes(&[0; 24]);
    out.u32(next_track);
    out.close(start);
}

fn trak(out: &mut Out, track: &Track, id: u32, timing: &Timing, runs: &[Run], co64: bool) {
    let trak = out.open(b"trak");
    tkhd(out, track, id, timing.track);
    edts(out, timing);
    let mdia = out.open(b"mdia");
    mdhd(out, track, timing.media);
    out.bytes(&track.hdlr);
    let minf = out.open(b"minf");
    out.bytes(&track.media_header);
    out.bytes(&track.dinf);
    stbl(out, track, runs, co64);
    out.close(minf);
    out.close(mdia);
    out.close(trak);
}

fn tkhd(out: &mut Out, track: &Track, id: u32, duration: u64) {
    let version = version(duration);
    let audio = track.kind == Kind::Audio;
    // Cờ enabled | in_movie.
    let start = out.open_full(b"tkhd", version, 0x3);
    out.sized(version, 0);
    out.sized(version, 0);
    out.u32(id);
    out.u32(0);
    out.sized(version, duration);
    out.bytes(&[0; 8]);
    out.u16(0);
    // alternate_group: như ffmpeg, tiếng nhóm 1.
    out.u16(u16::from(audio));
    out.u16(if audio { 0x0100 } else { 0 });
    out.u16(0);
    out.bytes(&track.matrix);
    let (width, height) = if audio {
        (0, 0)
    } else {
        (track.width, track.height)
    };
    out.u32(width);
    out.u32(height);
    out.close(start);
}

/// Edit list một mục như ffmpeg: media_time bỏ phần DTS âm (B-frame, priming AAC), segment tính
/// theo PTS.
fn edts(out: &mut Out, timing: &Timing) {
    let edts = out.open(b"edts");
    let wide = u8::from(timing.wide);
    let elst = out.open_full(b"elst", wide, 0);
    out.u32(1);
    out.sized(wide, timing.segment);
    out.sized(wide, timing.media_time);
    out.u32(0x0001_0000);
    out.close(elst);
    out.close(edts);
}

fn mdhd(out: &mut Out, track: &Track, duration: u64) {
    let version = version(duration);
    let start = out.open_full(b"mdhd", version, 0);
    out.sized(version, 0);
    out.sized(version, 0);
    out.u32(track.timescale);
    out.sized(version, duration);
    out.u16(track.language);
    out.u16(0);
    out.close(start);
}

fn stbl(out: &mut Out, track: &Track, runs: &[Run], co64: bool) {
    let stbl = out.open(b"stbl");
    out.bytes(&track.stsd);
    stts(out, &track.samples);
    if !track.samples.iter().all(|sample| sample.sync) {
        stss(out, &track.samples);
    }
    if track.kind == Kind::Video && track.samples.iter().any(|sample| sample.cts != 0) {
        ctts(out, &track.samples);
    }
    stsc(out, runs);
    stsz(out, &track.samples);
    stco(out, runs, co64);
    if track.aac {
        roll(out, track.samples.len() as u32);
    }
    out.close(stbl);
}

fn stts(out: &mut Out, samples: &[Sample]) {
    let start = out.open_full(b"stts", 0, 0);
    let count = out.slot();
    let mut entries = 0;
    for group in samples.chunk_by(|left, right| left.duration == right.duration) {
        out.u32(group.len() as u32);
        out.u32(group[0].duration);
        entries += 1;
    }
    out.patch(count, entries);
    out.close(start);
}

fn stss(out: &mut Out, samples: &[Sample]) {
    let start = out.open_full(b"stss", 0, 0);
    let count = out.slot();
    let mut entries = 0;
    for (index, _) in samples.iter().enumerate().filter(|(_, sample)| sample.sync) {
        out.u32(index as u32 + 1);
        entries += 1;
    }
    out.patch(count, entries);
    out.close(start);
}

/// ctts version 0 (lệch luôn ≥ 0), gộp mẫu liền nhau cùng lệch như ffmpeg.
fn ctts(out: &mut Out, samples: &[Sample]) {
    let start = out.open_full(b"ctts", 0, 0);
    let count = out.slot();
    let mut entries = 0;
    for group in samples.chunk_by(|left, right| left.cts == right.cts) {
        out.u32(group.len() as u32);
        out.u32(group[0].cts);
        entries += 1;
    }
    out.patch(count, entries);
    out.close(start);
}

fn stsc(out: &mut Out, runs: &[Run]) {
    let start = out.open_full(b"stsc", 0, 0);
    let count = out.slot();
    let mut entries = 0;
    let mut previous = None;
    for (index, run) in runs.iter().enumerate() {
        if previous != Some(run.count) {
            out.u32(index as u32 + 1);
            out.u32(run.count);
            out.u32(1);
            entries += 1;
            previous = Some(run.count);
        }
    }
    out.patch(count, entries);
    out.close(start);
}

fn stsz(out: &mut Out, samples: &[Sample]) {
    let start = out.open_full(b"stsz", 0, 0);
    let uniform = samples
        .first()
        .map(|first| first.size)
        .filter(|size| *size != 0 && samples.iter().all(|sample| sample.size == *size));
    out.u32(uniform.unwrap_or(0));
    out.u32(samples.len() as u32);
    if uniform.is_none() {
        for sample in samples {
            out.u32(sample.size);
        }
    }
    out.close(start);
}

fn stco(out: &mut Out, runs: &[Run], co64: bool) {
    let start = out.open_full(if co64 { b"co64" } else { b"stco" }, 0, 0);
    out.u32(runs.len() as u32);
    for run in runs {
        if co64 {
            out.u64(run.offset);
        } else {
            out.u32(run.offset as u32);
        }
    }
    out.close(start);
}

/// AAC: nhóm 'roll' −1 cho mọi mẫu (ffmpeg ghi giống vậy): tua tới đâu giải mã trước một khung.
fn roll(out: &mut Out, count: u32) {
    let sgpd = out.open_full(b"sgpd", 1, 0);
    out.bytes(b"roll");
    out.u32(2);
    out.u32(1);
    out.u16(0xFFFF);
    out.close(sgpd);
    let sbgp = out.open_full(b"sbgp", 0, 0);
    out.bytes(b"roll");
    out.u32(1);
    out.u32(count);
    out.u32(1);
    out.close(sbgp);
}

struct Source {
    reader: BufReader<File>,
    pos: u64,
}

impl Source {
    /// Nhảy tới `offset`; nhảy ngắn (qua moof) vẫn giữ bộ đệm của reader.
    fn seek(&mut self, offset: u64) -> io::Result<()> {
        if offset != self.pos {
            let delta = i64::try_from(i128::from(offset) - i128::from(self.pos))
                .map_err(|_| io::Error::other("offset nguồn quá lớn"))?;
            self.reader.seek_relative(delta)?;
            self.pos = offset;
        }
        Ok(())
    }

    fn copy(&mut self, mut size: u64, writer: &mut impl Write) -> io::Result<()> {
        while size > 0 {
            let buffer = self.reader.fill_buf()?;
            if buffer.is_empty() {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            let take = buffer
                .len()
                .min(usize::try_from(size).unwrap_or(usize::MAX));
            writer.write_all(&buffer[..take])?;
            self.reader.consume(take);
            size -= take as u64;
            self.pos += take as u64;
        }
        Ok(())
    }

    /// Bỏ temporal delimiter đầu mẫu; mẫu không mở bằng nó thì moov đã ghi sai kích thước.
    fn skip_delimiter(&mut self) -> Result<(), Error> {
        let mut head = [0u8; DELIMITER.len()];
        self.reader.read_exact(&mut head)?;
        self.pos += head.len() as u64;
        if head != DELIMITER {
            return Err(unsupported("mẫu AV1 không mở bằng temporal delimiter"));
        }
        Ok(())
    }
}

/// Ghi `head` rồi chép từng chunk theo thứ tự; kiểm cờ huỷ mỗi chunk. Trả kích thước file ra.
pub(crate) fn write(
    head: &Head,
    tracks: &[Track],
    chunks: &[Chunk],
    sources: &[&Path],
    output: &Path,
    cancel: &AtomicBool,
) -> Result<u64, Error> {
    let mut sources = sources
        .iter()
        .map(|path| {
            Ok(Source {
                reader: BufReader::with_capacity(BUFFER, File::open(path)?),
                pos: 0,
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    let mut writer = BufWriter::with_capacity(BUFFER, File::create(output)?);
    writer.write_all(&head.bytes)?;
    for chunk in chunks {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let track = &tracks[chunk.track];
        let source = &mut sources[chunk.track];
        source.seek(chunk.source)?;
        if track.trim == 0 {
            source.copy(chunk.size, &mut writer)?;
            continue;
        }
        let first = chunk.first;
        for sample in &track.samples[first..first + chunk.count as usize] {
            source.skip_delimiter()?;
            source.copy(u64::from(sample.size), &mut writer)?;
        }
    }
    let file = writer
        .into_inner()
        .map_err(io::IntoInnerError::into_error)?;
    Ok(file.metadata()?.len())
}
