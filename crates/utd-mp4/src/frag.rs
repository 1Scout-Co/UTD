//! Đọc moof của fMP4: tfhd, tfdt, trun ⇒ thêm mẫu và run vào bảng của track.
//! Mặc định lấy theo thứ tự trun → tfhd → trex (ISO/IEC 14496-12, 8.8).

use crate::boxes::{Child, Cursor, broken, children, find, name, single};
use crate::table::{Run, Sample, Track};
use crate::{Error, unsupported};

// Cờ tfhd.
const BASE_DATA_OFFSET: u32 = 0x00_0001;
const SAMPLE_DESCRIPTION: u32 = 0x00_0002;
const DEFAULT_DURATION: u32 = 0x00_0008;
const DEFAULT_SIZE: u32 = 0x00_0010;
const DEFAULT_FLAGS: u32 = 0x00_0020;
const DURATION_IS_EMPTY: u32 = 0x01_0000;
// Cờ trun.
const DATA_OFFSET: u32 = 0x001;
const FIRST_SAMPLE_FLAGS: u32 = 0x004;
const SAMPLE_DURATION: u32 = 0x100;
const SAMPLE_SIZE: u32 = 0x200;
const SAMPLE_FLAGS: u32 = 0x400;
const SAMPLE_CTS: u32 = 0x800;
/// sample_is_non_sync_sample hoặc sample_depends_on = 1: không phải keyframe (cùng luật ffmpeg).
const NOT_SYNC: u32 = 0x0101_0000;
/// Trần số mẫu một track (≈ 6 ngày hình 60 fps), chặn file hỏng đòi cấp phát khổng lồ.
pub(crate) const MAX_SAMPLES: usize = 1 << 25;

const ENCRYPTED: [&[u8; 4]; 4] = [b"senc", b"saiz", b"saio", b"pssh"];
/// Box trong traf làm ffmpeg đổi cờ gói: sdtp (cờ disposable) mov.c đánh số theo cả track nên
/// đặt trong traf là lệch.
const UNCHECKED: [&[u8; 4]; 1] = [b"sdtp"];

/// Mặc định mẫu từ trex (moov/mvex), hoặc đã gộp với tfhd.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Defaults {
    pub sdi: u32,
    pub duration: u32,
    pub size: u32,
    pub flags: u32,
}

/// Track fMP4 đang đọc: bảng mẫu, mặc định trex và DTS nối tiếp của mẫu kế.
pub(crate) struct Stream {
    pub track: Track,
    pub trex: Defaults,
    pub decode_end: u64,
}

struct Header {
    base: u64,
    defaults: Defaults,
    empty: bool,
}

/// Đọc một moof đã nạp trọn, bắt đầu ở `start` trong file. Trả khoảng byte `[đầu, cuối)` các trun
/// trỏ tới (None nếu không có mẫu) để bên gọi kiểm nằm gọn trong mdat liền sau.
pub(crate) fn read_moof(
    moof: &[u8],
    start: u64,
    stream: &mut Stream,
) -> Result<Option<(u64, u64)>, Error> {
    let boxes = children(moof)?;
    reject_encrypted(&boxes)?;
    let mut trafs = boxes.iter().filter(|child| &child.kind == b"traf");
    let Some(traf) = trafs.next() else {
        return Ok(None);
    };
    if trafs.next().is_some() {
        return Err(unsupported("moof có nhiều traf"));
    }
    read_traf(traf.body, start, stream)
}

fn read_traf(body: &[u8], moof: u64, stream: &mut Stream) -> Result<Option<(u64, u64)>, Error> {
    let boxes = children(body)?;
    reject_encrypted(&boxes)?;
    if let Some(child) = boxes.iter().find(|child| UNCHECKED.contains(&&child.kind)) {
        return Err(unsupported(format!("traf có {}", name(&child.kind))));
    }
    let header = read_tfhd(single(&boxes, b"tfhd")?.body, moof, stream)?;
    if let Some(tfdt) = find(&boxes, b"tfdt") {
        let mut cursor = Cursor::new(tfdt.body);
        let (version, _) = cursor.full()?;
        align(stream, cursor.sized(version)?)?;
    }
    // trun không có data-offset nối tiếp ngay sau dữ liệu của trun trước.
    let mut next = header.base;
    let mut span: Option<(u64, u64)> = None;
    for trun in boxes.iter().filter(|child| &child.kind == b"trun") {
        let Some((first, end)) = read_trun(trun.body, &header, &mut next, stream)? else {
            continue;
        };
        if header.empty {
            return Err(unsupported("tfhd duration-is-empty mà có mẫu"));
        }
        span = Some(span.map_or((first, end), |(low, high)| (low.min(first), high.max(end))));
    }
    Ok(span)
}

fn reject_encrypted(boxes: &[Child<'_>]) -> Result<(), Error> {
    match boxes.iter().find(|child| ENCRYPTED.contains(&&child.kind)) {
        Some(child) => Err(unsupported(format!("mã hoá ({})", name(&child.kind)))),
        None => Ok(()),
    }
}

fn read_tfhd(body: &[u8], moof: u64, stream: &Stream) -> Result<Header, Error> {
    let mut cursor = Cursor::new(body);
    let (_, flags) = cursor.full()?;
    if cursor.u32()? != stream.track.id {
        return Err(unsupported("tfhd trỏ tới track khác"));
    }
    // Không có base-data-offset: base là đầu moof, dù có cờ default-base-is-moof hay không
    // (chỉ nhận một traf mỗi moof nên hai cách hiểu trùng nhau).
    let base = if flags & BASE_DATA_OFFSET != 0 {
        cursor.u64()?
    } else {
        moof
    };
    let mut defaults = stream.trex;
    if flags & SAMPLE_DESCRIPTION != 0 {
        defaults.sdi = cursor.u32()?;
    }
    if flags & DEFAULT_DURATION != 0 {
        defaults.duration = cursor.u32()?;
    }
    if flags & DEFAULT_SIZE != 0 {
        defaults.size = cursor.u32()?;
    }
    if flags & DEFAULT_FLAGS != 0 {
        defaults.flags = cursor.u32()?;
    }
    if defaults.sdi != 1 {
        return Err(unsupported(format!(
            "sample_description_index = {}",
            defaults.sdi
        )));
    }
    Ok(Header {
        base,
        defaults,
        empty: flags & DURATION_IS_EMPTY != 0,
    })
}

/// tfdt phải nối tiếp mẫu trước. Lớn hơn (thiếu đoạn) thì kéo dài mẫu trước như ffmpeg tính lại
/// từ DTS; lùi, hoặc fragment đầu không bắt đầu từ 0 (cần edit list), thì không ghép.
fn align(stream: &mut Stream, decode_time: u64) -> Result<(), Error> {
    if decode_time == stream.decode_end {
        return Ok(());
    }
    if decode_time < stream.decode_end {
        return Err(unsupported("tfdt lùi"));
    }
    let Some(last) = stream.track.samples.last_mut() else {
        return Err(unsupported("tfdt đầu tiên khác 0"));
    };
    let gap = decode_time - stream.decode_end;
    last.duration = u32::try_from(u64::from(last.duration) + gap)
        .map_err(|_| unsupported("khoảng trống tfdt quá lớn"))?;
    stream.decode_end = decode_time;
    Ok(())
}

fn read_trun(
    body: &[u8],
    header: &Header,
    next: &mut u64,
    stream: &mut Stream,
) -> Result<Option<(u64, u64)>, Error> {
    let mut cursor = Cursor::new(body);
    let (version, flags) = cursor.full()?;
    let count = cursor.u32()?;
    let start = if flags & DATA_OFFSET != 0 {
        let offset = cursor.u32()? as i32;
        header
            .base
            .checked_add_signed(i64::from(offset))
            .ok_or_else(|| broken("data_offset"))?
    } else {
        *next
    };
    let first_flags = if flags & FIRST_SAMPLE_FLAGS != 0 {
        Some(cursor.u32()?)
    } else {
        None
    };
    if first_flags.is_some() && flags & SAMPLE_FLAGS != 0 {
        return Err(unsupported(
            "trun có cả first-sample-flags lẫn sample-flags",
        ));
    }
    let layout = Layout {
        flags,
        signed: version == 1,
        first_flags,
    };
    let (bytes, duration) = read_samples(&mut cursor, &layout, count, header, stream)?;
    stream.decode_end += duration;
    *next = start.checked_add(bytes).ok_or_else(|| broken("trun"))?;
    if count == 0 {
        return Ok(None);
    }
    stream.track.runs.push(Run {
        offset: start,
        count,
    });
    Ok(Some((start, *next)))
}

/// Trường có mặt trong từng mẫu của một trun.
struct Layout {
    flags: u32,
    /// trun version 1: lệch cts có dấu.
    signed: bool,
    first_flags: Option<u32>,
}

/// Bảng mẫu của trun; trả tổng byte và tổng thời lượng.
fn read_samples(
    cursor: &mut Cursor<'_>,
    layout: &Layout,
    count: u32,
    header: &Header,
    stream: &mut Stream,
) -> Result<(u64, u64), Error> {
    let flags = layout.flags;
    let per_sample = 4 * (flags & 0xF00).count_ones() as usize;
    let samples = &mut stream.track.samples;
    if (count as usize).saturating_add(samples.len()) > MAX_SAMPLES
        || (count as usize).saturating_mul(per_sample) > cursor.remaining()
    {
        return Err(broken("số mẫu trun"));
    }
    let defaults = header.defaults;
    let (mut bytes, mut total) = (0u64, 0u64);
    for index in 0..count {
        let duration = field(cursor, flags & SAMPLE_DURATION, defaults.duration)?;
        let size = field(cursor, flags & SAMPLE_SIZE, defaults.size)?;
        let fallback = layout
            .first_flags
            .filter(|_| index == 0)
            .unwrap_or(defaults.flags);
        let sample_flags = field(cursor, flags & SAMPLE_FLAGS, fallback)?;
        let cts = field(cursor, flags & SAMPLE_CTS, 0)?;
        // mov.c đọc cts thành int; âm thì dịch DTS theo mức âm nhất đã gặp, đổi dần giữa các
        // fragment, nên chưa ghép.
        if (cts as i32) < 0 {
            return Err(unsupported(if layout.signed {
                "trun có lệch cts âm"
            } else {
                "lệch cts vượt 31 bit"
            }));
        }
        bytes += u64::from(size);
        total += u64::from(duration);
        samples.push(Sample {
            size,
            duration,
            sync: sample_flags & NOT_SYNC == 0,
            cts,
        });
    }
    Ok((bytes, total))
}

/// Trường có mặt (cờ khác 0) thì đọc, không thì dùng mặc định.
fn field(cursor: &mut Cursor<'_>, present: u32, default: u32) -> Result<u32, Error> {
    if present != 0 {
        cursor.u32()
    } else {
        Ok(default)
    }
}
