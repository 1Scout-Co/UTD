//! Cửa sổ chính đang active hay không, để thanh tiêu đề đậm/mờ đúng như khung gốc của Windows. Sự kiện focus
//! của Tauri trên Windows là focus của webview (GotFocus/LostFocus của WebView2), không phải trạng thái active
//! của cửa sổ: kích hoạt từ luồng khác (Alt+Tab, thanh tác vụ) thì focus có thể nằm lại ở cửa sổ chính chứ
//! không vào webview (không có sự kiện nào), và lúc app vừa mở sự kiện có thể đến trước khi giao diện kịp nghe.
//! Nên nghe thẳng WM_NCACTIVATE của cửa sổ chính — chính thông điệp khung gốc dùng để vẽ thanh tiêu đề đậm/mờ
//! (đo thật: kích hoạt lại từ luồng khác chỉ có WM_NCACTIVATE, không có WM_ACTIVATE) — báo qua `utd://active`.

use std::sync::OnceLock;

use tauri::{Emitter, WebviewWindow};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{
    DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, PostMessageW, RegisterWindowMessageW, WM_NCACTIVATE, WM_NCDESTROY,
};
use windows::core::w;

/// `true` khi cửa sổ chính được kích hoạt, `false` khi sang cửa sổ khác.
pub const ACTIVE_EVENT: &str = "utd://active";

/// Mã subclass riêng (Tauri dùng WM_USER + 1 cho vùng đổi cỡ).
const SUBCLASS_ID: usize = 0x5554_4401;

/// Cửa sổ chính có đang active không; lần đầu gọi thì bắt đầu nghe WM_NCACTIVATE. Chạy trên luồng giao diện
/// (lệnh đồng bộ của Tauri chạy ở đó): bắt đầu nghe rồi mới đọc ⇒ không lỡ lần đổi nào sau lời hỏi.
pub fn is_active(window: &WebviewWindow) -> tauri::Result<bool> {
    let hwnd = window.hwnd()?;
    watch(hwnd, window);
    Ok(unsafe { GetForegroundWindow() } == hwnd)
}

fn watch(hwnd: HWND, window: &WebviewWindow) {
    if unsafe { GetWindowSubclass(hwnd, Some(on_message), SUBCLASS_ID, None) }.as_bool() {
        return;
    }
    let data = Box::into_raw(Box::new(window.clone()));
    if !unsafe { SetWindowSubclass(hwnd, Some(on_message), SUBCLASS_ID, data as usize) }.as_bool() {
        // Không gắn được thì WM_NCDESTROY bên dưới không chạy: tự trả bộ nhớ.
        drop(unsafe { Box::from_raw(data) });
    }
}

/// Thông điệp riêng: báo trạng thái active sau WM_NCACTIVATE (0 nếu không đăng ký được).
fn report_message() -> u32 {
    static ID: OnceLock<u32> = OnceLock::new();
    *ID.get_or_init(|| unsafe { RegisterWindowMessageW(w!("UTD_WINDOW_ACTIVE")) })
}

unsafe extern "system" fn on_message(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    unsafe {
        let report = report_message();
        if msg == WM_NCACTIVATE && report != 0 {
            // Kích hoạt từ luồng khác gửi thông điệp này trong một lời gọi đồng bộ đầu vào: lúc đó WebView2 không
            // chạy được script, sự kiện báo ngay sẽ mất. Gửi lại cho chính mình, báo khi tới lượt (giữ thứ tự).
            let active = wparam.0 != 0;
            let _ = PostMessageW(Some(hwnd), report, WPARAM(active as usize), LPARAM(0));
        }
        if msg == report && report != 0 {
            // SAFETY: `data` là Box<WebviewWindow> do `watch` gắn, chỉ trả ở WM_NCDESTROY, cùng luồng.
            let window = &*(data as *const WebviewWindow);
            let _ = window.emit(ACTIVE_EVENT, wparam.0 != 0);
            return LRESULT(0);
        }
        if msg == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(on_message), SUBCLASS_ID);
            drop(Box::from_raw(data as *mut WebviewWindow));
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}
