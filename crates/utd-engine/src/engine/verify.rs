use std::fmt::Write as _;
use std::io;
use std::path::Path;

use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

pub(crate) enum HashOutcome {
    Hash(String),
    Cancelled,
}

pub(crate) async fn sha256_file(path: &Path, cancel: &CancellationToken) -> io::Result<HashOutcome> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = tokio::select! {
            _ = cancel.cancelled() => return Ok(HashOutcome::Cancelled),
            read = file.read(&mut buffer) => read?,
        };
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut hex, "{byte:02x}").expect("writing to a string cannot fail");
    }
    Ok(HashOutcome::Hash(hex))
}
