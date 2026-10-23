mod adaptive;
mod file;
mod http;
mod limiter;
mod session;
mod verify;
mod worker;

use std::time::Duration;

use serde::Serialize;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::model::{DownloadTask, HashState, TaskStatus};

pub use session::DownloadEngine;

pub(crate) const USER_AGENT: &str = concat!("utd/", env!("CARGO_PKG_VERSION"));

/// Trần số kết nối mặc định của một lượt tải. Số kết nối thật do engine tự dò (kiểu IDM).
const AUTO_CONNECTIONS: u16 = 16;

#[derive(Debug, Clone)]
pub struct EngineOptions {
    pub initial_connections: u16,
    pub max_connections: u16,
    pub host_connections: u16,
    pub global_connections: u16,
    pub max_attempts: u32,
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
    pub sample_interval: Duration,
    pub checkpoint_interval: Duration,
    pub balance_interval: Duration,
    pub user_agent: String,
    pub quarantine_dir: Option<String>,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            initial_connections: 4,
            max_connections: AUTO_CONNECTIONS,
            host_connections: 16,
            global_connections: 32,
            max_attempts: 8,
            connect_timeout: Duration::from_secs(15),
            idle_timeout: Duration::from_secs(30),
            sample_interval: Duration::from_millis(500),
            checkpoint_interval: Duration::from_secs(1),
            balance_interval: Duration::from_secs(3),
            user_agent: USER_AGENT.to_string(),
            quarantine_dir: None,
        }
    }
}

impl EngineOptions {
    pub fn validate(&self) -> crate::Result<()> {
        if self.initial_connections == 0 || self.initial_connections > self.max_connections {
            return Err(crate::Error::InvalidInput(
                "initial_connections must be 1..=max_connections".into(),
            ));
        }
        if self.max_connections > crate::model::MAX_CONNECTIONS {
            return Err(crate::Error::InvalidInput(format!(
                "max_connections cannot exceed {}",
                crate::model::MAX_CONNECTIONS
            )));
        }
        if self.host_connections == 0 || self.global_connections == 0 {
            return Err(crate::Error::InvalidInput(
                "connection limits must be at least 1".into(),
            ));
        }
        if self.max_attempts == 0 {
            return Err(crate::Error::InvalidInput(
                "max_attempts must be at least 1".into(),
            ));
        }
        if self.sample_interval.is_zero()
            || self.checkpoint_interval.is_zero()
            || self.balance_interval.is_zero()
        {
            return Err(crate::Error::InvalidInput(
                "engine intervals must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct DownloadRequest {
    pub url: String,
    pub dest_dir: String,
    pub file_name: Option<String>,
    /// Trần số kết nối (mặc định = tự động tới mức tối đa của engine).
    pub connections: u16,
    pub expected_sha256: Option<String>,
}

impl DownloadRequest {
    pub fn new(url: impl Into<String>, dest_dir: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            dest_dir: dest_dir.into(),
            file_name: None,
            connections: AUTO_CONNECTIONS,
            expected_sha256: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DownloadControl {
    pub cancel: CancellationToken,
    pub events: Option<UnboundedSender<EngineEvent>>,
}

impl DownloadControl {
    pub fn new() -> Self {
        Self {
            cancel: CancellationToken::new(),
            events: None,
        }
    }

    pub fn with_events(events: UnboundedSender<EngineEvent>) -> Self {
        Self {
            cancel: CancellationToken::new(),
            events: Some(events),
        }
    }
}

impl Default for DownloadControl {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressSnapshot {
    pub task_id: String,
    pub status: TaskStatus,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub bytes_per_second: u64,
    pub connections: u16,
    pub segments_done: u32,
    pub segments_total: u32,
    pub hash_state: HashState,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EngineEvent {
    Task { task: Box<DownloadTask> },
    Progress { progress: ProgressSnapshot },
}

#[derive(Debug, Clone)]
pub enum DownloadOutcome {
    Completed(DownloadTask),
    Paused(DownloadTask),
    Failed(DownloadTask),
    Quarantined(DownloadTask),
}

impl DownloadOutcome {
    pub fn task(&self) -> &DownloadTask {
        match self {
            Self::Completed(task)
            | Self::Paused(task)
            | Self::Failed(task)
            | Self::Quarantined(task) => task,
        }
    }

    pub fn task_id(&self) -> &str {
        &self.task().id
    }

    pub fn is_completed(&self) -> bool {
        matches!(self, Self::Completed(_))
    }
}
