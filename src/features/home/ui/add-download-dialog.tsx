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

import { parseLinks } from "../model/batch-links"
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
          <div className="text-sm font-semibold">Đây là link video</div>
          <div className="mt-0.5 truncate text-xs">
            {host} · chọn chất lượng ở bước sau
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
        <div className="text-sm">
          Dán URL để xem trước tên file và máy chủ.
        </div>
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
          {name || "Tên file do máy chủ đề xuất"}
        </div>
        <div className="mt-0.5 truncate text-xs text-muted-foreground">
          {host || "URL chưa hợp lệ"}
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
  onBatch,
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
  /** Dán nhiều link một lúc: đóng hộp này, mở hộp Thêm hàng loạt với các link đó. */
  onBatch?: (urls: string[]) => void
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
      onOpenChange(false)
    }
  }

  const submit = (event: FormEvent) => {
    event.preventDefault()
    if (!url.trim()) {
      setLocalError("Nhập URL cần tải.")
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
      setLocalError("SHA-256 phải có đúng 64 ký tự hex.")
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
          <DialogTitle>Thêm download</DialogTitle>
          <DialogDescription>
            Dán URL, UTD kiểm tra máy chủ trước khi bắt đầu.
          </DialogDescription>
        </DialogHeader>

        <form className="space-y-4" onSubmit={submit}>
          <UrlPreview
            url={url}
            host={host}
            name={effectiveName}
            mediaPage={mediaPage}
          />

          <div className="space-y-2">
            <FieldLabel>URL</FieldLabel>
            <div className="flex gap-2">
              <Input
                aria-label="URL cần tải"
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
                Dán
              </Button>
            </div>
          </div>

          {mediaPage ? null : (
            <>
              <div className="grid gap-3 sm:grid-cols-2">
                <div className="space-y-1.5">
                  <FieldLabel>Tên file</FieldLabel>
                  <Input
                    aria-label="Tên file"
                    value={fileName}
                    onChange={(event) => {
                      setNameTouched(true)
                      setFileName(event.target.value)
                    }}
                    placeholder={suggestedName || "Tự lấy từ URL"}
                    disabled={busy}
                  />
                </div>
                <div className="space-y-1.5">
                  <FieldLabel>Lưu tại</FieldLabel>
                  <button
                    type="button"
                    disabled={!desktop || busy}
                    title={
                      desktop ? effectiveDir : "Chọn thư mục chỉ có trong app UTD"
                    }
                    onClick={() => {
                      if (!desktop) return
                      void openDialog({
                        directory: true,
                        multiple: false,
                        title: "Chọn thư mục tải",
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
                    className="flex h-9 w-full items-center gap-2 rounded-lg border border-input px-3 text-left text-sm text-muted-foreground transition-colors hover:border-foreground/25 disabled:cursor-not-allowed disabled:opacity-70"
                  >
                    <FolderOpen className="size-4 shrink-0" />
                    <span className="truncate">{effectiveDir || "Downloads\\UTD"}</span>
                  </button>
                </div>
              </div>

              <div className="space-y-1.5">
                <FieldLabel>SHA-256 (tuỳ chọn)</FieldLabel>
                <Input
                  aria-label="SHA-256 mong đợi"
                  value={sha256}
                  onChange={(event) => setSha256(event.target.value)}
                  placeholder="64 ký tự hex để xác minh sau khi tải"
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
              Huỷ
            </Button>
            <Button
              data-primary
              type="submit"
              disabled={busy || !url.trim()}
            >
              {mediaPage ? <Film /> : <Download />}
              {mediaPage ? "Tải video" : busy ? "Đang thêm…" : "Bắt đầu"}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}
