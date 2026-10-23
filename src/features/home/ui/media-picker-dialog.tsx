import { useState, type FormEvent } from "react"

import { AlertCircle, Download } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

import { mediaProbe } from "../api/downloads"
import type { AddMediaPayload, MediaProbe } from "../model/types"
import {
  FormatList,
  ProbeCard,
  ProbeSkeleton,
  SupportedSites,
  UrlField,
} from "./media-picker-parts"

export function MediaPickerDialog({
  open,
  onOpenChange,
  desktop,
  busy,
  error,
  onSubmit,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  desktop: boolean
  busy: boolean
  error: string | null
  onSubmit: (payload: AddMediaPayload) => void | Promise<void>
}) {
  const [url, setUrl] = useState("")
  const [probe, setProbe] = useState<MediaProbe | null>(null)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [localError, setLocalError] = useState<string | null>(null)

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
        if (text.trim()) void analyze(text)
      })
      .catch(() => undefined)
  }

  const analyze = async (value = url) => {
    if (!desktop) {
      setLocalError("Mở app UTD để phân tích và tải media.")
      return
    }
    if (!/^https?:\/\//i.test(value.trim())) {
      setLocalError("Nhập URL HTTP/HTTPS của video hoặc luồng media.")
      return
    }
    setLoading(true)
    setLocalError(null)
    setProbe(null)
    setSelectedId(null)
    try {
      const result = await mediaProbe(value.trim())
      setProbe(result)
      setSelectedId(
        result.formats.find((format) => format.recommended)?.id ??
          result.formats[0]?.id ??
          null,
      )
    } catch (reason) {
      setLocalError(reason instanceof Error ? reason.message : String(reason))
    } finally {
      setLoading(false)
    }
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
            onAnalyze={() => void analyze()}
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
          {localError || error ? (
            <p
              role="alert"
              className="rounded-lg bg-destructive/5 px-3 py-2 text-sm text-destructive"
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
