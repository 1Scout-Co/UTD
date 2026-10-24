import { useCallback, useEffect, useRef, useState, type FormEvent } from "react"

import { AlertCircle, Download } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

import {
  channelPageUrl,
  mediaProbe,
  subscribeMediaRequest,
  takeMediaRequest,
} from "../api/downloads"
import { isChannelPage } from "../model/media-pages"
import type { AddMediaPayload, MediaProbe } from "../model/types"
import {
  FormatList,
  ProbeCard,
  ProbeSkeleton,
  SupportedSites,
  UrlField,
} from "./media-picker-parts"

const DESKTOP_ONLY = "Mở app UTD để phân tích và tải media."

/**
 * Link trang video từ trình duyệt (chuột phải / popup): `onUrl` cần ổn định (useCallback). Link tới
 * trước khi kịp nghe (UTD vừa mở) được app giữ lại: lấy một lần ngay sau khi nghe.
 */
export function useMediaRequests(
  desktop: boolean,
  onUrl: (url: string) => void,
) {
  useEffect(() => {
    if (!desktop) return
    let disposed = false
    let unlisten: (() => void) | undefined
    subscribeMediaRequest((request) => {
      if (!disposed) onUrl(request.url)
    })
      .then(async (off) => {
        if (disposed) {
          off()
          return
        }
        unlisten = off
        const early = await takeMediaRequest()
        if (early && !disposed) onUrl(early.url)
      })
      .catch((reason: unknown) => console.error("UTD media request:", reason))
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [desktop, onUrl])
}

/**
 * `initialUrl`: mở sẵn link này và tự phân tích (từ hộp Thêm hoặc trình duyệt). Đổi link thì đổi `key`
 * để dựng lại hộp với trạng thái mới.
 */
export function MediaPickerDialog({
  open,
  onOpenChange,
  desktop,
  busy,
  error,
  initialUrl,
  onSubmit,
  onChannel,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  desktop: boolean
  busy: boolean
  error: string | null
  initialUrl?: string | null
  onSubmit: (payload: AddMediaPayload) => void | Promise<void>
  /** Link là kênh (dán vào đây, hoặc link rút gọn app thấy trỏ tới kênh): mở hộp Tải cả kênh. */
  onChannel: (url: string) => void
}) {
  const autoAnalyze = Boolean(initialUrl) && desktop
  const [url, setUrl] = useState(initialUrl ?? "")
  const [probe, setProbe] = useState<MediaProbe | null>(null)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [loading, setLoading] = useState(autoAnalyze)
  const [localError, setLocalError] = useState<string | null>(
    initialUrl && !desktop ? DESKTOP_ONLY : null,
  )
  const autoStarted = useRef(false)

  const selected =
    probe?.formats.find((format) => format.id === selectedId) ??
    probe?.formats[0] ??
    null

  const updateUrl = (value: string) => {
    setUrl(value)
    setProbe(null)
    setSelectedId(null)
    setLocalError(null)
  }

  const close = () => {
    if (!busy) {
      setLocalError(null)
      onOpenChange(false)
    }
  }

  const paste = () => {
    void navigator.clipboard
      ?.readText()
      .then((text) => {
        updateUrl(text.trim())
        if (text.trim()) analyze(text)
      })
      .catch(() => undefined)
  }

  // Chỉ đổi state sau `await`: gọi được từ effect tự phân tích bên dưới.
  const runProbe = useCallback(async (value: string) => {
    try {
      const result = await mediaProbe(value)
      setProbe(result)
      setSelectedId(
        result.formats.find((format) => format.recommended)?.id ??
          result.formats[0]?.id ??
          null,
      )
    } catch (reason) {
      const channel = channelPageUrl(reason)
      if (channel) onChannel(channel)
      else setLocalError(reason instanceof Error ? reason.message : String(reason))
    } finally {
      setLoading(false)
    }
  }, [onChannel])

  // Mở với link sẵn ⇒ phân tích ngay (ref chặn chạy hai lần ở StrictMode).
  useEffect(() => {
    if (!autoAnalyze || !initialUrl || autoStarted.current) return
    autoStarted.current = true
    void runProbe(initialUrl.trim())
  }, [autoAnalyze, initialUrl, runProbe])

  const analyze = (value = url) => {
    if (!desktop) {
      setLocalError(DESKTOP_ONLY)
      return
    }
    if (!/^https?:\/\//i.test(value.trim())) {
      setLocalError("Nhập URL HTTP/HTTPS của video hoặc luồng media.")
      return
    }
    if (isChannelPage(value)) {
      onChannel(value.trim())
      return
    }
    setLoading(true)
    setLocalError(null)
    setProbe(null)
    setSelectedId(null)
    void runProbe(value.trim())
  }

  const submit = (event: FormEvent) => {
    event.preventDefault()
    if (!probe || !selected) {
      setLocalError("Phân tích URL trước để chọn định dạng.")
      return
    }
    setLocalError(null)
    void onSubmit({
      url: probe.webpageUrl || url.trim(),
      title: probe.title,
      selector: selected.selector,
      audioFormat: selected.audioFormat ?? undefined,
      pageUrl: probe.webpageUrl || undefined,
    })
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent data-variant="soniox" className="sm:max-w-[600px]">
        <DialogHeader>
          <DialogTitle>Tải media</DialogTitle>
          <DialogDescription className="sr-only">
            Dán link video để chọn chất lượng và tải về.
          </DialogDescription>
        </DialogHeader>

        <form className="space-y-4" onSubmit={submit}>
          <UrlField
            url={url}
            disabled={busy || loading}
            loading={loading}
            onChange={updateUrl}
            onAnalyze={() => analyze()}
            onPaste={paste}
          />

          {loading ? <ProbeSkeleton /> : null}
          {!loading && !probe ? <SupportedSites /> : null}

          {probe ? (
            <>
              <ProbeCard probe={probe} />
              <FormatList
                formats={probe.formats}
                selectedId={selected?.id ?? null}
                onSelect={setSelectedId}
              />
              {!probe.ffmpegAvailable ? (
                <div className="flex items-start gap-2 rounded-lg bg-amber-50 px-3 py-2 text-sm text-amber-800">
                  <AlertCircle className="mt-0.5 size-4 shrink-0" />
                  Chưa có ffmpeg nên có thể không ghép được hình và tiếng. Tải ở
                  Cài đặt → Công cụ media.
                </div>
              ) : null}
            </>
          ) : null}
          {/* text-red-700: text-destructive trên nền này chỉ đạt 4,49:1, dưới mức 4,5:1 của WCAG AA. */}
          {localError || error ? (
            <p
              role="alert"
              className="rounded-lg bg-destructive/5 px-3 py-2 text-sm text-red-700"
            >
              {localError ?? error}
            </p>
          ) : null}

          <div className="flex items-center justify-end gap-2">
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
              disabled={busy || loading || !probe || !selected}
            >
              <Download />
              {busy ? "Đang thêm…" : "Tải về"}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}
