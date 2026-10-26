//! Tải hàng loạt.
//!
//! - `add_batch_downloads` (hộp Thêm hàng loạt): tạo cả lô (file HTTP và link trang video với một chất
//!   lượng chung) rồi xếp hàng đợi MỘT lần ⇒ đúng thứ tự trong lô, dù chen đầu hay xếp cuối hàng.
//! - Lô link từ trình duyệt (chuột phải "Tải tất cả link bằng UTD"): không tạo task nào, chỉ mở hộp Thêm
//!   hàng loạt với danh sách đó để người dùng lọc, chọn rồi mới tải. Đường đi: extension gom link → native
//!   host (`native_host.rs`, kiểm lần đầu) → named pipe (`ipc.rs`, `clean_links` kiểm lại) → phát
//!   [`crate::events::BATCH_REQUEST_EVENT`]. Giao diện chưa kịp nghe (UTD vừa mở) thì giữ lô mới nhất trong
//!   [`PendingBatch`], giao diện lấy bằng `take_batch_request`.

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use utd_engine::DownloadTask;

use crate::commands::AddDownloadInput;
use crate::events::{BATCH_REQUEST_EVENT, BatchRequest};
use crate::media::AddMediaInput;
use crate::queue::{Entry, Place};
use crate::state::AppState;

/// Số link tối đa một lần thêm (bằng trần của mẫu `[001-120]` ở hộp Thêm hàng loạt).
pub const MAX_BATCH_ITEMS: usize = 10_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchInput {
    pub urls: Vec<String>,
    /// Không gửi ⇒ thư mục mặc định trong Cài đặt.
    pub dest_dir: Option<String>,
    /// Chất lượng chung cho link trang video (xem `AddMediaInput::quality`); mặc định `best`.
    pub quality: Option<String>,
    /// `true` ⇒ "Thêm vào hàng đợi" (cuối hàng); mặc định "Bắt đầu ngay" (chen đầu hàng, giữ thứ tự lô).
    #[serde(default)]
    pub later: bool,
    /// Lần cho phép đăng nhập của trang gửi lô (`BatchRequest::auth_grant`): chỉ link cùng trang kèm cookie,
    /// link sang trang khác tải không cookie (chống trang lạ mượn đăng nhập, notes/auth-plan.md §7 T8).
    #[serde(default)]
    pub auth_grant: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchFailure {
    pub url: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    pub tasks: Vec<DownloadTask>,
    pub failed: Vec<BatchFailure>,
}

/// Ngoài luồng giao diện: kiểm thư mục, tạo hàng nghìn task trong journal đều chạm đĩa.
#[tauri::command]
pub async fn add_batch_downloads(app: AppHandle, input: BatchInput) -> Result<BatchResult, String> {
    tauri::async_runtime::spawn_blocking(move || add_batch(&app, input))
        .await
        .map_err(|error| error.to_string())?
}

fn add_batch(app: &AppHandle, input: BatchInput) -> Result<BatchResult, String> {
    if input.urls.is_empty() || input.urls.len() > MAX_BATCH_ITEMS {
        return Err("[batch.invalid] Danh sách link không hợp lệ.".into());
    }
    let quality = input.quality.unwrap_or_else(|| "best".into());
    let state = app.state::<AppState>();
    let folder = crate::commands::download_dir_for(app, input.dest_dir.as_deref())?;
    let mut names = Names::default();
    let (mut tasks, mut entries, mut failed) = (Vec::new(), Vec::new(), Vec::new());
    for url in input.urls {
        let grant = crate::commands::signed_in(app, input.auth_grant.as_deref(), &url);
        let made = if crate::media_pages::is_media_page(&url) {
            let mut media = AddMediaInput::with_quality(url.clone(), quality.clone());
            media.auth_grant = grant.as_ref().map(|grant| grant.id.clone());
            crate::media::create_media(&state.journal, &folder, media, grant.as_deref())
                .map(|task| (Entry::new(task.id.clone()), task))
        } else if crate::media_pages::is_channel_page(&url) {
            // Không tải trang HTML của kênh: lô giữ lại link này kèm cách làm.
            Err(crate::media_pages::CHANNEL_IN_BATCH.to_string())
        } else if crate::media_pages::is_drive_folder(&url) {
            Err(crate::media_pages::DRIVE_FOLDER.to_string())
        } else if crate::media_pages::is_social_profile(&url) {
            Err(crate::media_pages::SOCIAL_PROFILE.to_string())
        } else {
            add_file(&state, &folder, &mut names, url.clone(), grant.as_deref())
        };
        match made {
            Ok((entry, task)) => {
                if let Some(grant) = &grant {
                    state.auth.bind(&task.id, &grant.id);
                }
                entries.push(entry);
                tasks.push(task);
            }
            Err(message) => failed.push(BatchFailure { url, message }),
        }
    }
    let place = if input.later {
        Place::Back
    } else {
        Place::Front
    };
    state.queue.submit(entries, place)?;
    Ok(BatchResult { tasks, failed })
}

/// Một file HTTP của lô. Tên trùng (link chỉ khác `?id=`, file có sẵn trong thư mục) ⇒ `tên (1).đuôi`,
/// không để lượt tải sau báo "file đã có".
fn add_file(
    state: &AppState,
    folder: &Path,
    names: &mut Names,
    url: String,
    grant: Option<&crate::auth::Grant>,
) -> Result<(Entry, DownloadTask), String> {
    let input = AddDownloadInput {
        url,
        dest_dir: None,
        file_name: None,
        connections: None,
        expected_sha256: None,
        later: false,
        auth_grant: None,
    };
    let (mut task, mut entry) =
        crate::commands::create_download(&state.engine, folder, input, grant)?;
    let taken = |name: &str| names.used(name) || folder.join(name).exists();
    if taken(&task.file_name) {
        let name = free_name(&task.file_name, taken);
        let tag = task.id.get(..8).unwrap_or(&task.id).to_string();
        task.temp_path = folder
            .join(format!("{name}.{tag}.part"))
            .to_string_lossy()
            .into_owned();
        task.file_name = name;
        // Tên đã chọn riêng: không để tên server đề xuất làm trùng lại.
        entry.rename = false;
        state
            .journal
            .save_task(&task)
            .map_err(|error| error.to_string())?;
    }
    names.insert(&task.file_name);
    Ok((entry, task))
}

/// Tên đã dùng trong lô (Windows không phân biệt hoa thường).
#[derive(Default)]
struct Names(HashSet<String>);

impl Names {
    fn used(&self, name: &str) -> bool {
        self.0.contains(&name.to_lowercase())
    }

    fn insert(&mut self, name: &str) {
        self.0.insert(name.to_lowercase());
    }
}

/// `ảnh.jpg` → `ảnh (1).jpg`, `ảnh (2).jpg`… tên đầu tiên chưa bị chiếm.
fn free_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
        _ => (name, String::new()),
    };
    (1..10_000)
        .map(|counter| format!("{stem} ({counter}){extension}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| format!("{stem} {}{extension}", utd_engine::now_ms()))
}

/// Số link tối đa một lô (extension cắt bớt trước khi gửi).
pub const MAX_LINKS: usize = 2000;
/// Độ dài tối đa một link, như link đơn ở `ipc.rs`.
pub const MAX_LINK_BYTES: usize = 8192;
/// Câu trả cho extension.
pub const BATCH_NOTE: &str = "Đã mở UTD để chọn link";

/// Bỏ link sai (không phải http/https, quá dài), bỏ trùng, giữ thứ tự. Lô rỗng hoặc quá lớn ⇒ lỗi.
pub fn clean_links(raw: &[String]) -> Result<Vec<String>, &'static str> {
    if raw.len() > MAX_LINKS {
        return Err("Quá nhiều link trong một lần gửi.");
    }
    let mut links: Vec<String> = Vec::new();
    for link in raw {
        let link = link.trim();
        let usable = link.len() <= MAX_LINK_BYTES && utd_engine::validate_url(link).is_ok();
        if usable && !links.iter().any(|known| known == link) {
            links.push(link.to_string());
        }
    }
    if links.is_empty() {
        return Err("Không có link hợp lệ.");
    }
    Ok(links)
}

/// Lô tới lúc giao diện chưa nghe sự kiện: giữ lô mới nhất tới khi giao diện hỏi; sau lần hỏi đầu thì
/// phát thẳng (cùng cách với `media_pages::PendingMedia`).
#[derive(Default)]
pub struct PendingBatch(Mutex<Pending>);

#[derive(Default)]
struct Pending {
    ready: bool,
    request: Option<BatchRequest>,
}

impl PendingBatch {
    /// Giao diện chưa sẵn sàng ⇒ giữ lại (thay lô cũ) và trả `true`.
    fn hold(&self, request: &BatchRequest) -> bool {
        let mut pending = self.lock();
        if !pending.ready {
            pending.request = Some(request.clone());
        }
        !pending.ready
    }

    fn take(&self) -> Option<BatchRequest> {
        let mut pending = self.lock();
        pending.ready = true;
        pending.request.take()
    }

    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Giao diện vừa nghe `utd://batch-request`: lô trình duyệt gửi tới trước đó (nếu có).
#[tauri::command]
pub fn take_batch_request(pending: State<'_, PendingBatch>) -> Option<BatchRequest> {
    pending.take()
}

/// Hiện + focus cửa sổ chính rồi đưa lô sang giao diện.
pub fn open_batch_flow(app: &AppHandle, request: BatchRequest) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    if app
        .try_state::<PendingBatch>()
        .is_some_and(|pending| pending.hold(&request))
    {
        return;
    }
    if let Err(error) = app.emit(BATCH_REQUEST_EVENT, request) {
        log::warn!("Batch request event failed: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_names_get_a_counter() {
        let taken = ["a.jpg", "a (1).jpg", "download"];
        let is_taken = |name: &str| taken.contains(&name);
        assert_eq!(free_name("a.jpg", is_taken), "a (2).jpg");
        assert_eq!(free_name("download", is_taken), "download (1)");
        assert_eq!(free_name(".htaccess", is_taken), ".htaccess (1)");
        let mut names = Names::default();
        names.insert("Ảnh.JPG");
        assert!(names.used("ảnh.jpg"));
    }

    fn links(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|link| link.to_string()).collect()
    }

    #[test]
    fn drops_junk_and_duplicates_in_order() {
        let cleaned = clean_links(&links(&[
            " https://example.com/a.zip ",
            "javascript:alert(1)",
            "https://example.com/a.zip",
            "ftp://example.com/b.zip",
            "not a link",
            "https://example.com/b.jpg",
        ]))
        .unwrap();
        assert_eq!(
            cleaned,
            links(&["https://example.com/a.zip", "https://example.com/b.jpg"])
        );
    }

    #[test]
    fn rejects_empty_and_oversized_batches() {
        assert!(clean_links(&links(&["mailto:x@example.com"])).is_err());
        assert!(clean_links(&[]).is_err());
        let long = format!("https://example.com/{}", "a".repeat(MAX_LINK_BYTES));
        assert!(clean_links(&[long]).is_err(), "link quá dài bị bỏ");
        let many: Vec<String> = (0..=MAX_LINKS)
            .map(|index| format!("https://example.com/{index}.jpg"))
            .collect();
        assert_eq!(
            clean_links(&many).unwrap_err(),
            "Quá nhiều link trong một lần gửi."
        );
        assert_eq!(clean_links(&many[..MAX_LINKS]).unwrap().len(), MAX_LINKS);
    }

    #[test]
    fn early_batches_wait_for_the_ui() {
        let pending = PendingBatch::default();
        let request = |link: &str| BatchRequest {
            urls: links(&[link]),
            page_url: None,
            auth_grant: None,
        };
        assert!(pending.hold(&request("https://example.com/1.zip")));
        assert!(pending.hold(&request("https://example.com/2.zip")));
        assert_eq!(pending.take(), Some(request("https://example.com/2.zip")));
        assert_eq!(pending.take(), None);
        assert!(!pending.hold(&request("https://example.com/3.zip")));
    }
}
