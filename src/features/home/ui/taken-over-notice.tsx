import { useCallback, useEffect, useState } from "react"

import { Download } from "lucide-react"

import { t } from "@/shared/i18n/i18n"

import { subscribeTakenOver, type TakenOver } from "../api/downloads"
import { useAppEvent } from "./use-app-event"

const SHOW_MS = 4000

/**
 * S-77: UTD vừa nhận một download của trình duyệt (tự bắt link tải): báo nhỏ vài giây, không mở hộp nào. Dòng tải
 * mới tự hiện trong danh sách.
 */
export function TakenOverNotice({ desktop }: { desktop: boolean }) {
  const [shown, setShown] = useState<TakenOver | null>(null)
  const onTaken = useCallback((taken: TakenOver) => setShown(taken), [])
  useAppEvent(desktop, subscribeTakenOver, onTaken)
  useEffect(() => {
    if (!shown) return
    const timer = window.setTimeout(() => setShown(null), SHOW_MS)
    return () => window.clearTimeout(timer)
  }, [shown])
  if (!shown) return null
  return (
    <div
      role="status"
      data-s="S-77"
      className="fixed end-4 bottom-4 z-40 flex max-w-[min(26rem,calc(100vw-2rem))] items-center gap-2 rounded-full bg-card py-1.5 ps-3 pe-3.5 text-sm shadow-soft ring-1 ring-foreground/5"
    >
      <Download className="size-3.5 shrink-0 text-primary" />
      <span className="shrink-0 text-muted-foreground">{t("takeover.taken")}</span>
      <span dir="auto" className="truncate font-medium">
        {shown.fileName}
      </span>
    </div>
  )
}
