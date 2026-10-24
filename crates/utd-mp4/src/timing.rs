//! Mốc thời gian của file ra, tính y như ffmpeg 8.1 ghép `ffmpeg -i hình -i tiếng -c copy -f mp4`:
//! mov.c đọc (DTS = tfdt − media_time của elst, PTS = DTS + cts), fftools dịch từng đầu vào để
//! start_time về 0, rồi movenc.c ghi tkhd/mdhd/mvhd và edit list theo PTS đầu/cuối. Nhờ vậy file
//! ra có B-frame vẫn trùng từng số với bản ffmpeg (và yt-dlp).

use crate::table::Track;
use crate::{Error, unsupported};

/// AV_TIME_BASE: fftools giữ ts_offset theo micro giây.
const MICROS: i64 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Timing {
    /// tkhd (mvhd lấy lớn nhất các track), theo timescale movie.
    pub track: u64,
    /// mdhd, theo timescale của track.
    pub media: u64,
    /// Mục elst duy nhất: segment theo timescale movie.
    pub segment: u64,
    /// media_time của elst: phần DTS âm (B-frame, priming AAC) bỏ khi phát.
    pub media_time: u64,
    /// elst version 1: ffmpeg xét segment trước khi trừ phần PTS âm, không xét media_time.
    pub wide: bool,
}

#[derive(Clone, Copy)]
enum Round {
    Down,
    Up,
    Near,
}

/// av_rescale_rnd: `value · mul / div`; số âm làm tròn đối xứng như ffmpeg (Down là về −∞).
fn rescale(value: i64, mul: i64, div: i64, round: Round) -> i64 {
    if value < 0 {
        let mirrored = match round {
            Round::Down => Round::Up,
            Round::Up => Round::Down,
            Round::Near => Round::Near,
        };
        return -rescale(-value, mul, div, mirrored);
    }
    let bias = match round {
        Round::Down => 0,
        Round::Up => div - 1,
        Round::Near => div / 2,
    };
    let scaled = (i128::from(value) * i128::from(mul) + i128::from(bias)) / i128::from(div);
    i64::try_from(scaled).unwrap_or(i64::MAX)
}

/// Mốc tkhd/mdhd/elst của `track` (mẫu duration > 0, cts ≥ 0) trong movie có timescale `movie`.
pub(crate) fn timing(track: &Track, movie: u32) -> Result<Timing, Error> {
    let scale = i64::from(track.timescale);
    let movie = i64::from(movie);
    let edit = i64::try_from(track.edit).map_err(|_| unsupported("media_time elst quá lớn"))?;
    let first_cts = track
        .samples
        .first()
        .map_or(0, |sample| i64::from(sample.cts));
    // demux.c: start_time = PTS gói đầu; AAC cộng thêm start_pad (priming bỏ khi giải mã).
    let pad = if track.priming { edit } else { 0 };
    let start = first_cts - edit + pad;
    // ffmpeg_demux.c: ts_offset = −start_time (micro giây), cộng vào từng gói của đầu vào đó.
    let micros = rescale(start, MICROS, scale, Round::Near);
    let offset = -rescale(micros, scale, MICROS, Round::Near);
    // DTS gói đầu ở phía movenc (start_dts).
    let base = offset - edit;
    let (start_cts, end_pts) = pts_range(track, base);
    if end_pts <= 0 {
        return Err(unsupported("PTS cuối ≤ 0"));
    }
    // calc_samples_pts_duration (mdhd, elst) và calc_pts_duration (tkhd, mvhd: tính từ PTS 0).
    let media = end_pts - (base + start_cts);
    let whole = rescale(end_pts, movie, scale, Round::Up);
    // mov_write_edts_tag. Mẫu duration > 0 nên DTS tăng hẳn: gói đầu có DTS ≥ 0 thì chỉ nó tính
    // vào start_cts, nhánh "lấy lại cts của cluster[0]" không đổi gì.
    let duration = rescale(media, movie, scale, Round::Up);
    let delay = rescale(base + start_cts, movie, scale, Round::Down);
    if delay > 0 {
        return Err(unsupported("PTS đầu > 0 (ffmpeg thêm đoạn trống)"));
    }
    let wide = duration >= i64::from(i32::MAX);
    let media_time = -base.min(0);
    if !wide && media_time > i64::from(i32::MAX) {
        return Err(unsupported("media_time elst vượt 32 bit"));
    }
    Ok(Timing {
        track: whole as u64,
        media: u64::try_from(media).map_err(|_| unsupported("thời lượng mdhd âm"))?,
        segment: u64::try_from(duration + delay).map_err(|_| unsupported("segment elst âm"))?,
        media_time: media_time as u64,
        wide,
    })
}

/// ff_mov_write_packet: start_cts là cts gói đầu, hạ xuống cts nhỏ hơn của các gói có DTS ≤ 0;
/// end_pts là PTS + duration lớn nhất.
fn pts_range(track: &Track, base: i64) -> (i64, i64) {
    let mut dts = base;
    let mut start_cts = None;
    let mut end_pts = i64::MIN;
    for sample in &track.samples {
        let cts = i64::from(sample.cts);
        let duration = i64::from(sample.duration);
        match start_cts {
            Some(lowest) if dts > 0 || lowest <= cts => {}
            _ => start_cts = Some(cts),
        }
        end_pts = end_pts.max(dts + cts + duration);
        dts += duration;
    }
    (start_cts.unwrap_or(0), end_pts)
}
