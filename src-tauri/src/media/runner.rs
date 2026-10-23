use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use utd_engine::{
    now_ms, validate_file_name, DownloadTask, ErrorClass, HashState, Journal,
    ProgressSnapshot, TaskError, TaskStatus,
};

use crate::events::{PROGRESS_EVENT, TASK_EVENT};
use crate::media::{ffmpeg_location, tokio_command, tools};

enum StreamMessage {
    Out(String),
    Err(String),
    Closed,
}

struct ProgressLine {
    finished: bool,
    downloaded: u64,
    total: Option<u64>,
    speed: Option<u64>,
}

pub async fn run(
    app: AppHandle,
    journal: Arc<Journal>,
    task_id: String,
    cancel: CancellationToken,
) -> Result<(), String> {
    let mut task = journal
        .load_task(&task_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Không tìm thấy download.".to_string())?;
    if task.status == TaskStatus::Completed {
        return Ok(());
    }
    let job = journal
        .load_media_job(&task_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Không tìm thấy cấu hình media.".to_string())?;

    // Dò công cụ có chạy `--version` (cache nguội ~1 s): không chặn luồng async của tokio.
    let resolved = tauri::async_runtime::spawn_blocking(tools::resolve)
        .await
        .map_err(|error| error.to_string())?;
    let tools = match resolved {
        Ok(tools) => tools,
        Err(message) => {
            fail(
                &app,
                &journal,
                &mut task,
                TaskError {
                    class: ErrorClass::User,
                    code: "tool.ytdlp_missing".into(),
                    message,
                },
            )?;
            return Ok(());
        }
    };
    if job.audio_format.is_some() && tools.ffmpeg.is_none() {
        fail(
            &app,
            &journal,
            &mut task,
            TaskError {
                class: ErrorClass::User,
                code: "tool.ffmpeg_missing".into(),
                message: "Cần ffmpeg để tách hoặc chuyển định dạng âm thanh. Tải ffmpeg ở Cài đặt → Công cụ media rồi thử lại.".into(),
            },
        )?;
        return Ok(());
    }
    if cancel.is_cancelled() {
        let downloaded = task.downloaded_bytes;
        pause(&app, &journal, &mut task, downloaded)?;
        return Ok(());
    }

    task.status = TaskStatus::Downloading;
    task.error = None;
    task.updated_at = now_ms();
    journal.save_task(&task).map_err(|error| error.to_string())?;
    emit_task(&app, &task);

    let destination = PathBuf::from(&task.dest_dir);
    std::fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    let stem = Path::new(&task.file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("media")
        .replace('%', "%%");
    let template = format!("{stem}.%(ext)s");

    let mut command = tokio_command(&tools.yt_dlp);
    // --print ngầm bật --quiet nên phải bật lại --progress; --encoding giữ đường dẫn Unicode.
    command
        .arg("--encoding")
        .arg("utf-8")
        .arg("--progress")
        .arg("--newline")
        .arg("--no-warnings")
        .arg("--no-playlist")
        .arg("--continue")
        .arg("--retries")
        .arg("5")
        .arg("--fragment-retries")
        .arg("5")
        .arg("--concurrent-fragments")
        .arg("4")
        .arg("--socket-timeout")
        .arg("15")
        .arg("-P")
        .arg(&destination)
        .arg("-o")
        .arg(&template)
        .arg("--progress-template")
        .arg("download:UTD_PROGRESS:%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s")
        .arg("--print")
        .arg("after_move:UTD_FILE:%(filepath)s")
        .arg("-f")
        .arg(&job.selector);
    if let Some(format) = &job.audio_format {
        command.arg("-x").arg("--audio-format").arg(format);
    } else {
        command.arg("--merge-output-format").arg("mp4");
    }
    if let Some(location) = ffmpeg_location(&tools) {
        command.arg("--ffmpeg-location").arg(location);
    }
    command
        .arg(&job.url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            fail(
                &app,
                &journal,
                &mut task,
                TaskError {
                    class: ErrorClass::Permanent,
                    code: "media.spawn".into(),
                    message: format!("Không chạy được yt-dlp: {error}"),
                },
            )?;
            return Ok(());
        }
    };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Không đọc được đầu ra yt-dlp.".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Không đọc được lỗi yt-dlp.".to_string())?;
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let out_task = tokio::spawn(forward_lines(stdout, sender.clone(), StreamMessage::Out));
    let err_task = tokio::spawn(forward_lines(stderr, sender.clone(), StreamMessage::Err));
    drop(sender);

    let mut downloaded = task.downloaded_bytes;
    let mut total = task.total_bytes;
    let mut parts = PartProgress::default();
    let mut speed = 0u64;
    let mut file_path: Option<PathBuf> = None;
    let mut stderr_tail = String::new();
    let mut closed_streams = 0;
    let mut ticker = interval(Duration::from_millis(500));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_checkpoint = Instant::now();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                out_task.abort();
                err_task.abort();
                pause(&app, &journal, &mut task, downloaded)?;
                return Ok(());
            }
            message = receiver.recv() => {
                match message {
                    Some(StreamMessage::Out(line)) => {
                        if let Some(progress) = parse_progress(&line) {
                            speed = progress.speed.unwrap_or(0);
                            let (part_downloaded, part_total) = parts.update(&progress);
                            downloaded = clamp(part_downloaded, part_total.or(total));
                            if part_total.is_some() {
                                total = part_total;
                            }
                        } else if let Some(path) = line.strip_prefix("UTD_FILE:") {
                            file_path = Some(PathBuf::from(path.trim()));
                        }
                    }
                    Some(StreamMessage::Err(line)) => {
                        stderr_tail.push_str(&line);
                        stderr_tail.push('\n');
                        if stderr_tail.len() > 8192 {
                            let keep = stderr_tail.len() - 8192;
                            stderr_tail = stderr_tail.chars().skip(keep).collect();
                        }
                    }
                    Some(StreamMessage::Closed) => {
                        closed_streams += 1;
                    }
                    None => break,
                }
                if closed_streams >= 2 {
                    break;
                }
            }
            _ = ticker.tick() => {
                task.downloaded_bytes = clamp(downloaded, total);
                task.total_bytes = total;
                task.updated_at = now_ms();
                if last_checkpoint.elapsed() >= Duration::from_secs(1) {
                    journal.save_task(&task).map_err(|error| error.to_string())?;
                    last_checkpoint = Instant::now();
                }
                emit_progress(&app, &task, downloaded, speed);
            }
        }
        if closed_streams >= 2 {
            break;
        }
    }

    // Khi process kết thúc, stdout/stderr đóng. Chờ mã thoát để phân loại.
    let status = child.wait().await.map_err(|error| error.to_string())?;
    if cancel.is_cancelled() {
        pause(&app, &journal, &mut task, downloaded)?;
        return Ok(());
    }
    if !status.success() {
        task.downloaded_bytes = clamp(downloaded, total);
        task.total_bytes = total;
        fail(
            &app,
            &journal,
            &mut task,
            classify_failure(&stderr_tail),
        )?;
        return Ok(());
    }

    let reported = file_path.map(|path| {
        if path.is_absolute() {
            path
        } else {
            destination.join(path)
        }
    });
    let original_stem = Path::new(&task.file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("media")
        .to_string();
    // Dòng UTD_FILE có thể thiếu hoặc hỏng mã hoá: tìm file theo tên gốc trong thư mục đích.
    let path = match reported.filter(|path| path.is_file()) {
        Some(path) => path,
        None => match find_output(&destination, &original_stem) {
            Some(path) => path,
            None => {
                fail(
                    &app,
                    &journal,
                    &mut task,
                    TaskError {
                        class: ErrorClass::Permanent,
                        code: "media.no_output".into(),
                        message: "yt-dlp báo hoàn tất nhưng không thấy file trong thư mục tải."
                            .into(),
                    },
                )?;
                return Ok(());
            }
        },
    };
    let destination_canonical = destination
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let file_canonical = path.canonicalize().map_err(|error| error.to_string())?;
    if !file_canonical.starts_with(&destination_canonical) {
        fail(
            &app,
            &journal,
            &mut task,
            TaskError {
                class: ErrorClass::Security,
                code: "media.path_escape".into(),
                message: "File media được ghi ra ngoài thư mục đã chọn.".into(),
            },
        )?;
        return Ok(());
    }
    let size = std::fs::metadata(&file_canonical)
        .map_err(|error| error.to_string())?
        .len();
    if size == 0 {
        fail(
            &app,
            &journal,
            &mut task,
            TaskError {
                class: ErrorClass::Permanent,
                code: "media.empty_output".into(),
                message: "File media tải về rỗng.".into(),
            },
        )?;
        return Ok(());
    }
    let actual_name = file_canonical
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&task.file_name)
        .to_string();
    if validate_file_name(&actual_name).is_ok() {
        task.file_name = actual_name;
    }
    let guard = match utd_engine::safety::inspect(&file_canonical, &task.file_name) {
        Ok(report) => report,
        Err(_) => {
            fail(
                &app,
                &journal,
                &mut task,
                TaskError {
                    class: ErrorClass::Permanent,
                    code: "guard.read".into(),
                    message: "Không đọc được file để kiểm tra an toàn.".into(),
                },
            )?;
            return Ok(());
        }
    };
    if guard.action == utd_engine::safety::GuardAction::Quarantine {
        let quarantine = crate::ipc::data_dir()
            .map(|directory| directory.join("quarantine"))
            .ok();
        if utd_engine::safety::quarantine(&file_canonical, &task.file_name, quarantine.as_deref())
            .is_err()
        {
            task.status = TaskStatus::Failed;
            task.error = Some(TaskError {
                class: ErrorClass::Permanent,
                code: "guard.quarantine_failed".into(),
                message: "Không chuyển được file nguy hiểm vào khu cách ly.".into(),
            });
            task.updated_at = now_ms();
            journal.save_task(&task).map_err(|error| error.to_string())?;
            emit_task(&app, &task);
            return Ok(());
        }
        task.status = TaskStatus::Quarantined;
        task.total_bytes = Some(size);
        task.downloaded_bytes = size;
        task.error = Some(TaskError {
            class: ErrorClass::Security,
            code: guard.code,
            message: guard.reason,
        });
        task.updated_at = now_ms();
        journal.save_task(&task).map_err(|error| error.to_string())?;
        emit_task(&app, &task);
        return Ok(());
    }
    let _ = utd_engine::safety::mark_of_the_web(&file_canonical);
    task.status = TaskStatus::Completed;
    task.total_bytes = Some(size);
    task.downloaded_bytes = size;
    task.hash_state = HashState::Unavailable;
    task.completed_at = Some(now_ms());
    task.updated_at = now_ms();
    task.error = None;
    journal.save_task(&task).map_err(|error| error.to_string())?;
    emit_task(&app, &task);
    if let Ok(metadata) = std::fs::metadata(&task.temp_path)
        && metadata.is_file()
    {
        let _ = std::fs::remove_file(&task.temp_path);
    }
    let _ = out_task.await;
    let _ = err_task.await;
    Ok(())
}

/// Đọc từng dòng theo byte rồi giải mã lossy: một byte không phải UTF-8 không được làm mất phần còn lại.
async fn forward_lines<R>(
    stream: R,
    sender: mpsc::UnboundedSender<StreamMessage>,
    wrap: fn(String) -> StreamMessage,
) where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(stream);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let line = String::from_utf8_lossy(&buffer)
                    .trim_end_matches(['\r', '\n'])
                    .to_string();
                if sender.send(wrap(line)).is_err() {
                    return;
                }
            }
        }
    }
    let _ = sender.send(StreamMessage::Closed);
}

/// Cộng dồn tiến độ khi yt-dlp tải lần lượt video rồi audio (mỗi phần bắt đầu lại từ 0).
#[derive(Default)]
struct PartProgress {
    finished: u64,
    current: u64,
}

impl PartProgress {
    fn update(&mut self, progress: &ProgressLine) -> (u64, Option<u64>) {
        if progress.downloaded < self.current {
            self.finished += self.current;
        }
        self.current = progress.downloaded;
        let total = progress.total.map(|total| self.finished + total);
        let downloaded = self.finished + self.current;
        if progress.finished {
            self.finished += self.current;
            self.current = 0;
        }
        (downloaded, total)
    }
}

const MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "webm", "mov", "m4a", "mp3", "opus", "ogg", "aac", "flac", "wav",
];

/// File cuối có đúng tên gốc + đuôi media; bỏ file tạm (.part, .ytdl) và file tách định dạng (.f137.mp4).
fn find_output(destination: &Path, stem: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(destination).ok()?;
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() || metadata.len() == 0 {
                return None;
            }
            let file_stem = path.file_stem()?.to_str()?;
            let extension = path.extension()?.to_str()?.to_ascii_lowercase();
            (file_stem == stem && MEDIA_EXTENSIONS.contains(&extension.as_str()))
                .then(|| (metadata.modified().ok(), path))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

fn parse_progress(line: &str) -> Option<ProgressLine> {
    let payload = line.strip_prefix("UTD_PROGRESS:")?;
    let parts: Vec<&str> = payload.split('|').collect();
    if parts.len() < 6 {
        return None;
    }
    Some(ProgressLine {
        finished: parts[0].trim() == "finished",
        downloaded: parse_u64(parts[1]).unwrap_or(0),
        total: parse_u64(parts[2]).or_else(|| parse_u64(parts[3])),
        speed: parse_u64(parts[4]),
    })
}

fn parse_u64(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("na") || value == "None" {
        return None;
    }
    value.parse::<f64>().ok().map(|number| number.max(0.0) as u64)
}

fn clamp(downloaded: u64, total: Option<u64>) -> u64 {
    total.map_or(downloaded, |total| downloaded.min(total))
}

fn classify_failure(stderr: &str) -> TaskError {
    let message = stderr.to_ascii_lowercase();
    if message.contains("drm") {
        return error(
            ErrorClass::Security,
            "media.drm",
            "Nội dung có DRM; UTD không vượt qua bảo vệ bản quyền.",
        );
    }
    if message.contains("ffmpeg") && (message.contains("not found") || message.contains("install")) {
        return error(
            ErrorClass::User,
            "tool.ffmpeg_missing",
            "Cần ffmpeg để ghép hoặc chuyển định dạng. Tải ffmpeg ở Cài đặt → Công cụ media rồi thử lại.",
        );
    }
    if message.contains("sign in")
        || message.contains("log in")
        || message.contains("cookies")
        || message.contains("not a bot")
    {
        return error(
            ErrorClass::User,
            "media.login",
            "Nguồn này cần đăng nhập hoặc cookie. UTD chưa gửi cookie ở bản này.",
        );
    }
    if message.contains("private video")
        || message.contains("video unavailable")
        || message.contains("has been removed")
    {
        return error(
            ErrorClass::Permanent,
            "media.unavailable",
            "Media không công khai hoặc đã bị gỡ.",
        );
    }
    if message.contains("unsupported url") {
        return error(
            ErrorClass::User,
            "media.unsupported",
            "URL này chưa được hỗ trợ.",
        );
    }
    if message.contains("http error 429") {
        return error(
            ErrorClass::Transient,
            "media.rate_limited",
            "Nguồn media đang giới hạn tốc độ. Thử lại sau.",
        );
    }
    if message.contains("http error 403") {
        return error(
            ErrorClass::User,
            "media.forbidden",
            "Nguồn media từ chối truy cập.",
        );
    }
    if message.contains("http error 404") {
        return error(
            ErrorClass::Permanent,
            "media.not_found",
            "Không tìm thấy media trên máy chủ.",
        );
    }
    if message.contains("timed out")
        || message.contains("connection")
        || message.contains("network")
    {
        return error(
            ErrorClass::Transient,
            "media.network",
            "Kết nối tới nguồn media bị gián đoạn.",
        );
    }
    error(
        ErrorClass::Transient,
        "media.failed",
        "Không tải được media. Kiểm tra URL rồi thử lại.",
    )
}

fn error(class: ErrorClass, code: &str, message: &str) -> TaskError {
    TaskError {
        class,
        code: code.into(),
        message: message.into(),
    }
}

fn emit_task(app: &AppHandle, task: &DownloadTask) {
    let _ = app.emit(TASK_EVENT, task);
}

fn emit_progress(
    app: &AppHandle,
    task: &DownloadTask,
    downloaded: u64,
    speed: u64,
) {
    let _ = app.emit(
        PROGRESS_EVENT,
        ProgressSnapshot {
            task_id: task.id.clone(),
            status: task.status,
            downloaded_bytes: downloaded,
            total_bytes: task.total_bytes,
            bytes_per_second: speed,
            connections: 1,
            segments_done: 0,
            segments_total: 0,
            hash_state: task.hash_state,
        },
    );
}

fn fail(
    app: &AppHandle,
    journal: &Journal,
    task: &mut DownloadTask,
    task_error: TaskError,
) -> Result<(), String> {
    task.status = TaskStatus::Failed;
    task.error = Some(task_error);
    task.updated_at = now_ms();
    journal.save_task(task).map_err(|error| error.to_string())?;
    emit_task(app, task);
    Ok(())
}

fn pause(
    app: &AppHandle,
    journal: &Journal,
    task: &mut DownloadTask,
    downloaded: u64,
) -> Result<(), String> {
    task.status = TaskStatus::Paused;
    task.downloaded_bytes = clamp(downloaded, task.total_bytes);
    task.updated_at = now_ms();
    task.error = None;
    journal.save_task(task).map_err(|error| error.to_string())?;
    emit_task(app, task);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(status: &str, downloaded: u64, total: u64) -> ProgressLine {
        parse_progress(&format!(
            "UTD_PROGRESS:{status}|{downloaded}|{total}|NA|1000|5"
        ))
        .expect("progress line")
    }

    #[tokio::test]
    async fn keeps_reading_after_a_non_utf8_line() {
        // "Cùng" theo cp1252: ù = 0xF9, không phải UTF-8 hợp lệ.
        let bytes: &[u8] =
            b"UTD_PROGRESS:downloading|1|2|NA|NA|NA\r\nUTD_FILE:C:/UTD/C\xf9ng.mp4\nUTD_FILE:C:/UTD/ok.mp4\n";
        let (sender, mut receiver) = mpsc::unbounded_channel();
        forward_lines(bytes, sender, StreamMessage::Out).await;
        let mut lines = Vec::new();
        while let Some(message) = receiver.recv().await {
            match message {
                StreamMessage::Out(line) => lines.push(line),
                StreamMessage::Closed => break,
                StreamMessage::Err(_) => {}
            }
        }
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "UTD_PROGRESS:downloading|1|2|NA|NA|NA");
        assert_eq!(lines[1], "UTD_FILE:C:/UTD/C\u{fffd}ng.mp4");
        assert_eq!(lines[2], "UTD_FILE:C:/UTD/ok.mp4");
    }

    #[test]
    fn adds_up_video_then_audio_parts() {
        let mut parts = PartProgress::default();
        assert_eq!(parts.update(&progress("downloading", 50, 100)), (50, Some(100)));
        assert_eq!(parts.update(&progress("finished", 100, 100)), (100, Some(100)));
        assert_eq!(parts.update(&progress("downloading", 10, 40)), (110, Some(140)));
        assert_eq!(parts.update(&progress("downloading", 40, 40)), (140, Some(140)));
    }

    #[test]
    fn treats_a_counter_reset_as_a_new_part() {
        let mut parts = PartProgress::default();
        parts.update(&progress("downloading", 90, 100));
        assert_eq!(parts.update(&progress("downloading", 5, 20)), (95, Some(110)));
    }

    #[test]
    fn finds_the_final_file_by_its_original_name() {
        let directory = std::env::temp_dir().join(format!("utd-runner-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let stem = "Đi Cùng Pew - Giao máy";
        for name in [
            format!("{stem}.f137.mp4"),
            format!("{stem}.mp4.part"),
            format!("{stem}.mp4.ytdl"),
            format!("{stem} (1).mp4"),
            "khac.mp4".to_string(),
        ] {
            std::fs::write(directory.join(name), b"x").unwrap();
        }
        std::fs::write(directory.join(format!("{stem}.webm")), b"").unwrap();
        assert_eq!(find_output(&directory, stem), None);

        let expected = directory.join(format!("{stem}.mp4"));
        std::fs::write(&expected, b"video").unwrap();
        assert_eq!(find_output(&directory, stem), Some(expected));
        let _ = std::fs::remove_dir_all(&directory);
    }
}
