//! Ngôn ngữ giao diện người dùng chọn trong Cài đặt, lưu bền trong bảng settings của journal (không chỉ
//! localStorage của WebView2: xoá dữ liệu WebView2 hay đổi bản cài vẫn giữ). Chưa chọn lần nào ⇒ None, giao
//! diện tự theo ngôn ngữ của Windows. Danh sách ngôn ngữ nằm ở src/shared/i18n/languages.ts; ở đây chỉ kiểm
//! dạng mã để không ghi rác vào journal.

use tauri::{AppHandle, Manager};

use crate::state::AppState;

const LANGUAGE_KEY: &str = "ui_language";

/// `vi`, `en`, `zh-CN`, `pt-BR`…: 2–3 chữ thường, có thể kèm `-` và 2 chữ hoa.
fn valid_code(code: &str) -> bool {
    let (primary, region) = match code.split_once('-') {
        Some((primary, region)) => (primary, Some(region)),
        None => (code, None),
    };
    (2..=3).contains(&primary.len())
        && primary.chars().all(|c| c.is_ascii_lowercase())
        && region.is_none_or(|region| region.len() == 2 && region.chars().all(|c| c.is_ascii_uppercase()))
}

#[tauri::command]
pub fn get_ui_language(app: AppHandle) -> Option<String> {
    let state = app.try_state::<AppState>()?;
    let saved = state.journal.get_setting(LANGUAGE_KEY).ok().flatten()?;
    valid_code(&saved).then_some(saved)
}

#[tauri::command]
pub fn set_ui_language(app: AppHandle, language: String) -> Result<(), String> {
    if !valid_code(&language) {
        return Err(format!("unknown language code {language:?}"));
    }
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "app is still starting".to_string())?;
    state
        .journal
        .set_setting(LANGUAGE_KEY, &language)
        .map_err(|error| format!("could not save the language: {error}"))
}

#[cfg(test)]
mod tests {
    use super::valid_code;

    #[test]
    fn accepts_the_ui_language_codes() {
        for code in ["vi", "en", "zh-CN", "zh-TW", "pt-BR", "ar", "fil"] {
            assert!(valid_code(code), "{code}");
        }
    }

    #[test]
    fn rejects_anything_else() {
        for code in ["", "v", "VI", "en-us", "zh-Hant", "pt-BRA", "en_US", "../x", "vi;drop", "en-"] {
            assert!(!valid_code(code), "{code}");
        }
    }
}
