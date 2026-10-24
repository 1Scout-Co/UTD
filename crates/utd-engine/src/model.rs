use std::path::{Component, Path};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::headers::{validate_request_headers, RequestHeader};
use crate::{Error, Result};

pub const MIN_SEGMENT_BYTES: u64 = 1024 * 1024;
pub const MAX_SEGMENTS: usize = 64;
pub const MAX_CONNECTIONS: u16 = 32;
pub const MAX_FILE_NAME_CHARS: usize = 180;
/// Trần byte mỗi yêu cầu HTTP nhỏ nhất được nhận (nhỏ hơn thì quá nhiều yêu cầu).
pub const MIN_REQUEST_BYTES: u64 = 64 * 1024;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Http,
    Hls,
    Dash,
    Media,
    /// Một luồng (video/âm thanh) của task media, engine tải nhiều kết nối; task con ẩn.
    MediaPart,
}

impl TaskKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Hls => "hls",
            Self::Dash => "dash",
            Self::Media => "media",
            Self::MediaPart => "media_part",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "http" => Ok(Self::Http),
            "hls" => Ok(Self::Hls),
            "dash" => Ok(Self::Dash),
            "media" => Ok(Self::Media),
            "media_part" => Ok(Self::MediaPart),
            other => Err(Error::Corrupt(format!("unknown task kind: {other:?}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Resolving,
    Downloading,
    Paused,
    Verifying,
    Completed,
    Failed,
    Quarantined,
}

impl TaskStatus {
    pub const ALL: [Self; 8] = [
        Self::Queued,
        Self::Resolving,
        Self::Downloading,
        Self::Paused,
        Self::Verifying,
        Self::Completed,
        Self::Failed,
        Self::Quarantined,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Resolving => "resolving",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Verifying => "verifying",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Quarantined => "quarantined",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
            .ok_or_else(|| Error::Corrupt(format!("unknown task status: {value:?}")))
    }

    pub const fn is_active(self) -> bool {
        matches!(self, Self::Resolving | Self::Downloading | Self::Verifying)
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Quarantined)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentStatus {
    Pending,
    Active,
    Done,
    Failed,
}

impl SegmentStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "active" => Ok(Self::Active),
            "done" => Ok(Self::Done),
            "failed" => Ok(Self::Failed),
            other => Err(Error::Corrupt(format!("unknown segment status: {other:?}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HashState {
    Pending,
    Verified,
    Mismatch,
    Unavailable,
}

impl HashState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Verified => "verified",
            Self::Mismatch => "mismatch",
            Self::Unavailable => "unavailable",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "verified" => Ok(Self::Verified),
            "mismatch" => Ok(Self::Mismatch),
            "unavailable" => Ok(Self::Unavailable),
            other => Err(Error::Corrupt(format!("unknown hash state: {other:?}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    User,
    Transient,
    Permanent,
    Security,
}

impl ErrorClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Transient => "transient",
            Self::Permanent => "permanent",
            Self::Security => "security",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "user" => Ok(Self::User),
            "transient" => Ok(Self::Transient),
            "permanent" => Ok(Self::Permanent),
            "security" => Ok(Self::Security),
            other => Err(Error::Corrupt(format!("unknown error class: {other:?}"))),
        }
    }

    pub const fn is_retryable(self) -> bool {
        matches!(self, Self::Transient)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskError {
    pub class: ErrorClass,
    pub code: String,
    pub message: String,
}

impl TaskError {
    pub fn new(class: ErrorClass, code: impl Into<String>, message: impl Into<String>) -> Result<Self> {
        let error = Self {
            class,
            code: code.into(),
            message: message.into(),
        };
        error.validate()?;
        Ok(error)
    }

    pub const fn is_retryable(&self) -> bool {
        self.class.is_retryable()
    }

    fn validate(&self) -> Result<()> {
        let valid_code = !self.code.is_empty()
            && self.code.len() <= 64
            && self
                .code
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
        if !valid_code {
            return Err(Error::InvalidInput("error code must be 1-64 ascii characters".into()));
        }
        if self.message.trim().is_empty() || self.message.chars().count() > 500 {
            return Err(Error::InvalidInput("error message must be 1-500 characters".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub index: u32,
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
    pub status: SegmentStatus,
}

impl Segment {
    pub const fn byte_len(&self) -> u64 {
        self.end.saturating_sub(self.start).saturating_add(1)
    }

    pub const fn remaining(&self) -> u64 {
        self.byte_len().saturating_sub(self.downloaded)
    }

    pub const fn is_done(&self) -> bool {
        self.downloaded >= self.byte_len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTask {
    pub id: String,
    pub url: String,
    pub final_url: Option<String>,
    pub file_name: String,
    pub dest_dir: String,
    pub temp_path: String,
    pub kind: TaskKind,
    pub status: TaskStatus,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub expected_sha256: Option<String>,
    pub hash_state: HashState,
    pub connections: u16,
    pub error: Option<TaskError>,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    /// Có ⇒ task con ẩn (luồng của một task media): không vào danh sách, xoá theo cha.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Mỗi yêu cầu HTTP xin tối đa chừng này byte (YouTube bóp yêu cầu lớn hơn ~10 MiB).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_request_bytes: Option<u64>,
    /// Header riêng cho mọi yêu cầu của task, chỉ gửi tới đúng origin của `url`.
    /// Không đưa ra JSON (giao diện không cần, tránh lộ giá trị).
    #[serde(default, skip_serializing)]
    pub request_headers: Vec<RequestHeader>,
}

impl DownloadTask {
    pub fn new(
        url: impl Into<String>,
        file_name: impl Into<String>,
        dest_dir: impl Into<String>,
        connections: u16,
    ) -> Result<Self> {
        let id = Uuid::new_v4().to_string();
        let url = url.into();
        let file_name = file_name.into();
        let dest_dir = dest_dir.into();
        validate_file_name(&file_name)?;
        let temp_path = Path::new(&dest_dir)
            .join(format!("{file_name}.{}.part", &id[..8]))
            .to_string_lossy()
            .into_owned();
        let timestamp = now_ms();
        let task = Self {
            id,
            url,
            final_url: None,
            file_name,
            dest_dir,
            temp_path,
            kind: TaskKind::Http,
            status: TaskStatus::Queued,
            total_bytes: None,
            downloaded_bytes: 0,
            etag: None,
            last_modified: None,
            expected_sha256: None,
            hash_state: HashState::Pending,
            connections,
            error: None,
            created_at: timestamp,
            updated_at: timestamp,
            completed_at: None,
            parent_id: None,
            max_request_bytes: None,
            request_headers: Vec::new(),
        };
        task.validate()?;
        Ok(task)
    }

    pub fn with_kind(mut self, kind: TaskKind) -> Self {
        self.kind = kind;
        self
    }

    /// Biến thành task con ẩn của `parent_id` (loại `MediaPart`).
    pub fn with_parent(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self.kind = TaskKind::MediaPart;
        self
    }

    pub fn is_child(&self) -> bool {
        self.parent_id.is_some()
    }

    pub fn with_total_bytes(mut self, total_bytes: u64) -> Self {
        self.total_bytes = Some(total_bytes);
        self
    }

    pub fn with_expected_sha256(mut self, sha256: impl Into<String>) -> Self {
        self.expected_sha256 = Some(sha256.into().to_ascii_lowercase());
        self.hash_state = HashState::Pending;
        self
    }

    pub fn progress_ratio(&self) -> f64 {
        match self.total_bytes {
            Some(total) if total > 0 => (self.downloaded_bytes as f64 / total as f64).clamp(0.0, 1.0),
            Some(_) => 0.0,
            None => 0.0,
        }
    }

    pub fn remaining_bytes(&self) -> Option<u64> {
        self.total_bytes.map(|total| total.saturating_sub(self.downloaded_bytes))
    }

    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty() || self.id.len() > 64 {
            return Err(Error::InvalidInput("task id must be 1-64 characters".into()));
        }
        validate_url(&self.url)?;
        if let Some(final_url) = &self.final_url {
            validate_url(final_url)?;
        }
        validate_file_name(&self.file_name)?;
        validate_absolute_path("dest_dir", &self.dest_dir)?;
        validate_absolute_path("temp_path", &self.temp_path)?;
        let dest_dir = Path::new(&self.dest_dir);
        let temp_path = Path::new(&self.temp_path);
        if !temp_path.starts_with(dest_dir) {
            return Err(Error::InvalidInput("temp_path must stay inside dest_dir".into()));
        }
        if dest_dir
            .components()
            .chain(temp_path.components())
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(Error::InvalidInput(
                "download paths must not contain parent-directory segments".into(),
            ));
        }
        if self.connections == 0 || self.connections > MAX_CONNECTIONS {
            return Err(Error::InvalidInput(format!(
                "connections must be 1-{MAX_CONNECTIONS}"
            )));
        }
        if let Some(total) = self.total_bytes
            && self.downloaded_bytes > total
        {
            return Err(Error::InvalidInput(
                "downloaded_bytes cannot exceed total_bytes".into(),
            ));
        }
        if let Some(sha256) = &self.expected_sha256 {
            validate_sha256(sha256)?;
        }
        if let Some(error) = &self.error {
            error.validate()?;
        }
        self.validate_extras()?;
        if self.updated_at < self.created_at {
            return Err(Error::InvalidInput("updated_at precedes created_at".into()));
        }
        if let Some(completed_at) = self.completed_at
            && completed_at < self.created_at
        {
            return Err(Error::InvalidInput("completed_at precedes created_at".into()));
        }
        if self.status == TaskStatus::Completed {
            if !matches!(self.hash_state, HashState::Verified | HashState::Unavailable) {
                return Err(Error::InvalidInput(
                    "completed task must be verified or have no expected hash".into(),
                ));
            }
            if self.total_bytes.is_some_and(|total| self.downloaded_bytes != total) {
                return Err(Error::InvalidInput(
                    "completed task must have all bytes downloaded".into(),
                ));
            }
        }
        Ok(())
    }

    fn validate_extras(&self) -> Result<()> {
        if let Some(parent) = &self.parent_id
            && (parent.is_empty() || parent.len() > 64 || *parent == self.id)
        {
            return Err(Error::InvalidInput(
                "parent id must be 1-64 characters and differ from the task id".into(),
            ));
        }
        if self.kind == TaskKind::MediaPart && self.parent_id.is_none() {
            return Err(Error::InvalidInput("a media part needs a parent task".into()));
        }
        if self
            .max_request_bytes
            .is_some_and(|bytes| bytes < MIN_REQUEST_BYTES)
        {
            return Err(Error::InvalidInput(format!(
                "max_request_bytes must be at least {MIN_REQUEST_BYTES}"
            )));
        }
        validate_request_headers(&self.request_headers)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaJob {
    pub task_id: String,
    pub url: String,
    pub page_url: Option<String>,
    pub selector: String,
    pub audio_format: Option<String>,
    pub title: String,
    pub created_at: i64,
}

impl MediaJob {
    pub fn validate(&self) -> Result<()> {
        if self.task_id.is_empty() || self.task_id.len() > 64 {
            return Err(Error::InvalidInput("media task id must be 1-64 characters".into()));
        }
        validate_url(&self.url)?;
        if let Some(page_url) = &self.page_url {
            validate_url(page_url)?;
        }
        if self.selector.trim().is_empty()
            || self.selector.len() > 512
            || self.selector.chars().any(char::is_control)
        {
            return Err(Error::InvalidInput(
                "media selector must be 1-512 characters without control characters".into(),
            ));
        }
        if let Some(audio_format) = &self.audio_format
            && !matches!(audio_format.as_str(), "m4a" | "mp3")
        {
            return Err(Error::InvalidInput(
                "audio_format must be m4a or mp3".into(),
            ));
        }
        if self.title.trim().is_empty() || self.title.chars().count() > 300 {
            return Err(Error::InvalidInput("media title must be 1-300 characters".into()));
        }
        Ok(())
    }
}

pub fn plan_segments(total_bytes: u64, connections: u16) -> Vec<Segment> {
    if total_bytes == 0 {
        return Vec::new();
    }
    let by_size = total_bytes.div_ceil(MIN_SEGMENT_BYTES).min(MAX_SEGMENTS as u64);
    let wanted = u64::from(connections.max(1)).min(by_size).max(1);
    let base = total_bytes / wanted;
    let extra = total_bytes % wanted;
    let mut start = 0u64;
    (0..wanted as usize)
        .map(|position| {
            let length = base + u64::from((position as u64) < extra);
            let end = start + length - 1;
            let segment = Segment {
                index: position as u32,
                start,
                end,
                downloaded: 0,
                status: SegmentStatus::Pending,
            };
            start = end + 1;
            segment
        })
        .collect()
}

pub fn validate_segments(total_bytes: Option<u64>, segments: &[Segment]) -> Result<()> {
    match total_bytes {
        None => {
            if segments.is_empty() {
                Ok(())
            } else {
                Err(Error::InvalidInput(
                    "segments require a known total size".into(),
                ))
            }
        }
        Some(0) => {
            if segments.is_empty() {
                Ok(())
            } else {
                Err(Error::InvalidInput("zero-byte task cannot have segments".into()))
            }
        }
        Some(total) => {
            if segments.is_empty() {
                return Err(Error::InvalidInput("segments must cover the total size".into()));
            }
            if segments.len() > MAX_SEGMENTS {
                return Err(Error::InvalidInput(format!(
                    "at most {MAX_SEGMENTS} segments are allowed"
                )));
            }
            let mut next_start = 0u64;
            for (position, segment) in segments.iter().enumerate() {
                if segment.index as usize != position {
                    return Err(Error::InvalidInput("segment indexes must be consecutive".into()));
                }
                if segment.start != next_start {
                    return Err(Error::InvalidInput(
                        "segments must be contiguous and start at zero".into(),
                    ));
                }
                if segment.end < segment.start {
                    return Err(Error::InvalidInput("segment end precedes start".into()));
                }
                if segment.downloaded > segment.byte_len() {
                    return Err(Error::InvalidInput(
                        "segment downloaded more bytes than its range".into(),
                    ));
                }
                if segment.status == SegmentStatus::Done && !segment.is_done() {
                    return Err(Error::InvalidInput(
                        "done segment must have all of its bytes".into(),
                    ));
                }
                next_start = segment
                    .end
                    .checked_add(1)
                    .ok_or_else(|| Error::InvalidInput("segment range overflows".into()))?;
            }
            if next_start != total {
                return Err(Error::InvalidInput(format!(
                    "segments end at {next_start}, expected {total}"
                )));
            }
            Ok(())
        }
    }
}

pub fn validate_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|error| Error::InvalidInput(format!("invalid url: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::InvalidInput("url scheme must be http or https".into()));
    }
    if url.host_str().is_none() {
        return Err(Error::InvalidInput("url must include a host".into()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::InvalidInput(
            "url must not embed credentials".into(),
        ));
    }
    Ok(url)
}

pub fn validate_file_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::InvalidInput("file name is empty".into()));
    }
    if name == "." || name == ".." {
        return Err(Error::InvalidInput("file name cannot be a relative dot".into()));
    }
    if name.chars().count() > MAX_FILE_NAME_CHARS {
        return Err(Error::InvalidInput(format!(
            "file name exceeds {MAX_FILE_NAME_CHARS} characters"
        )));
    }
    if name.trim() != name {
        return Err(Error::InvalidInput(
            "file name cannot start or end with whitespace".into(),
        ));
    }
    if name.ends_with('.') {
        return Err(Error::InvalidInput("file name cannot end with a dot".into()));
    }
    if name
        .chars()
        .any(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
    {
        return Err(Error::InvalidInput(
            "file name contains characters reserved by the filesystem".into(),
        ));
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end()
        .to_ascii_uppercase();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) {
        return Err(Error::InvalidInput(
            "file name is reserved by Windows".into(),
        ));
    }
    Ok(())
}

fn validate_absolute_path(label: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.contains('\0') {
        return Err(Error::InvalidInput(format!("{label} is empty or invalid")));
    }
    if !Path::new(value).is_absolute() {
        return Err(Error::InvalidInput(format!("{label} must be absolute")));
    }
    Ok(())
}

pub fn validate_sha256(value: &str) -> Result<()> {
    if value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(Error::InvalidInput(
            "sha256 must be 64 hexadecimal characters".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> DownloadTask {
        DownloadTask::new(
            "https://example.com/ubuntu.iso",
            "ubuntu-24.04.iso",
            if cfg!(windows) {
                "C:\\Users\\Admin\\Downloads"
            } else {
                "/Users/admin/Downloads"
            },
            8,
        )
        .expect("valid task")
    }

    #[test]
    fn plans_contiguous_non_empty_segments() {
        for total in [1, 1024, MIN_SEGMENT_BYTES, 10 * MIN_SEGMENT_BYTES + 7, 5_000_000_000] {
            for connections in [1, 4, 8, 16, 32] {
                let segments = plan_segments(total, connections);
                validate_segments(Some(total), &segments).expect("valid plan");
                assert_eq!(segments.first().unwrap().start, 0);
                assert_eq!(segments.last().unwrap().end, total - 1);
                assert!(segments.iter().all(|segment| segment.downloaded == 0));
            }
        }
    }

    #[test]
    fn keeps_small_files_in_one_segment() {
        let segments = plan_segments(64 * 1024, 32);
        assert_eq!(segments.len(), 1);
        assert_eq!((segments[0].start, segments[0].end), (0, 64 * 1024 - 1));
    }

    #[test]
    fn avoids_tiny_segments_for_medium_files() {
        let segments = plan_segments(3 * MIN_SEGMENT_BYTES, 16);
        assert_eq!(segments.len(), 3);
    }

    #[test]
    fn empty_file_has_no_segments() {
        assert!(plan_segments(0, 8).is_empty());
        validate_segments(Some(0), &[]).expect("empty plan");
    }

    #[test]
    fn segment_helpers_track_remaining_bytes() {
        let segment = plan_segments(10, 1).remove(0);
        assert_eq!(segment.byte_len(), 10);
        assert_eq!(segment.remaining(), 10);
        assert!(!segment.is_done());
    }

    #[test]
    fn rejects_gaps_overlaps_and_wrong_totals() {
        let total = 4 * MIN_SEGMENT_BYTES;
        let mut segments = plan_segments(total, 4);
        assert_eq!(segments.len(), 4);
        segments[1].start = 40;
        assert!(validate_segments(Some(total), &segments).is_err());

        let segments = plan_segments(total, 4);
        assert!(validate_segments(Some(total + 1), &segments).is_err());

        let mut segments = plan_segments(total, 4);
        segments[2].index = 7;
        assert!(validate_segments(Some(total), &segments).is_err());
    }

    #[test]
    fn rejects_done_segment_with_missing_bytes() {
        let total = 4 * MIN_SEGMENT_BYTES;
        let mut segments = plan_segments(total, 2);
        assert_eq!(segments.len(), 2);
        segments[0].status = SegmentStatus::Done;
        segments[0].downloaded = 10;
        assert!(validate_segments(Some(total), &segments).is_err());
    }

    #[test]
    fn rejects_unknown_total_with_segments() {
        assert!(validate_segments(None, &plan_segments(10, 2)).is_err());
    }

    #[test]
    fn validates_url_scheme_host_and_credentials() {
        assert!(validate_url("https://example.com/file.bin").is_ok());
        assert!(validate_url("ftp://example.com/file.bin").is_err());
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url("https://user:pass@example.com/file.bin").is_err());
        assert!(validate_url("not a url").is_err());
    }

    #[test]
    fn blocks_dangerous_file_names() {
        for name in [
            "",
            ".",
            "..",
            "CON",
            "con.txt",
            "NUL.iso",
            "a/b.iso",
            "a\\b.iso",
            "bad?.iso",
            "trailing.",
            " leading.iso",
            "trailing.iso ",
        ] {
            assert!(validate_file_name(name).is_err(), "accepted {name:?}");
        }
        for name in ["ubuntu-24.04.iso", "tải-về.mp4", "archive.tar.gz", "my file.iso"] {
            assert!(validate_file_name(name).is_ok(), "rejected {name:?}");
        }
    }

    #[test]
    fn new_task_starts_queued_with_unique_temp_file() {
        let first = task();
        let second = task();
        assert_eq!(first.status, TaskStatus::Queued);
        assert_eq!(first.hash_state, HashState::Pending);
        assert_ne!(first.temp_path, second.temp_path);
        assert!(first.temp_path.starts_with(&first.dest_dir));
        assert!(first.temp_path.ends_with(".part"));
        first.validate().expect("valid");
    }

    #[test]
    fn task_validation_catches_unsafe_paths_and_hash() {
        let mut value = task();
        value.temp_path = "C:\\Windows\\evil.part".into();
        assert!(value.validate().is_err());

        let mut value = task();
        value.dest_dir = "relative".into();
        assert!(value.validate().is_err());

        let mut value = task();
        value.expected_sha256 = Some("abc".into());
        assert!(value.validate().is_err());

        let mut value = task();
        value.connections = 0;
        assert!(value.validate().is_err());
    }

    #[test]
    fn completed_task_needs_verified_hash_and_all_bytes() {
        let mut value = task().with_total_bytes(100);
        value.downloaded_bytes = 100;
        value.status = TaskStatus::Completed;
        assert!(value.validate().is_err(), "pending hash cannot complete");

        value.hash_state = HashState::Verified;
        value.completed_at = Some(now_ms());
        value.validate().expect("verified completion");

        value.downloaded_bytes = 99;
        assert!(value.validate().is_err(), "missing bytes cannot complete");
    }

    #[test]
    fn progress_and_remaining_are_clamped() {
        let mut value = task().with_total_bytes(200);
        value.downloaded_bytes = 50;
        assert!((value.progress_ratio() - 0.25).abs() < f64::EPSILON);
        assert_eq!(value.remaining_bytes(), Some(150));
        assert_eq!(task().progress_ratio(), 0.0);
    }

    #[test]
    fn serializes_with_camel_case_contract() {
        let task = task();
        let json = serde_json::to_string(&task).expect("serialize");
        assert!(json.contains("\"fileName\""));
        assert!(json.contains("\"downloadedBytes\""));
        assert!(!json.contains("file_name"));
        let decoded: DownloadTask = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, task);
    }

    #[test]
    fn child_tasks_need_a_distinct_parent() {
        let child = task().with_parent("parent-1");
        assert_eq!(child.kind, TaskKind::MediaPart);
        assert!(child.is_child());
        child.validate().expect("valid child");
        assert_eq!(TaskKind::parse("media_part").unwrap(), TaskKind::MediaPart);

        let mut orphan = task();
        orphan.kind = TaskKind::MediaPart;
        assert!(orphan.validate().is_err());

        let mut own = task();
        own.parent_id = Some(own.id.clone());
        assert!(own.validate().is_err());
    }

    #[test]
    fn request_cap_and_headers_are_validated_and_kept_out_of_json() {
        let mut value = task();
        value.max_request_bytes = Some(MIN_REQUEST_BYTES - 1);
        assert!(value.validate().is_err());
        value.max_request_bytes = Some(10 * 1024 * 1024);
        value.request_headers = vec![RequestHeader::new("User-Agent", "Secret-UA/1").unwrap()];
        value.validate().expect("valid");
        let json = serde_json::to_string(&value).expect("serialize");
        assert!(!json.contains("Secret-UA"));
        assert!(json.contains("\"maxRequestBytes\""));
        assert!(!format!("{value:?}").contains("Secret-UA"));

        value.request_headers.push(RequestHeader {
            name: "Cookie".into(),
            value: "a".into(),
        });
        assert!(value.validate().is_err());
    }

    #[test]
    fn error_codes_are_constrained() {
        assert!(TaskError::new(ErrorClass::Transient, "http_503", "Server asked us to retry").is_ok());
        assert!(TaskError::new(ErrorClass::Security, "", "empty").is_err());
        assert!(TaskError::new(ErrorClass::Security, "bad code", "spaces").is_err());
        assert!(TaskError::new(ErrorClass::Security, "guard", " ").is_err());
        assert!(ErrorClass::Transient.is_retryable());
        assert!(!ErrorClass::Permanent.is_retryable());
    }

    #[test]
    fn validates_media_jobs() {
        let job = MediaJob {
            task_id: "task-1".into(),
            url: "https://example.com/video".into(),
            page_url: Some("https://example.com/watch".into()),
            selector: "bv*+ba/b".into(),
            audio_format: None,
            title: "Một video".into(),
            created_at: now_ms(),
        };
        job.validate().expect("valid media job");

        let mut broken = job.clone();
        broken.selector = "bad\nselector".into();
        assert!(broken.validate().is_err());

        let mut broken = job.clone();
        broken.audio_format = Some("wav".into());
        assert!(broken.validate().is_err());

        let mut broken = job;
        broken.url = "ftp://example.com/video".into();
        assert!(broken.validate().is_err());
    }
}
