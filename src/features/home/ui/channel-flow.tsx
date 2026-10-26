import { useCallback, useRef, useState } from "react"

import { Loader2, X } from "lucide-react"

import { Button } from "@/components/ui/button"
import { t } from "@/shared/i18n/i18n"

import { addMediaDownload } from "../api/downloads"
import { addSocialDownloads, closeSocial, subscribeSocialRequest, takeSocialRequest } from "../api/social"
import type { SocialRequest } from "../model/social"
import type { AddMediaPayload, EngineTask } from "../model/types"
import { ChannelDialog } from "./channel-dialog"
import { useAppEvent } from "./use-app-event"

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
  const [request, setRequest] = useState<{
    url: string
    key: number
    authGrant?: string
    social?: SocialRequest
  } | null>(null)
  const [open, setOpenState] = useState(false)
  const queue = useChannelSender(onAdded)
  const { send } = queue
  const socialId = request?.social?.listingId ?? null

  // Hộp tải hàng loạt đóng (kể cả sau khi bấm Tải): app bỏ danh sách, tiện ích dừng ở đợt sau.
  const setOpen = useCallback(
    (next: boolean) => {
      setOpenState(next)
      if (!next && socialId) void closeSocial(socialId).catch(() => undefined)
    },
    [socialId],
  )

  const openChannel = useCallback((url: string, authGrant?: string) => {
    setRequest((previous) => {
      if (previous?.social) void closeSocial(previous.social.listingId).catch(() => undefined)
      return { url, key: (previous?.key ?? 0) + 1, authGrant }
    })
    setOpenState(true)
  }, [])

  /** Danh sách tải hàng loạt từ trình duyệt (Facebook / Instagram / Threads). */
  const openSocial = useCallback((social: SocialRequest) => {
    setRequest((previous) => {
      // Hộp hàng loạt khác đang mở bị thay: bỏ danh sách cũ.
      if (previous?.social && previous.social.listingId !== social.listingId) {
        void closeSocial(previous.social.listingId).catch(() => undefined)
      }
      return { url: social.url, key: (previous?.key ?? 0) + 1, social }
    })
    setOpenState(true)
  }, [])

  const start = useCallback(
    (payloads: AddMediaPayload[]) => {
      send(payloads)
      setOpen(false)
    },
    [send, setOpen],
  )

  /** Cả lô vào hàng đợi một lần (cuối hàng), app đặt tên `<ngày> <chú thích> <id>`. */
  const startSocial = useCallback(
    (ids: string[], subfolder: string | null) => {
      if (!socialId) return
      addSocialDownloads({ listingId: socialId, ids, subfolder: subfolder ?? undefined, later: true })
        .then((result) => result.tasks.forEach(onAdded))
        .catch((reason: unknown) => console.error("UTD social:", reason))
        .finally(() => setOpen(false))
    },
    [socialId, onAdded, setOpen],
  )

  return { request, open, setOpen, queue, start, startSocial, openChannel, openSocial }
}

export function ChannelFlow({
  desktop,
  flow,
}: {
  desktop: boolean
  flow: ReturnType<typeof useChannelFlow>
}) {
  const { request, queue } = flow
  useAppEvent(desktop, subscribeSocialRequest, flow.openSocial, takeSocialRequest)
  return (
    <>
      {request ? (
        <ChannelDialog
          key={request.key}
          url={request.url}
          authGrant={request.authGrant}
          social={request.social}
          open={flow.open}
          onOpenChange={flow.setOpen}
          desktop={desktop}
          onStart={flow.start}
          onStartSocial={flow.startSocial}
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
