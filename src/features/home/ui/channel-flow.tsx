import { useCallback, useRef, useState } from "react"

import { Loader2, X } from "lucide-react"

import { Button } from "@/components/ui/button"
import { t } from "@/shared/i18n/i18n"

import { addMediaDownload } from "../api/downloads"
import type { AddMediaPayload, EngineTask } from "../model/types"
import { ChannelDialog } from "./channel-dialog"

/**
 * Gửi video của kênh vào hàng đợi chung của app: lần lượt từng video với `later` (cuối hàng, đúng thứ tự
 * chọn); app tự giới hạn số download chạy cùng lúc (Cài đặt → Tải cùng lúc). Việc gửi sống ở màn chính
 * nên đóng hộp vẫn gửi tiếp; "Dừng" thôi gửi phần còn lại (video đã vào hàng thì tạm dừng ở danh sách).
 * Kênh lớn gửi mất một lúc: bấm tải kênh khác giữa chừng thì nối vào sau, không bỏ phần còn lại.
 */
function useChannelSender(onAdded: (task: EngineTask) => void) {
  const [waiting, setWaiting] = useState(0)
  const [failed, setFailed] = useState(0)
  const pending = useRef<AddMediaPayload[]>([])
  const sending = useRef(false)

  const drain = useCallback(async () => {
    if (sending.current) return
    sending.current = true
    for (let payload = pending.current.shift(); payload; payload = pending.current.shift()) {
      setWaiting(pending.current.length + 1)
      try {
        onAdded(await addMediaDownload({ ...payload, later: true }))
      } catch (reason: unknown) {
        console.error("UTD channel:", reason)
        setFailed((count) => count + 1)
      }
    }
    sending.current = false
    setWaiting(0)
  }, [onAdded])

  const send = useCallback(
    (payloads: AddMediaPayload[]) => {
      if (!sending.current) setFailed(0)
      pending.current = [...pending.current, ...payloads]
      setWaiting(pending.current.length + (sending.current ? 1 : 0))
      void drain()
    },
    [drain],
  )

  const clear = useCallback(() => {
    pending.current = []
    setWaiting(0)
  }, [])

  return { waiting, failed, send, clear }
}

/**
 * Lối vào Tải cả kênh của màn chính: `openChannel(url)` mở hộp (link từ hộp Thêm, Tải media, trình duyệt).
 */
export function useChannelFlow(onAdded: (task: EngineTask) => void) {
  // `key` đổi ⇒ hộp dựng lại và liệt kê từ đầu.
  const [request, setRequest] = useState<{ url: string; key: number } | null>(null)
  const [open, setOpen] = useState(false)
  const queue = useChannelSender(onAdded)
  const { send } = queue

  const openChannel = useCallback((url: string) => {
    setRequest((previous) => ({ url, key: (previous?.key ?? 0) + 1 }))
    setOpen(true)
  }, [])

  const start = useCallback(
    (payloads: AddMediaPayload[]) => {
      send(payloads)
      setOpen(false)
    },
    [send],
  )

  return { request, open, setOpen, queue, start, openChannel }
}

export function ChannelFlow({
  desktop,
  flow,
}: {
  desktop: boolean
  flow: ReturnType<typeof useChannelFlow>
}) {
  const { request, queue } = flow
  return (
    <>
      {request ? (
        <ChannelDialog
          key={request.key}
          url={request.url}
          open={flow.open}
          onOpenChange={flow.setOpen}
          desktop={desktop}
          onStart={flow.start}
        />
      ) : null}
      {queue.waiting > 0 ? (
        <div
          role="status"
          className="fixed bottom-4 left-1/2 z-40 flex -translate-x-1/2 items-center gap-2 rounded-full bg-card py-1.5 ps-3.5 pe-1.5 text-sm shadow-soft ring-1 ring-foreground/5"
        >
          <Loader2 className="size-3.5 animate-spin text-primary" />
          <span className="tabular-nums">
            {t("channel.adding", { count: queue.waiting })}
          </span>
          {queue.failed > 0 ? (
            <span className="text-xs text-red-700 tabular-nums">
              · {t("channel.addFailed", { count: queue.failed })}
            </span>
          ) : null}
          <Button type="button" variant="ghost" size="xs" onClick={queue.clear}>
            <X />
            {t("channel.stop")}
          </Button>
        </div>
      ) : null}
    </>
  )
}
