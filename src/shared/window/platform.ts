import { isTauri } from "@tauri-apps/api/core"

export type DesktopPlatform = "windows" | "macos" | "linux"

/**
 * Hệ điều hành của app desktop; `null` khi chạy trên trình duyệt (bản xem trước, e2e).
 *
 * Đọc từ user agent: trong Tauri mỗi hệ điều hành chỉ có một engine webview (WebView2 ↔ Windows,
 * WKWebView ↔ macOS, WebKitGTK ↔ Linux) và app không tự đặt user agent, nên chuỗi này luôn đúng.
 * Đọc được ngay lúc vẽ lần đầu (không phải chờ lệnh bất đồng bộ ⇒ thanh tiêu đề không nhảy bố cục)
 * và không cần thêm plugin/quyền như @tauri-apps/plugin-os. `navigator.userAgentData` chỉ Chromium có.
 */
export function desktopPlatform(): DesktopPlatform | null {
  return isTauri() ? platformFromAgent(navigator.userAgent) : null
}

function platformFromAgent(agent: string): DesktopPlatform {
  if (/Windows/i.test(agent)) return "windows"
  if (/Macintosh|Mac OS X/i.test(agent)) return "macos"
  return "linux"
}
