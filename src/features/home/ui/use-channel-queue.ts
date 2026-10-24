import { useCallback, useEffect, useRef, useState } from "react"

import { addMediaDownload } from "../api/downloads"
import type { AddMediaPayload, EngineTask, EngineTaskStatus } from "../model/types"

/**
 * Số video của kênh tải cùng lúc. App chưa có hàng đợi chung giới hạn số download chạy song song, nên
 * hộp Tải cả kênh tự thêm lần lượt: chỉ thêm video kế tiếp khi số video kênh đang chạy dưới mức này.
 * Có hàng đợi chung ở app thì đẩy hết một lần và bỏ giới hạn ở đây.
 */
const CHANNEL_PARALLEL = 2

const RUNNING = new Set<EngineTaskStatus>(["queued", "resolving", "downloading", "verifying"])

/**
 * Hàng chờ video của kênh, sống ở màn chính (đóng hộp vẫn tải tiếp). Đóng app thì phần chưa thêm mất:
 * lần sau mở lại kênh, "chỉ video mới" vẫn chọn đúng phần chưa tải.
 */
export function useChannelQueue(tasks: EngineTask[], onAdded: (task: EngineTask) => void) {
  const [pending, setPending] = useState<AddMediaPayload[]>([])
  const [started, setStarted] = useState<string[]>([])
  const [failed, setFailed] = useState(0)
  const busy = useRef(false)

  const running = started.filter((id) => {
    const task = tasks.find((item) => item.id === id)
    return task !== undefined && RUNNING.has(task.status)
  }).length

  useEffect(() => {
    if (busy.current || pending.length === 0 || running >= CHANNEL_PARALLEL) return
    busy.current = true
    const next = pending[0]
    void addMediaDownload(next)
      .then((task) => {
        onAdded(task)
        setStarted((list) => [...list.slice(-50), task.id])
      })
      .catch((reason: unknown) => {
        console.error("UTD channel:", reason)
        setFailed((count) => count + 1)
      })
      .finally(() => {
        busy.current = false
        setPending((list) => (list[0] === next ? list.slice(1) : list))
      })
  }, [pending, running, onAdded])

  const enqueue = useCallback((payloads: AddMediaPayload[]) => {
    setFailed(0)
    setPending((list) => [...list, ...payloads])
  }, [])

  const clear = useCallback(() => setPending([]), [])

  return { waiting: pending.length, failed, enqueue, clear }
}
