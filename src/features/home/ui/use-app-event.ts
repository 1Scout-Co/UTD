import { useEffect } from "react"

import type { UnlistenFn } from "@tauri-apps/api/event"

/**
 * Nghe một sự kiện của app khi `active` (yêu cầu từ trình duyệt, tiến độ, bản mới…). `onEvent` cần ổn định
 * (useCallback / setState). Có `take`: giá trị phát trước khi kịp nghe (UTD vừa mở) app giữ lại, đọc một lần
 * ngay sau khi nghe.
 */
export function useAppEvent<T>(
  active: boolean,
  subscribe: (onEvent: (value: T) => void) => Promise<UnlistenFn>,
  onEvent: (value: T) => void,
  take?: () => Promise<T | null>,
) {
  useEffect(() => {
    if (!active) return
    let disposed = false
    let unlisten: (() => void) | undefined
    subscribe((value) => {
      if (!disposed) onEvent(value)
    })
      .then(async (off) => {
        if (disposed) {
          off()
          return
        }
        unlisten = off
        const early = await take?.()
        if (early && !disposed) onEvent(early)
      })
      .catch((reason: unknown) => console.error("UTD event:", reason))
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [active, subscribe, onEvent, take])
}
