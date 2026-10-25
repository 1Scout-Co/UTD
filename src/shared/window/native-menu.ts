import { desktopPlatform } from "./platform"

/**
 * App desktop: chặn menu chuột phải gốc của webview ở mọi chỗ trừ ô nhập chữ (nơi menu gốc cho Cắt/Chép/Dán).
 * WKWebView trên macOS hiện mỗi mục "Reload" (bấm nhầm là tải lại cả giao diện), WebView2 có Back/Refresh/Print.
 * Dòng trong bảng có menu riêng (Base UI) chạy trước, không bị ảnh hưởng. Trình duyệt (bản xem trước, e2e): không làm gì.
 */
export function blockNativeContextMenu() {
  if (!desktopPlatform()) return
  document.addEventListener("contextmenu", (event) => {
    const target = event.target
    if (
      target instanceof Element &&
      target.closest("input, textarea, [contenteditable='true']")
    ) {
      return
    }
    event.preventDefault()
  })
}
