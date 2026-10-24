//! Đọc moov: mỗi trak lấy timescale/language (mdhd), kích thước/matrix (tkhd) và chép nguyên byte
//! stsd, hdlr, dinf, vmhd/smhd. File thường đọc luôn bảng mẫu stbl; fMP4 lấy mặc định từ trex.

use crate::Kind;
use crate::boxes::{Child, Cursor, FourCc, broken, children, find, name, single};
use crate::frag::Defaults;
use crate::table::Track;
use crate::{Error, stbl, unsupported};

pub(crate) struct Movie {
    /// Có mvex: mẫu nằm trong các moof.
    pub fragmented: bool,
    pub tracks: Vec<Track>,
    /// Mặc định trex theo track_ID.
    pub trex: Vec<(u32, Defaults)>,
}

pub(crate) fn read_moov(body: &[u8]) -> Result<Movie, Error> {
    let boxes = children(body)?;
    if find(&boxes, b"pssh").is_some() {
        return Err(unsupported("mã hoá (pssh)"));
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
        .map(|trak| read_trak(trak.body, timescale, fragmented))
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

fn read_trak(body: &[u8], movie_timescale: u32, fragmented: bool) -> Result<Track, Error> {
    let boxes = children(body)?;
    let (mut track, stbl) = read_media(single(&boxes, b"mdia")?.body)?;
    read_tkhd(single(&boxes, b"tkhd")?.body, &mut track)?;
    let edts = find(&boxes, b"edts");
    if fragmented {
        if stbl::sample_count(&stbl)? != 0 {
            return Err(unsupported("fMP4 có mẫu ngay trong moov"));
        }
        if edts.is_some() {
            return Err(unsupported("init có edts"));
        }
    } else {
        (track.samples, track.runs) = stbl::read(&stbl)?;
        check_edit(edts, movie_timescale, &track)?;
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
    let boxes = children(entry.body.get(28..).ok_or_else(|| broken("mp4a"))?)?;
    let esds = single(&boxes, b"esds")?;
    let object_type = esds_object_type(esds.body)?;
    // 0x40 MPEG-4 Audio; 0x66–0x68 AAC của MPEG-2 (ffmpeg cũng coi là AAC).
    Ok(matches!(object_type, 0x40 | 0x66..=0x68))
}

/// objectTypeIndication trong ES_Descriptor → DecoderConfigDescriptor.
fn esds_object_type(body: &[u8]) -> Result<u8, Error> {
    let mut cursor = Cursor::new(body);
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
    cursor.u8()
}

/// Thẻ descriptor và độ dài (1–4 byte, bit cao = còn tiếp).
fn descriptor(cursor: &mut Cursor<'_>, tag: u8) -> Result<(), Error> {
    if cursor.u8()? != tag {
        return Err(broken("esds"));
    }
    for _ in 0..4 {
        if cursor.u8()? & 0x80 == 0 {
            return Ok(());
        }
    }
    Err(broken("esds"))
}

/// File thường: chỉ nhận edit list "không làm gì" (một mục, media_time 0, tốc độ 1, phủ cả track),
/// vì file ra luôn ghi lại elst (duration, 0).
fn check_edit(edts: Option<Child<'_>>, movie_timescale: u32, track: &Track) -> Result<(), Error> {
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
