import { useEffect, useState } from "react"

import { Loader2, RefreshCw, Wrench } from "lucide-react"

import { Button } from "@/components/ui/button"

import {
  mediaToolsAuto,
  retryMediaTools,
  subscribeToolProgress,
  subscribeToolsAuto,
} from "../api/media-tools"
import type {
  MediaToolName,
  MediaToolProgress,
  MediaToolsAuto,
} from "../model/types"
import { percentOf, StageProgress } from "./stage-progress"

// App tự tải yt-dlp/deno/ffmpeg lần đầu (không hỏi) và tự cập nhật: thẻ nhỏ ở thanh bên + một dòng
// trong hộp Tải media. Không chặn dùng app; tắt được ở Cài đặt → Công cụ media.

/** Trạng thái tự tải + tiến độ byte của công cụ đang tải (sự kiện từ app). */
export function useToolsAuto(desktop: boolean) {
  const [state, setState] = useState<MediaToolsAuto | null>(null)
  const [progress, setProgress] = useState<MediaToolProgress | null>(null)

  useEffect(() => {
    if (!desktop) return
    let disposed = false
    const stops: Array<() => void> = []
    const keep = (stop: () => void) => {
      if (disposed) stop()
      else stops.push(stop)
    }
    void subscribeToolsAuto((value) => {
      if (!disposed) setState(value)
    })
      .then(keep)
      .catch(() => undefined)
    void subscribeToolProgress((value) => {
      if (!disposed) setProgress(value)
    })
      .then(keep)
      .catch(() => undefined)
    // Đọc sau khi nghe: không lỡ trạng thái đổi trước lúc giao diện sẵn sàng.
    void mediaToolsAuto()
      .then((value) => {
        if (!disposed) setState(value)
      })
      .catch(() => undefined)
    return () => {
      disposed = true
      stops.forEach((stop) => stop())
    }
  }, [desktop])

  return { state, setState, progress }
}

/** Giây còn lại tới lần thử lại (đếm lùi mỗi giây). */
function useSecondsUntil(at: number | null): number | null {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (at === null) return
    const timer = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(timer)
  }, [at])
  if (at === null) return null
  return Math.max(0, Math.ceil((at - now) / 1000))
}

type Shown = {
  tool: MediaToolName
  /** null = chưa biết % (đang lấy SHA-256, giải nén…). */
  percent: number | null
  text: string
  /** Lỗi: giây còn lại tới lần tự thử lại (null = đang thử lại). */
  wait: number | null
  failed: boolean
}

const STAGE_TEXT: Record<MediaToolProgress["stage"], string> = {
  checksum: "Chuẩn bị",
  download: "Đang tải",
  extract: "Giải nén",
  check: "Chạy thử",
  install: "Đang cài",
}

/** Việc đang làm, gói trong vài chữ; null = không có gì để hiện. */
function shown(
  state: MediaToolsAuto | null,
  progress: MediaToolProgress | null,
  seconds: number | null
): Shown | null {
  if (!state?.enabled || state.paused) return null
  const busy = state.active ?? state.updating
  if (busy) {
    const current = progress?.tool === busy ? progress : null
    const stage = current?.stage ?? "checksum"
    const percent =
      stage === "download"
        ? percentOf(current?.downloadedBytes ?? 0, current?.totalBytes ?? null)
        : stage === "checksum"
          ? null
          : 100
    const verb = state.active ? STAGE_TEXT[stage] : "Cập nhật"
    const text = `${verb} ${busy}`
    return { tool: busy, percent, text, wait: null, failed: false }
  }
  const next = state.failed[0] ?? state.pending[0]
  if (!next || !state.error) return null
  const wait = seconds !== null && seconds > 0 ? seconds : null
  const text = `Chưa tải được ${next}`
  return { tool: next, percent: null, text, wait, failed: true }
}

function RetryButton({ onDone }: { onDone: (state: MediaToolsAuto) => void }) {
  return (
    <Button
      type="button"
      variant="ghost"
      size="xs"
      className="-ml-2"
      onClick={() =>
        void retryMediaTools()
          .then(onDone)
          .catch(() => undefined)
      }
    >
      <RefreshCw />
      Thử lại
    </Button>
  )
}

/** Thẻ ở thanh bên (trên thẻ ổ đĩa) khi app đang tự tải công cụ, hoặc tải lỗi và đang chờ thử lại. */
export function ToolsAutoCard({ desktop }: { desktop: boolean }) {
  const { state, setState, progress } = useToolsAuto(desktop)
  const seconds = useSecondsUntil(state?.retryAt ?? null)
  const view = shown(state, progress, seconds)
  if (!view) return null
  return (
    <div
      role="status"
      data-testid="tools-auto"
      title={view.failed ? (state?.error ?? undefined) : undefined}
      className="space-y-2 rounded-xl bg-card p-3 shadow-soft ring-1 ring-foreground/5"
    >
      <div className="flex items-center justify-between gap-2 text-xs">
        <span className="inline-flex items-center gap-1.5 text-muted-foreground">
          <Wrench className="size-3.5" />
          Công cụ media
        </span>
        {view.percent !== null ? (
          <span className="font-semibold tabular-nums">{view.percent}%</span>
        ) : null}
      </div>
      {view.failed ? null : (
        <StageProgress
          percent={view.percent}
          label={`Tiến độ tải ${view.tool}`}
        />
      )}
      <p
        className={
          view.failed
            ? "text-xs text-amber-700"
            : "truncate text-xs text-muted-foreground"
        }
      >
        {view.text}
      </p>
      {view.failed ? (
        <div className="flex items-center gap-1 text-xs text-muted-foreground">
          {view.wait !== null ? (
            <>
              <RetryButton onDone={setState} />
              <span className="tabular-nums">hoặc chờ {view.wait} s</span>
            </>
          ) : (
            "Đang thử lại…"
          )}
        </div>
      ) : null}
    </div>
  )
}

/**
 * Dòng nhỏ trong hộp Tải media lúc đang phân tích mà yt-dlp/deno còn đang tự tải (lần đầu mở app):
 * cho biết vì sao phải chờ.
 */
export function ToolsAutoNote({ desktop }: { desktop: boolean }) {
  const { state, progress } = useToolsAuto(desktop)
  const seconds = useSecondsUntil(state?.retryAt ?? null)
  const view = shown(state, progress, seconds)
  const needed = view && (view.tool === "yt-dlp" || view.tool === "deno")
  if (!view || !needed || (!state?.active && !view.failed)) return null
  return (
    <p
      role="status"
      className="flex items-center gap-1.5 text-sm text-muted-foreground"
    >
      <Loader2 className="size-3.5 shrink-0 animate-spin" />
      {view.failed
        ? `${view.text}. UTD đang tự thử lại…`
        : `Lần đầu dùng: ${view.text.toLowerCase()}${view.percent !== null ? ` ${view.percent}%` : ""}…`}
    </p>
  )
}
