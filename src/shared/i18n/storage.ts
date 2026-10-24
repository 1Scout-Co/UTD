import { invoke, isTauri } from "@tauri-apps/api/core"

// Ngôn ngữ người dùng chọn: app desktop lưu trong cài đặt của journal (src-tauri/src/language.rs);
// bản xem trước trên trình duyệt chỉ có localStorage. Chưa chọn lần nào ⇒ null (theo hệ điều hành).

const PREVIEW_KEY = "utd.language"

export async function savedLanguage(): Promise<string | null> {
  if (isTauri()) return invoke<string | null>("get_ui_language")
  try {
    return window.localStorage.getItem(PREVIEW_KEY)
  } catch {
    return null
  }
}

/** App desktop: báo Rust ngôn ngữ đang hiện (macOS dựng lại menu app theo ngôn ngữ đó). */
export function applyNativeLanguage(code: string) {
  if (!isTauri()) return
  void invoke("apply_ui_language", { language: code }).catch(() => undefined)
}

export async function saveLanguage(code: string): Promise<void> {
  if (isTauri()) {
    await invoke("set_ui_language", { language: code })
    return
  }
  try {
    window.localStorage.setItem(PREVIEW_KEY, code)
  } catch {
    // Trình duyệt chặn bộ nhớ: lựa chọn chỉ còn trong lần mở này.
  }
}
