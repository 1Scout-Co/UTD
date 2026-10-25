//! Đăng nhập của trình duyệt (docs/specs/2026-10-23-utd-v1/notes/auth-plan.md).
//!
//! Cookie từ tiện ích chỉ nằm trong RAM của app, gói trong một "lần cho phép" (`Grant`, mã ngẫu nhiên) ứng
//! với đúng một cú bấm trên trình duyệt. Task sinh ra từ cú bấm đó (và các lần tải tiếp của chúng) dùng
//! lần cho phép ấy; journal chỉ ghi task cần đăng nhập ở trang nào. Không còn task nào dùng thì bỏ, giá trị
//! bị xoá khỏi bộ nhớ (`Zeroizing` trong `BrowserCookie`). Giao diện chỉ thấy mã và tên trang.

pub(crate) mod flow;
pub(crate) mod pipe;
pub(crate) mod settings;
pub(crate) mod site;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Deserialize;
use utd_engine::Journal;
use utd_engine::auth::{BrowserCookie, CookieJar, MAX_COOKIES};
use uuid::Uuid;
use zeroize::Zeroizing;

/// Lần cho phép chưa task nào dùng (vd hộp Tải media đang mở) thì giữ chừng này rồi bỏ.
const IDLE_GRANT: Duration = Duration::from_secs(30 * 60);
/// Một tin nhắn của tiện ích mang tối đa chừng này byte cookie (tên + giá trị).
pub(crate) const MAX_COOKIE_BYTES: usize = 64 * 1024;
const MAX_USER_AGENT: usize = 512;
/// Cài đặt (S-73): "0" = tắt "Dùng đăng nhập của trình duyệt". Mặc định bật (câu Q2).
pub(crate) const ENABLED_SETTING: &str = "browser_sign_in";

/// Phần `auth` trong tin nhắn của tiện ích.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AuthPayload {
    /// Tên miền gốc tiện ích đã xin quyền (app kiểm lại bằng danh sách hậu tố công khai).
    pub site: String,
    #[serde(default)]
    pub cookies: Vec<CookieInput>,
    #[serde(default)]
    pub user_agent: Option<String>,
}

/// Một cookie như `chrome.cookies.Cookie` (tiện ích đổi `expirationDate` thành giây nguyên).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CookieInput {
    pub name: String,
    pub value: String,
    pub domain: String,
    #[serde(default)]
    pub host_only: bool,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub secure: bool,
    #[serde(default)]
    pub expires: Option<i64>,
}

impl std::fmt::Debug for CookieInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CookieInput")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .field("domain", &self.domain)
            .finish()
    }
}

/// Cookie đã kiểm, sẵn sàng thành một `Grant`.
pub(crate) struct Accepted {
    pub site: String,
    pub cookies: Vec<BrowserCookie>,
    pub user_agent: Option<String>,
}

/// Kiểm phần `auth` của một tin nhắn cho các link `urls` (link tải và trang đang mở).
///
/// - Trang tiện ích khai phải đúng là trang của một trong các link (tính lại bằng danh sách hậu tố).
/// - Trang nhạy cảm chỉ nhận khi người dùng vừa xác nhận trong popup (`confirmed`).
/// - Chỉ giữ cookie thuộc đúng trang đó; cắt ở `MAX_COOKIES` và `MAX_COOKIE_BYTES`.
pub(crate) fn accept(
    payload: AuthPayload,
    urls: &[&str],
    confirmed: bool,
) -> Result<Accepted, &'static str> {
    let site = payload.site.trim().to_ascii_lowercase();
    if !urls
        .iter()
        .filter_map(|url| site::of_url(url))
        .any(|own| own == site)
    {
        return Err("auth.site_mismatch");
    }
    if site::is_sensitive(&site) && !confirmed {
        return Err("auth.sensitive");
    }
    let mut budget = MAX_COOKIE_BYTES;
    let mut cookies = Vec::new();
    for input in payload.cookies {
        let value = Zeroizing::new(input.value);
        let size = input.name.len() + value.len();
        if !site::owns(&site, &input.domain) || size > budget || cookies.len() >= MAX_COOKIES {
            continue;
        }
        budget -= size;
        cookies.push(BrowserCookie {
            name: input.name,
            value,
            domain: input.domain,
            host_only: input.host_only,
            path: input.path,
            secure: input.secure,
            expires: input.expires,
        });
    }
    let user_agent = payload.user_agent.filter(|agent| {
        !agent.is_empty()
            && agent.len() <= MAX_USER_AGENT
            && agent.trim() == agent
            && agent.bytes().all(|byte| (b' '..=b'~').contains(&byte))
    });
    Ok(Accepted {
        site,
        cookies,
        user_agent,
    })
}

/// "Dùng đăng nhập của trình duyệt" trong Cài đặt (S-73), mặc định bật.
pub(crate) fn enabled(journal: &Journal) -> bool {
    journal
        .get_setting(ENABLED_SETTING)
        .ok()
        .flatten()
        .is_none_or(|value| value != "0")
}

/// Một lần cho phép: cookie của một trang, đọc trong một cú bấm.
pub struct Grant {
    pub id: String,
    pub site: String,
    pub jar: Arc<CookieJar>,
    pub user_agent: Option<String>,
    last_used: Mutex<Instant>,
}

impl std::fmt::Debug for Grant {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Grant")
            .field("site", &self.site)
            .field("cookies", &self.jar.len())
            .finish()
    }
}

impl Grant {
    fn touch(&self) {
        if let Ok(mut last) = self.last_used.lock() {
            *last = Instant::now();
        }
    }

    fn idle_for(&self) -> Duration {
        self.last_used
            .lock()
            .map(|last| last.elapsed())
            .unwrap_or_default()
    }
}

#[derive(Default)]
struct Inner {
    grants: HashMap<String, Arc<Grant>>,
    /// Task → lần cho phép nó dùng.
    tasks: HashMap<String, String>,
    /// Hộp Tải media / Tải cả kênh đang chờ đăng nhập: (trang, link, lúc bắt đầu chờ).
    dialogs: Vec<(String, String, Instant)>,
}

/// Kho lần cho phép, chỉ ở RAM (`AppState::auth`).
#[derive(Default)]
pub(crate) struct AuthStore {
    inner: Mutex<Inner>,
}

impl AuthStore {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Lần cho phép mới. Lần cũ của cùng trang mà task còn dùng thì giữ nguyên tới khi được gắn lại.
    pub fn add(&self, accepted: Accepted) -> Arc<Grant> {
        let grant = Arc::new(Grant {
            id: Uuid::new_v4().simple().to_string(),
            site: accepted.site,
            jar: Arc::new(CookieJar::new(accepted.cookies)),
            user_agent: accepted.user_agent,
            last_used: Mutex::new(Instant::now()),
        });
        let mut inner = self.lock();
        sweep(&mut inner);
        inner.grants.insert(grant.id.clone(), grant.clone());
        grant
    }

    pub fn get(&self, grant_id: &str) -> Option<Arc<Grant>> {
        let grant = self.lock().grants.get(grant_id).cloned();
        if let Some(grant) = &grant {
            grant.touch();
        }
        grant
    }

    pub fn bind(&self, task_id: &str, grant_id: &str) {
        let mut inner = self.lock();
        if inner.grants.contains_key(grant_id) {
            inner
                .tasks
                .insert(task_id.to_string(), grant_id.to_string());
        }
    }

    pub fn for_task(&self, task_id: &str) -> Option<Arc<Grant>> {
        let inner = self.lock();
        let grant = inner
            .tasks
            .get(task_id)
            .and_then(|grant_id| inner.grants.get(grant_id))
            .cloned();
        if let Some(grant) = &grant {
            grant.touch();
        }
        grant
    }

    /// Task xong hoặc bị xoá: không giữ cookie cho nó nữa.
    pub fn release(&self, task_id: &str) {
        let mut inner = self.lock();
        inner.tasks.remove(task_id);
        sweep(&mut inner);
    }

    /// "Quên cookie" (S-73): bỏ hết; task đang dùng phải xin lại.
    pub fn forget_all(&self) {
        let mut inner = self.lock();
        inner.grants.clear();
        inner.tasks.clear();
        inner.dialogs.clear();
    }

    /// Các trang đang giữ cookie (S-73), không trùng.
    pub fn sites(&self) -> Vec<String> {
        let mut inner = self.lock();
        sweep(&mut inner);
        let mut sites: Vec<String> = inner
            .grants
            .values()
            .map(|grant| grant.site.clone())
            .collect();
        sites.sort();
        sites.dedup();
        sites
    }

    /// Hộp Tải media / Tải cả kênh gặp trang cần đăng nhập: popup sẽ đếm vào số "đang chờ".
    pub fn wait_dialog(&self, site: &str, url: &str) {
        let mut inner = self.lock();
        sweep(&mut inner);
        if !inner.dialogs.iter().any(|(_, waiting, _)| waiting == url) {
            inner
                .dialogs
                .push((site.to_string(), url.to_string(), Instant::now()));
        }
    }

    /// Link các hộp đang chờ đăng nhập ở trang `site`.
    pub fn waiting_dialogs(&self, site: &str) -> Vec<String> {
        let mut inner = self.lock();
        sweep(&mut inner);
        inner
            .dialogs
            .iter()
            .filter(|(waiting, _, _)| waiting == site)
            .map(|(_, url, _)| url.clone())
            .collect()
    }

    /// Người dùng vừa cho phép trang `site`: các hộp đang chờ thôi chờ (giao diện tự phân tích lại).
    pub fn clear_dialogs(&self, site: &str) {
        self.lock()
            .dialogs
            .retain(|(waiting, _, _)| waiting != site);
    }
}

/// Bỏ lần cho phép không task nào dùng đã lâu và hộp chờ quá hạn.
fn sweep(inner: &mut Inner) {
    let used: Vec<String> = inner.tasks.values().cloned().collect();
    inner
        .grants
        .retain(|id, grant| used.contains(id) || grant.idle_for() < IDLE_GRANT);
    inner
        .dialogs
        .retain(|(_, _, since)| since.elapsed() < IDLE_GRANT);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str, domain: &str) -> CookieInput {
        CookieInput {
            name: name.into(),
            value: format!("v-{name}"),
            domain: domain.into(),
            host_only: false,
            path: "/".into(),
            secure: true,
            expires: None,
        }
    }

    fn payload(site: &str, cookies: Vec<CookieInput>) -> AuthPayload {
        AuthPayload {
            site: site.into(),
            cookies,
            user_agent: Some("Mozilla/5.0 (Windows NT 10.0) Chrome/156".into()),
        }
    }

    #[test]
    fn keeps_only_cookies_of_the_site_the_links_belong_to() {
        let accepted = accept(
            payload(
                "google.com",
                vec![
                    input("SID", ".google.com"),
                    input("drive", "drive.google.com"),
                    input("yt", ".youtube.com"),
                ],
            ),
            &[
                "https://drive.usercontent.google.com/download?id=1",
                "https://drive.google.com/file/d/1/view",
            ],
            false,
        )
        .unwrap();
        let names: Vec<&str> = accepted
            .cookies
            .iter()
            .map(|cookie| cookie.name.as_str())
            .collect();
        assert_eq!(names, ["SID", "drive"]);
        assert_eq!(accepted.site, "google.com");
        assert!(accepted.user_agent.is_some());
    }

    #[test]
    fn refuses_a_site_that_is_not_the_links_site_or_a_silent_sensitive_one() {
        let wrong = accept(
            payload("youtube.com", vec![]),
            &["https://drive.google.com/x"],
            false,
        );
        assert_eq!(wrong.err(), Some("auth.site_mismatch"));
        let bank = || {
            payload(
                "vietcombank.com.vn",
                vec![input("s", ".vietcombank.com.vn")],
            )
        };
        let url = ["https://portal.vietcombank.com.vn/statement.pdf"];
        assert_eq!(accept(bank(), &url, false).err(), Some("auth.sensitive"));
        assert_eq!(accept(bank(), &url, true).unwrap().cookies.len(), 1);
    }

    #[test]
    fn caps_size_and_drops_bad_user_agents() {
        let mut big = input("big", ".example.com");
        big.value = "x".repeat(MAX_COOKIE_BYTES);
        let mut request = payload(
            "example.com",
            vec![input("a", "example.com"), big, input("b", "example.com")],
        );
        request.user_agent = Some("ua\r\nX-Evil: 1".into());
        let accepted = accept(request, &["https://example.com/a.zip"], false).unwrap();
        let names: Vec<&str> = accepted
            .cookies
            .iter()
            .map(|cookie| cookie.name.as_str())
            .collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(accepted.user_agent, None);
    }

    #[test]
    fn grants_live_while_a_task_uses_them_and_never_print_values() {
        let store = AuthStore::default();
        let grant = store.add(
            accept(
                payload("google.com", vec![input("SID", ".google.com")]),
                &["https://drive.google.com/"],
                false,
            )
            .unwrap(),
        );
        assert!(!format!("{grant:?}").contains("v-SID"));
        store.bind("task-1", &grant.id);
        assert_eq!(
            store.for_task("task-1").map(|found| found.id.clone()),
            Some(grant.id.clone())
        );
        assert_eq!(store.sites(), ["google.com"]);
        store.release("task-1");
        assert!(store.for_task("task-1").is_none());
        // Vừa dùng nên chưa bị bỏ; "Quên cookie" thì bỏ ngay.
        assert!(store.get(&grant.id).is_some());
        store.forget_all();
        assert!(store.get(&grant.id).is_none());
        assert!(store.sites().is_empty());
    }

    #[test]
    fn dialogs_wait_per_site() {
        let store = AuthStore::default();
        store.wait_dialog("youtube.com", "https://www.youtube.com/watch?v=1");
        store.wait_dialog("youtube.com", "https://www.youtube.com/watch?v=1");
        store.wait_dialog("tiktok.com", "https://www.tiktok.com/@a");
        assert_eq!(store.waiting_dialogs("youtube.com").len(), 1);
        store.clear_dialogs("youtube.com");
        assert!(store.waiting_dialogs("youtube.com").is_empty());
        assert_eq!(store.waiting_dialogs("tiktok.com").len(), 1);
    }
}
