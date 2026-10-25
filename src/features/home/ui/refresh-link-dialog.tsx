import { useEffect, useRef, useState, type FormEvent } from "react"

import { Link2, Loader2 } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { commandErrorText, t } from "@/shared/i18n/i18n"

import {
  awaitDownloadLink,
  onLinkUpdated,
  updateDownloadLink,
} from "../api/recover"
import type { DownloadItem } from "../model/types"

type FormProps = {
  item: DownloadItem
  desktop: boolean
  onClose: () => void
}

/** Trong lúc hộp mở: link tải lại từ trình duyệt gắn vào download này (app báo qua sự kiện). */
function useBrowserLink(
  item: DownloadItem,
  desktop: boolean,
  onDone: () => void,
  onError: (message: string) => void,
) {
  const handlers = useRef({ onDone, onError })
  useEffect(() => {
    handlers.current = { onDone, onError }
  })
  useEffect(() => {
    if (!desktop) return
    let disposed = false
    let off: (() => void) | undefined
    void awaitDownloadLink(item.id).catch(() => undefined)
    void onLinkUpdated((update) => {
      if (disposed || update.taskId !== item.id) return
      if (update.error) handlers.current.onError(commandErrorText(update.error))
      else handlers.current.onDone()
    }).then((unlisten) => {
      if (disposed) unlisten()
      else off = unlisten
    })
    return () => {
      disposed = true
      off?.()
      void awaitDownloadLink(null).catch(() => undefined)
    }
  }, [item.id, desktop])
}

function RefreshLinkForm({ item, desktop, onClose }: FormProps) {
  const [url, setUrl] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useBrowserLink(item, desktop, onClose, setError)

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    const next = url.trim()
    if (!next || !desktop) return
    setBusy(true)
    setError(null)
    try {
      await updateDownloadLink(item.id, next)
      onClose()
    } catch (reason) {
      setError(commandErrorText(reason))
    } finally {
      setBusy(false)
    }
  }

  return (
    <form className="space-y-4" onSubmit={(event) => void submit(event)}>
      <div
        dir="auto"
        className="truncate rounded-lg bg-muted px-3 py-2 text-sm font-medium [text-align:match-parent]"
        title={item.name}
      >
        {item.name}
      </div>
      <Input
        aria-label={t("recover.dialog.label")}
        placeholder="https://"
        value={url}
        autoFocus
        spellCheck={false}
        disabled={busy}
        onChange={(event) => setUrl(event.target.value)}
      />
      <p className="text-sm text-muted-foreground">
        {t("recover.dialog.browser")}
      </p>
      {error ? (
        <p
          role="alert"
          className="rounded-lg bg-destructive/5 px-3 py-2 text-sm text-destructive"
        >
          {error}
        </p>
      ) : null}
      <div className="flex justify-end gap-2">
        <Button type="button" variant="ghost" disabled={busy} onClick={onClose}>
          {t("app.cancel")}
        </Button>
        <Button
          data-primary
          type="submit"
          disabled={busy || !url.trim() || !desktop}
        >
          {busy ? <Loader2 className="animate-spin" /> : <Link2 />}
          {busy ? t("recover.dialog.checking") : t("recover.dialog.submit")}
        </Button>
      </div>
    </form>
  )
}

/**
 * Cập nhật link (kiểu IDM "Refresh download address"): link ký hết hạn / cần đăng nhập lại thì dán link
 * mới, hoặc tải lại file từ trình duyệt. App chỉ nhận khi cùng file và giữ phần đã tải.
 */
export function RefreshLinkDialog({
  item,
  desktop,
  onClose,
}: {
  item: DownloadItem | null
  desktop: boolean
  onClose: () => void
}) {
  return (
    <Dialog
      open={item !== null}
      onOpenChange={(open) => (open ? null : onClose())}
    >
      <DialogContent
        data-variant="soniox"
        data-s="S-21"
        className="sm:max-w-[520px]"
      >
        <DialogHeader>
          <DialogTitle>{t("recover.dialog.title")}</DialogTitle>
          <DialogDescription>{t("recover.dialog.description")}</DialogDescription>
        </DialogHeader>
        {item ? (
          <RefreshLinkForm
            key={item.id}
            item={item}
            desktop={desktop}
            onClose={onClose}
          />
        ) : null}
      </DialogContent>
    </Dialog>
  )
}
