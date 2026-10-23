use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(crate) struct PartFile {
    file: File,
}

impl PartFile {
    pub fn open(path: &Path, total: Option<u64>) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        if let Some(total) = total {
            file.set_len(total)?;
        }
        Ok(Self { file })
    }

    pub fn reset(&self, total: Option<u64>) -> io::Result<()> {
        self.file.set_len(0)?;
        if let Some(total) = total {
            self.file.set_len(total)?;
        }
        Ok(())
    }

    pub fn write_at(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        let mut written = 0usize;
        while written < data.len() {
            let count = write_at(&self.file, &data[written..], offset + written as u64)?;
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "positional write returned zero bytes",
                ));
            }
            written += count;
        }
        Ok(())
    }
}

pub(crate) fn target_path(dest_dir: &str, file_name: &str) -> PathBuf {
    Path::new(dest_dir).join(file_name)
}

pub(crate) async fn move_no_clobber(temp: &Path, target: &Path) -> io::Result<()> {
    let temp = temp.to_path_buf();
    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || move_blocking(&temp, &target))
        .await
        .map_err(io::Error::other)?
}

#[cfg(windows)]
fn move_blocking(temp: &Path, target: &Path) -> io::Result<()> {
    std::fs::rename(temp, target)
}

#[cfg(unix)]
fn move_blocking(temp: &Path, target: &Path) -> io::Result<()> {
    std::fs::hard_link(temp, target)?;
    std::fs::remove_file(temp)
}

#[cfg(windows)]
fn write_at(file: &File, data: &[u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_write(data, offset)
}

#[cfg(unix)]
fn write_at(file: &File, data: &[u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.write_at(data, offset)
}
