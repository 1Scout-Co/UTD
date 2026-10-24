//! Bảng mẫu trong RAM và cách xếp chunk cho file ra: mỗi chunk ≤ 1 giây và ≤ 1 MiB, không vượt
//! ranh giới một run (khoảng byte liền ở file nguồn), xen kẽ các track theo DTS.

use std::cmp::Ordering;

use crate::Kind;
use crate::boxes::FourCc;

/// Trần chunk theo byte; theo thời gian là 1 giây (= timescale của track).
pub(crate) const CHUNK_BYTES: u64 = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sample {
    pub size: u32,
    pub duration: u32,
    pub sync: bool,
    /// Lệch PTS − DTS (B-frame); chỉ nhận ≥ 0 và vừa i32.
    pub cts: u32,
}

/// `count` mẫu kế tiếp nằm liền nhau từ `offset` ở file nguồn (một trun, hoặc một chunk của file thường).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Run {
    pub offset: u64,
    pub count: u32,
}

/// Một track đọc từ nguồn: thông tin để ghi lại moov và bảng mẫu.
#[derive(Debug, Clone)]
pub(crate) struct Track {
    pub kind: Kind,
    /// track_ID ở file nguồn (để khớp tfhd); file ra đánh số lại.
    pub id: u32,
    pub timescale: u32,
    /// Mã ngôn ngữ đóng gói của mdhd.
    pub language: u16,
    pub matrix: [u8; 36],
    /// Kích thước 16.16 của tkhd.
    pub width: u32,
    pub height: u32,
    pub codec: FourCc,
    /// mp4a chứa AAC: ghi thêm nhóm 'roll' như ffmpeg.
    pub aac: bool,
    /// Số byte bỏ ở đầu mỗi mẫu khi chép (temporal delimiter của AV1). `samples` giữ kích thước
    /// ở file ra; ở nguồn mỗi mẫu dài thêm `trim` byte.
    pub trim: u32,
    /// media_time của edit list trong init fMP4 (0 nếu không có): ffmpeg lùi DTS từng ấy.
    pub edit: u64,
    /// AAC có edit list: ffmpeg coi `edit` là priming bỏ khi giải mã, giờ bắt đầu vẫn là 0.
    pub priming: bool,
    /// Box chép nguyên byte từ nguồn.
    pub stsd: Vec<u8>,
    pub hdlr: Vec<u8>,
    pub dinf: Vec<u8>,
    pub media_header: Vec<u8>,
    pub samples: Vec<Sample>,
    pub runs: Vec<Run>,
}

impl Track {
    /// Tổng thời lượng theo timescale của track.
    pub fn duration(&self) -> u64 {
        self.samples
            .iter()
            .map(|sample| u64::from(sample.duration))
            .sum()
    }

    pub fn bytes(&self) -> u64 {
        self.samples
            .iter()
            .map(|sample| u64::from(sample.size))
            .sum()
    }
}

/// Một chunk của file ra: `count` mẫu liền nhau (từ mẫu `first`) của track `track`, chép từ một
/// khoảng byte liền ở nguồn bắt đầu tại `source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Chunk {
    pub track: usize,
    pub first: usize,
    pub count: u32,
    pub source: u64,
    /// Số byte ở file ra (đã bỏ `trim`).
    pub size: u64,
    /// DTS của mẫu đầu, theo timescale của track.
    pub dts: u64,
}

/// Mọi chunk theo thứ tự ghi trong mdat.
pub(crate) fn plan(tracks: &[Track]) -> Vec<Chunk> {
    let lists: Vec<Vec<Chunk>> = tracks
        .iter()
        .enumerate()
        .map(|(index, track)| split(index, track))
        .collect();
    interleave(tracks, &lists)
}

fn split(index: usize, track: &Track) -> Vec<Chunk> {
    let limit = u64::from(track.timescale);
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut samples = track.samples.iter().enumerate();
    let mut dts = 0;
    for run in &track.runs {
        let mut offset = run.offset;
        // Chunk cuối còn nhận thêm mẫu không (mỗi run mở chunk mới) và thời lượng của nó.
        let mut open = false;
        let mut span = 0;
        for (number, sample) in samples.by_ref().take(run.count as usize) {
            let size = u64::from(sample.size);
            let duration = u64::from(sample.duration);
            match chunks.last_mut() {
                Some(chunk)
                    if open && chunk.size + size <= CHUNK_BYTES && span + duration <= limit =>
                {
                    chunk.count += 1;
                    chunk.size += size;
                    span += duration;
                }
                _ => {
                    chunks.push(Chunk {
                        track: index,
                        first: number,
                        count: 1,
                        source: offset,
                        size,
                        dts,
                    });
                    open = true;
                    span = duration;
                }
            }
            offset += size + u64::from(track.trim);
            dts += duration;
        }
    }
    chunks
}

/// Trộn chunk các track theo DTS (mỗi danh sách đã tăng dần); bằng nhau thì track trước đi trước.
fn interleave(tracks: &[Track], lists: &[Vec<Chunk>]) -> Vec<Chunk> {
    let total = lists.iter().map(Vec::len).sum();
    let mut heads = vec![0; lists.len()];
    let mut order = Vec::with_capacity(total);
    loop {
        let next = (0..lists.len())
            .filter(|&track| heads[track] < lists[track].len())
            .min_by(|&left, &right| {
                earlier(
                    tracks,
                    &lists[left][heads[left]],
                    &lists[right][heads[right]],
                )
            });
        let Some(track) = next else {
            return order;
        };
        order.push(lists[track][heads[track]]);
        heads[track] += 1;
    }
}

/// So `a.dts / ts(a)` với `b.dts / ts(b)` bằng nhân chéo u128 (không làm tròn).
fn earlier(tracks: &[Track], a: &Chunk, b: &Chunk) -> Ordering {
    let left = u128::from(a.dts) * u128::from(tracks[b.track].timescale);
    let right = u128::from(b.dts) * u128::from(tracks[a.track].timescale);
    left.cmp(&right).then(a.track.cmp(&b.track))
}
