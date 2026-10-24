//! Thanh tiêu đề riêng của app. Kiểu khung đặt theo nền tảng: `tauri.windows.conf.json` tắt khung gốc
//! (giao diện tự vẽ thanh kéo + 3 nút), `tauri.macos.conf.json` giữ đèn giao thông gốc phủ lên nền app.
//! Rust lo phần chuột của 3 nút trên Windows (`caption.rs`) và nhấp đúp thanh trên macOS theo cài đặt hệ thống.

#[cfg(windows)]
mod activation;
#[cfg(windows)]
mod caption;

use serde::{Deserialize, Serialize};

/// Nút cửa sổ do giao diện vẽ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptionButton {
    Minimize,
    Maximize,
    Close,
}

/// Ô của một nút, tính bằng pixel thật (đã nhân devicePixelRatio) từ góc trên trái webview.
#[derive(Debug, Clone, Copy, Deserialize)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct CaptionRect {
    pub button: CaptionButton,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Giao diện báo chỗ 3 nút cửa sổ mỗi khi bố cục đổi (đổi cỡ, phóng to, đổi DPI) để Windows nhận
/// chuột ở đó như nút thật: Snap Layouts khi rê lên nút Phóng to, bấm được cả khi đang mở hộp thoại.
#[tauri::command]
pub fn set_caption_buttons(
    window: tauri::WebviewWindow,
    buttons: Vec<CaptionRect>,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        caption::place(window, buttons).map_err(|error| error.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = (window, buttons);
        Ok(())
    }
}

/// Alt+Space: mở menu hệ thống của cửa sổ (Khôi phục · Di chuyển · Cỡ · Thu nhỏ · Phóng to · Đóng).
/// WebView2 giữ phím này cho trang, không chuyển lên cửa sổ, nên giao diện bắt phím rồi gọi lệnh này.
#[tauri::command]
pub fn show_window_menu(window: tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(windows)]
    {
        caption::show_window_menu(&window).map_err(|error| error.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        Ok(())
    }
}

/// Cửa sổ có đang active không (thanh tiêu đề đậm hay mờ); giao diện hỏi khi vừa nghe xong sự kiện.
/// Windows: so với cửa sổ đang ở trước và từ đó báo `utd://active` theo WM_NCACTIVATE (`activation.rs`) —
/// `is_focused`/sự kiện focus của Tauri ở đây là focus của webview, không phải cửa sổ active.
/// macOS/Linux: `is_focused` của Tauri đúng (cửa sổ key/active).
#[tauri::command]
pub fn is_window_active(window: tauri::WebviewWindow) -> Result<bool, String> {
    #[cfg(windows)]
    {
        activation::is_active(&window).map_err(|error| error.to_string())
    }
    #[cfg(not(windows))]
    {
        window.is_focused().map_err(|error| error.to_string())
    }
}

/// Việc của nhấp đúp thanh tiêu đề (cài đặt macOS "Nhấp đúp vào thanh tiêu đề của cửa sổ để").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoubleClick {
    Zoom,
    Minimize,
    Nothing,
}

/// `AppleActionOnDoubleClick`: Maximize (Phóng to) · Minimize · None (Không làm gì) · Fill (macOS 15, Lấp đầy —
/// Tauri không có, gần nhất là phóng to). Chưa đặt thì xem khoá cũ `AppleMiniaturizeOnDoubleClick` (1 = thu nhỏ),
/// cũng không có thì phóng to như mặc định của macOS.
fn double_click_action(action: Option<&str>, miniaturize: Option<&str>) -> DoubleClick {
    match action.map(str::trim) {
        Some("Minimize") => DoubleClick::Minimize,
        Some("None") => DoubleClick::Nothing,
        Some(_) => DoubleClick::Zoom,
        None if miniaturize.map(str::trim) == Some("1") => DoubleClick::Minimize,
        None => DoubleClick::Zoom,
    }
}

/// Một khoá trong cài đặt chung của macOS (`defaults read -g`); chưa đặt (hoặc không đọc được) ⇒ `None`.
fn global_default(key: &str) -> Option<String> {
    let output = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", key])
        .output()
        .ok()?;
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !value.is_empty()).then_some(value)
}

fn system_double_click() -> DoubleClick {
    if !cfg!(target_os = "macos") {
        return DoubleClick::Zoom;
    }
    double_click_action(
        global_default("AppleActionOnDoubleClick").as_deref(),
        global_default("AppleMiniaturizeOnDoubleClick").as_deref(),
    )
}

/// Nhấp đúp thanh tiêu đề trên macOS. Thanh phủ lên webview nên AppKit không tự làm (nhấp đúp vào
/// `data-tauri-drag-region` thì Tauri luôn phóng to): đọc cài đặt hệ thống mỗi lần (người dùng đổi lúc nào cũng
/// đúng) rồi làm như thanh gốc. Toàn màn hình thì không làm gì: macOS sẽ áp trạng thái phóng to đó khi thoát.
#[tauri::command]
pub async fn title_bar_double_click(window: tauri::WebviewWindow) -> Result<(), String> {
    let action = tauri::async_runtime::spawn_blocking(system_double_click)
        .await
        .map_err(|error| error.to_string())?;
    run_double_click(&window, action).map_err(|error| error.to_string())
}

fn run_double_click(window: &tauri::WebviewWindow, action: DoubleClick) -> tauri::Result<()> {
    if window.is_fullscreen()? {
        return Ok(());
    }
    match action {
        DoubleClick::Minimize => window.minimize(),
        DoubleClick::Zoom if window.is_resizable()? => {
            if window.is_maximized()? {
                window.unmaximize()
            } else {
                window.maximize()
            }
        }
        DoubleClick::Zoom | DoubleClick::Nothing => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{DoubleClick, double_click_action};

    /// Cửa sổ chính trong một file cấu hình (`app.windows[0]`).
    fn main_window(source: &str) -> serde_json::Map<String, Value> {
        let config: Value = serde_json::from_str(source).expect("JSON hợp lệ");
        config["app"]["windows"][0]
            .as_object()
            .expect("có app.windows[0]")
            .clone()
    }

    // Cấu hình theo nền tảng gộp kiểu JSON Merge Patch: mảng `app.windows` bị thay nguyên mảng, nên file
    // của Windows/macOS phải chép đủ cửa sổ chính. Bài này giữ chúng khớp tauri.conf.json, chỉ khác khoá khung.
    #[test]
    fn platform_window_configs_only_change_frame_keys() {
        let base = main_window(include_str!("../tauri.conf.json"));
        let platforms = [
            (
                include_str!("../tauri.windows.conf.json"),
                &["decorations"][..],
            ),
            (
                include_str!("../tauri.macos.conf.json"),
                &[
                    "titleBarStyle",
                    "hiddenTitle",
                    "trafficLightPosition",
                    "acceptFirstMouse",
                ][..],
            ),
        ];
        for (source, frame_keys) in platforms {
            let mut window = main_window(source);
            for key in frame_keys {
                assert!(window.remove(*key).is_some(), "thiếu khoá khung {key}");
            }
            assert_eq!(window, base, "cửa sổ theo nền tảng lệch tauri.conf.json");
        }
    }

    #[test]
    fn windows_drops_native_frame_and_macos_overlays_traffic_lights() {
        let windows = main_window(include_str!("../tauri.windows.conf.json"));
        assert_eq!(windows["decorations"], Value::Bool(false));
        let macos = main_window(include_str!("../tauri.macos.conf.json"));
        assert_eq!(macos["titleBarStyle"], "Overlay");
        assert_eq!(macos["hiddenTitle"], Value::Bool(true));
        // Overlay: chuột ở dải đầu vào webview ⇒ phải nhận cú nhấn đầu thì mới kéo được cửa sổ đang ở sau.
        assert_eq!(macos["acceptFirstMouse"], Value::Bool(true));
        assert!(
            macos.get("decorations").is_none(),
            "đèn giao thông cần khung gốc"
        );
    }

    #[test]
    fn double_click_follows_macos_setting() {
        let cases = [
            (Some("Maximize"), None, DoubleClick::Zoom),
            (Some("Fill"), None, DoubleClick::Zoom),
            (Some(" Minimize "), Some("0"), DoubleClick::Minimize),
            (Some("None"), Some("1"), DoubleClick::Nothing),
            (None, Some("1"), DoubleClick::Minimize),
            (None, Some("0"), DoubleClick::Zoom),
            (None, None, DoubleClick::Zoom),
        ];
        for (action, miniaturize, expected) in cases {
            assert_eq!(
                double_click_action(action, miniaturize),
                expected,
                "{action:?} / {miniaturize:?}"
            );
        }
    }
}
