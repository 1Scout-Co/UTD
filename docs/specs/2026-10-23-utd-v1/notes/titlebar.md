# Thanh tiêu đề riêng (Windows + macOS)

Ngày: 2026-10-24. Yêu cầu: thanh tiêu đề gốc (nền trắng, chữ "UTD" nhỏ, nút hệ thống) lệch style app ⇒ làm thanh
riêng hoà vào nền gradient của Soniox, đúng kiểu từng hệ điều hành.

## Kết quả

- Thanh cao 40px, trong suốt trên nền gradient; trái là logo "utd desktop" (chuyển từ đầu thanh bên lên, không lặp),
  thẻ nội dung bắt đầu ngay dưới thanh. Bản xem trước trên trình duyệt vẫn có thanh (logo), không có nút cửa sổ.
- **Windows**: bỏ khung gốc; 3 nút Thu nhỏ · Phóng to/Khôi phục · Đóng vẽ bằng lucide theo style app: ô 46×40 sát
  mép phải, nền rê là ô bo góc bên trong (xám nhạt), Đóng rê thành đỏ chữ trắng, icon đổi sang "Khôi phục" khi phóng to,
  rê yên 0,5 s thì hiện chú thích dưới nút (Thu nhỏ · Phóng to/Khôi phục · Đóng), logo + icon mờ đi khi cửa sổ không
  active (đúng lúc khung gốc mờ).
- **macOS**: giữ 3 đèn giao thông gốc phủ lên nền app (`titleBarStyle: Overlay`, `hiddenTitle`), đèn đặt thẳng giữa
  thanh 40px (`trafficLightPosition {x:16, y:18}`), logo lùi sang phải 84px để không đè đèn; toàn màn hình (đèn ẩn)
  thì logo về lề 22px như Windows. Không vẽ nút giả. Nhấp đúp thanh làm theo cài đặt hệ thống (phóng to / thu nhỏ /
  không làm gì), kéo được cả cửa sổ đang ở sau.
- Menu chuột phải, menu ⋯, tooltip không bao giờ lấn vào dải 40px của thanh (mép trên chừa 44px).

## Quyết định

| Việc | Chọn | Lý do |
|---|---|---|
| Cấu hình theo nền tảng | `src-tauri/tauri.windows.conf.json` (`decorations: false`), `src-tauri/tauri.macos.conf.json` (Overlay) | Tauri tự gộp theo nền tảng lúc build; Linux giữ khung gốc. Gộp kiểu JSON Merge Patch **thay nguyên mảng** `app.windows` nên mỗi file chép đủ cửa sổ chính; test `titlebar::tests` bắt lệch so với `tauri.conf.json`. |
| Khung Windows | `decorations: false` + `shadow` mặc định | tao giữ `WS_CAPTION | WS_THICKFRAME`, chỉ bỏ khung bằng `WM_NCCALCSIZE` ⇒ DWM vẫn bo góc + bóng Windows 11, Aero Snap, menu hệ thống; Tauri tự lo kéo mép đổi cỡ (mép trên bằng cửa sổ con `TAURI_DRAG_RESIZE_BORDERS`). |
| Vùng kéo Windows | CSS `app-region: drag` (utility `drag-region` trong `src/index.css`) | wry bật `IsNonClientRegionSupportEnabled`: WebView2 tạo cửa sổ con trả `HTCAPTION` ⇒ kéo, nhấp đúp, Aero Snap, chuột phải ra menu hệ thống đều do Windows làm, không qua IPC. Vùng kéo không tính thứ tự lớp ⇒ đang mở hộp thoại vẫn kéo được. `data-tauri-drag-region="deep"` để làm đường lui (WebView2 cũ). |
| Nút cửa sổ Windows | DOM vẽ + cửa sổ con Rust `UTD_CAPTION_BUTTONS` (`src-tauri/src/titlebar/caption.rs`) | Webview phủ kín cửa sổ nên Windows không biết nút ở đâu. Giao diện báo ô từng nút (pixel thật = CSS × devicePixelRatio, báo lại khi đổi cỡ/phóng to/đổi DPI); Rust đặt một cửa sổ con trong suốt cùng luồng đè đúng chỗ, trả `HTMINBUTTON`/`HTMAXBUTTON` ⇒ **Windows 11 hiện Snap Layouts khi rê lên Phóng to**, góc trên phải màn hình khi phóng to trúng Đóng. Đóng trả `HTBORDER` chứ không `HTCLOSE`: với `HTCLOSE` Windows tự hiện chú thích "Close" kiểu cũ (tiếng Anh, nền vàng, còn kẹt lại sau khi rời nút) chồng lên chú thích của app, kể cả khi không chuyển chuột cho xử lý mặc định. Bấm/nhả như nút thật (kéo ra ngoài rồi nhả là huỷ) rồi gửi `WM_SYSCOMMAND` (SC_MINIMIZE/MAXIMIZE/RESTORE/CLOSE) — Đóng đi qua sự kiện đóng của app. Trạng thái rê/nhấn gửi về giao diện qua `utd://caption` để tô. Mép trên: dải `TAURI_DRAG_RESIZE_BORDERS` của Tauri chỉ cao SM_CYFRAME (4px ở 125%), luôn trả `HTTOP` và nhấp đúp không làm gì ⇒ khi không phóng to, cửa sổ con tự trả `HTTOP` cho cả độ dày mép gốc (`SM_CYSIZEFRAME + SM_CXPADDEDBORDER` theo DPI, 9px ở 125%), `HTTOPRIGHT` ở góc phải, và chuyển `WM_NCLBUTTONDOWN`/`WM_NCLBUTTONDBLCLK` cho cửa sổ chính ⇒ kéo đổi cỡ, nhấp đúp kéo dài hết chiều cao như khung gốc. Chuột không vào webview ⇒ bấm nút không đóng hộp thoại đang mở. |
| Chú thích nút cửa sổ | App tự vẽ dưới nút (`CaptionControl`), hiện sau 0,5 s rê yên theo `utd://caption` (đường lui: chuột của webview), tắt ngay khi nhấn/rời | `title` không bao giờ hiện: chuột trên nút đi vào cửa sổ con chứ không vào webview. Chú thích của Windows thì Thu nhỏ không có, Đóng là "Close" kiểu cũ kẹt lại (đã bỏ, xem trên). Phóng to: bảng Snap Layouts của Windows 11 che chú thích, như nút gốc. |
| Không dùng `tauri-plugin-decorum` | — | Plugin giả Snap Layouts bằng cách bấm hộ Win+Z khi rê chuột và tự chèn nút; cách hit-test thật ở trên cho đúng hành vi gốc, không thêm phụ thuộc. Không thêm thư viện nào (chỉ bật `Win32_UI_HiDpi`, `Win32_UI_Input_KeyboardAndMouse` của `windows`, tao đã bật sẵn cả hai). |
| Alt+Space | Giao diện bắt phím ⇒ lệnh `show_window_menu` ⇒ `WM_SYSCOMMAND SC_KEYMENU` | WebView2 giữ Alt+Space cho trang, không chuyển lên cửa sổ (đã thử: trang nhận phím, không có menu). Alt+F4 thì WebView2 tự chuyển. |
| Trạng thái khung | `onResized` ⇒ `isMaximized`/`isFullscreen` (hỏi lại khi nghe xong) | Đọc của Tauri, quyền đọc đã có trong `core:default`. |
| Active / mờ | Windows: `WM_NCACTIVATE` của cửa sổ chính (subclass trong `src-tauri/src/titlebar/activation.rs`) ⇒ sự kiện `utd://active`; macOS/Linux: `onFocusChanged`. Mọi nền tảng: nghe xong thì hỏi lệnh `is_window_active` (sự kiện đến sau lời hỏi thì thắng) | Đo thật trên Windows: sự kiện focus của Tauri là focus của **webview** (GotFocus/LostFocus của WebView2) — kích hoạt từ luồng khác (Alt+Tab, thanh tác vụ, `SetForegroundWindow`) để focus nằm lại ở cửa sổ chính, không có sự kiện nào; `isFocused()` của Tauri trả false cả khi cửa sổ đang active (focus trong webview); `document.hasFocus()` trả true cả khi cửa sổ không active. Kích hoạt từ luồng khác chỉ gửi `WM_NCACTIVATE`, không có `WM_ACTIVATE`, và gửi trong lời gọi đồng bộ đầu vào (WebView2 không chạy được script lúc đó) ⇒ subclass gửi lại cho chính nó (`PostMessage`) rồi mới báo, giữ thứ tự. `is_window_active` (Windows: `GetForegroundWindow() == hwnd`) gắn subclass ở lần gọi đầu, gắn xong mới đọc ⇒ không lỡ lần đổi nào; giao diện gọi khi đã nghe xong ⇒ sửa ca cửa sổ mất focus trong lúc app đang mở. |
| Menu, tooltip | `collisionPadding` mép trên 44px cho positioner của `ContextMenu`, `DropdownMenu` (cả menu con), `Tooltip` — hằng `POPUP_COLLISION_PADDING` (`src/shared/window/metrics.ts`) | Dải 40px trên cùng do chính Windows nhận chuột (vùng `app-region`, cửa sổ con của 3 nút) bất kể lớp DOM: menu lật lên ở cửa sổ thấp (Base UI mặc định chừa 5px) từng có mục "Mở file" nằm dưới nút Thu nhỏ/Phóng to/Đóng — bấm là kéo cửa sổ/thu nhỏ/đóng app. `z-index` không chặn được cửa sổ con native nên phải giữ menu ra khỏi dải. Không đủ chỗ thì menu lật hoặc thu cao lại, cuộn bên trong. |
| Nhận nền tảng | User agent (`src/shared/window/platform.ts`) | Trong Tauri mỗi hệ điều hành chỉ có một engine (WebView2 ↔ Windows, WKWebView ↔ macOS, WebKitGTK ↔ Linux) và app không đổi UA ⇒ luôn đúng; đọc đồng bộ ngay lần vẽ đầu (không nhảy bố cục); không cần plugin-os (thêm crate + gói npm + quyền chỉ để biết tên OS). `navigator.userAgentData` WKWebView không có. Không có Tauri (`isTauri()` sai) ⇒ `null` ⇒ không nút, không vùng kéo. |
| Hộp thoại | Thanh `z-60` trên lớp phủ (`z-50`); `DialogContent` (`src/components/ui/dialog.tsx`) có class `max-h-[calc(100svh-5rem)] overflow-y-auto` (40px trên + 40px dưới), dài hơn thì cuộn | Như thanh gốc: mở hộp thoại vẫn kéo/thu nhỏ/phóng to/đóng được và hộp thoại vẫn mở; hộp không bao giờ lấn lên thanh (nút cửa sổ không đè nội dung). Là class Tailwind (không CSS tự viết ngoài `@layer`) nên hộp nào cần thì `className` vẫn đè được. |
| macOS: kéo, nhấp đúp | `useTitleBarMouse` (`src/shared/window/use-window-frame.ts`): nhấn ⇒ `startDragging`; nhấp đúp (lúc nhả, chuột chưa dời) ⇒ lệnh `title_bar_double_click` đọc `defaults read -g AppleActionOnDoubleClick` (Maximize/Fill ⇒ phóng to, Minimize ⇒ thu nhỏ, None ⇒ không làm gì; chưa đặt thì xem `AppleMiniaturizeOnDoubleClick`, mặc định phóng to). Toàn màn hình: không kéo, nhấp đúp không làm gì (Rust cũng chặn). Mọi nền tảng: nhấn lên thanh `preventDefault` ⇒ không lấy focus | Overlay để chuột ở dải đầu đi vào WKWebView nên AppKit không tự kéo/nhấp đúp. `drag.js` của Tauri (`data-tauri-drag-region`) luôn phóng to khi nhấp đúp (bỏ qua cài đặt hệ thống), vẫn chạy khi toàn màn hình (tao lật âm thầm trạng thái phóng to sẽ áp khi thoát toàn màn hình) và không `preventDefault` ở cú nhấn thứ hai ⇒ focus rơi về `body`, ra khỏi hộp thoại đang mở. Đọc cài đặt mỗi lần (đổi lúc nào cũng đúng), chạy ngoài luồng giao diện. "Fill" (macOS 15) Tauri không có, dùng phóng to. |
| macOS: cửa sổ đang ở sau | `acceptFirstMouse: true` (`tauri.macos.conf.json`) | wry mặc định không nhận cú nhấn đầu vào cửa sổ không active ⇒ với Overlay phải nhấn hai lần mới kéo được (tài liệu `TitleBarStyle::Overlay`, tauri#4316). Đổi lại: cú nhấn đầu vào cửa sổ đang ở sau cũng tới nội dung — giống Windows. |
| Quyền | `capabilities/titlebar-windows.json` (start-dragging, minimize, toggle-maximize, close — chỉ Windows), `capabilities/titlebar-macos.json` (start-dragging — chỉ macOS) | Đọc trạng thái (`is-maximized`, `is-fullscreen`, `is-focused`) và `internal-toggle-maximize` (nhấp đúp ở đường lui `drag.js`) đã có trong `core:window:default`. `set_caption_buttons`, `show_window_menu`, `is_window_active`, `title_bar_double_click` là lệnh của app. |
| Vị trí code | `src/shared/window/` (`TitleBar`, hook, api) | Khung cửa sổ dùng chung cho mọi màn, không thuộc tính năng nào; `features/home` chỉ đặt `<TitleBar>` vào bố cục Soniox. |

## Cách kiểm trên Windows thật

Build riêng không đụng bản đang chạy: `$env:CARGO_TARGET_DIR="…\target-titlebar"; pnpm exec tauri build --no-bundle`,
chạy `target-titlebar\release\UTD.exe` với `UTD_DATA_DIR=<thư mục tạm>` và
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9420` (đọc DOM qua Playwright `connectOverCDP`).
Chuột/phím thật bằng `SetCursorPos` + `mouse_event`/`keybd_event` (luôn kiểm cửa sổ thử đang ở trước rồi mới gửi),
mã vùng bằng `WindowFromPoint` + `WM_NCHITTEST`, ảnh cả cửa sổ (bóng, góc bo) bằng `Graphics.CopyFromScreen` theo
`DWMWA_EXTENDED_FRAME_BOUNDS`. Máy kiểm: Windows 11 Pro 26200, 2 màn hình 125%, cỡ chữ hệ thống 110% ⇒
devicePixelRatio 1,375; WebView2 155.

Mọi dòng dưới đây làm bằng chuột/phím thật trên bản build `target-titlebar` (trừ chỗ ghi CDP). Bản build cuối (sau
vài sửa nhỏ: hộp thoại `− 5rem`, `allow(dead_code)` ngoài Windows, gọn hàm `handle`) kiểm lại không dùng chuột/phím
thật (người dùng đang dùng máy): mã vùng thanh = 2, 3 nút = 8/9/20, mép trên ngay trên Đóng = 12 (đo ở bản trước
lần gọn hàm cuối, phần hit-test không đổi); gửi thẳng
`WM_NCMOUSEMOVE`/`WM_NCLBUTTONDOWN`/`WM_LBUTTONUP` vào cửa sổ con ⇒ chuỗi sự kiện `utd://caption` đúng
(rê → rời; nhấn → nhả trên Phóng to ⇒ phóng to, lần hai ⇒ khôi phục; nhấn Đóng nhả ngoài nút ⇒ không đóng);
`WM_CLOSE` ⇒ thoát. Thư mục `target-titlebar` đã xoá sau khi kiểm
(1,7 GB; để lại thì Tailwind quét cả thư mục này, build chậm vài phút và CSS dính class rác).

| Hành vi | Kết quả đo |
|---|---|
| Kéo thanh tiêu đề | kéo (−150, +100) ⇒ khung dời đúng (−150, +100); `WM_NCHITTEST` ở thanh = 2 (HTCAPTION, cửa sổ con của WebView2) |
| Nhấp đúp thanh | phóng to = đúng vùng làm việc 3440×1380 (không đè thanh tác vụ); nhấp đúp lần nữa về cỡ cũ |
| Aero Snap | kéo lên mép trên ⇒ phóng to; kéo xuống từ lúc phóng to ⇒ về cỡ cũ theo chuột; kéo sang mép phải ⇒ nửa phải 1720×1380, Snap Assist hiện; kéo ra ⇒ về cỡ cũ |
| Đổi cỡ | mép phải +100, mép dưới +50, mép trái, góc dưới phải, mép trên −50 (vùng `TAURI_DRAG_RESIZE_BORDERS`, kể cả ngay trên nút Đóng) đều đúng; mã vùng 11/15/10/12 |
| 3 nút | mã vùng 8/9/20 đúng tâm từng nút; Thu nhỏ ⇒ thu xuống thanh tác vụ; Phóng to ⇒ phóng to + icon "Khôi phục"; Khôi phục ⇒ về cỡ cũ; Đóng ⇒ app thoát; nhấn Đóng rồi kéo ra ngoài mới nhả ⇒ không đóng. Khi phóng to, điểm góc trên phải màn hình (3439, 0) = HTCLOSE |
| Snap Layouts | rê lên Phóng to ~1 s ⇒ bảng Snap Layouts của Windows 11 hiện ngay dưới nút (`ui-demos/titlebar-snap-layouts.png`) |
| Rê chuột | Đóng đỏ chữ trắng, Thu nhỏ/Phóng to nền xám bo góc (`titlebar-hover-close.png`) |
| Mất focus | focus sang cửa sổ khác ⇒ logo + icon nút mờ (`titlebar-unfocused.png`) |
| Menu hệ thống | chuột phải thanh tiêu đề ⇒ menu gốc (Restore · Move · Size · Minimize · Maximize · Close Alt+F4), Restore xám đúng lúc |
| Alt+Space | menu hệ thống mở ở góc trên trái (gửi phím qua CDP `Input.dispatchKeyEvent`). Phím thật trên máy kiểm bị một launcher chiếm Alt+Space toàn cục trước khi tới bất kỳ app nào, nên không kiểm được bằng phím thật |
| Alt+F4 | phím thật, focus đang trong trang ⇒ app đóng |
| Hộp thoại đang mở | ("Thêm download") kéo thanh ⇒ cửa sổ dời; bấm Phóng to rồi Khôi phục ⇒ được; hộp thoại vẫn mở sau cả ba. Hộp Cài đặt (cao nhất) khi cửa sổ thấp nhất (bản xem trước 1080×618): hộp cao tối đa màn − 80px nên đỉnh không lên quá 40px, phần thừa cuộn trong hộp |
| DPI | máy kiểm 125% × cỡ chữ 110% ⇒ devicePixelRatio 1,375 (tỉ lệ lẻ): ô cửa sổ con khớp nút đến từng pixel (190×55 px = 3×46×40 CSS) |

### Sửa sau rà soát (cùng ngày, bản build cuối)

Cũng build riêng vào `target-titlebar`, dữ liệu tạm, CDP cổng 9481, 10 download thật từ máy chủ file cục bộ. Người
dùng đang dùng máy nên: mã vùng đo bằng cách đi cây cửa sổ của chính app (thứ tự lớp, vùng cửa sổ, `HTTRANSPARENT`)
rồi gửi `WM_NCHITTEST` — không phụ thuộc cửa sổ app khác đè lên; chuột thật chỉ gửi khi máy rảnh ≥ 20 s, dừng ngay
nếu thấy input lạ, chỉ bấm khi điểm bấm thuộc app thử, luôn nhả chuột, trả con trỏ và cửa sổ đang ở trước như cũ.
Ảnh menu chụp bằng `PrintWindow` (cửa sổ thử bị cửa sổ khác che), góc bo cắt lại theo khung.

| Hành vi | Kết quả đo |
|---|---|
| Menu ở cửa sổ thấp nhất | cửa sổ 1350×850 px (982×620 CSS): menu ⋯ và menu chuột phải của cả 10 hàng có đỉnh ≥ 44 CSS; hàng có nút ở y 341/378 (trước đây lật lên tới y 5) giờ đỉnh đúng 44, menu thu còn 293/330 px, cuộn trong. Mã vùng tại mục "Mở file" (y 50, 3 điểm ngang) = 1 (HTCLIENT của webview); cùng cột ở y 30 = 2 / 9 (thanh, Phóng to) (`titlebar-menu-min-height.png`) |
| Mép trên trên vùng nút | dy 0–3 = 12 (dải Tauri), dy 4–8 = 12 (cửa sổ con), từ dy 9 = nút 8/9/18; góc phải dy 4–8 = 14; phóng to ⇒ cả hàng 0 = nút. Con trỏ thật: ↕ trên Đóng, ⤢ ở góc, mũi tên trên nút. Nhấp đúp thật ở dy 5 trên Đóng ⇒ cửa sổ kéo dài 0–1380 (hết vùng làm việc, không phóng to); kéo thật ở dy 5 trên Thu nhỏ lên 40px ⇒ mép trên −40, mép dưới giữ nguyên |
| Active / mờ | mở app rồi chuyển ngay sang cửa sổ khác ⇒ thanh mờ (`is_window_active` false; lúc đó `document.hasFocus()` vẫn true); kích hoạt từ luồng khác ⇒ đậm (nhật ký: chỉ có `utd://active` true, Tauri không báo focus); chuyển đi ⇒ mờ; bấm thật lên thanh của cửa sổ đang ở sau ⇒ đậm |
| Chú thích nút | rê thật 1,5 s: Thu nhỏ ⇒ "Thu nhỏ" dưới nút (`titlebar-tooltip.png`), Đóng ⇒ "Đóng", không còn chú thích "Close" của Windows (trước khi đổi mã Đóng: còn, và kẹt lại khi rê sang Thu nhỏ); Phóng to ⇒ bảng Snap Layouts che chú thích |
| 3 nút (bấm thật) | Phóng to ⇒ phóng to; Thu nhỏ ⇒ thu xuống; Đóng nhấn rồi kéo ra ngoài nhả ⇒ không đóng; Đóng ⇒ app thoát |

Kiểm tự động (bản cuối): `cargo test -p app --lib` 125 qua (4 bài `titlebar`: cấu hình theo nền tảng, khoá khung
gồm `acceptFirstMouse`, cài đặt nhấp đúp macOS, mép trên đổi cỡ), `cargo clippy -p app --all-targets -- -D warnings`
sạch, `pnpm lint` sạch, `pnpm build` + size-limit (JS 175,9/200 kB, CSS 15,5/20 kB), knip/depcruise/jscpd không có
lỗi mới, e2e 40/40 — `e2e/titlebar.spec.ts` 11 bài: bản xem trước, hộp thoại trên lớp phủ, menu ⋯/chuột phải của
mọi hàng không vào dải thanh ở 1080×680, Windows giả lập (3 nút, ô báo sang Rust, Khôi phục, tô theo `utd://caption`,
active theo `utd://active` chứ không theo focus webview, mở app khi không active ⇒ mờ ngay, chú thích hiện/tắt, a11y),
macOS giả lập (không nút giả, chừa 84px/22px, nhấn ⇒ kéo, nhấp đúp ⇒ `title_bar_double_click` không phải
`internal_toggle_maximize`, ô đang gõ trong hộp thoại giữ focus, toàn màn hình không kéo/không nhấp đúp).

Lưu ý máy kiểm: lần kiểm đầu, `pnpm lint` trong checkout chính báo lỗi phân tích ở `.claude/worktrees/*` (worktree
của phiên khác nằm trong thư mục repo; `eslint . --ignore-pattern ".claude/**"` thì sạch) — lần kiểm sau rà soát
`pnpm lint` sạch. Vite dev lần đầu mất 1–2 phút (Tailwind quét cả các worktree và `target-titlebar` nếu còn) nên
e2e chạy lạnh cần `--timeout` dài. `target-titlebar` (1,7 GB) xoá sau mỗi lần kiểm. Không do thay đổi này.

### Kiểm cuối (cùng ngày, không còn ai sửa song song trên checkout)

Không sửa code ở bước này. Tự động: `pnpm lint` sạch; `pnpm build` + `size-limit` (JS 175,9/200 kB, CSS 15,5/20 kB);
`pnpm exec playwright test` 40/40 (`PORT=5291`, dev server làm nóng trước); `cargo test -p app --lib` 125 qua (1 bỏ qua
sẵn có); `cargo clippy -p app --all-targets -- -D warnings` sạch. App thật: build riêng `target-titlebar`, dữ liệu tạm,
CDP cổng 9420; cửa sổ thử đặt sang màn hình phụ, sau nó là một tấm nền trơn của script (ảnh không dính cửa sổ khác,
phiên khác đang mở app của họ ở màn hình chính); chỉ gửi chuột thật khi máy rảnh ≥ 20 s.

| Hành vi (chuột thật) | Kết quả đo |
|---|---|
| Kéo thanh | (−150, +100) ⇒ khung dời đúng (−150, +100); mã vùng thanh = 2 |
| Nhấp đúp thanh | phóng to đúng vùng làm việc 2560×1380, nút thành "Khôi phục"; nhấp đúp lần nữa ⇒ về đúng khung cũ |
| Đổi cỡ | mép phải +100, mép dưới +50, mép trái −80, góc dưới phải (−80, −90), mép trên ngay trên nút Đóng −40 (mép dưới giữ nguyên); mã vùng 11/15/10/17/12, góc trên phải = 14. Kéo mép dưới chạm đáy vùng làm việc ⇒ Windows kéo dài hết chiều cao (như khung gốc) |
| Aero Snap | kéo lên mép trên ⇒ phóng to, kéo xuống ⇒ về cỡ cũ 1652×1077; kéo sang mép trái ⇒ nửa trái (rộng tối thiểu của app), kéo ra ⇒ về cỡ cũ |
| 3 nút | Phóng to ⇒ phóng to; Khôi phục ⇒ về cỡ cũ; Thu nhỏ ⇒ thu xuống; nhấn Đóng kéo ra ngoài nhả ⇒ không đóng; Đóng ⇒ app thoát. Khi phóng to: điểm góc trên phải màn hình = Đóng (18), hàng trên cùng trên Thu nhỏ = 8 |
| Rê chuột | Đóng đỏ; rê yên Thu nhỏ ⇒ chú thích "Thu nhỏ"; rê Phóng to ⇒ bảng Snap Layouts hiện |
| Hộp thoại "Thêm download" đang mở | kéo thanh (+120, +60) ⇒ cửa sổ dời, hộp thoại vẫn mở, ô URL giữ focus (`titlebar-dialog.png`) |
| Mất focus | kích hoạt cửa sổ khác ⇒ `data-focused=false`, logo + icon mờ; kích hoạt lại ⇒ đậm |

Sự cố trong lúc kiểm (do script, không do app): một lần kéo mép trên bị script tự huỷ giữa chừng (con trỏ chạm mép màn
hình) khi chuột còn nhấn ⇒ cửa sổ thử bị kéo đổi cỡ theo con trỏ cho tới lúc nhả; script sau đó luôn nhả chuột trong
`finally`. Không đụng cửa sổ nào khác.

## Chưa kiểm / giới hạn

- **macOS chưa chạy thật** (máy kiểm là Windows). Cấu hình theo tài liệu Tauri; `trafficLightPosition.y = 18` tính
  từ mã tao (`inset_traffic_lights`: tâm đèn ≈ y + 2 ⇒ giữa thanh 40px), có thể lệch 1–2px. Kéo (`startDragging`),
  nhấp đúp, giữ focus mới chỉ chạy giả lập trên Chromium với UA mac (e2e); WKWebView thật chưa chạy. Lệnh
  `title_bar_double_click` biên dịch và có test trên Windows (chỉ khác: không gọi `defaults`), lời gọi
  `/usr/bin/defaults` chưa chạy thật. `acceptFirstMouse` chưa kiểm trên mac: cú nhấn đầu vào cửa sổ đang ở sau giờ
  cũng tới nội dung (vd bấm trúng nút) — đổi lại kéo được ngay.
- Góc trên trái (Windows): góc chéo chỉ ở viền ẩn ngoài cửa sổ, trong cửa sổ là `HTTOP` (dải của Tauri + WebView2);
  góc trên phải trong cửa sổ chỉ chéo ở dy 4–8 (dy 0–3 là dải của Tauri, luôn `HTTOP`). Dải của Tauri luôn được Tauri
  đưa lên trên cùng mỗi lần đổi cỡ nên cửa sổ con không nằm trên nó được.
- **DPI 100% / 150% / 200% chưa đổi để kiểm** (đổi tỉ lệ màn hình là đổi cài đặt hệ thống); chỉ kiểm ở 137,5% hiệu
  dụng. Mã chỉ dựa vào devicePixelRatio và nghe đổi tỉ lệ (`matchMedia(resolution)`), không gắn với mức cụ thể.
  Kéo sang màn hình khác DPI chưa kiểm (2 màn hình của máy đều 125%).
- Windows 10: không có Snap Layouts, góc vuông — chưa kiểm (kể cả Windows có tự hiện thêm chú thích "Maximize"
  cạnh chú thích của app không). Linux giữ khung gốc (thanh chỉ còn logo) — chưa kiểm.
- Như nút gốc, 3 nút không nằm trong thứ tự Tab; bàn phím dùng Alt+Space, Alt+F4, Win+↑/↓. Chuột phải lên chính
  3 nút chưa mở menu hệ thống (khung gốc thì có).
- Alt+Space: menu mở ở chỗ mặc định của Windows (góc trên trái, đè một phần logo).

## Ảnh

`ui-demos/titlebar-normal.png` (bình thường), `titlebar-maximized.png` (phóng to), `titlebar-unfocused.png`
(mất focus), `titlebar-hover-close.png` (rê nút Đóng), `titlebar-dialog.png` (đang mở hộp thoại),
`titlebar-snap-layouts.png` (Snap Layouts khi rê Phóng to), `titlebar-tooltip.png` (chú thích khi rê Thu nhỏ) — chụp
lại ở bước kiểm cuối; `titlebar-menu-min-height.png` (menu ⋯ ở cửa sổ thấp nhất dừng dưới thanh, cuộn bên trong) — từ
lần kiểm sau rà soát.
