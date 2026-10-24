//! Ngôn ngữ giao diện người dùng chọn trong Cài đặt, lưu bền trong bảng settings của journal (không chỉ
//! localStorage của WebView2: xoá dữ liệu WebView2 hay đổi bản cài vẫn giữ). Chưa chọn lần nào ⇒ None, giao
//! diện tự theo ngôn ngữ của hệ điều hành. Danh sách ngôn ngữ nằm ở src/shared/i18n/languages.ts; ở đây chỉ
//! kiểm dạng mã để không ghi rác vào journal.
//!
//! macOS: menu app (About, Hide, Quit, Edit…) do Rust dựng, chữ lấy từ chính file dịch của giao diện (khoá
//! `macMenu.*`), dựng lại mỗi khi giao diện báo ngôn ngữ (`apply_ui_language`). Windows không có menu app.

use std::collections::HashMap;

use tauri::menu::{
    AboutMetadata, Menu, PredefinedMenuItem, Submenu, HELP_SUBMENU_ID, WINDOW_SUBMENU_ID,
};
use tauri::{AppHandle, Manager, Runtime};

use crate::state::AppState;

const LANGUAGE_KEY: &str = "ui_language";

/// File dịch của giao diện: nguồn chữ cho menu app. Thêm ngôn ngữ ở languages.ts thì thêm dòng ở đây.
#[cfg(any(target_os = "macos", test))]
const LOCALES: &[(&str, &str)] = &[
    ("vi", include_str!("../../src/shared/i18n/locales/vi.json")),
    ("en", include_str!("../../src/shared/i18n/locales/en.json")),
    ("zh-CN", include_str!("../../src/shared/i18n/locales/zh-CN.json")),
    ("zh-TW", include_str!("../../src/shared/i18n/locales/zh-TW.json")),
    ("ja", include_str!("../../src/shared/i18n/locales/ja.json")),
    ("ko", include_str!("../../src/shared/i18n/locales/ko.json")),
    ("th", include_str!("../../src/shared/i18n/locales/th.json")),
    ("id", include_str!("../../src/shared/i18n/locales/id.json")),
    ("hi", include_str!("../../src/shared/i18n/locales/hi.json")),
    ("es", include_str!("../../src/shared/i18n/locales/es.json")),
    ("pt-BR", include_str!("../../src/shared/i18n/locales/pt-BR.json")),
    ("fr", include_str!("../../src/shared/i18n/locales/fr.json")),
    ("de", include_str!("../../src/shared/i18n/locales/de.json")),
    ("ru", include_str!("../../src/shared/i18n/locales/ru.json")),
    ("tr", include_str!("../../src/shared/i18n/locales/tr.json")),
    ("ar", include_str!("../../src/shared/i18n/locales/ar.json")),
];

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

/// Giao diện báo ngôn ngữ đang hiện (lúc mở app và mỗi lần đổi): macOS dựng lại menu app theo ngôn ngữ đó.
#[tauri::command]
pub fn apply_ui_language(app: AppHandle, language: String) -> Result<(), String> {
    if !valid_code(&language) {
        return Err(format!("unknown language code {language:?}"));
    }
    #[cfg(target_os = "macos")]
    {
        let menu = app_menu(&app, &menu_labels(&language)).map_err(|error| error.to_string())?;
        app.set_menu(menu).map_err(|error| error.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
    Ok(())
}

/// Chữ menu app (`macMenu.*`) của một ngôn ngữ; khoá thiếu lấy tiếng Anh.
#[cfg(any(target_os = "macos", test))]
fn menu_labels(code: &str) -> HashMap<String, String> {
    let parse = |code: &str| {
        LOCALES
            .iter()
            .find(|(name, _)| *name == code)
            .and_then(|(_, json)| serde_json::from_str::<HashMap<String, String>>(json).ok())
            .unwrap_or_default()
    };
    let mut labels = parse("en");
    labels.extend(parse(code));
    labels.retain(|key, _| key.starts_with("macMenu."));
    labels
}

/// Như `Menu::default` của Tauri trên macOS (app · File · Edit · View · Window · Help), chữ theo `labels`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn app_menu<R: Runtime>(app: &AppHandle<R>, labels: &HashMap<String, String>) -> tauri::Result<Menu<R>> {
    let package = app.package_info();
    let text = |key: &str| {
        labels
            .get(&format!("macMenu.{key}"))
            .map_or_else(|| key.to_string(), |label| label.replace("{app}", &package.name))
    };
    let about = AboutMetadata {
        name: Some(package.name.clone()),
        version: Some(package.version.to_string()),
        copyright: app.config().bundle.copyright.clone(),
        authors: app.config().bundle.publisher.clone().map(|publisher| vec![publisher]),
        ..Default::default()
    };
    let app_menu = Submenu::with_items(
        app,
        &package.name,
        true,
        &[
            &PredefinedMenuItem::about(app, Some(text("about").as_str()), Some(about))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, Some(text("services").as_str()))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some(text("hide").as_str()))?,
            &PredefinedMenuItem::hide_others(app, Some(text("hideOthers").as_str()))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some(text("quit").as_str()))?,
        ],
    )?;
    let file = Submenu::with_items(
        app,
        text("file"),
        true,
        &[&PredefinedMenuItem::close_window(app, Some(text("closeWindow").as_str()))?],
    )?;
    let edit = Submenu::with_items(
        app,
        text("edit"),
        true,
        &[
            &PredefinedMenuItem::undo(app, Some(text("undo").as_str()))?,
            &PredefinedMenuItem::redo(app, Some(text("redo").as_str()))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, Some(text("cut").as_str()))?,
            &PredefinedMenuItem::copy(app, Some(text("copy").as_str()))?,
            &PredefinedMenuItem::paste(app, Some(text("paste").as_str()))?,
            &PredefinedMenuItem::select_all(app, Some(text("selectAll").as_str()))?,
        ],
    )?;
    let view = Submenu::with_items(
        app,
        text("view"),
        true,
        &[&PredefinedMenuItem::fullscreen(app, Some(text("fullscreen").as_str()))?],
    )?;
    // Id riêng của Window/Help: macOS gắn danh sách cửa sổ và ô tìm trợ giúp vào đúng hai menu này.
    let window = Submenu::with_id_and_items(
        app,
        WINDOW_SUBMENU_ID,
        text("window"),
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some(text("minimize").as_str()))?,
            &PredefinedMenuItem::maximize(app, Some(text("zoom").as_str()))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, Some(text("closeWindow").as_str()))?,
        ],
    )?;
    let help = Submenu::with_id_and_items(app, HELP_SUBMENU_ID, text("help"), true, &[])?;
    Menu::with_items(app, &[&app_menu, &file, &edit, &view, &window, &help])
}

#[cfg(test)]
mod tests {
    use super::{menu_labels, valid_code, LOCALES};

    const MENU_KEYS: &[&str] = &[
        "about", "services", "hide", "hideOthers", "quit", "file", "closeWindow", "edit", "undo", "redo",
        "cut", "copy", "paste", "selectAll", "view", "fullscreen", "window", "minimize", "zoom", "help",
    ];

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

    /// Menu macOS của mọi ngôn ngữ đủ chữ, tên app nằm trong About/Hide/Quit.
    #[test]
    fn every_language_has_the_mac_menu_labels() {
        for (code, _) in LOCALES {
            let labels = menu_labels(code);
            for key in MENU_KEYS {
                let label = labels.get(&format!("macMenu.{key}"));
                assert!(label.is_some_and(|text| !text.trim().is_empty()), "{code}: macMenu.{key}");
            }
            for key in ["about", "hide", "quit"] {
                assert!(labels[&format!("macMenu.{key}")].contains("{app}"), "{code}: macMenu.{key}");
            }
        }
        assert_eq!(menu_labels("vi")["macMenu.quit"], "Thoát {app}");
    }
}
