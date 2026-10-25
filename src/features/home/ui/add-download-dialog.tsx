import { useMemo, useState, type FormEvent, type ReactNode } from "react"

import { open as openDialog } from "@tauri-apps/plugin-dialog"
import { Download, Film, FolderOpen, Link2, ListVideo } from "lucide-react"

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

import { preflightLink } from "../api/detail"
import { parseLinks } from "../model/batch-links"
import { fileNameFromUrl } from "../model/download-mapper"
import { isChannelPage, isMediaPage } from "../model/media-pages"
import type { AddDownloadPayload } from "../model/types"
import type { WarnedLink } from "../model/warnings"
import { SingleWarning } from "./warning-panel"

function FieldLabel({ children }: { children: ReactNode }) {
  return (
    <div className="text-sm font-medium">
      {children}
    </div>
  )
}

/**
 * Thẻ trên ô URL: gợi ý dán, tên file + máy chủ, hoặc báo đây là link video (chuyển sang Tải media) / link
 * kênh (chuyển sang Tải cả kênh).
 */
function UrlPreview({
  url,
  host,
  name,
  mediaPage,
  channelPage,
}: {
  url: string
  host: string
  name: string
  mediaPage: boolean
  channelPage: boolean
}) {
  if (mediaPage || channelPage) {
    const Icon = channelPage ? ListVideo : Film
    return (
      <div
        role="status"
        className="flex items-center gap-3 rounded-xl bg-accent p-3 text-accent-foreground"
      >
        <div className="grid size-10 place-items-center rounded-lg bg-card text-primary shadow-soft">
          <Icon className="size-4" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-sm font-semibold">
            {channelPage ? t("add.channelLink") : t("add.videoLink")}
          </div>
          <div className="mt-0.5 truncate text-xs">
            {channelPage
              ? t("add.channelLinkHint", { host })
              : t("add.videoLinkHint", { host })}
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

/**
 * S-22 trước khi bắt đầu: kiểm link (dò tên + kiểu nội dung, luật của Download Guard); có cảnh báo thì giữ
 * lại để người dùng chọn Vẫn tải / Huỷ, không có thì gửi luôn. Bản xem trước trên trình duyệt không kiểm.
 */
function usePreflight(
  desktop: boolean,
  onSubmit: (payload: AddDownloadPayload) => void | Promise<void>
) {
  const [checking, setChecking] = useState(false)
  const [warning, setWarning] = useState<{
    link: WarnedLink
    payload: AddDownloadPayload
  } | null>(null)
  const start = async (payload: AddDownloadPayload) => {
    if (!desktop) return onSubmit(payload)
    setChecking(true)
    const link = await preflightLink(payload.url, payload.fileName).catch(() => null)
    setChecking(false)
    if (link) setWarning({ link, payload })
    else await onSubmit(payload)
  }
  const proceed = () => {
    if (!warning) return
    setWarning(null)
    void onSubmit(warning.payload)
  }
  return { checking, warning, start, proceed, reset: () => setWarning(null) }
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
  onBatch,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  defaultDir: string
  desktop: boolean
  busy: boolean
  error: string | null
  onSubmit: (payload: AddDownloadPayload) => void | Promise<void>
  /** Link trang video hoặc kênh: đóng hộp này, mở hộp Tải media / Tải cả kênh với link đó. */
  onMedia: (url: string) => void
  /** Dán nhiều link một lúc: đóng hộp này, mở hộp Thêm hàng loạt với các link đó. */
  onBatch?: (urls: string[]) => void
}) {
  const [url, setUrl] = useState("")
  const [fileName, setFileName] = useState("")
  const [nameTouched, setNameTouched] = useState(false)
  const [sha256, setSha256] = useState("")
  const [destDir, setDestDir] = useState<string | null>(null)
  const [localError, setLocalError] = useState<string | null>(null)
  const preflight = usePreflight(desktop, onSubmit)

  const host = useMemo(() => {
    try {
      return new URL(url).hostname.replace(/^www\./, "")
    } catch {
      return ""
    }
  }, [url])

  const channelPage = isChannelPage(url)
  // Link kênh cũng rời hộp này (sang Tải cả kênh): như link video, không có tên file/thư mục.
  const mediaPage = isMediaPage(url) || channelPage
  const suggestedName = fileNameFromUrl(url)
  const effectiveName = fileName.trim() || suggestedName
  const effectiveDir = destDir ?? defaultDir

  const updateUrl = (value: string) => {
    setLocalError(null)
    setUrl(value)
    if (!nameTouched) setFileName(fileNameFromUrl(value))
  }

  // Chữ dán có từ 2 link trở lên ⇒ hộp Thêm hàng loạt (ô một dòng làm mất dấu xuống dòng).
  const toBatch = (text: string) => {
    const links = parseLinks(text).links
    if (links.length < 2 || !onBatch) return false
    setUrl("")
    onBatch(links)
    return true
  }

  const close = () => {
    if (!busy) {
      setLocalError(null)
      preflight.reset()
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
    void preflight.start({
      url: url.trim(),
      destDir: effectiveDir || undefined,
      fileName: fileName.trim() || undefined,
      expectedSha256: hash ? hash.toLowerCase() : undefined,
    })
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) preflight.reset()
        onOpenChange(next)
      }}
    >
      <DialogContent data-variant="soniox" className="sm:max-w-[600px]">
        <DialogHeader>
          <DialogTitle>{t("add.title")}</DialogTitle>
          <DialogDescription>{t("add.description")}</DialogDescription>
        </DialogHeader>

        {preflight.warning ? (
          <SingleWarning
            link={preflight.warning.link}
            busy={busy}
            onProceed={preflight.proceed}
            onCancel={close}
          />
        ) : null}
        {/* Đang hỏi S-22: giấu ô nhập (giữ nguyên chữ đã gõ), hiện lại khi Vẫn tải mà lỗi. */}
        <form className="space-y-4" hidden={Boolean(preflight.warning)} onSubmit={submit}>
          <UrlPreview
            url={url}
            host={host}
            name={effectiveName}
            mediaPage={mediaPage}
            channelPage={channelPage}
          />

          <div className="space-y-2">
            <FieldLabel>{t("add.url")}</FieldLabel>
            <div className="flex gap-2">
              <Input
                aria-label={t("add.urlLabel")}
                value={url}
                onChange={(event) => updateUrl(event.target.value)}
                onPaste={(event) => {
                  const text = event.clipboardData.getData("text")
                  if (/\n/.test(text) && toBatch(text)) event.preventDefault()
                }}
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
                    .then((text) => toBatch(text) || updateUrl(text.trim()))
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
              disabled={busy || preflight.checking || !url.trim()}
            >
              {channelPage ? <ListVideo /> : mediaPage ? <Film /> : <Download />}
              {channelPage
                ? t("add.downloadChannel")
                : mediaPage
                  ? t("add.downloadVideo")
                  : busy
                    ? t("app.adding")
                    : preflight.checking
                      ? t("app.checking")
                      : t("add.start")}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}
