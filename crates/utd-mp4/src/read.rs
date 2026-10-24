//! Đọc một file MP4/fMP4 ra bảng mẫu: duyệt box cấp cao; moov và từng moof (vài KB) đọc trọn vào
//! RAM, mdat chỉ kiểm kích thước rồi nhảy qua; sidx, free… bỏ qua.

use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use crate::boxes::{FourCc, broken, name};
use crate::frag::{self, Stream};
use crate::init::{self, Movie};
use crate::table::Track;
use crate::{Error, unsupported};

const SCAN_BUFFER: usize = 16 * 1024;
const MAX_MOOV: u64 = 512 << 20;
const MAX_MOOF: u64 = 64 << 20;
/// Box đầu file hợp lệ; khác thế (WebM, file hỏng) thì thôi ngay, không đọc tiếp.
const FIRST_BOXES: [&FourCc; 6] = [b"ftyp", b"styp", b"moov", b"free", b"skip", b"wide"];

struct Header {
    kind: FourCc,
    start: u64,
    body: u64,
    end: u64,
}

#[derive(Default)]
struct Scan {
    movie: Option<Movie>,
    stream: Option<Stream>,
    /// Khoảng byte các trun của moof vừa đọc trỏ tới: phải nằm gọn trong mdat liền sau.
    pending: Option<(u64, u64)>,
    /// Payload các mdat (file thường kiểm chunk nằm trong đó).
    mdats: Vec<(u64, u64)>,
}

/// Mọi trak của file kèm bảng mẫu. fMP4 chỉ nhận một trak.
pub(crate) fn read_file(path: &Path) -> Result<Vec<Track>, Error> {
    let file = File::open(path)?;
    let length = file.metadata()?.len();
    let mut reader = BufReader::with_capacity(SCAN_BUFFER, file);
    let first = reader.fill_buf()?.get(4..8).map(<[u8]>::to_vec);
    if !first.is_some_and(|kind| FIRST_BOXES.iter().any(|known| known[..] == kind[..])) {
        return Err(unsupported("không phải file MP4"));
    }
    let mut scan = Scan::default();
    let mut pos = 0;
    while pos < length {
        let header = next_header(&mut reader, pos, length)?;
        let consumed = scan.visit(&mut reader, &header)?;
        let skip = i64::try_from(header.end - consumed).map_err(|_| broken("box quá lớn"))?;
        reader.seek_relative(skip)?;
        pos = header.end;
    }
    scan.finish()
}

fn next_header(reader: &mut impl Read, start: u64, length: u64) -> Result<Header, Error> {
    let mut head = [0u8; 8];
    reader.read_exact(&mut head)?;
    let size = u64::from(u32::from_be_bytes([head[0], head[1], head[2], head[3]]));
    let kind = [head[4], head[5], head[6], head[7]];
    let (size, header) = match size {
        0 => (length - start, 8),
        1 => {
            let mut large = [0u8; 8];
            reader.read_exact(&mut large)?;
            (u64::from_be_bytes(large), 16)
        }
        size => (size, 8),
    };
    let end = start.checked_add(size).filter(|end| *end <= length);
    match end {
        Some(end) if size >= header => Ok(Header {
            kind,
            start,
            body: start + header,
            end,
        }),
        // Thường là file tải chưa hết.
        _ => Err(broken(&format!("box {} vượt cuối file", name(&kind)))),
    }
}

/// Đọc thân box (đã đọc header) vào RAM, có trần kích thước.
fn read_body(reader: &mut impl Read, header: &Header, limit: u64) -> Result<Vec<u8>, Error> {
    let size = header.end - header.body;
    if size > limit {
        return Err(unsupported(format!(
            "{} quá lớn ({size} B)",
            name(&header.kind)
        )));
    }
    let mut body = vec![0; size as usize];
    reader.read_exact(&mut body)?;
    Ok(body)
}

impl Scan {
    /// Xử lý một box cấp cao; trả vị trí reader đang đứng (đầu thân, hoặc cuối box nếu đã đọc thân).
    fn visit(&mut self, reader: &mut impl Read, header: &Header) -> Result<u64, Error> {
        if self.pending.is_some() && &header.kind != b"mdat" {
            return Err(unsupported("moof không có mdat liền sau"));
        }
        match &header.kind {
            b"moov" => {
                let body = read_body(reader, header, MAX_MOOV)?;
                self.open_movie(init::read_moov(&body)?)?;
                Ok(header.end)
            }
            b"moof" => {
                let stream = self
                    .stream
                    .as_mut()
                    .ok_or_else(|| unsupported("moof ngoài fMP4 (trước moov hoặc thiếu mvex)"))?;
                let body = read_body(reader, header, MAX_MOOF)?;
                self.pending = frag::read_moof(&body, header.start, stream)?;
                Ok(header.end)
            }
            b"mdat" => {
                if let Some((low, high)) = self.pending.take()
                    && (low < header.body || high > header.end)
                {
                    return Err(unsupported("dữ liệu trun nằm ngoài mdat liền sau moof"));
                }
                self.mdats.push((header.body, header.end));
                Ok(header.body)
            }
            _ => Ok(header.body),
        }
    }

    fn open_movie(&mut self, mut movie: Movie) -> Result<(), Error> {
        if self.movie.is_some() || self.stream.is_some() {
            return Err(unsupported("có nhiều moov"));
        }
        if !movie.fragmented {
            self.movie = Some(movie);
            return Ok(());
        }
        if movie.tracks.len() != 1 {
            return Err(unsupported(format!("fMP4 có {} trak", movie.tracks.len())));
        }
        let track = movie.tracks.remove(0);
        let trex = movie
            .trex
            .iter()
            .find(|(id, _)| *id == track.id)
            .map(|(_, defaults)| *defaults)
            .ok_or_else(|| broken("thiếu trex của track"))?;
        self.stream = Some(Stream {
            track,
            trex,
            decode_end: 0,
        });
        Ok(())
    }

    fn finish(self) -> Result<Vec<Track>, Error> {
        if self.pending.is_some() {
            return Err(unsupported("moof cuối không có mdat"));
        }
        if let Some(stream) = self.stream {
            return Ok(vec![stream.track]);
        }
        let movie = self.movie.ok_or_else(|| unsupported("không có moov"))?;
        let inside = |offset: u64, end: u64| {
            self.mdats
                .iter()
                .any(|&(low, high)| offset >= low && end <= high)
        };
        for track in &movie.tracks {
            let mut sizes = track.samples.iter().map(|sample| u64::from(sample.size));
            for run in &track.runs {
                let bytes: u64 = sizes.by_ref().take(run.count as usize).sum();
                if !run
                    .offset
                    .checked_add(bytes)
                    .is_some_and(|end| inside(run.offset, end))
                {
                    return Err(unsupported("chunk nằm ngoài mdat"));
                }
            }
        }
        Ok(movie.tracks)
    }
}
