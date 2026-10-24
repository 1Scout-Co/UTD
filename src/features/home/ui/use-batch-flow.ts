import { useCallback, useEffect, useState } from "react"

import { subscribeBatchRequest, takeBatchRequest } from "../api/downloads"

/** Lô link từ trình duyệt; lô tới trước khi kịp nghe (UTD vừa mở) được app giữ lại, lấy một lần. */
function useBatchRequests(desktop: boolean, onUrls: (urls: string[]) => void) {
  useEffect(() => {
    if (!desktop) return
    let disposed = false
    let unlisten: (() => void) | undefined
    subscribeBatchRequest((request) => {
      if (!disposed) onUrls(request.urls)
    })
      .then(async (off) => {
        if (disposed) {
          off()
          return
        }
        unlisten = off
        const early = await takeBatchRequest()
        if (early && !disposed) onUrls(early.urls)
      })
      .catch((reason: unknown) => console.error("UTD batch request:", reason))
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [desktop, onUrls])
}

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
  const [state, setState] = useState<{ open: boolean; key: number; urls: string[]; fromBrowser: boolean }>({
    open: false,
    key: 0,
    urls: [],
    fromBrowser: false,
  })
  const show = useCallback(
    (urls: string[], fromBrowser: boolean) => {
      beforeOpen()
      setState((previous) => ({ open: true, key: previous.key + 1, urls, fromBrowser }))
    },
    [beforeOpen],
  )
  const openEmpty = useCallback(() => show([], false), [show])
  const fromBrowser = useCallback((urls: string[]) => show(urls, true), [show])
  /** Dán nhiều link vào hộp Thêm URL ⇒ chuyển sang đây với các link đó. */
  const openWith = useCallback((urls: string[]) => show(urls, false), [show])
  useBatchRequests(desktop, fromBrowser)
  useBatchShortcut(openEmpty)
  // Ổn định giữa các lần vẽ: màn chính dùng trong callback của luồng media/kênh.
  const setOpen = useCallback(
    (open: boolean) => setState((previous) => ({ ...previous, open })),
    [],
  )
  return { ...state, openEmpty, openWith, setOpen }
}
