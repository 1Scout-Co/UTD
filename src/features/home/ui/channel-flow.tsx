import { useCallback, useState } from "react"

import { Loader2, X } from "lucide-react"

import { Button } from "@/components/ui/button"

import type { AddMediaPayload, EngineTask } from "../model/types"
import { ChannelDialog } from "./channel-dialog"
import { useChannelQueue } from "./use-channel-queue"

/**
 * Lối vào Tải cả kênh của màn chính: `openChannel(url)` mở hộp (link từ hộp Thêm, Tải media, trình duyệt);
 * hàng chờ video sống ở đây nên đóng hộp vẫn tải tiếp.
 */
export function useChannelFlow(tasks: EngineTask[], onAdded: (task: EngineTask) => void) {
  // `key` đổi ⇒ hộp dựng lại và liệt kê từ đầu.
  const [request, setRequest] = useState<{ url: string; key: number } | null>(null)
  const [open, setOpen] = useState(false)
  const queue = useChannelQueue(tasks, onAdded)
  const { enqueue } = queue

  const openChannel = useCallback((url: string) => {
    setRequest((previous) => ({ url, key: (previous?.key ?? 0) + 1 }))
    setOpen(true)
  }, [])

  const start = useCallback(
    (payloads: AddMediaPayload[]) => {
      enqueue(payloads)
      setOpen(false)
    },
    [enqueue],
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
          className="fixed bottom-4 left-1/2 z-40 flex -translate-x-1/2 items-center gap-2 rounded-full bg-card py-1.5 pr-1.5 pl-3.5 text-sm shadow-soft ring-1 ring-foreground/5"
        >
          <Loader2 className="size-3.5 animate-spin text-primary" />
          <span className="tabular-nums">Kênh: còn {queue.waiting} video chờ</span>
          {queue.failed > 0 ? (
            <span className="text-xs text-red-700 tabular-nums">· {queue.failed} lỗi</span>
          ) : null}
          <Button type="button" variant="ghost" size="xs" onClick={queue.clear}>
            <X />
            Dừng
          </Button>
        </div>
      ) : null}
    </>
  )
}
