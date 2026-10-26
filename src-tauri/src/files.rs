//! Thao tác trên file đã tải (menu chuột phải kiểu IDM): mở, mở thư mục chứa, đổi tên, chuyển,
//! bỏ vào Thùng rác, icon thật. Mọi lệnh đọc task từ journal và chỉ chạm file của đúng task đó
//! (`dest_dir\file_name` và `temp_path`); lỗi trả về là câu tiếng Việt ngắn.

mod disk;
mod icons;
mod paths;
mod shell;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State, Window};
use tokio_util::sync::CancellationToken;
use utd_engine::safety::{self, GuardAction};
use utd_engine::{DownloadTask, Journal, TaskKind, TaskStatus, now_ms};

use crate::events::TASK_EVENT;
use crate::state::{AppState, RunningTasks};

/// Câu báo lỗi hiện thẳng cho người dùng.
mod text {
    pub(super) const NOT_FOUND: &str = "[file.not_found] Không tìm thấy download.";
    pub(super) const RUNNING: &str = "[file.running] Tạm dừng tải trước đã.";
    pub(super) const NOT_DONE: &str = "[file.not_done] File chưa tải xong.";
    pub(super) const QUARANTINED: &str = "[file.quarantined] File đang bị cách ly.";
    pub(super) const FILE_GONE: &str = "[file.gone] File không còn ở thư mục tải.";
    pub(super) const FOLDER_GONE: &str = "[file.folder_gone] Thư mục tải không còn nữa.";
    pub(super) const OUTSIDE: &str = "[file.outside] File nằm ngoài thư mục tải.";
    pub(super) const EXISTS: &str = "[file.target_exists] Đã có file trùng tên ở đó.";
    pub(super) const IN_USE: &str = "[file.in_use] File đang mở trong ứng dụng khác.";
    pub(super) const DENIED: &str = "[file.denied] Không có quyền với file này.";
    pub(super) const DISK_FULL: &str = "[file.disk_full] Ổ đích không đủ chỗ.";
    pub(super) const TOO_LONG: &str = "[file.too_long] Đường dẫn quá dài.";
    pub(super) const COPY_SHORT: &str = "[file.copy_short] Chép file không trọn vẹn.";
    pub(super) const FAILED: &str = "[file.failed] Không làm được, thử lại sau.";
    pub(super) const EMPTY_NAME: &str = "[file.empty_name] Chưa nhập tên file.";
    pub(super) const BAD_NAME: &str = "[file.bad_name] Tên file không hợp lệ.";
    pub(super) const HIDDEN_CHARS: &str = "[file.hidden_chars] Tên có ký tự ẩn hoặc đảo chiều.";
    pub(super) const SPOOFED: &str = "[file.spoofed] Tên giả đuôi file, hãy đặt tên khác.";
    pub(super) const RUNNABLE: &str = "[file.runnable] Không đổi sang loại file chạy được.";
    pub(super) const READ_FAILED: &str = "[file.read_failed] Không đọc được file.";
    pub(super) const BLOCKED: &str = "[file.blocked] UTD chặn mở file có thể chạy mã.";
    pub(super) const OPEN_FAILED: &str = "[file.open_failed] Không mở được.";
    pub(super) const NO_RECYCLE_BIN: &str = "[file.no_recycle_bin] Ổ này không có Thùng rác, chưa xoá gì.";
    pub(super) const RECYCLE_FAILED: &str = "[file.recycle_failed] Không chuyển được vào Thùng rác.";
    pub(super) const MEDIA_RENAME: &str = "[file.media_rename] Đợi tải xong rồi đổi tên.";
    pub(super) const MEDIA_MOVE: &str = "[file.media_move] Đợi tải xong rồi chuyển.";
    pub(super) const SAVE_FAILED: &str = "[file.save_failed] Không lưu được thay đổi.";
    #[cfg(not(any(windows, target_os = "macos")))]
    pub(super) const WINDOWS_ONLY: &str = "[file.windows_only] Chỉ có trên Windows.";
}

/// Giới hạn một lần hỏi icon (giao diện chỉ hỏi các dòng đang hiện).
const MAX_ICON_TASKS: usize = 500;

/// "Mở trang" (S-14, S-72): mở trang cần đăng nhập bằng trình duyệt mặc định để người dùng bấm biểu tượng
/// UTD ở đó. Chỉ link http/https không kèm tên/mật khẩu: không bao giờ mở `file:` hay giao thức lạ.
#[tauri::command]
pub async fn open_web_page(url: String) -> Result<(), String> {
    let url = utd_engine::validate_url(url.trim()).map_err(|_| text::OPEN_FAILED.to_string())?;
    blocking(move || shell::with_com(|| shell::open_web_page(url.as_str()))).await
}

/// Mở file bằng ứng dụng mặc định. Chỉ task đã xong, file còn đó, không bị cách ly.
#[tauri::command]
pub async fn open_download(state: State<'_, AppState>, task_id: String) -> Result<(), String> {
    let task = load(&state.journal, &task_id)?;
    match task.status {
        TaskStatus::Completed => {}
        TaskStatus::Quarantined => return Err(text::QUARANTINED.into()),
        _ => return Err(text::NOT_DONE.into()),
    }
    blocking(move || {
        let file = paths::final_file(&task)?.ok_or_else(|| text::FILE_GONE.to_string())?;
        // File bị thay bằng file chạy được sau khi tải: Guard sẽ cách ly thì cũng không mở.
        match safety::inspect(&file, &task.file_name) {
            Ok(report) if report.action == GuardAction::Allow => {}
            Ok(_) => return Err(text::BLOCKED.into()),
            Err(_) => return Err(text::READ_FAILED.into()),
        }
        shell::with_com(|| shell::open_file(&file))
    })
    .await
}

/// Mở Explorer và chọn sẵn file; file mất thì mở thư mục; bị cách ly thì mở `<data>\quarantine`.
#[tauri::command]
pub async fn reveal_download(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> Result<(), String> {
    let task = load(&state.journal, &task_id)?;
    let data = crate::commands::app_data_dir(&app)?;
    blocking(move || shell::with_com(|| reveal(&task, &data))).await
}

fn reveal(task: &DownloadTask, data: &Path) -> Result<(), String> {
    if task.status == TaskStatus::Quarantined {
        let quarantine = data.join("quarantine");
        if let Some(copy) = paths::quarantined_copy(&quarantine, &task.file_name) {
            return shell::select_in_folder(&copy);
        }
        std::fs::create_dir_all(&quarantine).map_err(|_| text::OPEN_FAILED.to_string())?;
        return shell::open_folder(&quarantine);
    }
    let file = if task.status == TaskStatus::Completed {
        paths::final_file(task)
    } else {
        paths::part_file(task)
    };
    if let Ok(Some(file)) = file {
        return shell::select_in_folder(&file);
    }
    let folder = Path::new(&task.dest_dir);
    if !folder.is_dir() {
        return Err(text::FOLDER_GONE.into());
    }
    shell::open_folder(folder)
}

/// Đổi tên (cả file đang dở). Không ghi đè; trả task đã cập nhật.
#[tauri::command]
pub async fn rename_download(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    new_name: String,
) -> Result<DownloadTask, String> {
    let journal = state.journal.clone();
    let running = state.running.clone();
    let task = blocking(move || {
        let _busy = Busy::claim(&running, &task_id)?;
        rename_task(&journal, &task_id, &new_name)
    })
    .await?;
    let _ = app.emit(TASK_EVENT, &task);
    Ok(task)
}

fn rename_task(journal: &Journal, task_id: &str, new_name: &str) -> Result<DownloadTask, String> {
    let mut task = load(journal, task_id)?;
    let name = paths::check_new_name(new_name)?;
    if name == task.file_name {
        return Ok(task);
    }
    let case_only = paths::same_name(&name, &task.file_name);
    if !case_only && disk::occupied(&Path::new(&task.dest_dir).join(&name)) {
        return Err(text::EXISTS.into());
    }
    match task.status {
        TaskStatus::Quarantined => Err(text::QUARANTINED.into()),
        TaskStatus::Completed => {
            let from = paths::final_file(&task)?.ok_or_else(|| text::FILE_GONE.to_string())?;
            paths::guard_allows(&from, &task.file_name, &name)?;
            let to = from.with_file_name(&name);
            disk::rename_no_replace(&from, &to).map_err(|error| disk::describe(&error))?;
            task.file_name = name;
            if let Err(message) = save(journal, &mut task) {
                let _ = disk::rename_no_replace(&to, &from);
                return Err(message);
            }
            Ok(task)
        }
        // yt-dlp tự đặt tên file tạm theo tên cũ: đổi lúc này làm mất phần đã tải.
        _ if task.kind == TaskKind::Media => Err(text::MEDIA_RENAME.into()),
        // File dở: engine ghép `.part` thành `dest_dir\file_name` khi xong ⇒ chỉ đổi journal.
        _ => {
            task.file_name = name;
            save(journal, &mut task)?;
            Ok(task)
        }
    }
}

/// Chuyển file sang thư mục khác (kiểm thư mục y như Cài đặt). Trả task đã cập nhật.
#[tauri::command]
pub async fn move_download(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    dest_dir: String,
) -> Result<DownloadTask, String> {
    let data = crate::commands::app_data_dir(&app)?;
    let journal = state.journal.clone();
    let running = state.running.clone();
    let task = blocking(move || {
        let _busy = Busy::claim(&running, &task_id)?;
        move_task(&journal, &data, &task_id, &dest_dir)
    })
    .await?;
    let _ = app.emit(TASK_EVENT, &task);
    Ok(task)
}

fn move_task(
    journal: &Journal,
    data: &Path,
    task_id: &str,
    raw_dir: &str,
) -> Result<DownloadTask, String> {
    let mut task = load(journal, task_id)?;
    let completed = match task.status {
        TaskStatus::Quarantined => return Err(text::QUARANTINED.into()),
        TaskStatus::Completed => true,
        // File tạm của yt-dlp không nằm trong journal: chuyển dở sẽ bỏ lại rác và tải lại từ đầu.
        _ if task.kind == TaskKind::Media => return Err(text::MEDIA_MOVE.into()),
        _ => false,
    };
    let folder = crate::commands::prepare_download_dir(raw_dir, data)?;
    if paths::same_dir(Path::new(&task.dest_dir), &folder) {
        return Ok(task);
    }
    let new_part = folder.join(paths::part_name(&task)?);
    let new_final = folder.join(&task.file_name);
    if disk::occupied(&new_final) || (!completed && disk::occupied(&new_part)) {
        return Err(text::EXISTS.into());
    }
    let (source, target): (Option<PathBuf>, PathBuf) = if completed {
        let file = paths::final_file(&task)?.ok_or_else(|| text::FILE_GONE.to_string())?;
        (Some(file), new_final)
    } else {
        (paths::part_file(&task)?, new_part.clone())
    };
    if let Some(from) = &source {
        disk::move_file(from, &target, completed)?;
    }
    task.dest_dir = folder.to_string_lossy().into_owned();
    task.temp_path = new_part.to_string_lossy().into_owned();
    if let Err(message) = save(journal, &mut task) {
        if let Some(from) = &source {
            disk::move_back(&target, from, completed);
        }
        return Err(message);
    }
    Ok(task)
}

/// Chuyển file (và `.part`) vào Thùng rác rồi bỏ task khỏi danh sách. Không bao giờ xoá hẳn.
#[tauri::command]
pub async fn delete_download_file(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> Result<(), String> {
    let data = crate::commands::app_data_dir(&app)?;
    let journal = state.journal.clone();
    let running = state.running.clone();
    blocking(move || {
        let _busy = Busy::claim(&running, &task_id)?;
        delete_task(&journal, &data, &task_id)
    })
    .await
}

fn delete_task(journal: &Journal, data: &Path, task_id: &str) -> Result<(), String> {
    let task = load(journal, task_id)?;
    let mut doomed: Vec<PathBuf> = Vec::new();
    match task.status {
        TaskStatus::Completed => doomed.extend(paths::final_file(&task)?),
        TaskStatus::Quarantined => doomed.extend(paths::quarantined_copy(
            &data.join("quarantine"),
            &task.file_name,
        )),
        // File dở: `dest_dir\file_name` có thể là file khác có sẵn (lỗi trùng tên) ⇒ không đụng.
        _ => {}
    }
    if paths::part_name(&task).is_ok() {
        doomed.extend(paths::part_file(&task)?);
    }
    doomed.extend(media_part_files(journal, &task)?);
    if !doomed.is_empty() {
        shell::with_com(|| shell::recycle(&doomed))?;
    }
    match journal.delete_task(task_id) {
        Ok(()) | Err(utd_engine::Error::NotFound(_)) => {
            let downloaded = task.status == TaskStatus::Completed;
            crate::media::channel::forget(journal, task_id, downloaded);
            Ok(())
        }
        Err(_) => Err(text::SAVE_FAILED.into()),
    }
}

/// Task media: file của các luồng con ẩn (`.part`, luồng đã tải xong) cũng là của task này. Dòng
/// của chúng journal tự xoá theo task cha. Luồng chưa xong mà có file trùng tên thì không đụng.
fn media_part_files(journal: &Journal, task: &DownloadTask) -> Result<Vec<PathBuf>, String> {
    if task.kind != TaskKind::Media {
        return Ok(Vec::new());
    }
    let children = journal
        .list_children(&task.id)
        .map_err(|_| text::FAILED.to_string())?;
    let mut files = Vec::new();
    for child in &children {
        if child.status == TaskStatus::Completed {
            files.extend(paths::final_file(child)?);
        }
        if paths::part_name(child).is_ok() {
            files.extend(paths::part_file(child)?);
        }
    }
    Ok(files)
}

/// Icon thật của file cho từng task: `task_id → data:image/png;base64,…`. Chỉ trả task lấy được icon.
#[tauri::command]
pub async fn file_icons(
    window: Window,
    state: State<'_, AppState>,
    task_ids: Vec<String>,
) -> Result<HashMap<String, String>, String> {
    let wanted: HashSet<String> = task_ids.into_iter().take(MAX_ICON_TASKS).collect();
    if wanted.is_empty() {
        return Ok(HashMap::new());
    }
    let size = icons::icon_px(window.scale_factor().unwrap_or(1.0));
    let journal = state.journal.clone();
    let found = tauri::async_runtime::spawn_blocking(move || {
        let tasks: Vec<DownloadTask> = wanted
            .iter()
            .filter_map(|id| journal.load_task(id).ok().flatten())
            .collect();
        icons::icons_for(&tasks, size)
    })
    .await
    .unwrap_or_default();
    Ok(found)
}

/// Giữ chỗ trong RunningTasks khi đang đổi tên/chuyển/xoá: "Tiếp tục" không chạy chen vào giữa.
struct Busy {
    running: Arc<RunningTasks>,
    task_id: String,
}

impl Busy {
    fn claim(running: &Arc<RunningTasks>, task_id: &str) -> Result<Self, String> {
        // Hỏi `contains` trước: `insert` thay token của lượt tải đang chạy nếu đã có.
        if running.contains(task_id) || !running.insert(task_id, CancellationToken::new()) {
            return Err(text::RUNNING.into());
        }
        Ok(Self {
            running: running.clone(),
            task_id: task_id.to_string(),
        })
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.running.remove(&self.task_id);
    }
}

fn load(journal: &Journal, task_id: &str) -> Result<DownloadTask, String> {
    journal
        .load_task(task_id)
        .map_err(|_| text::FAILED.to_string())?
        .ok_or_else(|| text::NOT_FOUND.to_string())
}

fn save(journal: &Journal, task: &mut DownloadTask) -> Result<(), String> {
    task.updated_at = now_ms().max(task.created_at);
    task.validate()
        .and_then(|()| journal.save_task(task))
        .map_err(|_| text::SAVE_FAILED.to_string())
}

/// Việc đụng đĩa/shell chạy ngoài luồng giao diện.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| text::FAILED.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Thư mục thử, xoá sau cùng (journal SQLite phải đóng trước thì Windows mới cho xoá).
    struct TempRoot(PathBuf);

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct Fixture {
        root: PathBuf,
        journal: Journal,
        _cleanup: TempRoot,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("utd-files-cmd-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(root.join("Tải về")).unwrap();
            let journal = Journal::open(root.join("data").join("utd.db")).unwrap();
            Self {
                _cleanup: TempRoot(root.clone()),
                root,
                journal,
            }
        }

        fn downloads(&self) -> PathBuf {
            self.root.join("Tải về")
        }

        fn task(&self, name: &str, status: TaskStatus) -> DownloadTask {
            let dir = self.downloads().to_string_lossy().into_owned();
            let mut task = DownloadTask::new("https://example.com/f", name, dir, 4).unwrap();
            task.status = status;
            if status == TaskStatus::Completed {
                task.hash_state = utd_engine::HashState::Unavailable;
                task.total_bytes = Some(4);
                task.downloaded_bytes = 4;
                std::fs::write(self.downloads().join(name), b"data").unwrap();
            }
            self.journal.save_task(&task).unwrap();
            task
        }
    }

    #[test]
    fn rename_completed_file_on_disk_then_journal() {
        let fx = Fixture::new();
        let task = fx.task("bao cao.pdf", TaskStatus::Completed);
        std::fs::write(fx.downloads().join("trung.pdf"), b"khac").unwrap();
        assert_eq!(
            rename_task(&fx.journal, &task.id, "trung.pdf").unwrap_err(),
            text::EXISTS
        );
        assert_eq!(
            rename_task(&fx.journal, &task.id, "x.pdf.exe").unwrap_err(),
            text::SPOOFED
        );
        let renamed = rename_task(&fx.journal, &task.id, " Báo cáo quý.pdf ").unwrap();
        assert_eq!(renamed.file_name, "Báo cáo quý.pdf");
        assert!(fx.downloads().join("Báo cáo quý.pdf").is_file());
        assert!(!fx.downloads().join("bao cao.pdf").exists());
        assert_eq!(
            std::fs::read(fx.downloads().join("trung.pdf")).unwrap(),
            b"khac"
        );
        let saved = fx.journal.load_task(&task.id).unwrap().unwrap();
        assert_eq!(saved.file_name, "Báo cáo quý.pdf");
        #[cfg(windows)]
        assert_eq!(
            rename_task(&fx.journal, &task.id, "BÁO CÁO QUÝ.pdf")
                .unwrap()
                .file_name,
            "BÁO CÁO QUÝ.pdf",
            "đổi hoa/thường"
        );
    }

    #[test]
    fn rename_unfinished_only_touches_the_journal() {
        let fx = Fixture::new();
        let task = fx.task("phim.mkv", TaskStatus::Paused);
        std::fs::write(&task.temp_path, b"part").unwrap();
        let renamed = rename_task(&fx.journal, &task.id, "Phim hay.mkv").unwrap();
        assert_eq!(renamed.file_name, "Phim hay.mkv");
        assert_eq!(renamed.temp_path, task.temp_path, ".part giữ nguyên chỗ");
        assert!(Path::new(&task.temp_path).is_file());
        let mut media = fx.task("clip.mp4", TaskStatus::Paused);
        media.kind = TaskKind::Media;
        fx.journal.save_task(&media).unwrap();
        assert_eq!(
            rename_task(&fx.journal, &media.id, "b.mp4").unwrap_err(),
            text::MEDIA_RENAME
        );
        let quarantined = fx.task("x.exe", TaskStatus::Quarantined);
        assert_eq!(
            rename_task(&fx.journal, &quarantined.id, "y.exe").unwrap_err(),
            text::QUARANTINED
        );
    }

    #[test]
    fn move_completed_and_unfinished_files() {
        let fx = Fixture::new();
        let data = fx.root.join("data");
        let target = fx.root.join("Phim");
        let raw = target.to_string_lossy().into_owned();
        let done = fx.task("a.mp4", TaskStatus::Completed);
        let moved = move_task(&fx.journal, &data, &done.id, &raw).unwrap();
        assert_eq!(PathBuf::from(&moved.dest_dir), target);
        assert!(target.join("a.mp4").is_file());
        assert!(!fx.downloads().join("a.mp4").exists());
        assert!(Path::new(&moved.temp_path).starts_with(&target));

        let paused = fx.task("b.zip", TaskStatus::Paused);
        std::fs::write(&paused.temp_path, b"part").unwrap();
        let moved = move_task(&fx.journal, &data, &paused.id, &raw).unwrap();
        assert!(Path::new(&moved.temp_path).is_file());
        assert!(!Path::new(&paused.temp_path).exists());
        assert_eq!(
            fx.journal.load_task(&paused.id).unwrap().unwrap().temp_path,
            moved.temp_path
        );

        let clash = fx.task("a.mp4", TaskStatus::Paused);
        assert_eq!(
            move_task(&fx.journal, &data, &clash.id, &raw).unwrap_err(),
            text::EXISTS
        );
        let inside = data.join("tai").to_string_lossy().into_owned();
        assert!(
            move_task(&fx.journal, &data, &clash.id, &inside).is_err(),
            "không vào thư mục dữ liệu"
        );
        assert!(move_task(&fx.journal, &data, &clash.id, "relative").is_err());
    }

    #[test]
    fn running_tasks_are_left_alone() {
        let running = Arc::new(RunningTasks::default());
        let token = CancellationToken::new();
        assert!(running.insert("t1", token.clone()));
        assert_eq!(
            Busy::claim(&running, "t1").err().as_deref(),
            Some(text::RUNNING)
        );
        running.cancel("t1");
        assert!(token.is_cancelled(), "token của lượt tải vẫn là token cũ");
        {
            let _busy = Busy::claim(&running, "t2").unwrap();
            assert!(running.contains("t2"));
            assert!(Busy::claim(&running, "t2").is_err());
        }
        assert!(!running.contains("t2"));
    }

    #[test]
    fn media_delete_also_takes_its_hidden_stream_files() {
        let fx = Fixture::new();
        let mut media = fx.task("Clip.mp4", TaskStatus::Paused);
        media.kind = TaskKind::Media;
        fx.journal.save_task(&media).unwrap();
        let dir = fx.downloads().to_string_lossy().into_owned();
        let child = |name: &str| {
            DownloadTask::new("https://rr1.googlevideo.com/videoplayback", name, dir.clone(), 16)
                .unwrap()
                .with_parent(media.id.clone())
        };
        let video = child("Clip.f399.mp4");
        let mut audio = child("Clip.f251.webm");
        audio.status = TaskStatus::Completed;
        audio.hash_state = utd_engine::HashState::Unavailable;
        audio.total_bytes = Some(4);
        audio.downloaded_bytes = 4;
        fx.journal.save_task(&video).unwrap();
        fx.journal.save_task(&audio).unwrap();
        std::fs::write(&video.temp_path, b"part").unwrap();
        // Luồng hình chưa xong mà có file trùng tên: không phải của UTD.
        std::fs::write(fx.downloads().join("Clip.f399.mp4"), b"user").unwrap();
        std::fs::write(fx.downloads().join("Clip.f251.webm"), b"done").unwrap();

        let mut names: Vec<String> = media_part_files(&fx.journal, &media)
            .unwrap()
            .iter()
            .map(|file| file.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        let part = Path::new(&video.temp_path).file_name().unwrap();
        assert_eq!(names, ["Clip.f251.webm", &*part.to_string_lossy()]);
        let plain = fx.task("d.zip", TaskStatus::Paused);
        assert!(media_part_files(&fx.journal, &plain).unwrap().is_empty());
    }

    #[test]
    fn delete_of_a_missing_file_just_removes_the_task() {
        let fx = Fixture::new();
        let task = fx.task("c.iso", TaskStatus::Failed);
        delete_task(&fx.journal, &fx.root.join("data"), &task.id).unwrap();
        assert!(fx.journal.load_task(&task.id).unwrap().is_none());
        assert_eq!(
            delete_task(&fx.journal, &fx.root.join("data"), &task.id).unwrap_err(),
            text::NOT_FOUND
        );
    }
}
