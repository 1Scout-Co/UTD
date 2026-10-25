//! Luồng đăng nhập giữa tiện ích và app (qua pipe IPC): cú bấm kèm cookie, popup hỏi việc đang chờ, popup
//! cho phép rồi tải tiếp (notes/auth-plan.md §8.1). App không bao giờ tự xin cookie: mọi lần đọc cookie
//! đều do người dùng bấm trên trình duyệt.

use std::sync::Arc;

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use utd_engine::{DownloadTask, Journal, TaskStatus};

use super::{AuthPayload, Grant, accept, enabled, site};
use crate::events::{AUTH_GRANTED_EVENT, AuthGranted};
use crate::state::AppState;

/// Lỗi của task "Cần đăng nhập" (S-72): cho phép trang đó trong popup là tải tiếp được.
pub(crate) const SIGN_IN_CODES: &[&str] = &[
    "auth.required",
    "auth.expired",
    "auth.pipe",
    "http.401",
    "http.403",
    "media.login",
];
/// Popup đọc cookie cho tối đa chừng này link đang chờ.
const MAX_PENDING_URLS: usize = 20;

/// Cú bấm trên trình duyệt có kèm cookie ⇒ lần cho phép mới. Tắt trong Cài đặt, trang nhạy cảm (phải xác
/// nhận trong popup) hoặc trang không khớp link ⇒ bỏ cookie, vẫn tải như thường.
pub(crate) fn grant_from_click(
    app: &AppHandle,
    auth: Option<AuthPayload>,
    urls: &[&str],
) -> Option<Arc<Grant>> {
    let auth = auth?;
    let state = app.try_state::<AppState>()?;
    if !enabled(&state.journal) {
        return None;
    }
    match accept(auth, urls, false) {
        Ok(accepted) => Some(state.auth.add(accepted)),
        Err(code) => {
            log::info!("sign-in cookies not used: {code}");
            None
        }
    }
}

/// Task đang chờ đăng nhập ở trang `site`.
pub(crate) fn waiting_tasks(journal: &Journal, site: &str) -> Vec<DownloadTask> {
    journal
        .list_tasks()
        .unwrap_or_default()
        .into_iter()
        .filter(|task| {
            task.status == TaskStatus::Failed
                && task
                    .error
                    .as_ref()
                    .is_some_and(|error| SIGN_IN_CODES.contains(&error.code.as_str()))
                && (task.auth_sites.iter().any(|own| own == site)
                    || site::of_url(&task.url).as_deref() == Some(site))
        })
        .collect()
}

/// Câu Q7 của kế hoạch: task cần đăng nhập tải xong thì bỏ link sau chuyển hướng khỏi journal (thường là
/// link ký, chính nó là token ngắn hạn); giữ link người dùng đã bấm.
pub(crate) fn forget_signed_link(journal: &Journal, task: &DownloadTask) {
    if task.auth_sites.is_empty() || task.final_url.is_none() {
        return;
    }
    if let Ok(Some(mut stored)) = journal.load_task(&task.id) {
        stored.final_url = None;
        if let Err(error) = journal.save_task(&stored) {
            log::warn!("could not forget the signed link of a finished task: {error}");
        }
    }
}

/// `auth-pending` (popup vừa mở): trang của tab, mẫu quyền cần xin, có nhạy cảm không, việc đang chờ.
pub(crate) fn pending(app: &AppHandle, url: &str) -> Result<serde_json::Value, &'static str> {
    let site = site::of_url(url).ok_or("Chỉ hỗ trợ URL HTTP/HTTPS.")?;
    let state = app.state::<AppState>();
    let mut urls: Vec<String> = waiting_tasks(&state.journal, &site)
        .into_iter()
        .map(|task| task.url)
        .collect();
    urls.extend(state.auth.waiting_dialogs(&site));
    let pending = urls.len();
    urls.truncate(MAX_PENDING_URLS);
    Ok(json!({
        "site": site,
        "origin": site::permission_pattern(&site),
        "sensitive": site::is_sensitive(&site),
        "enabled": enabled(&state.journal),
        "pending": pending,
        "urls": urls,
    }))
}

/// `auth-renew` (người dùng bấm "Dùng đăng nhập" / "Tiếp tục" trong popup): lần cho phép mới cho trang của
/// tab; task đang chờ gắn vào rồi tải tiếp; hộp Tải media / Tải cả kênh đang chờ phân tích lại.
pub(crate) fn renew(
    app: &AppHandle,
    url: &str,
    auth: AuthPayload,
) -> Result<serde_json::Value, &'static str> {
    let state = app.state::<AppState>();
    if !enabled(&state.journal) {
        return Err("auth.disabled");
    }
    let accepted = accept(auth, &[url], true)?;
    let site = accepted.site.clone();
    let grant = state.auth.add(accepted);
    let mut resumed = 0;
    for mut task in waiting_tasks(&state.journal, &site) {
        if task.auth_sites.is_empty() {
            task.auth_sites = vec![site.clone()];
            if state.journal.save_task(&task).is_err() {
                continue;
            }
        }
        state.auth.bind(&task.id, &grant.id);
        if crate::commands::resume_task(&state, &task.id).is_ok() {
            resumed += 1;
        }
    }
    state.auth.clear_dialogs(&site);
    let granted = AuthGranted {
        site: site.clone(),
        auth_grant: grant.id.clone(),
    };
    if let Err(error) = app.emit(AUTH_GRANTED_EVENT, granted) {
        log::warn!("auth granted event failed: {error}");
    }
    Ok(json!({ "site": site, "resumed": resumed }))
}
