/**
 * Giao diện đang chạy trên macOS (WebView của macOS ghi "Macintosh" trong user agent): đổi nhãn theo hệ
 * điều hành (phím ⌘, tên file/nguồn tải công cụ). Bắt phím, đường dẫn… do app (Rust) lo.
 */
export const isMac = typeof navigator !== "undefined" && /Mac/.test(navigator.userAgent)
