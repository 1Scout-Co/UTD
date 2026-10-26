//! Đưa các mục đã chọn vào hàng đợi chung: ảnh/video có link file trên CDN tải thẳng bằng HTTP (link đã có chữ
//! ký của trang, không cookie); video chỉ có trang bài viết thì là task media (yt-dlp). Mục lấy từ danh sách
//! app đang giữ (đã kiểm ở `entries`), giao diện chỉ gửi id.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use utd_engine::DownloadTask;

use super::entries::{Kind, SocialEntry};
use crate::commands::AddDownloadInput;
use crate::media::AddMediaInput;
use crate::media::channel::ChannelPick;
use crate::queue::{Entry, Place};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialDownloadInput {
    pub listing_id: String,
    pub ids: Vec<String>,
    /// Thư mục con (tên trang / người dùng) trong thư mục tải.
    pub subfolder: Option<String>,
    pub dest_dir: Option<String>,
    /// `true` ⇒ cuối hàng đợi.
    #[serde(default)]
    pub later: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialDownloadResult {
    pub tasks: Vec<DownloadTask>,
    pub failed: usize,
}

#[tauri::command]
pub async fn add_social_downloads(
    app: AppHandle,
    input: SocialDownloadInput,
) -> Result<SocialDownloadResult, String> {
    tauri::async_runtime::spawn_blocking(move || add(&app, input))
        .await
        .map_err(|error| error.to_string())?
}

fn add(app: &AppHandle, input: SocialDownloadInput) -> Result<SocialDownloadResult, String> {
    let (key, entries) = super::registry()
        .entries(&input.listing_id)
        .ok_or_else(|| "[social.gone] Danh sách đã hết hạn. Bấm lại trên trang.".to_string())?;
    let wanted: HashSet<&str> = input.ids.iter().map(String::as_str).collect();
    let picked: Vec<&SocialEntry> = entries
        .iter()
        .filter(|entry| wanted.contains(entry.id.as_str()))
        .collect();
    if picked.is_empty() {
        return Err("[social.empty] Chưa chọn ảnh/video nào.".into());
    }
    let state = app.state::<AppState>();
    let folder = crate::commands::download_dir_for(app, input.dest_dir.as_deref())?;
    let folder = crate::media::with_subfolder(folder, input.subfolder.as_deref())?;
    let mut used = HashSet::new();
    let (mut tasks, mut queued, mut failed) = (Vec::new(), Vec::new(), 0);
    for entry in picked {
        let made = match &entry.file {
            Some(file) => add_file(&state, &folder, &mut used, entry, file),
            None => add_page(&state, &folder, entry),
        };
        match made {
            Ok((task, item)) => {
                if let Some(key) = &key {
                    let pick = ChannelPick {
                        key: key.clone(),
                        video_id: entry.id.clone(),
                    };
                    if let Err(error) = crate::media::channel::remember(&state.journal, &pick, &task.id) {
                        log::warn!("social: không nhớ được mục đã tải ({error})");
                    }
                }
                queued.push(item);
                tasks.push(task);
            }
            Err(error) => {
                log::warn!("social: không thêm được một mục ({error})");
                failed += 1;
            }
        }
    }
    let place = if input.later { Place::Back } else { Place::Front };
    state.queue.submit(queued, place)?;
    Ok(SocialDownloadResult { tasks, failed })
}

fn add_file(
    state: &AppState,
    folder: &Path,
    used: &mut HashSet<String>,
    entry: &SocialEntry,
    file: &str,
) -> Result<(DownloadTask, Entry), String> {
    let name = free_name(&file_name(entry, file), |name| {
        used.contains(&name.to_lowercase()) || folder.join(name).exists()
    });
    used.insert(name.to_lowercase());
    let input = AddDownloadInput {
        url: file.to_string(),
        dest_dir: None,
        file_name: Some(name),
        connections: None,
        expected_sha256: None,
        later: false,
        auth_grant: None,
    };
    crate::commands::create_download(&state.engine, folder, input, None)
}

fn add_page(state: &AppState, folder: &Path, entry: &SocialEntry) -> Result<(DownloadTask, Entry), String> {
    let page = entry.page.clone().ok_or("[social.no_link] Mục này không có link tải.")?;
    let mut input = AddMediaInput::with_quality(page.clone(), "best".into());
    input.title = base_name(entry);
    input.page_url = Some(page);
    let task = crate::media::create_media(&state.journal, folder, input, None)?;
    let item = Entry::new(task.id.clone());
    Ok((task, item))
}

/// `<ngày> <chú thích ngắn> <id>`: sắp theo ngày trong thư mục, id giữ tên không trùng giữa các mục.
fn base_name(entry: &SocialEntry) -> String {
    let date = entry.timestamp.and_then(day).unwrap_or_default();
    let caption: String = entry.title.chars().take(60).collect();
    let id = entry.id.replace(':', "_");
    let parts = [date.as_str(), caption.trim(), entry.album.as_deref().unwrap_or(""), id.as_str()];
    let joined = parts
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    crate::media::safe_base(&joined)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn file_name(entry: &SocialEntry, file: &str) -> String {
    let path = utd_engine::validate_url(file)
        .ok()
        .map(|url| url.path().to_ascii_lowercase())
        .unwrap_or_default();
    let known = ["jpg", "jpeg", "png", "webp", "heic", "gif", "mp4", "mov", "webm", "m4a"];
    let extension = path
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .filter(|extension| known.contains(extension))
        .unwrap_or(match entry.kind {
            Kind::Photo => "jpg",
            Kind::Video => "mp4",
        });
    format!("{}.{extension}", base_name(entry))
}

/// Ngày `YYYY-MM-DD` (UTC) của giây Unix.
fn day(seconds: i64) -> Option<String> {
    let days = seconds.div_euclid(86_400);
    // Thuật toán ngày dân sự của Howard Hinnant.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (1970..=9999).contains(&y).then(|| format!("{y:04}-{m:02}-{d:02}"))
}

/// `a.jpg` → `a (1).jpg`… tên đầu tiên chưa bị chiếm.
fn free_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_string();
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
        _ => (name, String::new()),
    };
    (1..10_000)
        .map(|counter| format!("{stem} ({counter}){extension}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| format!("{stem} {}{extension}", utd_engine::now_ms()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::social::entries::{Platform, parse_entry};
    use serde_json::json;

    fn entry(value: serde_json::Value) -> SocialEntry {
        parse_entry(Platform::Instagram, &value, 1_790_000_000).unwrap()
    }

    #[test]
    fn names_start_with_the_day_and_keep_the_id() {
        let photo = entry(json!({
            "id": "3301:2", "source": "posts", "kind": "photo", "timestamp": 1_700_000_000,
            "url": "https://scontent.cdninstagram.com/v/t51/123_n.webp?stp=dst-webp", "title": "Hoa: đào / mai?",
        }));
        assert_eq!(file_name(&photo, photo.file.as_deref().unwrap()), "2023-11-14 Hoa đào mai 3301_2.webp");
        let story = entry(json!({
            "id": "77", "source": "highlights", "kind": "video", "album": "Đà Lạt",
            "url": "https://scontent.cdninstagram.com/o1/v/t16/f2/m69/AQ.mp4?efg=x",
        }));
        assert_eq!(file_name(&story, story.file.as_deref().unwrap()), "Đà Lạt 77.mp4");
        let odd = entry(json!({ "id": "5", "source": "posts", "kind": "photo", "url": "https://scontent.cdninstagram.com/v/abc" }));
        assert_eq!(file_name(&odd, odd.file.as_deref().unwrap()), "5.jpg");
    }

    #[test]
    fn days_are_utc_calendar_dates() {
        assert_eq!(day(0).as_deref(), Some("1970-01-01"));
        assert_eq!(day(951_782_400).as_deref(), Some("2000-02-29"));
        assert_eq!(day(1_709_164_800).as_deref(), Some("2024-02-29"));
        assert_eq!(day(-1), None);
    }

    #[test]
    fn taken_names_get_a_counter() {
        let taken = ["a.jpg", "a (1).jpg"];
        assert_eq!(free_name("a.jpg", |name| taken.contains(&name)), "a (2).jpg");
        assert_eq!(free_name("b.jpg", |name| taken.contains(&name)), "b.jpg");
    }
}
