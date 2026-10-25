import { useCallback, useEffect, useRef, useState } from "react"

import { downloadDetail, subscribeSegments, watchDownload } from "../api/detail"
import type { DetailLoad, DownloadDetail } from "../model/detail"
import { demoDetail } from "../model/mock-data"
import type { DownloadItem } from "../model/types"

/** Kết quả đọc gần nhất, kèm task của nó: khác task đang mở ⇒ đang đọc (S-17). */
interface DetailState {
  id: string | null
  load: DetailLoad
  detail: DownloadDetail | null
  error: string | null
}

const NONE: DetailState = { id: null, load: "loading", detail: null, error: null }

/**
 * Dữ liệu màn Chi tiết của một task: đọc journal một lần, rồi nghe `utd://segments` (app chỉ đẩy cho task
 * đang mở, theo nhịp tiến độ) — giao diện không tự hỏi lại theo giờ. `version` đổi (trạng thái hay mã lỗi
 * của task đổi) ⇒ đọc lại lịch sử, giữ màn cũ trong lúc đọc.
 */
export function useDownloadDetail(
  taskId: string | null,
  open: boolean,
  desktop: boolean,
  version: string,
  demoItem: DownloadItem | undefined
) {
  const [state, setState] = useState<DetailState>(NONE)
  const current = useRef<string | null>(null)

  // Kết quả về sau khi đã đổi sang task khác (hoặc đã đóng) thì bỏ.
  const read = useCallback((id: string) => {
    downloadDetail(id).then(
      (detail) => {
        if (current.current !== id) return
        setState({ id, load: detail ? "ready" : "missing", detail, error: null })
      },
      (reason: unknown) => {
        if (current.current !== id) return
        const error = reason instanceof Error ? reason.message : String(reason)
        setState({ id, load: "error", detail: null, error })
      }
    )
  }, [])

  // Mở / đổi task: đọc, báo app task đang mở, nghe bản đồ đoạn. Đóng: báo app thôi đẩy (giữ dữ liệu
  // cũ để vẽ nốt lúc hộp mờ dần).
  useEffect(() => {
    current.current = open ? taskId : null
    if (!taskId || !open || !desktop) return
    let disposed = false
    let unlisten: (() => void) | undefined
    read(taskId)
    void watchDownload(taskId).catch(() => undefined)
    void subscribeSegments((update) => {
      if (disposed || update.taskId !== taskId) return
      setState((previous) =>
        previous.id === taskId && previous.detail
          ? { ...previous, detail: { ...previous.detail, parts: update.parts } }
          : previous
      )
    })
      .then((off) => {
        if (disposed) off()
        else unlisten = off
      })
      .catch(() => undefined)
    return () => {
      disposed = true
      unlisten?.()
      void watchDownload(null).catch(() => undefined)
    }
  }, [taskId, open, desktop, read])

  // Trạng thái đổi (dừng, lỗi, xong…): lịch sử và kết quả mới nằm trong journal. Mở task khác thì
  // hiệu ứng trên đã đọc rồi.
  const seen = useRef("")
  useEffect(() => {
    const key = `${taskId}|${version}`
    const previous = seen.current
    seen.current = key
    if (!taskId || !open || !desktop) return
    if (previous !== key && previous.startsWith(`${taskId}|`)) read(taskId)
  }, [version, taskId, open, desktop, read])

  const retry = useCallback(() => {
    if (!taskId || !desktop) return
    setState(NONE)
    read(taskId)
  }, [taskId, desktop, read])

  if (!desktop) {
    // Bản xem trước trên trình duyệt: chi tiết mẫu dựng từ dòng mẫu.
    return demoItem
      ? { load: "ready" as const, detail: demoDetail(demoItem), error: null, retry }
      : { load: "missing" as const, detail: null, error: null, retry }
  }
  const shown = state.id === taskId ? state : NONE
  return { load: shown.load, detail: shown.detail, error: shown.error, retry }
}
