//! Cài đặt › Tích hợp › Đăng nhập (S-73): bật/tắt dùng đăng nhập của trình duyệt, xem trang nào đang được
//! giữ cookie, quên hết. Giao diện chỉ thấy tên trang, không bao giờ thấy giá trị.

use serde::Serialize;
use tauri::State;

use super::{ENABLED_SETTING, enabled};
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    pub enabled: bool,
    /// Trang đang được giữ cookie trong RAM.
    pub sites: Vec<String>,
}

fn status(state: &AppState) -> AuthStatus {
    AuthStatus {
        enabled: enabled(&state.journal),
        sites: state.auth.sites(),
    }
}

#[tauri::command]
pub fn auth_status(state: State<'_, AppState>) -> AuthStatus {
    status(&state)
}

/// Tắt thì quên luôn cookie đang giữ; task cần đăng nhập sẽ chờ tới khi bật lại và cho phép lại.
#[tauri::command]
pub fn set_auth_enabled(state: State<'_, AppState>, enabled: bool) -> Result<AuthStatus, String> {
    state
        .journal
        .set_setting(ENABLED_SETTING, if enabled { "1" } else { "0" })
        .map_err(|error| error.to_string())?;
    if !enabled {
        state.auth.forget_all();
    }
    Ok(status(&state))
}

#[tauri::command]
pub fn forget_auth_cookies(state: State<'_, AppState>) -> AuthStatus {
    state.auth.forget_all();
    status(&state)
}
