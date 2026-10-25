pub mod auth;
pub mod db;
pub mod engine;
pub mod fetch;
pub mod headers;
pub mod model;
pub mod safety;

pub use db::Journal;
pub use engine::{
    DownloadControl, DownloadEngine, DownloadOutcome, DownloadRequest, EngineEvent, EngineOptions,
    ProgressSnapshot, RemoteFile,
};
pub use headers::{
    allowed_request_headers, validate_request_headers, RequestHeader, MAX_REQUEST_HEADERS,
};
pub use model::*;

pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("database: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("codec: {0}")]
    Codec(#[from] serde_json::Error),
    #[error("unsupported database schema version {found} (max {max})")]
    UnsupportedSchema { found: u32, max: u32 },
    #[error("corrupt journal: {0}")]
    Corrupt(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::ENGINE_VERSION;

    #[test]
    fn exposes_engine_version() {
        assert!(!ENGINE_VERSION.is_empty());
    }
}
