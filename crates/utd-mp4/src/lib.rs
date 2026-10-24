//! Ghép các luồng fMP4 (DASH) thành một file MP4 thường, moov ở đầu file, trong một lượt ghi.
//! Chỉ chép byte mẫu, không giải mã; stsd, hdlr, dinf, vmhd/smhd giữ nguyên byte của init.
//! Gặp cấu trúc chưa kiểm (mã hoá, B-frame, nhiều traf…) thì trả `Error::Unsupported` kèm lý do
//! để bên gọi chuyển sang ffmpeg.

mod av1;
mod boxes;
mod frag;
mod init;
mod read;
mod stbl;
mod table;
mod write;

#[cfg(test)]
mod tests;

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::table::{Chunk, Track};

/// Loại track cần lấy từ một đầu vào.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Video,
    Audio,
}

/// Một file nguồn và loại track phải có trong file đó (mỗi file đúng một trak).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    pub path: PathBuf,
    pub kind: Kind,
}

/// Vỏ đầu ra: quyết định ftyp và các track được phép.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Brand {
    /// Một hình, có thể thêm một tiếng: `.mp4`.
    Mp4,
    /// Chỉ một tiếng AAC: `.m4a`.
    M4a,
}

/// Số liệu một lần ghép (để ghi log).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    pub samples: u64,
    pub chunks: u64,
    pub moov_bytes: u64,
    pub file_bytes: u64,
    /// Offset chunk vượt 32 bit nên dùng co64.
    pub co64: bool,
}

#[derive(Debug)]
pub enum Error {
    /// Cấu trúc chưa hỗ trợ hoặc file lạ: bên gọi nên ghép bằng ffmpeg.
    Unsupported(String),
    Cancelled,
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(reason) => write!(formatter, "chưa hỗ trợ: {reason}"),
            Self::Cancelled => formatter.write_str("đã huỷ"),
            Self::Io(error) => write!(formatter, "lỗi đọc/ghi: {error}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) fn unsupported(reason: impl Into<String>) -> Error {
    Error::Unsupported(reason.into())
}

/// Ghép `inputs` vào `output` (ghi đè). Lỗi hay huỷ thì xoá `output`, không để lại file dở.
pub fn merge(
    inputs: &[Input],
    output: &Path,
    brand: Brand,
    cancel: &AtomicBool,
) -> Result<Report, Error> {
    let result = merge_into(inputs, output, brand, cancel);
    if result.is_err() {
        let _ = std::fs::remove_file(output);
    }
    result
}

fn merge_into(
    inputs: &[Input],
    output: &Path,
    brand: Brand,
    cancel: &AtomicBool,
) -> Result<Report, Error> {
    let (tracks, sources) = load(inputs, brand, cancel)?;
    let chunks = table::plan(&tracks);
    let head = write::head(brand, &tracks, &chunks)?;
    let written = write::write(&head, &tracks, &chunks, &sources, output, cancel)?;
    verify(output, &tracks, &chunks, &head, written)?;
    Ok(Report {
        samples: tracks.iter().map(|track| track.samples.len() as u64).sum(),
        chunks: chunks.len() as u64,
        moov_bytes: head.moov_bytes,
        file_bytes: written,
        co64: head.co64,
    })
}

/// Đọc từng đầu vào, kiểm loại track, xếp hình trước tiếng (track id 1 hình, 2 tiếng).
fn load<'a>(
    inputs: &'a [Input],
    brand: Brand,
    cancel: &AtomicBool,
) -> Result<(Vec<Track>, Vec<&'a Path>), Error> {
    let mut loaded = Vec::new();
    for input in inputs {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let name = input.path.display();
        let mut tracks = read::read_file(&input.path)?;
        if tracks.len() != 1 {
            return Err(unsupported(format!("{name}: có {} trak", tracks.len())));
        }
        let mut track = tracks.remove(0);
        if track.kind != input.kind {
            return Err(unsupported(format!(
                "{name}: không phải track {:?}",
                input.kind
            )));
        }
        if track.samples.is_empty() {
            return Err(unsupported(format!("{name}: không có mẫu nào")));
        }
        av1::strip_delimiters(&mut track, &input.path)?;
        loaded.push((track, input.path.as_path()));
    }
    loaded.sort_by_key(|(track, _)| track.kind != Kind::Video);
    let videos = loaded
        .iter()
        .filter(|(track, _)| track.kind == Kind::Video)
        .count();
    let audios = loaded.len() - videos;
    let fits = match brand {
        Brand::Mp4 => videos == 1 && audios <= 1,
        Brand::M4a => videos == 0 && audios == 1,
    };
    if !fits {
        return Err(unsupported(format!(
            "{brand:?} không nhận {videos} hình + {audios} tiếng"
        )));
    }
    Ok(loaded.into_iter().unzip())
}

/// Tự kiểm trước khi trả Ok: kích thước file, tổng mẫu = payload mdat, và đọc lại moov vừa ghi
/// bằng chính bộ đọc để so bảng mẫu và vị trí từng chunk.
fn verify(
    output: &Path,
    tracks: &[Track],
    chunks: &[Chunk],
    head: &write::Head,
    written: u64,
) -> Result<(), Error> {
    let expected = head.bytes.len() as u64 + head.payload;
    if written != expected {
        return Err(unsupported(format!(
            "tự kiểm: file ra {written} B, tính trước {expected} B"
        )));
    }
    let samples: u64 = tracks.iter().map(Track::bytes).sum();
    if samples != head.payload {
        return Err(unsupported(
            "tự kiểm: tổng kích thước mẫu khác payload mdat",
        ));
    }
    let again = read::read_file(output)?;
    let runs = write::chunk_runs(tracks.len(), chunks, head.bytes.len() as u64);
    let same = again.len() == tracks.len()
        && again
            .iter()
            .zip(tracks)
            .zip(&runs)
            .all(|((back, track), runs)| back.samples == track.samples && back.runs == *runs);
    if !same {
        return Err(unsupported(
            "tự kiểm: đọc lại bảng mẫu của file ra không khớp",
        ));
    }
    Ok(())
}
