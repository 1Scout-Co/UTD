//! Bảng mẫu của file MP4 thường (stsz/stz2, stco/co64, stsc, stts, stss) mở thành danh sách mẫu;
//! mỗi chunk là một run (khoảng byte liền).

use crate::boxes::{Child, Cursor, broken, find, single};
use crate::frag::MAX_SAMPLES;
use crate::table::{Run, Sample};
use crate::{Error, unsupported};

/// Số mẫu khai trong stsz/stz2 (0 nếu không có bảng), để kiểm init của fMP4.
pub(crate) fn sample_count(stbl: &[Child<'_>]) -> Result<u32, Error> {
    let Some(table) = find(stbl, b"stsz").or_else(|| find(stbl, b"stz2")) else {
        return Ok(0);
    };
    let mut cursor = Cursor::new(table.body);
    cursor.skip(8)?;
    cursor.u32()
}

pub(crate) fn read(stbl: &[Child<'_>]) -> Result<(Vec<Sample>, Vec<Run>), Error> {
    if let Some(ctts) = find(stbl, b"ctts") {
        check_ctts(ctts.body)?;
    }
    let mut samples = read_sizes(stbl)?;
    read_stts(single(stbl, b"stts")?.body, &mut samples)?;
    if let Some(stss) = find(stbl, b"stss") {
        read_stss(stss.body, &mut samples)?;
    }
    let offsets = read_offsets(stbl)?;
    let runs = read_stsc(single(stbl, b"stsc")?.body, &offsets, samples.len())?;
    Ok((samples, runs))
}

/// Mục đầu của bảng: số phần tử, kiểm đủ byte cho `width` byte mỗi phần tử.
fn entries(cursor: &mut Cursor<'_>, width: usize) -> Result<usize, Error> {
    let count = cursor.u32()? as usize;
    if count > MAX_SAMPLES || count.saturating_mul(width) > cursor.remaining() {
        return Err(broken("số mục bảng mẫu"));
    }
    Ok(count)
}

fn sample(size: u32) -> Sample {
    Sample {
        size,
        duration: 0,
        sync: true,
    }
}

fn read_sizes(stbl: &[Child<'_>]) -> Result<Vec<Sample>, Error> {
    if let Some(stsz) = find(stbl, b"stsz") {
        let mut cursor = Cursor::new(stsz.body);
        cursor.full()?;
        let uniform = cursor.u32()?;
        let count = entries(&mut cursor, if uniform == 0 { 4 } else { 0 })?;
        return (0..count)
            .map(|_| match uniform {
                0 => cursor.u32().map(sample),
                size => Ok(sample(size)),
            })
            .collect();
    }
    let stz2 = find(stbl, b"stz2").ok_or_else(|| broken("thiếu stsz"))?;
    let mut cursor = Cursor::new(stz2.body);
    cursor.full()?;
    cursor.skip(3)?;
    let field = cursor.u8()?;
    let count = entries(&mut cursor, 0)?;
    match field {
        4 => {
            let packed = cursor.bytes(count.div_ceil(2))?;
            let nibble = |index: usize| (packed[index / 2] >> (4 * (1 - index % 2))) & 0x0F;
            Ok((0..count)
                .map(|index| sample(nibble(index).into()))
                .collect())
        }
        8 => (0..count)
            .map(|_| Ok(sample(cursor.u8()?.into())))
            .collect(),
        16 => (0..count)
            .map(|_| Ok(sample(cursor.u16()?.into())))
            .collect(),
        _ => Err(broken("stz2 field_size")),
    }
}

fn read_stts(body: &[u8], samples: &mut [Sample]) -> Result<(), Error> {
    let mut cursor = Cursor::new(body);
    cursor.full()?;
    let count = entries(&mut cursor, 8)?;
    let mut next = samples.iter_mut();
    let mut covered = 0usize;
    for _ in 0..count {
        let run = cursor.u32()? as usize;
        let delta = cursor.u32()?;
        covered = covered.saturating_add(run);
        for sample in next.by_ref().take(run) {
            sample.duration = delta;
        }
    }
    if covered != samples.len() {
        return Err(broken("stts khác số mẫu"));
    }
    Ok(())
}

fn read_stss(body: &[u8], samples: &mut [Sample]) -> Result<(), Error> {
    let mut cursor = Cursor::new(body);
    cursor.full()?;
    let count = entries(&mut cursor, 4)?;
    samples.iter_mut().for_each(|sample| sample.sync = false);
    for _ in 0..count {
        let number = cursor.u32()? as usize;
        let sample = number
            .checked_sub(1)
            .and_then(|index| samples.get_mut(index))
            .ok_or_else(|| broken("stss"))?;
        sample.sync = true;
    }
    Ok(())
}

/// ctts toàn 0 không đổi gì; khác 0 là có B-frame, bản đầu chưa ghép.
fn check_ctts(body: &[u8]) -> Result<(), Error> {
    let mut cursor = Cursor::new(body);
    cursor.full()?;
    let count = entries(&mut cursor, 8)?;
    for _ in 0..count {
        cursor.u32()?;
        if cursor.u32()? != 0 {
            return Err(unsupported("có ctts (B-frame)"));
        }
    }
    Ok(())
}

fn read_offsets(stbl: &[Child<'_>]) -> Result<Vec<u64>, Error> {
    let (table, wide) = match (find(stbl, b"stco"), find(stbl, b"co64")) {
        (Some(stco), None) => (stco, false),
        (None, Some(co64)) => (co64, true),
        _ => return Err(broken("cần đúng một stco/co64")),
    };
    let mut cursor = Cursor::new(table.body);
    cursor.full()?;
    let count = entries(&mut cursor, if wide { 8 } else { 4 })?;
    (0..count)
        .map(|_| {
            if wide {
                cursor.u64()
            } else {
                cursor.u32().map(u64::from)
            }
        })
        .collect()
}

/// stsc: (first_chunk, samples_per_chunk, sdi) ⇒ một run mỗi chunk.
fn read_stsc(body: &[u8], offsets: &[u64], total: usize) -> Result<Vec<Run>, Error> {
    let mut cursor = Cursor::new(body);
    cursor.full()?;
    let count = entries(&mut cursor, 12)?;
    let mut table = Vec::with_capacity(count);
    for _ in 0..count {
        let first = cursor.u32()? as usize;
        let per_chunk = cursor.u32()?;
        if cursor.u32()? != 1 {
            return Err(unsupported("stsc trỏ tới mục stsd khác 1"));
        }
        table.push((first, per_chunk));
    }
    let mut runs = Vec::with_capacity(offsets.len());
    for (index, &(first, per_chunk)) in table.iter().enumerate() {
        let end = table
            .get(index + 1)
            .map_or(offsets.len() + 1, |next| next.0);
        let valid = first >= 1 && first < end && end <= offsets.len() + 1 && per_chunk > 0;
        if !valid || (index == 0 && first != 1) {
            return Err(broken("stsc"));
        }
        runs.extend(offsets[first - 1..end - 1].iter().map(|&offset| Run {
            offset,
            count: per_chunk,
        }));
    }
    let listed: u64 = runs.iter().map(|run| u64::from(run.count)).sum();
    if runs.len() != offsets.len() || listed != total as u64 {
        return Err(broken("stsc khác số mẫu"));
    }
    Ok(runs)
}
