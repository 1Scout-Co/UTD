// Tải cần đăng nhập (S-14, S-72; notes/auth-plan.md §8): mã lỗi nghĩa là "cho phép trang này trong popup
// tiện ích là tải tiếp được". Cùng danh sách với `SIGN_IN_CODES` ở src-tauri/src/auth/flow.rs. `auth.pipe`
// (yt-dlp không nhận được cookie) không nằm đây: đăng nhập lại không sửa được.
const SIGN_IN_CODES = new Set([
  "auth.required",
  "auth.expired",
  "http.401",
  "http.403",
  "media.login",
])

export function needsSignIn(code: string | null | undefined): boolean {
  return Boolean(code && SIGN_IN_CODES.has(code))
}

/** Mã của câu lỗi lệnh Rust dạng `[nhóm.tên] câu` (Phân tích, liệt kê kênh). */
export function commandCode(reason: unknown): string | null {
  const message = reason instanceof Error ? reason.message : String(reason)
  return /^\[([a-z][a-z0-9_]*(?:\.[a-z0-9_]+)+)\] /.exec(message)?.[1] ?? null
}

/** Trang (`site`, tên miền gốc app gửi) có phải trang của link không. */
export function sameSite(url: string, site: string): boolean {
  try {
    const host = new URL(url).hostname.toLowerCase()
    return host === site || host.endsWith(`.${site}`)
  } catch {
    return false
  }
}
