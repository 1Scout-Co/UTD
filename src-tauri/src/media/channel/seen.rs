//! Nhớ video đã tải của từng kênh theo id video, để lần sau "Tải video mới" chỉ chọn phần chưa có. Lưu
//! trong bảng `settings` của journal (không thêm bảng/migration). Khoá cài đặt tối đa 64 ký tự mà khoá
//! kênh TikTok dài ~80 (`tiktok.com:<secUid 55 ký tự>`) ⇒ khoá là `channel.seen.<băm FNV-1a 64 bit>`,
//! giá trị JSON `{ "key": khoá kênh, "ids": { id video: id task } }` (khác `key` ⇒ coi như trống).
//!
//! Video tính là "đã có" khi task của nó còn trong danh sách và chưa lỗi, hoặc đã tải xong rồi mới bị bỏ
//! khỏi danh sách (vẫn giữ file). Task lỗi thì lần sau được chọn lại. Video được nhớ ngay lúc xếp hàng
//! (tải cả kênh gửi hết vào hàng đợi chung), nên bỏ khỏi danh sách task chưa xong (vd xoá các task đang
//! chờ để ngưng tải kênh) thì quên video đó (`forget`): khoá ngược `channel.task.<id task>` = khoá kênh.

use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use utd_engine::{Journal, TaskStatus};

const PREFIX: &str = "channel.seen.";
const TASK_PREFIX: &str = "channel.task.";
/// Một kênh nhớ tối đa chừng này id (bỏ id nhỏ nhất ⇒ thường là video cũ nhất).
const MAX_IDS: usize = 20_000;

/// Đọc-sửa-ghi một khoá: hai lệnh thêm cùng lúc không làm mất id của nhau.
static WRITE: Mutex<()> = Mutex::new(());

/// Video này thuộc kênh nào (gửi kèm `add_media_download` khi tải cả kênh).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPick {
    pub key: String,
    pub video_id: String,
}

pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 200
        && key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.@:".contains(character))
}

fn valid_video_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Stored {
    key: String,
    ids: BTreeMap<String, String>,
}

/// Khoá cài đặt của một kênh (≤ 64 ký tự).
fn setting_key(key: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{PREFIX}{hash:016x}")
}

/// Khoá ngược task → kênh; id task quá dài (không phải uuid) thì không có.
fn task_key(task_id: &str) -> Option<String> {
    let key = format!("{TASK_PREFIX}{task_id}");
    (key.len() <= 64).then_some(key)
}

fn save_map(journal: &Journal, key: &str, ids: BTreeMap<String, String>) -> Result<(), String> {
    let stored = Stored {
        key: key.to_string(),
        ids,
    };
    let value = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
    journal
        .set_setting(&setting_key(key), &value)
        .map_err(|error| error.to_string())
}

fn load_map(journal: &Journal, key: &str) -> BTreeMap<String, String> {
    journal
        .get_setting(&setting_key(key))
        .ok()
        .flatten()
        .and_then(|value| serde_json::from_str::<Stored>(&value).ok())
        .filter(|stored| stored.key == key)
        .map(|stored| stored.ids)
        .unwrap_or_default()
}

/// Ghi nhớ sau khi đã tạo task. Lỗi chỉ làm lần sau video này hiện là "chưa tải".
pub fn remember(journal: &Journal, pick: &ChannelPick, task_id: &str) -> Result<(), String> {
    if !valid_key(&pick.key) || !valid_video_id(&pick.video_id) {
        return Err("[channel.bad_key] Khoá kênh hoặc id video không hợp lệ.".into());
    }
    let _guard = WRITE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut ids = load_map(journal, &pick.key);
    ids.insert(pick.video_id.clone(), task_id.to_string());
    while ids.len() > MAX_IDS {
        ids.pop_first();
    }
    save_map(journal, &pick.key, ids)?;
    match task_key(task_id) {
        Some(reverse) => journal
            .set_setting(&reverse, &pick.key)
            .map_err(|error| error.to_string()),
        None => Ok(()),
    }
}

/// Task vừa bị bỏ khỏi danh sách. `downloaded` (task đã tải xong, file còn đó) ⇒ vẫn nhớ video; chưa
/// xong (đang chờ, dừng, lỗi) ⇒ quên, lần sau "Chỉ video mới" chọn lại. Task không thuộc kênh nào: thôi.
pub fn forget(journal: &Journal, task_id: &str, downloaded: bool) {
    let Some(reverse) = task_key(task_id) else {
        return;
    };
    let Some(key) = journal.get_setting(&reverse).ok().flatten() else {
        return;
    };
    if !downloaded {
        let _guard = WRITE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut ids = load_map(journal, &key);
        let before = ids.len();
        ids.retain(|_, task| task != task_id);
        if ids.len() != before
            && let Err(error) = save_map(journal, &key, ids)
        {
            log::warn!("channel: không quên được video của task bị bỏ ({error})");
            return;
        }
    }
    let _ = journal.delete_setting(&reverse);
}

/// Ảnh chụp các video đã có của một kênh, đọc một lần lúc bắt đầu liệt kê.
#[derive(Debug, Default)]
pub struct Downloaded {
    ids: BTreeMap<String, String>,
    failed_tasks: HashSet<String>,
}

impl Downloaded {
    pub fn load(journal: &Journal, key: &str) -> Self {
        let ids = load_map(journal, key);
        if ids.is_empty() {
            return Self::default();
        }
        let failed_tasks = journal
            .list_tasks()
            .unwrap_or_default()
            .into_iter()
            .filter(|task| task.status == TaskStatus::Failed)
            .map(|task| task.id)
            .collect();
        Self { ids, failed_tasks }
    }

    pub fn contains(&self, video_id: &str) -> bool {
        self.ids
            .get(video_id)
            .is_some_and(|task_id| !self.failed_tasks.contains(task_id))
    }
}

/// Bài thử ngoài mô-đun (lệnh xoá task): video của kênh còn tính là "đã có" không.
#[cfg(test)]
pub(crate) fn is_downloaded(journal: &Journal, key: &str, video_id: &str) -> bool {
    Downloaded::load(journal, key).contains(video_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::channel::tests::Scratch;
    use utd_engine::{DownloadTask, HashState};

    /// Khoá thật: secUid TikTok dài 55 ký tự (vượt giới hạn 64 ký tự của khoá cài đặt nếu dùng thẳng).
    const KEY: &str = "tiktok.com:MS4wLjABAAAAv7iSuuXDJGDvJkmH_vz1qkDZYo1apxgzaxdBSeIuPiM";

    fn pick(video_id: &str) -> ChannelPick {
        ChannelPick {
            key: KEY.into(),
            video_id: video_id.into(),
        }
    }

    fn task(journal: &Journal, status: TaskStatus) -> String {
        let mut task = DownloadTask::new(
            "https://www.tiktok.com/@a/video/1",
            "a.mp4",
            std::env::temp_dir().to_string_lossy().into_owned(),
            1,
        )
        .unwrap();
        task.status = status;
        task.hash_state = HashState::Unavailable;
        journal.save_task(&task).unwrap();
        task.id
    }

    #[test]
    fn remembers_ids_and_offers_failed_ones_again() {
        let scratch = Scratch::new();
        let journal = scratch.journal();
        let done = task(journal, TaskStatus::Completed);
        let failed = task(journal, TaskStatus::Failed);
        remember(journal, &pick("v1"), &done).unwrap();
        remember(journal, &pick("v2"), &failed).unwrap();
        // Task đã xong rồi bị bỏ khỏi danh sách: journal không còn task, `forget` giữ video.
        remember(journal, &pick("v3"), "task-removed-from-list").unwrap();

        let seen = Downloaded::load(journal, KEY);
        assert!(seen.contains("v1"));
        assert!(!seen.contains("v2"), "task lỗi ⇒ tải lại được");
        assert!(seen.contains("v3"), "bỏ khỏi danh sách vẫn giữ file");
        assert!(!seen.contains("v4"));
        assert!(!Downloaded::load(journal, "youtube.com:UC1").contains("v1"));
    }

    /// Tải cả kênh nhớ video lúc xếp hàng: bỏ task chưa tải xong khỏi danh sách (ngưng tải kênh) thì video
    /// được chọn lại lần sau; task đã xong bị bỏ khỏi danh sách vẫn là "Đã tải".
    #[test]
    fn removing_an_unfinished_task_forgets_its_video() {
        let scratch = Scratch::new();
        let journal = scratch.journal();
        let done = task(journal, TaskStatus::Completed);
        let waiting = task(journal, TaskStatus::Queued);
        remember(journal, &pick("v1"), &done).unwrap();
        remember(journal, &pick("v2"), &waiting).unwrap();
        assert!(Downloaded::load(journal, KEY).contains("v2"), "đang chờ: không chọn trùng");

        journal.delete_task(&waiting).unwrap();
        forget(journal, &waiting, false);
        journal.delete_task(&done).unwrap();
        forget(journal, &done, true);
        let seen = Downloaded::load(journal, KEY);
        assert!(!seen.contains("v2"), "chưa tải xong ⇒ lần sau chọn lại");
        assert!(seen.contains("v1"), "đã tải xong ⇒ vẫn giữ file");
        assert_eq!(journal.get_setting(&task_key(&waiting).unwrap()).unwrap(), None);
        assert_eq!(journal.get_setting(&task_key(&done).unwrap()).unwrap(), None);
        forget(journal, "task-ngoai-kenh", false);
        assert!(Downloaded::load(journal, KEY).contains("v1"));
    }

    #[test]
    fn setting_keys_fit_the_journal_and_belong_to_one_channel() {
        let scratch = Scratch::new();
        let journal = scratch.journal();
        assert!(setting_key(KEY).len() <= 64);
        assert_ne!(setting_key(KEY), setting_key("youtube.com:UC1"));
        // Giá trị ghi cho kênh khác (trùng băm, hoặc bị sửa tay) không được tính.
        let other = r#"{"key":"youtube.com:UC1","ids":{"v1":"t"}}"#;
        journal.set_setting(&setting_key(KEY), other).unwrap();
        assert!(!Downloaded::load(journal, KEY).contains("v1"));
        remember(journal, &pick("v2"), "t2").unwrap();
        assert!(Downloaded::load(journal, KEY).contains("v2"));
        assert!(!Downloaded::load(journal, KEY).contains("v1"));
    }

    #[test]
    fn rejects_unsafe_keys_and_ids() {
        let scratch = Scratch::new();
        let journal = scratch.journal();
        let mut bad = pick("v1");
        bad.key = "tiktok.com:../x y".into();
        assert!(remember(journal, &bad, "t").is_err());
        assert!(remember(journal, &pick(""), "t").is_err());
        assert!(remember(journal, &pick("a\nb"), "t").is_err());
        assert!(valid_key("youtube.com:UC_x-1"));
        assert!(!valid_key(""));
    }
}
