//! Box ISO BMFF ở mức byte: con trỏ đọc số big-endian trong box đã nạp vào RAM, liệt kê box con,
//! và bộ ghi box tự vá kích thước.

use crate::{Error, unsupported};

pub(crate) type FourCc = [u8; 4];

pub(crate) fn broken(what: &str) -> Error {
    unsupported(format!("file hỏng hoặc lạ ({what})"))
}

pub(crate) fn name(kind: &FourCc) -> String {
    String::from_utf8_lossy(kind).into_owned()
}

/// Con trỏ đọc trong một box; đọc quá cuối là lỗi, không panic.
pub(crate) struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn bytes(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let end = self
            .pos
            .checked_add(count)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| broken("box ngắn hơn nội dung"))?;
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    pub fn skip(&mut self, count: usize) -> Result<(), Error> {
        self.bytes(count).map(|_| ())
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let mut out = [0; N];
        out.copy_from_slice(self.bytes(N)?);
        Ok(out)
    }

    pub fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.bytes(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16, Error> {
        self.array().map(u16::from_be_bytes)
    }

    pub fn u32(&mut self) -> Result<u32, Error> {
        self.array().map(u32::from_be_bytes)
    }

    pub fn u64(&mut self) -> Result<u64, Error> {
        self.array().map(u64::from_be_bytes)
    }

    pub fn fourcc(&mut self) -> Result<FourCc, Error> {
        self.array()
    }

    /// Version (8 bit) và flags (24 bit) của full box.
    pub fn full(&mut self) -> Result<(u8, u32), Error> {
        let word = self.u32()?;
        Ok(((word >> 24) as u8, word & 0x00FF_FFFF))
    }

    /// Trường 32 bit ở version 0, 64 bit ở version 1.
    pub fn sized(&mut self, version: u8) -> Result<u64, Error> {
        if version == 1 {
            self.u64()
        } else {
            self.u32().map(u64::from)
        }
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
}

/// Một box con đã nạp vào RAM.
#[derive(Clone, Copy)]
pub(crate) struct Child<'a> {
    pub kind: FourCc,
    /// Thân box, không gồm header.
    pub body: &'a [u8],
    /// Cả box gồm header, để chép nguyên byte.
    pub whole: &'a [u8],
}

/// Các box nằm liền nhau trong `data` (thân của một box chứa).
pub(crate) fn children(data: &[u8]) -> Result<Vec<Child<'_>>, Error> {
    let mut list = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        if rest.len() < 8 {
            // Vài muxer cũ để 4 byte 0 cuối box chứa.
            if rest.iter().all(|byte| *byte == 0) {
                break;
            }
            return Err(broken("thừa byte cuối box"));
        }
        let mut cursor = Cursor::new(rest);
        let size = cursor.u32()?;
        let kind = cursor.fourcc()?;
        let (size, header) = match size {
            0 => (rest.len() as u64, 8),
            1 => (cursor.u64()?, 16),
            size => (u64::from(size), 8),
        };
        if size < header || size > rest.len() as u64 {
            return Err(broken(&format!("kích thước box {}", name(&kind))));
        }
        let (whole, tail) = rest.split_at(size as usize);
        list.push(Child {
            kind,
            body: &whole[header as usize..],
            whole,
        });
        rest = tail;
    }
    Ok(list)
}

pub(crate) fn find<'a>(list: &[Child<'a>], kind: &FourCc) -> Option<Child<'a>> {
    list.iter().find(|child| &child.kind == kind).copied()
}

/// Đúng một box `kind` trong `list`; thiếu hay thừa đều không nhận.
pub(crate) fn single<'a>(list: &[Child<'a>], kind: &FourCc) -> Result<Child<'a>, Error> {
    let mut found = list.iter().filter(|child| &child.kind == kind);
    match (found.next(), found.next()) {
        (Some(child), None) => Ok(*child),
        (None, _) => Err(broken(&format!("thiếu {}", name(kind)))),
        (Some(_), Some(_)) => Err(unsupported(format!("có nhiều {}", name(kind)))),
    }
}

/// Bộ ghi box vào RAM: `open` chừa chỗ cho kích thước, `close` vá lại.
#[derive(Default)]
pub(crate) struct Out {
    pub buf: Vec<u8>,
}

impl Out {
    pub fn u16(&mut self, value: u16) {
        self.buf.extend_from_slice(&value.to_be_bytes());
    }

    pub fn u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_be_bytes());
    }

    pub fn u64(&mut self, value: u64) {
        self.buf.extend_from_slice(&value.to_be_bytes());
    }

    pub fn bytes(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Trường 32 bit ở version 0, 64 bit ở version 1.
    pub fn sized(&mut self, version: u8, value: u64) {
        if version == 1 {
            self.u64(value);
        } else {
            self.u32(value as u32);
        }
    }

    pub fn open(&mut self, kind: &FourCc) -> usize {
        let start = self.buf.len();
        self.u32(0);
        self.bytes(kind);
        start
    }

    pub fn open_full(&mut self, kind: &FourCc, version: u8, flags: u32) -> usize {
        let start = self.open(kind);
        self.u32((u32::from(version) << 24) | flags);
        start
    }

    /// Vá kích thước box mở ở `start`. Bên gọi kiểm moov < 4 GiB nên không tràn.
    pub fn close(&mut self, start: usize) {
        let size = (self.buf.len() - start) as u32;
        self.patch(start, size);
    }

    /// Chừa một ô u32 để vá sau (số mục của bảng).
    pub fn slot(&mut self) -> usize {
        let at = self.buf.len();
        self.u32(0);
        at
    }

    pub fn patch(&mut self, at: usize, value: u32) {
        self.buf[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }
}
