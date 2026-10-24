import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type MouseEvent,
  type RefObject,
} from "react"

import {
  reportCaptionButtons,
  showWindowMenu,
  startWindowDrag,
  titleBarDoubleClick,
  watchCaption,
  watchWindowFrame,
  type CaptionButton,
  type CaptionRect,
  type CaptionState,
  type WindowFrame,
} from "./api"
import type { DesktopPlatform } from "./platform"

/** Trạng thái khung cửa sổ; trên trình duyệt (không có Tauri) giữ mặc định. */
export function useWindowFrame(platform: DesktopPlatform | null): WindowFrame {
  const [frame, setFrame] = useState<WindowFrame>(() => ({
    maximized: false,
    fullscreen: false,
    focused: typeof document === "undefined" || document.hasFocus(),
  }))
  useEffect(() => {
    if (!platform) return
    return watchWindowFrame(platform, (patch) =>
      setFrame((previous) => ({ ...previous, ...patch }))
    )
  }, [platform])
  return frame
}

const IDLE: CaptionState = { hover: null, pressed: null }

/** Nút cửa sổ đang rê/nhấn theo Rust (chỉ Windows desktop có). */
export function useCaptionState(): CaptionState {
  const [state, setState] = useState<CaptionState>(IDLE)
  useEffect(() => watchCaption(setState), [])
  return state
}

/** Chờ trước khi hiện chú thích nút cửa sổ, gần với nút gốc của Windows. */
const TIP_DELAY_MS = 500

/**
 * Nút đang hiện chú thích: rê yên trên một nút `TIP_DELAY_MS` thì hiện; đổi nút, rời đi hoặc nhấn thì tắt ngay
 * (rê lại từ đầu thì chờ lại).
 */
export function useCaptionTip(
  hover: CaptionButton | null,
  pressed: CaptionButton | null
): CaptionButton | null {
  const [ready, setReady] = useState<CaptionButton | null>(null)
  useEffect(() => {
    if (!hover || pressed) return
    const timer = window.setTimeout(() => setReady(hover), TIP_DELAY_MS)
    return () => {
      window.clearTimeout(timer)
      setReady(null)
    }
  }, [hover, pressed])
  return hover && !pressed && ready === hover ? hover : null
}

/** Ô các nút `[data-caption]` trong `root`, pixel thật (nhân devicePixelRatio). */
function measure(root: HTMLElement): CaptionRect[] {
  const scale = window.devicePixelRatio || 1
  return Array.from(root.querySelectorAll<HTMLElement>("[data-caption]")).map(
    (node) => {
      const box = node.getBoundingClientRect()
      const x = Math.round(box.left * scale)
      const y = Math.round(box.top * scale)
      return {
        button: node.dataset.caption as CaptionButton,
        x,
        y,
        width: Math.round(box.right * scale) - x,
        height: Math.round(box.bottom * scale) - y,
      }
    }
  )
}

/** Báo Rust chỗ các nút mỗi khi bố cục đổi (đổi cỡ, phóng to, đổi DPI); trùng lần trước thì thôi. */
export function useCaptionReport(root: RefObject<HTMLElement | null>) {
  useLayoutEffect(() => {
    const element = root.current
    if (!element) return
    let last = ""
    const report = () => {
      const buttons = measure(element)
      const key = JSON.stringify(buttons)
      if (key === last) return
      last = key
      void reportCaptionButtons(buttons).catch(() => {
        last = ""
      })
    }
    report()
    const observer = new ResizeObserver(report)
    observer.observe(element)
    window.addEventListener("resize", report)
    const stopScale = watchScale(report)
    return () => {
      observer.disconnect()
      window.removeEventListener("resize", report)
      stopScale()
      void reportCaptionButtons([]).catch(() => undefined)
    }
  }, [root])
}

/** Gọi `onChange` khi devicePixelRatio đổi (kéo sang màn hình DPI khác: cỡ CSS có thể không đổi nên không có `resize`). */
function watchScale(onChange: () => void): () => void {
  let query: MediaQueryList | null = null
  const listen = () => {
    query = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`)
    query.addEventListener("change", changed, { once: true })
  }
  const changed = () => {
    onChange()
    listen()
  }
  listen()
  return () => query?.removeEventListener("change", changed)
}

/** Alt+Space mở menu hệ thống như cửa sổ có khung gốc (WebView2 không chuyển phím này lên cửa sổ). */
export function useWindowMenuKey() {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.code !== "Space" || !event.altKey) return
      if (event.ctrlKey || event.shiftKey || event.metaKey) return
      event.preventDefault()
      void showWindowMenu().catch(() => undefined)
    }
    window.addEventListener("keydown", onKey, true)
    return () => window.removeEventListener("keydown", onKey, true)
  }, [])
}

/**
 * Chuột trên thanh tiêu đề. Mọi nền tảng: nhấn lên thanh không lấy focus (ô đang gõ, hộp thoại đang mở giữ focus,
 * như thanh gốc) ⇒ con của thanh chỉ nên là nội dung tĩnh (logo, tên). `macDrag` (macOS, không toàn màn hình): thanh
 * phủ lên webview nên trang tự lo như thanh gốc — nhấn để kéo; nhấp đúp làm theo cài đặt hệ thống lúc nhả chuột,
 * chuột đã dời thì huỷ (như drag.js của Tauri). Toàn màn hình: không kéo, nhấp đúp không làm gì.
 */
export function useTitleBarMouse(macDrag: boolean) {
  const second = useRef<{ x: number; y: number } | null>(null)
  const onMouseDown = (event: MouseEvent<HTMLElement>) => {
    if (event.button !== 0) return
    event.preventDefault()
    second.current =
      macDrag && event.detail === 2
        ? { x: event.clientX, y: event.clientY }
        : null
    if (macDrag && event.detail === 1)
      void startWindowDrag().catch(() => undefined)
  }
  const onMouseUp = (event: MouseEvent<HTMLElement>) => {
    const start = second.current
    second.current = null
    if (!macDrag || !start || event.button !== 0 || event.detail !== 2) return
    if (start.x !== event.clientX || start.y !== event.clientY) return
    void titleBarDoubleClick().catch(() => undefined)
  }
  return { onMouseDown, onMouseUp }
}
