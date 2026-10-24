//! Chuột của 3 nút cửa sổ tự vẽ trên Windows. Webview phủ kín cửa sổ nên Windows không biết nút nằm đâu:
//! một cửa sổ con trong suốt (cùng luồng giao diện) đặt đè đúng chỗ 3 nút, trả mã vùng như nút thật
//! (HTMINBUTTON · HTMAXBUTTON; Đóng là HTBORDER, xem `hit_code`). Nhờ vậy Windows 11 hiện Snap Layouts khi rê
//! lên nút Phóng to, và bấm vẫn được khi giao diện đang có lớp phủ (hộp thoại). Hình nút vẫn do webview vẽ: trạng thái
//! rê/nhấn gửi sang giao diện qua sự kiện `utd://caption`. Khi cửa sổ không phóng to, mấy hàng pixel trên
//! cùng là mép trên đổi cỡ (góc phải: góc chéo) như khung gốc, chuyển cho cửa sổ chính xử lý.

use std::cell::RefCell;

use serde::Serialize;
use tauri::{Emitter, WebviewWindow};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, FindWindowExW, GWL_STYLE, GWLP_HINSTANCE,
    GWLP_USERDATA, GetClientRect, GetParent, GetWindowLongPtrW, HTBORDER, HTMAXBUTTON, HTMINBUTTON,
    HTTOP, HTTOPRIGHT, HTTRANSPARENT, IDC_ARROW, IsZoomed, LoadCursorW, PostMessageW,
    RegisterClassExW, SC_CLOSE, SC_KEYMENU, SC_MAXIMIZE, SC_MINIMIZE, SC_RESTORE,
    SM_CXPADDEDBORDER, SM_CYSIZEFRAME, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, WINDOW_EX_STYLE, WM_CAPTURECHANGED, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP,
    WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_SYSCOMMAND, WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS,
    WS_THICKFRAME, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

use super::{CaptionButton, CaptionRect};

/// Trạng thái rê/nhấn gửi sang giao diện để tô nút.
pub const CAPTION_EVENT: &str = "utd://caption";

const CLASS: PCWSTR = w!("UTD_CAPTION_BUTTONS");
/// Cửa sổ con vùng kéo-đổi-cỡ mép trên của Tauri: nút nằm dưới nó để mép trên vẫn kéo đổi cỡ được.
const TAURI_RESIZE_BORDERS: PCWSTR = w!("TAURI_DRAG_RESIZE_BORDERS");

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptionState {
    hover: Option<CaptionButton>,
    pressed: Option<CaptionButton>,
}

struct Overlay {
    window: WebviewWindow,
    /// Ô từng nút, toạ độ trong cửa sổ con.
    buttons: Vec<(CaptionButton, RECT)>,
    state: CaptionState,
    tracking: bool,
}

impl Overlay {
    fn button_at(&self, point: POINT) -> Option<CaptionButton> {
        self.buttons
            .iter()
            .find(|(_, rect)| {
                point.x >= rect.left
                    && point.x < rect.right
                    && point.y >= rect.top
                    && point.y < rect.bottom
            })
            .map(|(button, _)| *button)
    }

    /// Đổi trạng thái rê/nhấn; khác trước thì báo giao diện.
    fn set(&mut self, hover: Option<CaptionButton>, pressed: Option<CaptionButton>) {
        let state = CaptionState { hover, pressed };
        if self.state != state {
            self.state = state;
            let _ = self.window.emit(CAPTION_EVENT, self.state.clone());
        }
    }
}

/// Đặt (hoặc tạo) cửa sổ con đè đúng chỗ các nút. Chạy trên luồng giao diện: cửa sổ con phải cùng
/// luồng với cửa sổ chính thì mới nhận chuột thay webview.
pub fn place(window: WebviewWindow, buttons: Vec<CaptionRect>) -> tauri::Result<()> {
    let parent = window.hwnd()?.0 as isize;
    let target = window.clone();
    window.run_on_main_thread(move || {
        if let Err(error) = apply(HWND(parent as _), target, &buttons) {
            log::warn!("Caption buttons: {error}");
        }
    })
}

fn apply(
    parent: HWND,
    window: WebviewWindow,
    buttons: &[CaptionRect],
) -> windows::core::Result<()> {
    let overlay = find_or_create(parent, window)?;
    let Some(bounds) = union(buttons) else {
        unsafe {
            let _ = ShowWindow(overlay, SW_HIDE);
        }
        return Ok(());
    };
    if let Some(data) = overlay_data(overlay) {
        let mut data = data.borrow_mut();
        data.buttons = buttons
            .iter()
            .map(|item| {
                let left = item.x - bounds.left;
                let top = item.y - bounds.top;
                let rect = RECT {
                    left,
                    top,
                    right: left + item.width,
                    bottom: top + item.height,
                };
                (item.button, rect)
            })
            .collect();
    }
    // Nằm ngay dưới vùng đổi cỡ mép trên của Tauri (nếu có), trên webview.
    let above = unsafe { FindWindowExW(Some(parent), None, TAURI_RESIZE_BORDERS, PCWSTR::null()) }
        .ok()
        .unwrap_or_default();
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;
    unsafe {
        SetWindowPos(
            overlay,
            Some(above),
            bounds.left,
            bounds.top,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
    }
}

fn union(buttons: &[CaptionRect]) -> Option<RECT> {
    let first = buttons.first()?;
    let mut rect = RECT {
        left: first.x,
        top: first.y,
        right: first.x + first.width,
        bottom: first.y + first.height,
    };
    for item in &buttons[1..] {
        rect.left = rect.left.min(item.x);
        rect.top = rect.top.min(item.y);
        rect.right = rect.right.max(item.x + item.width);
        rect.bottom = rect.bottom.max(item.y + item.height);
    }
    (rect.right > rect.left && rect.bottom > rect.top).then_some(rect)
}

fn find_or_create(parent: HWND, window: WebviewWindow) -> windows::core::Result<HWND> {
    if let Ok(existing) = unsafe { FindWindowExW(Some(parent), None, CLASS, PCWSTR::null()) } {
        return Ok(existing);
    }
    let instance = unsafe { GetWindowLongPtrW(parent, GWLP_HINSTANCE) };
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(overlay_proc),
        hInstance: windows::Win32::Foundation::HINSTANCE(instance as _),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        lpszClassName: CLASS,
        ..Default::default()
    };
    // Đăng ký lần hai (lớp đã có) trả 0: bỏ qua, CreateWindowExW vẫn dùng được lớp cũ.
    unsafe { RegisterClassExW(&class) };
    let data = Box::new(RefCell::new(Overlay {
        window,
        buttons: Vec::new(),
        state: CaptionState::default(),
        tracking: false,
    }));
    let raw = Box::into_raw(data);
    let created = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS,
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            Some(parent),
            None,
            Some(class.hInstance),
            Some(raw as _),
        )
    };
    if created.is_err() {
        // Không tạo được cửa sổ thì WM_NCDESTROY không chạy: tự trả bộ nhớ.
        drop(unsafe { Box::from_raw(raw) });
    }
    created
}

fn overlay_data<'a>(hwnd: HWND) -> Option<&'a RefCell<Overlay>> {
    let raw = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const RefCell<Overlay>;
    // SAFETY: con trỏ do `find_or_create` đặt (Box), chỉ trả lại ở WM_NCDESTROY, cùng một luồng.
    unsafe { raw.as_ref() }
}

unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        if msg == WM_NCDESTROY {
            let raw = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut RefCell<Overlay>;
            if !raw.is_null() {
                drop(Box::from_raw(raw));
            }
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let handled = overlay_data(hwnd).and_then(|data| handle(hwnd, data, msg, wparam, lparam));
        handled.unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam))
    }
}

/// Xử lý chuột; `None` = để Windows làm mặc định. Không giữ `borrow` khi gọi API có thể gửi lại
/// thông điệp vào chính cửa sổ này (SetCapture/ReleaseCapture gửi WM_CAPTURECHANGED ngay).
fn handle(
    hwnd: HWND,
    data: &RefCell<Overlay>,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    match msg {
        WM_NCHITTEST => {
            let screen = lparam_point(lparam);
            let code = match resize_edge(hwnd, screen) {
                Some(edge) => edge as i32,
                None => data
                    .try_borrow()
                    .ok()?
                    .button_at(to_client(hwnd, screen))
                    .map_or(HTTRANSPARENT, hit_code),
            };
            Some(LRESULT(code as isize))
        }
        WM_NCMOUSEMOVE => {
            let button = from_hit_code(wparam.0);
            hover(hwnd, data, button);
            match button {
                // Để Windows xử lý tiếp: Snap Layouts của Windows 11 dựa vào rê chuột trên HTMAXBUTTON.
                Some(CaptionButton::Maximize) | None => None,
                // Thu nhỏ/Đóng: không cần xử lý mặc định nào (chú thích do app vẽ).
                Some(_) => Some(LRESULT(0)),
            }
        }
        WM_NCMOUSELEAVE => {
            let mut overlay = data.try_borrow_mut().ok()?;
            overlay.tracking = false;
            let pressed = overlay.state.pressed;
            overlay.set(pressed, pressed);
            Some(LRESULT(0))
        }
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => press(hwnd, data, msg, wparam, lparam),
        // Không để Windows tự xử lý nhả chuột trên nút giả (nó sẽ gửi lệnh hệ thống cho cửa sổ con).
        WM_NCLBUTTONUP => Some(LRESULT(0)),
        WM_MOUSEMOVE => {
            let mut overlay = data.try_borrow_mut().ok()?;
            let pressed = overlay.state.pressed?;
            let inside = overlay.button_at(lparam_point(lparam)) == Some(pressed);
            overlay.set(inside.then_some(pressed), Some(pressed));
            Some(LRESULT(0))
        }
        WM_LBUTTONUP => {
            release(data, lparam_point(lparam));
            Some(LRESULT(0))
        }
        WM_CAPTURECHANGED => {
            let mut overlay = data.try_borrow_mut().ok()?;
            let hover = overlay.state.hover;
            overlay.set(hover, None);
            Some(LRESULT(0))
        }
        _ => None,
    }
}

/// Nhấn chuột: trên nút thì bắt chuột tới lúc nhả (`release`); trên mép trên thì để cửa sổ chính làm như
/// khung gốc (nhấn: kéo đổi cỡ; nhấp đúp: kéo dài hết chiều cao màn hình).
fn press(
    hwnd: HWND,
    data: &RefCell<Overlay>,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    if is_edge_code(wparam.0) {
        let parent = unsafe { GetParent(hwnd) }.ok()?;
        unsafe {
            let _ = PostMessageW(Some(parent), msg, wparam, lparam);
        }
        return Some(LRESULT(0));
    }
    let button = from_hit_code(wparam.0)?;
    data.try_borrow_mut().ok()?.set(Some(button), Some(button));
    unsafe { SetCapture(hwnd) };
    Some(LRESULT(0))
}

/// Mép trên đổi cỡ như khung gốc. Dải `TAURI_DRAG_RESIZE_BORDERS` phía trên chỉ cao SM_CYFRAME và luôn trả
/// HTTOP, nên trên vùng nút cửa sổ con tự trả HTTOP cho cả độ dày mép gốc, HTTOPRIGHT ở góc phải. Không làm
/// khi phóng to (hàng trên cùng màn hình phải trúng nút) hoặc khi cửa sổ không đổi cỡ được (cả toàn màn hình).
fn resize_edge(hwnd: HWND, screen: POINT) -> Option<u32> {
    let parent = unsafe { GetParent(hwnd) }.ok()?;
    let style = unsafe { GetWindowLongPtrW(parent, GWL_STYLE) } as u32;
    if style & WS_THICKFRAME.0 == 0 || unsafe { IsZoomed(parent) }.as_bool() {
        return None;
    }
    let mut client = RECT::default();
    unsafe { GetClientRect(parent, &mut client) }.ok()?;
    let thickness = unsafe {
        let dpi = GetDpiForWindow(parent);
        GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
    };
    edge_code(to_client(parent, screen), client.right, thickness)
}

/// Mã vùng mép trên tại `point` (toạ độ trong cửa sổ chính rộng `width`), mép dày `thickness`.
fn edge_code(point: POINT, width: i32, thickness: i32) -> Option<u32> {
    if point.y >= thickness {
        return None;
    }
    Some(if point.x >= width - thickness {
        HTTOPRIGHT
    } else {
        HTTOP
    })
}

fn is_edge_code(code: usize) -> bool {
    matches!(code as u32, HTTOP | HTTOPRIGHT)
}

fn hover(hwnd: HWND, data: &RefCell<Overlay>, button: Option<CaptionButton>) {
    let Ok(mut overlay) = data.try_borrow_mut() else {
        return;
    };
    if !overlay.tracking {
        let mut track = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE | TME_NONCLIENT,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        };
        overlay.tracking = unsafe { TrackMouseEvent(&mut track) }.is_ok();
    }
    let pressed = overlay.state.pressed;
    overlay.set(button, pressed);
}

/// Nhả chuột: còn nằm trên đúng nút đã nhấn thì làm việc của nút (như nút thật: kéo ra ngoài rồi nhả là huỷ).
fn release(data: &RefCell<Overlay>, point: POINT) {
    let (pressed, under, parent) = {
        let Ok(overlay) = data.try_borrow() else {
            return;
        };
        (
            overlay.state.pressed,
            overlay.button_at(point),
            overlay.window.hwnd().ok(),
        )
    };
    unsafe {
        let _ = ReleaseCapture();
    }
    let action = pressed.filter(|button| under == Some(*button));
    if let Ok(mut overlay) = data.try_borrow_mut() {
        // Sau khi bấm (thu nhỏ, phóng to) chuột không còn trên nút: bỏ tô ngay.
        let hover = if action.is_some() { None } else { under };
        overlay.set(hover, None);
    }
    if let (Some(button), Some(parent)) = (action, parent) {
        run(parent, button);
    }
}

/// Gửi lệnh hệ thống như nút thật của Windows (thu nhỏ có hiệu ứng, Đóng đi qua sự kiện đóng của app).
fn run(parent: HWND, button: CaptionButton) {
    let command = match button {
        CaptionButton::Minimize => SC_MINIMIZE,
        CaptionButton::Maximize if unsafe { IsZoomed(parent) }.as_bool() => SC_RESTORE,
        CaptionButton::Maximize => SC_MAXIMIZE,
        CaptionButton::Close => SC_CLOSE,
    };
    unsafe {
        let _ = PostMessageW(
            Some(parent),
            WM_SYSCOMMAND,
            WPARAM(command as usize),
            LPARAM(0),
        );
    }
}

/// Menu hệ thống như Alt+Space trên cửa sổ có khung gốc: Windows tự đặt chỗ và bật/tắt từng mục.
pub fn show_window_menu(window: &WebviewWindow) -> tauri::Result<()> {
    let parent = window.hwnd()?;
    unsafe {
        let space = LPARAM(b' ' as isize);
        let _ = PostMessageW(
            Some(parent),
            WM_SYSCOMMAND,
            WPARAM(SC_KEYMENU as usize),
            space,
        );
    }
    Ok(())
}

/// Mã vùng của từng nút. Đóng không trả HTCLOSE: với mã đó Windows tự hiện chú thích "Close" kiểu cũ (tiếng Anh,
/// còn kẹt lại sau khi rời nút) chồng lên chú thích của app, dù không chuyển chuột cho xử lý mặc định. HTBORDER
/// không kéo theo hành vi nào (không đổi cỡ, không menu); bấm/nhả vẫn do cửa sổ con này lo như nút thật.
fn hit_code(button: CaptionButton) -> i32 {
    (match button {
        CaptionButton::Minimize => HTMINBUTTON,
        CaptionButton::Maximize => HTMAXBUTTON,
        CaptionButton::Close => HTBORDER,
    }) as i32
}

fn from_hit_code(code: usize) -> Option<CaptionButton> {
    match code as u32 {
        HTMINBUTTON => Some(CaptionButton::Minimize),
        HTMAXBUTTON => Some(CaptionButton::Maximize),
        HTBORDER => Some(CaptionButton::Close),
        _ => None,
    }
}

/// Toạ độ chuột trong `lparam` (màn hình với thông điệp NC*, trong cửa sổ con với WM_MOUSEMOVE/WM_LBUTTONUP).
fn lparam_point(lparam: LPARAM) -> POINT {
    POINT {
        x: (lparam.0 & 0xffff) as i16 as i32,
        y: ((lparam.0 >> 16) & 0xffff) as i16 as i32,
    }
}

fn to_client(hwnd: HWND, mut point: POINT) -> POINT {
    unsafe {
        let _ = ScreenToClient(hwnd, &mut point);
    }
    point
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{HTTOP, HTTOPRIGHT};

    use super::{edge_code, is_edge_code};

    #[test]
    fn top_rows_over_buttons_resize_like_native_frame() {
        let width = 1650;
        let at = |x, y| edge_code(POINT { x, y }, width, 10);
        assert_eq!(at(1500, 0), Some(HTTOP));
        assert_eq!(at(1500, 9), Some(HTTOP));
        assert_eq!(at(1500, 10), None, "dưới mép là nút");
        assert_eq!(at(1640, 0), Some(HTTOPRIGHT));
        assert_eq!(at(1649, 9), Some(HTTOPRIGHT));
        assert_eq!(at(1639, 5), Some(HTTOP));
        assert!(is_edge_code(HTTOP as usize) && is_edge_code(HTTOPRIGHT as usize));
    }
}
