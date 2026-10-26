import { useCallback, useEffect, useState } from "react"

import { subscribeBatchRequest, takeBatchRequest, type BatchRequest } from "../api/downloads"
import { useAppEvent } from "./use-app-event"

/** Ctrl+B (B = batch) mở hộp Thêm hàng loạt. */
function useBatchShortcut(onOpen: () => void) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && !event.shiftKey && event.key.toLowerCase() === "b") {
        event.preventDefault()
        onOpen()
      }
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [onOpen])
}

/**
 * Trạng thái của hộp Thêm hàng loạt cho màn chính: mở bằng nút, phím tắt hoặc lô từ trình duyệt.
 * `key` đổi ⇒ hộp dựng lại với danh sách mới. `beforeOpen` đóng các hộp khác.
 */
export function useBatchFlow(desktop: boolean, beforeOpen: () => void) {
  const [state, setState] = useState<{
    open: boolean
    key: number
    urls: string[]
    fromBrowser: boolean
    authGrant?: string
  }>({
    open: false,
    key: 0,
    urls: [],
    fromBrowser: false,
  })
  const show = useCallback(
    (urls: string[], fromBrowser: boolean, authGrant?: string) => {
      beforeOpen()
      setState((previous) => ({ open: true, key: previous.key + 1, urls, fromBrowser, authGrant }))
    },
    [beforeOpen],
  )
  const openEmpty = useCallback(() => show([], false), [show])
  // Lô link từ trình duyệt; lô tới trước khi kịp nghe (UTD vừa mở) được app giữ lại, lấy một lần.
  const fromBrowser = useCallback(
    (request: BatchRequest) => show(request.urls, true, request.authGrant),
    [show],
  )
  /** Dán nhiều link vào hộp Thêm URL ⇒ chuyển sang đây với các link đó. */
  const openWith = useCallback((urls: string[]) => show(urls, false), [show])
  useAppEvent(desktop, subscribeBatchRequest, fromBrowser, takeBatchRequest)
  useBatchShortcut(openEmpty)
  // Ổn định giữa các lần vẽ: màn chính dùng trong callback của luồng media/kênh.
  const setOpen = useCallback(
    (open: boolean) => setState((previous) => ({ ...previous, open })),
    [],
  )
  return { ...state, openEmpty, openWith, setOpen }
}
