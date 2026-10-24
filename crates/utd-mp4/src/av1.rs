//! AV1: ffmpeg bỏ temporal delimiter (OBU `12 00`) ở đầu mỗi mẫu khi ghi MP4, như AV1-ISOBMFF
//! khuyên. Làm giống vậy để file ra trùng từng gói với bản ffmpeg ghép (và yt-dlp).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::Error;
use crate::table::Track;

/// obu_type = 2 (temporal delimiter), có trường size, size = 0.
pub(crate) const DELIMITER: [u8; 2] = [0x12, 0x00];

/// Mẫu đầu mở bằng delimiter thì coi mọi mẫu đều vậy: trừ 2 byte mỗi mẫu, lúc chép kiểm lại từng
/// mẫu (thiếu thì báo Unsupported để ffmpeg ghép). Mẫu đầu không có thì chép nguyên.
pub(crate) fn strip_delimiters(track: &mut Track, path: &Path) -> Result<(), Error> {
    let Some(first) = track.runs.first() else {
        return Ok(());
    };
    let width = DELIMITER.len() as u32;
    if track.codec != *b"av01" || track.samples.iter().any(|sample| sample.size < width) {
        return Ok(());
    }
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(first.offset))?;
    let mut head = [0u8; 2];
    file.read_exact(&mut head)?;
    if head != DELIMITER {
        return Ok(());
    }
    for sample in &mut track.samples {
        sample.size -= width;
    }
    track.trim = width;
    Ok(())
}
