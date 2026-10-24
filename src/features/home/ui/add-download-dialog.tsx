import { useMemo, useState, type FormEvent, type ReactNode } from "react"

import { open as openDialog } from "@tauri-apps/plugin-dialog"
import { Download, Film, FolderOpen, Link2 } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { t } from "@/shared/i18n/i18n"

import { fileNameFromUrl } from "../model/download-mapper"
import { isMediaPage } from "../model/media-pages"
import type { AddDownloadPayload } from "../model/types"

function FieldLabel({ children }: { children: ReactNode }) {
  return (
    <div className="text-sm font-medium">
      {children}
    </div>
  )
}

/** Thẻ trên ô URL: gợi ý dán, tên file + máy chủ, hoặc báo đây là link video (chuyển sang Tải media). */
function UrlPreview({
  url,
  host,
  name,
  mediaPage,
}: {
  url: string
  host: string
  name: string
  mediaPage: boolean
}) {
  if (mediaPage) {
    return (
      <div
        role="status"
        className="flex items-center gap-3 rounded-xl bg-accent p-3 text-accent-foreground"
      >
        <div className="grid size-10 place-items-center rounded-lg bg-card text-primary shadow-soft">
          <Film className="size-4" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-sm font-semibold">{t("add.videoLink")}</div>
          <div className="mt-0.5 truncate text-xs">
            {t("add.videoLinkHint", { host })}
          </div>
        </div>
      </div>
    )
  }
  if (!url.trim()) {
    return (
      <div className="flex items-center gap-3 rounded-xl bg-muted p-3 text-muted-foreground">
        <div className="grid size-10 place-items-center rounded-lg bg-card shadow-soft">
          <Link2 className="size-4" />
        </div>
        <div className="text-sm">{t("add.previewHint")}</div>
      </div>
    )
  }
  return (
    <div className="flex items-center gap-3 rounded-xl bg-muted p-3">
      <div className="grid size-10 place-items-center rounded-lg bg-card text-primary shadow-soft">
        <Link2 className="size-4" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-semibold">
          {name || t("add.serverName")}
        </div>
        <div className="mt-0.5 truncate text-xs text-muted-foreground">
          {host || t("add.invalidUrl")}
        </div>
      </div>
    </div>
  )
}

export function AddDownloadDialog({
  open,
  onOpenChange,
  defaultDir,
  desktop,
  busy,
  error,
  onSubmit,
  onMedia,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  defaultDir: string
  desktop: boolean
  busy: boolean
  error: string | null
  onSubmit: (payload: AddDownloadPayload) => void | Promise<void>
  /** Link trang video: đóng hộp này, mở hộp Tải media với link đó. */
  onMedia: (url: string) => void
}) {
  const [url, setUrl] = useState("")
  const [fileName, setFileName] = useState("")
  const [nameTouched, setNameTouched] = useState(false)
  const [sha256, setSha256] = useState("")
  const [destDir, setDestDir] = useState<string | null>(null)
  const [localError, setLocalError] = useState<string | null>(null)

  const host = useMemo(() => {
    try {
      return new URL(url).hostname.replace(/^www\./, "")
    } catch {
      return ""
    }
  }, [url])

  const mediaPage = isMediaPage(url)
  const suggestedName = fileNameFromUrl(url)
  const effectiveName = fileName.trim() || suggestedName
  const effectiveDir = destDir ?? defaultDir

  const updateUrl = (value: string) => {
    setLocalError(null)
    setUrl(value)
    if (!nameTouched) setFileName(fileNameFromUrl(value))
  }

  const close = () => {
    if (!busy) {
      setLocalError(null)
      onOpenChange(false)
    }
  }

  const submit = (event: FormEvent) => {
    event.preventDefault()
    if (!url.trim()) {
      setLocalError(t("add.needUrl"))
      return
    }
    if (mediaPage) {
      onMedia(url.trim())
      setUrl("")
      setFileName("")
      setNameTouched(false)
      return
    }
    const hash = sha256.trim()
    if (hash && !/^[a-fA-F0-9]{64}$/.test(hash)) {
      setLocalError(t("add.badHash"))
      return
    }
    setLocalError(null)
    void onSubmit({
      url: url.trim(),
      destDir: effectiveDir || undefined,
      fileName: fileName.trim() || undefined,
      expectedSha256: hash ? hash.toLowerCase() : undefined,
    })
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent data-variant="soniox" className="sm:max-w-[600px]">
        <DialogHeader>
          <DialogTitle>{t("add.title")}</DialogTitle>
          <DialogDescription>{t("add.description")}</DialogDescription>
        </DialogHeader>

        <form className="space-y-4" onSubmit={submit}>
          <UrlPreview
            url={url}
            host={host}
            name={effectiveName}
            mediaPage={mediaPage}
          />

          <div className="space-y-2">
            <FieldLabel>{t("add.url")}</FieldLabel>
            <div className="flex gap-2">
              <Input
                aria-label={t("add.urlLabel")}
                value={url}
                onChange={(event) => updateUrl(event.target.value)}
                placeholder="https://example.com/file.iso"
                disabled={busy}
                autoComplete="off"
              />
              <Button
                variant="outline"
                type="button"
                className="rounded-lg"
                disabled={busy}
                onClick={() => {
                  void navigator.clipboard
                    ?.readText()
                    .then(updateUrl)
                    .catch(() => undefined)
                }}
              >
                <Link2 />
                {t("app.paste")}
              </Button>
            </div>
          </div>

          {mediaPage ? null : (
            <>
              <div className="grid gap-3 sm:grid-cols-2">
                <div className="space-y-1.5">
                  <FieldLabel>{t("add.fileName")}</FieldLabel>
                  <Input
                    aria-label={t("add.fileName")}
                    value={fileName}
                    onChange={(event) => {
                      setNameTouched(true)
                      setFileName(event.target.value)
                    }}
                    placeholder={suggestedName || t("add.fileNameAuto")}
                    disabled={busy}
                  />
                </div>
                <div className="space-y-1.5">
                  <FieldLabel>{t("add.saveTo")}</FieldLabel>
                  <button
                    type="button"
                    disabled={!desktop || busy}
                    title={
                      desktop ? effectiveDir : t("add.pickDesktopOnly")
                    }
                    onClick={() => {
                      if (!desktop) return
                      void openDialog({
                        directory: true,
                        multiple: false,
                        title: t("app.pickFolder"),
                        defaultPath: effectiveDir || undefined,
                      })
                        .then((picked) => {
                          if (typeof picked === "string") setDestDir(picked)
                        })
                        .catch((reason: unknown) =>
                          setLocalError(
                            reason instanceof Error
                              ? reason.message
                              : String(reason),
                          ),
                        )
                    }}
                    className="flex h-9 w-full items-center gap-2 rounded-lg border border-input px-3 text-start text-sm text-muted-foreground transition-colors hover:border-foreground/25 disabled:cursor-not-allowed disabled:opacity-70"
                  >
                    <FolderOpen className="size-4 shrink-0" />
                    <span className="truncate">{effectiveDir || "Downloads\\UTD"}</span>
                  </button>
                </div>
              </div>

              <div className="space-y-1.5">
                <FieldLabel>{t("add.hash")}</FieldLabel>
                <Input
                  aria-label={t("add.hashLabel")}
                  value={sha256}
                  onChange={(event) => setSha256(event.target.value)}
                  placeholder={t("add.hashPlaceholder")}
                  disabled={busy}
                  spellCheck={false}
                  autoComplete="off"
                  className="font-mono"
                />
              </div>
            </>
          )}

          {/* text-red-700: text-destructive trên nền này chỉ đạt 4,49:1, dưới mức 4,5:1 của WCAG AA. */}
          {localError || error ? (
            <p
              role="alert"
              className="rounded-lg bg-destructive/5 px-3 py-2 text-sm text-red-700"
            >
              {localError ?? error}
            </p>
          ) : null}

          <div className="flex justify-end gap-2 pt-1">
            <Button
              variant="ghost"
              type="button"
              onClick={close}
              disabled={busy}
            >
              {t("app.cancel")}
            </Button>
            <Button
              data-primary
              type="submit"
              disabled={busy || !url.trim()}
            >
              {mediaPage ? <Film /> : <Download />}
              {mediaPage
                ? t("add.downloadVideo")
                : busy
                  ? t("app.adding")
                  : t("add.start")}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}
