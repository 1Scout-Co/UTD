//! Đọc moov: mỗi trak lấy timescale/language (mdhd), kích thước/matrix (tkhd) và chép nguyên byte
//! stsd, hdlr, dinf, vmhd/smhd. File thường đọc luôn bảng mẫu stbl; fMP4 lấy mặc định từ trex và
//! edit list một mục của init (media_time ≥ 0).

use crate::Kind;
use crate::boxes::{Child, Cursor, FourCc, broken, children, find, name, single};
use crate::frag::Defaults;
use crate::table::Track;
use crate::{Error, stbl, unsupported};

/// File đang đọc là đầu vào (kiểm đủ luật ghép) hay file ra của crate (đọc lại để tự kiểm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Input,
    Output,
}

pub(crate) struct Movie {
    /// Có mvex: mẫu nằm trong các moof.
    pub fragmented: bool,
    pub tracks: Vec<Track>,
    /// Mặc định trex theo track_ID.
    pub trex: Vec<(u32, Defaults)>,
}

pub(crate) fn read_moov(body: &[u8], role: Role) -> Result<Movie, Error> {
    let boxes = children(body)?;
    if find(&boxes, b"pssh").is_some() {
        return Err(unsupported("mã hoá (pssh)"));
    }
    // Thẻ priming của iTunes: mov.c lấy làm start_pad của AAC, dời giờ bắt đầu.
    if body.windows(8).any(|window| window == b"iTunSMPB") {
        return Err(unsupported("có iTunSMPB"));
    }
    let mut mvhd = Cursor::new(single(&boxes, b"mvhd")?.body);
    let (version, _) = mvhd.full()?;
    mvhd.skip(if version == 1 { 16 } else { 8 })?;
    let timescale = mvhd.u32()?;
    if timescale == 0 {
        return Err(broken("mvhd timescale 0"));
    }
    let mvex = find(&boxes, b"mvex");
    let trex = match mvex {
        Some(mvex) => read_trex(mvex.body)?,
        None => Vec::new(),
    };
    let fragmented = mvex.is_some();
    let tracks = boxes
        .iter()
        .filter(|child| &child.kind == b"trak")
        .map(|trak| read_trak(trak.body, timescale, fragmented, role))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Movie {
        fragmented,
        tracks,
        trex,
    })
}

fn read_trex(mvex: &[u8]) -> Result<Vec<(u32, Defaults)>, Error> {
    let boxes = children(mvex)?;
    boxes
        .iter()
        .filter(|child| &child.kind == b"trex")
        .map(|trex| {
            let mut cursor = Cursor::new(trex.body);
            cursor.full()?;
            let id = cursor.u32()?;
            let defaults = Defaults {
                sdi: cursor.u32()?,
                duration: cursor.u32()?,
                size: cursor.u32()?,
                flags: cursor.u32()?,
            };
            Ok((id, defaults))
        })
        .collect()
}

fn read_trak(
    body: &[u8],
    movie_timescale: u32,
    fragmented: bool,
    role: Role,
) -> Result<Track, Error> {
    let boxes = children(body)?;
    let (mut track, stbl) = read_media(single(&boxes, b"mdia")?.body)?;
    read_tkhd(single(&boxes, b"tkhd")?.body, &mut track)?;
    let edts = find(&boxes, b"edts")
        .map(|_| single(&boxes, b"edts"))
        .transpose()?;
    stbl::check_sdtp(&stbl)?;
    if fragmented {
        if stbl::sample_count(&stbl)? != 0 {
            return Err(unsupported("fMP4 có mẫu ngay trong moov"));
        }
        if let Some(edts) = edts {
            read_init_edit(edts.body, &mut track)?;
        }
    } else {
        (track.samples, track.runs) = stbl::read(&stbl)?;
        if role == Role::Input {
            check_plain(edts, movie_timescale, &track)?;
        }
    }
    Ok(track)
}

fn read_tkhd(body: &[u8], track: &mut Track) -> Result<(), Error> {
    let mut cursor = Cursor::new(body);
    let (version, _) = cursor.full()?;
    cursor.skip(if version == 1 { 16 } else { 8 })?;
    track.id = cursor.u32()?;
    cursor.skip(4)?;
    cursor.sized(version)?;
    // reserved 8, layer, alternate_group, volume, reserved.
    cursor.skip(16)?;
    track.matrix.copy_from_slice(cursor.bytes(36)?);
    track.width = cursor.u32()?;
    track.height = cursor.u32()?;
    // Vài init để tkhd 0×0: lấy kích thước của mục mẫu (stsd 16 B + header mục 8 B + 24 B),
    // giống ffmpeg ghi tkhd theo codec.
    if track.kind == Kind::Video
        && (track.width == 0 || track.height == 0)
        && let Some(size) = track.stsd.get(48..52)
    {
        track.width = u32::from(u16::from_be_bytes([size[0], size[1]])) << 16;
        track.height = u32::from(u16::from_be_bytes([size[2], size[3]])) << 16;
    }
    Ok(())
}

/// mdia: mdhd, hdlr, minf (vmhd/smhd, dinf, stsd). Trả kèm box con của stbl để đọc bảng mẫu sau.
fn read_media(body: &[u8]) -> Result<(Track, Vec<Child<'_>>), Error> {
    let mdia = children(body)?;
    let mut mdhd = Cursor::new(single(&mdia, b"mdhd")?.body);
    let (version, _) = mdhd.full()?;
    mdhd.skip(if version == 1 { 16 } else { 8 })?;
    let timescale = mdhd.u32()?;
    mdhd.sized(version)?;
    let language = mdhd.u16()? & 0x7FFF;
    if timescale == 0 {
        return Err(broken("mdhd timescale 0"));
    }
    let hdlr = single(&mdia, b"hdlr")?;
    let kind = match hdlr.body.get(8..12) {
        Some(b"vide") => Kind::Video,
        Some(b"soun") => Kind::Audio,
        Some(other) => {
            return Err(unsupported(format!(
                "track {}",
                String::from_utf8_lossy(other)
            )));
        }
        None => return Err(broken("hdlr")),
    };
    let minf = children(single(&mdia, b"minf")?.body)?;
    let media_header = single(
        &minf,
        if kind == Kind::Video {
            b"vmhd"
        } else {
            b"smhd"
        },
    )?;
    let dinf = single(&minf, b"dinf")?;
    check_dinf(dinf.body)?;
    let stbl = children(single(&minf, b"stbl")?.body)?;
    let stsd = single(&stbl, b"stsd")?;
    let (codec, aac) = read_stsd(stsd.body, kind)?;
    let track = Track {
        kind,
        id: 0,
        timescale,
        language,
        matrix: [0; 36],
        width: 0,
        height: 0,
        codec,
        aac,
        trim: 0,
        edit: 0,
        priming: false,
        stsd: stsd.whole.to_vec(),
        hdlr: hdlr.whole.to_vec(),
        dinf: dinf.whole.to_vec(),
        media_header: media_header.whole.to_vec(),
        samples: Vec::new(),
        runs: Vec::new(),
    };
    Ok((track, stbl))
}

/// Dữ liệu phải nằm ngay trong file: một mục dref 'url ' cờ self-contained.
fn check_dinf(body: &[u8]) -> Result<(), Error> {
    let dinf = children(body)?;
    let dref = single(&dinf, b"dref")?;
    let entries = children(dref.body.get(8..).ok_or_else(|| broken("dref"))?)?;
    let local = match entries.as_slice() {
        [entry] => &entry.kind == b"url " && entry.body.get(3).is_some_and(|flags| flags & 1 == 1),
        _ => false,
    };
    if local {
        Ok(())
    } else {
        Err(unsupported("dữ liệu mẫu nằm ở file khác (dref)"))
    }
}

/// Đúng một mục mẫu, codec đã kiểm. mp4a chỉ nhận AAC (ISO, không phải bản QuickTime).
fn read_stsd(body: &[u8], kind: Kind) -> Result<(FourCc, bool), Error> {
    let mut cursor = Cursor::new(body);
    cursor.full()?;
    let count = cursor.u32()?;
    let entries = children(cursor.bytes(cursor.remaining())?)?;
    let ([entry], 1) = (entries.as_slice(), count) else {
        return Err(unsupported(format!("stsd có {count} mục")));
    };
    match (kind, &entry.kind) {
        (_, b"encv" | b"enca") => Err(unsupported("mã hoá (stsd)")),
        (Kind::Video, b"av01" | b"avc1" | b"avc3" | b"hvc1" | b"hev1") => Ok((entry.kind, false)),
        (Kind::Audio, b"mp4a") if is_aac(entry)? => Ok((entry.kind, true)),
        (_, b"mp4a") => Err(unsupported("mp4a không phải AAC")),
        (_, other) => Err(unsupported(format!("codec {}", name(other)))),
    }
}

fn is_aac(entry: &Child<'_>) -> Result<bool, Error> {
    // AudioSampleEntry: 8 byte chung + version (0 = ISO) ... đủ 28 byte rồi tới box con.
    let version = entry.body.get(8..10).ok_or_else(|| broken("mp4a"))?;
    if version != [0, 0] {
        return Err(unsupported("mp4a bản QuickTime"));
    }
    let (object_type, _) = esds(entry)?;
    // 0x40 MPEG-4 Audio; 0x66–0x68 AAC của MPEG-2 (ffmpeg cũng coi là AAC).
    Ok(matches!(object_type, 0x40 | 0x66..=0x68))
}

/// objectTypeIndication trong ES_Descriptor → DecoderConfigDescriptor, kèm DecoderSpecificInfo
/// (AudioSpecificConfig) nếu có.
fn esds<'a>(entry: &Child<'a>) -> Result<(u8, Option<&'a [u8]>), Error> {
    let boxes = children(entry.body.get(28..).ok_or_else(|| broken("mp4a"))?)?;
    let mut cursor = Cursor::new(single(&boxes, b"esds")?.body);
    cursor.full()?;
    descriptor(&mut cursor, 0x03)?;
    cursor.skip(2)?;
    let flags = cursor.u8()?;
    if flags & 0x80 != 0 {
        cursor.skip(2)?;
    }
    if flags & 0x40 != 0 {
        let length = cursor.u8()?;
        cursor.skip(length.into())?;
    }
    if flags & 0x20 != 0 {
        cursor.skip(2)?;
    }
    descriptor(&mut cursor, 0x04)?;
    let object_type = cursor.u8()?;
    // streamType, bufferSizeDB, maxBitrate, avgBitrate; DecoderSpecificInfo có thể vắng.
    let config = cursor
        .skip(12)
        .and_then(|()| descriptor(&mut cursor, 0x05))
        .and_then(|length| cursor.bytes(length as usize))
        .ok();
    Ok((object_type, config))
}

/// Thẻ descriptor và độ dài (1–4 byte, bit cao = còn tiếp).
fn descriptor(cursor: &mut Cursor<'_>, tag: u8) -> Result<u32, Error> {
    if cursor.u8()? != tag {
        return Err(broken("esds"));
    }
    let mut length = 0;
    for _ in 0..4 {
        let byte = cursor.u8()?;
        length = (length << 7) | u32::from(byte & 0x7F);
        if byte & 0x80 == 0 {
            return Ok(length);
        }
    }
    Err(broken("esds"))
}

/// File thường: chưa nhận B-frame (mov.c sửa mốc theo edit list bằng mov_fix_index, có bỏ gói,
/// khác hẳn fMP4) và chỉ nhận edit list "không làm gì" (một mục, media_time 0, tốc độ 1, phủ cả
/// track), vì file ra tính lại elst như thể không có.
fn check_plain(edts: Option<Child<'_>>, movie_timescale: u32, track: &Track) -> Result<(), Error> {
    if track.samples.iter().any(|sample| sample.cts != 0) {
        return Err(unsupported("có ctts (B-frame trong file thường)"));
    }
    let Some(edts) = edts else {
        return Ok(());
    };
    let boxes = children(edts.body)?;
    let Some(elst) = find(&boxes, b"elst") else {
        return Ok(());
    };
    let mut cursor = Cursor::new(elst.body);
    let (version, _) = cursor.full()?;
    if cursor.u32()? != 1 {
        return Err(unsupported("edit list nhiều mục"));
    }
    let segment = cursor.sized(version)?;
    let media_time = cursor.sized(version)?;
    let rate = cursor.u32()?;
    let whole =
        u128::from(track.duration()) * u128::from(movie_timescale) / u128::from(track.timescale);
    if media_time != 0 || rate != 0x0001_0000 || (segment != 0 && u128::from(segment) < whole) {
        return Err(unsupported("edit list dịch hoặc cắt thời gian"));
    }
    Ok(())
}

/// Edit list của init fMP4. mov.c (fMP4 tắt advanced_editlist) chỉ lấy media_time của mục đầu
/// làm time_offset (DTS lùi từng ấy) và bỏ qua segment_duration; với AAC còn lấy làm start_pad.
/// Chỉ nhận một mục, media_time ≥ 0, tốc độ 1.
fn read_init_edit(edts: &[u8], track: &mut Track) -> Result<(), Error> {
    let boxes = children(edts)?;
    if find(&boxes, b"elst").is_none() {
        return Ok(());
    }
    let mut cursor = Cursor::new(single(&boxes, b"elst")?.body);
    let (version, _) = cursor.full()?;
    let count = cursor.u32()?;
    let width = if version == 1 { 20 } else { 12 };
    if cursor.remaining() as u64 != u64::from(count) * width {
        return Err(broken("elst"));
    }
    match count {
        0 => return Ok(()),
        1 => {}
        _ => return Err(unsupported("edit list nhiều mục")),
    }
    cursor.sized(version)?;
    let media_time = cursor.sized(version)?;
    let sign = if version == 1 { 63 } else { 31 };
    if media_time >> sign != 0 {
        return Err(unsupported("edit list có đoạn trống"));
    }
    if cursor.u32()? != 0x0001_0000 {
        return Err(unsupported("edit list đổi tốc độ"));
    }
    if track.kind == Kind::Audio && media_time > 0 {
        // demux.c cộng start_pad vào start_time theo tần số mẫu của codec: chỉ nhận khi tần số
        // đó chắc chắn bằng timescale (AAC-LC không SBR, xem aac_lc_rate).
        if !track.aac || aac_lc_rate(&track.stsd) != Some(track.timescale) {
            return Err(unsupported("priming AAC với cấu hình chưa kiểm"));
        }
        track.priming = true;
    }
    track.edit = media_time;
    Ok(())
}

/// Tần số mẫu ffmpeg đọc từ AudioSpecificConfig, chỉ khi chắc: AAC-LC (object type 2), tần số theo
/// bảng, có kênh, không core coder; sau 16 bit đầu là byte 0 hoặc khối sync báo vắng SBR
/// (`56 E5`, bit sbr = 0 — ffmpeg tự ghi khi mã hoá AAC). Cấu hình khác trả None.
fn aac_lc_rate(stsd: &[u8]) -> Option<u32> {
    const RATES: [u32; 13] = [
        96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
    ];
    let boxes = children(stsd).ok()?;
    let entries = children(single(&boxes, b"stsd").ok()?.body.get(8..)?).ok()?;
    let (_, config) = esds(entries.first()?).ok()?;
    let (&[first, second], rest) = config?.split_first_chunk::<2>()?;
    let channels = (second >> 3) & 0x0F;
    let no_sbr = rest.iter().all(|byte| *byte == 0)
        || matches!(rest, [0x56, 0xE5, third, ..] if third & 0x80 == 0);
    if first >> 3 != 2 || channels == 0 || second & 0x03 != 0 || !no_sbr {
        return None;
    }
    RATES
        .get(usize::from(((first & 0x07) << 1) | (second >> 7)))
        .copied()
}
