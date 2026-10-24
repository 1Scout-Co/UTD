import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { getCurrentWindow, type Window } from "@tauri-apps/api/window"

import type { DesktopPlatform } from "./platform"

export type CaptionButton = "minimize" | "maximize" | "close"

/** Trạng thái khung cửa sổ mà thanh tiêu đề cần biết. */
export interface WindowFrame {
  maximized: boolean
  fullscreen: boolean
  focused: boolean
}

/** Nút đang được rê/nhấn (Windows: do Rust báo, vì chuột trên nút đi vào cửa sổ con chứ không vào webview). */
export interface CaptionState {
  hover: CaptionButton | null
  pressed: CaptionButton | null
}

/** Ô một nút cửa sổ, pixel thật từ góc trên trái webview. */
export interface CaptionRect {
  button: CaptionButton
  x: number
  y: number
  width: number
  height: number
}

/** Cửa sổ hiện tại; `null` khi môi trường giả Tauri thiếu metadata cửa sổ (bản thử), để không làm vỡ giao diện. */
function currentWindow(): Window | null {
  try {
    return getCurrentWindow()
  } catch {
    return null
  }
}

export async function runCaption(button: CaptionButton): Promise<void> {
  const current = currentWindow()
  if (!current) return
  if (button === "minimize") return current.minimize()
  if (button === "maximize") return current.toggleMaximize()
  return current.close()
}

export function reportCaptionButtons(buttons: CaptionRect[]): Promise<void> {
  return invoke("set_caption_buttons", { buttons })
}

/** Menu hệ thống của cửa sổ (Alt+Space). */
export function showWindowMenu(): Promise<void> {
  return invoke("show_window_menu")
}

/** Bắt đầu kéo cửa sổ theo chuột đang nhấn (macOS: thanh tiêu đề phủ lên webview nên trang tự gọi). */
export async function startWindowDrag(): Promise<void> {
  await currentWindow()?.startDragging()
}

/** Nhấp đúp thanh tiêu đề trên macOS: Rust làm theo cài đặt hệ thống (phóng to · thu nhỏ · không làm gì). */
export function titleBarDoubleClick(): Promise<void> {
  return invoke("title_bar_double_click")
}

/**
 * Gom các lời đăng ký bất đồng bộ; huỷ trước khi đăng ký xong thì huỷ luôn khi xong.
 * `onReady` chạy khi đăng ký xong (chưa huỷ): từ lúc đó mọi thay đổi đều đến qua sự kiện.
 */
function collect(
  pending: Promise<UnlistenFn>[],
  onReady?: () => void
): () => void {
  let disposed = false
  let offs: UnlistenFn[] = []
  void Promise.all(pending)
    .then((list) => {
      if (disposed) list.forEach((off) => off())
      else {
        offs = list
        onReady?.()
      }
    })
    .catch(() => undefined)
  return () => {
    disposed = true
    offs.forEach((off) => off())
  }
}

/**
 * Theo dõi phóng to / toàn màn hình / focus của cửa sổ hiện tại; trả hàm huỷ. Focus trên Windows là cửa sổ
 * active (WM_NCACTIVATE, sự kiện `utd://active` của app) như khung gốc: sự kiện focus của Tauri ở đó là focus của
 * webview, không báo khi kích hoạt bằng Alt+Tab/thanh tác vụ mà focus chưa vào webview.
 */
export function watchWindowFrame(
  platform: DesktopPlatform,
  onChange: (patch: Partial<WindowFrame>) => void
): () => void {
  const current = currentWindow()
  if (!current) return () => undefined
  const refresh = () => {
    void Promise.all([current.isMaximized(), current.isFullscreen()])
      .then(([maximized, fullscreen]) => onChange({ maximized, fullscreen }))
      .catch(() => undefined)
  }
  // Focus chỉ báo qua sự kiện, mỗi lần đổi một lần: cửa sổ mất focus (hoặc chưa từng active) trước khi đăng ký
  // xong thì không có sự kiện nào ⇒ hỏi lại khi đăng ký xong. Sự kiện đến sau lời hỏi thì mới hơn kết quả hỏi ⇒
  // bỏ kết quả đó. Hỏi lệnh của app, không dùng `isFocused()`: trên Windows lệnh đó sai khi focus ở trong webview.
  let focusEvents = 0
  const onFocus = (focused: boolean) => {
    focusEvents += 1
    onChange({ focused })
  }
  const askFocus = () => {
    const seen = focusEvents
    void invoke<boolean>("is_window_active")
      .then((focused) => {
        if (seen === focusEvents && typeof focused === "boolean")
          onChange({ focused })
      })
      .catch(() => undefined)
  }
  refresh()
  return collect(
    [
      current.onResized(refresh),
      platform === "windows"
        ? listen<boolean>("utd://active", ({ payload }) => onFocus(payload))
        : current.onFocusChanged(({ payload }) => onFocus(payload)),
    ],
    () => {
      refresh()
      askFocus()
    }
  )
}

export function watchCaption(
  onChange: (state: CaptionState) => void
): () => void {
  return collect([
    listen<CaptionState>("utd://caption", (event) => onChange(event.payload)),
  ])
}
