import { useEffect, useRef, useState } from "react"

import { Info, RefreshCw } from "lucide-react"

import { Button } from "@/components/ui/button"
import { commandErrorText, t } from "@/shared/i18n/i18n"

import {
  cancelMediaToolInstall,
  installMediaTool,
  mediaToolsStatus,
  setMediaToolsAuto,
  subscribeToolProgress,
} from "../api/media-tools"
import type {
  MediaToolName,
  MediaToolProgress,
  MediaToolsAuto,
  MediaToolsStatus,
} from "../model/types"
import {
  AutoTools,
  ConsentNote,
  InstallProgress,
  ToolAction,
  type ToolCheck,
  TOOLS,
  ToolState,
} from "./settings-media-tools-parts"
import { useToolsAuto } from "./tools-auto"

/** own = false: lần cài bắt đầu trước khi mở lại Cài đặt, theo dõi bằng cách hỏi lại. */
type Busy = { tool: MediaToolName; own: boolean }

function infoOf(status: MediaToolsStatus | null, tool: MediaToolName) {
  if (tool === "yt-dlp") return status?.ytDlp ?? null
  return (tool === "deno" ? status?.deno : status?.ffmpeg) ?? null
}

/** Tiến độ tải công cụ (sự kiện từ app). */
function useToolProgress(desktop: boolean) {
  const [progress, setProgress] = useState<MediaToolProgress | null>(null)
  useEffect(() => {
    if (!desktop) return
    let disposed = false
    let unlisten: (() => void) | undefined
    void subscribeToolProgress((value) => {
      if (!disposed) setProgress(value)
    })
      .then((stop) => {
        if (disposed) stop()
        else unlisten = stop
      })
      .catch(() => undefined)
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [desktop])
  return [progress, setProgress] as const
}

/** Trạng thái công cụ + tiến độ tải. */
function useToolStatus(desktop: boolean) {
  const [status, setStatus] = useState<MediaToolsStatus | null>(null)
  const [progress, setProgress] = useToolProgress(desktop)
  const [busy, setBusy] = useState<Busy | null>(null)
  const [error, setError] = useState<string | null>(null)
  // Lần hỏi trạng thái thứ mấy: bấm "Thử lại" thì tăng để hỏi lại.
  const [attempt, setAttempt] = useState(0)

  useEffect(() => {
    if (!desktop) return
    let disposed = false
    void mediaToolsStatus()
      .then((value) => {
        if (disposed) return
        setStatus(value)
        if (value.installing) setBusy({ tool: value.installing, own: false })
      })
      .catch((reason: unknown) => {
        if (!disposed) setError(commandErrorText(reason))
      })
    return () => {
      disposed = true
    }
  }, [desktop, attempt])

  const retry = () => {
    setError(null)
    setAttempt((value) => value + 1)
  }

  const adopted = busy !== null && !busy.own
  useEffect(() => {
    if (!adopted) return
    const timer = window.setInterval(() => {
      void mediaToolsStatus()
        .then((value) => {
          setStatus(value)
          if (!value.installing) {
            setBusy(null)
            setProgress(null)
          }
        })
        .catch(() => undefined)
    }, 1500)
    return () => window.clearInterval(timer)
  }, [adopted, setProgress])

  return {
    status,
    setStatus,
    progress,
    setProgress,
    busy,
    setBusy,
    error,
    setError,
    retry,
  }
}

/** App tự tải công cụ khi Cài đặt đang mở: theo dõi như lần cài bắt đầu từ trước (hỏi lại đến khi xong). */
function useAutoTools(desktop: boolean, state: ReturnType<typeof useToolStatus>) {
  const auto = useToolsAuto(desktop)
  const active = auto.state?.active ?? auto.state?.updating ?? null
  const { busy, setBusy } = state
  useEffect(() => {
    if (active && !busy) setBusy({ tool: active, own: false })
  }, [active, busy, setBusy])

  const toggle = (enabled: boolean) => {
    void setMediaToolsAuto(enabled)
      .then(auto.setState)
      .catch((reason: unknown) => state.setError(commandErrorText(reason)))
  }
  return { auto: auto.state, toggle }
}

function useMediaTools(desktop: boolean) {
  const state = useToolStatus(desktop)
  const { auto, toggle } = useAutoTools(desktop, state)
  const [notice, setNotice] = useState<string | null>(null)
  // Người dùng tự bấm Huỷ: báo nhẹ, không hiện như lỗi.
  const cancelled = useRef(false)

  const install = async (tool: MediaToolName) => {
    const before = infoOf(state.status, tool)?.version
    cancelled.current = false
    state.setBusy({ tool, own: true })
    state.setError(null)
    state.setProgress(null)
    setNotice(null)
    try {
      const next = await installMediaTool(tool)
      state.setStatus(next)
      const after = infoOf(next, tool)?.version ?? ""
      setNotice(
        after && after === before
          ? t("tools.upToDate", { tool, version: after })
          : t("tools.installed", { tool, version: after })
      )
    } catch (reason) {
      if (cancelled.current) setNotice(commandErrorText(reason))
      else state.setError(commandErrorText(reason))
    } finally {
      state.setBusy(null)
      state.setProgress(null)
    }
  }

  const cancel = () => {
    cancelled.current = true
    void cancelMediaToolInstall().catch(() => undefined)
  }

  return { ...state, auto, toggle, notice, install, cancel }
}

/** Mục "Công cụ media" trong Cài đặt: dùng bản có sẵn, chỉ tải khi thiếu (S-31). */
export function SettingsMediaTools({ desktop }: { desktop: boolean }) {
  const tools = useMediaTools(desktop)
  // Hỏi lỗi thì thôi quay: báo "chưa kiểm tra được" và cho bấm Thử lại.
  const check: ToolCheck =
    tools.status !== null ? "ready" : tools.error ? "failed" : "loading"
  const missing = TOOLS.some((meta) => !infoOf(tools.status, meta.name))

  return (
    <div className="space-y-2.5" data-s={missing ? "S-31" : undefined}>
      <AutoTools
        enabled={tools.auto?.enabled ?? true}
        disabled={!desktop || tools.auto === null}
        onChange={tools.toggle}
      />
      {TOOLS.map((meta) => {
        const info = infoOf(tools.status, meta.name)
        const running = tools.busy?.tool === meta.name
        const offered = !info || meta.name === "yt-dlp"
        return (
          <div
            key={meta.name}
            className="space-y-2.5 rounded-xl bg-muted p-3.5"
          >
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0">
                <p className="text-sm font-semibold">
                  {meta.name}{" "}
                  <span className="font-normal text-muted-foreground">
                    · {meta.role()}
                  </span>
                </p>
                <ToolState info={info} desktop={desktop} check={check} />
              </div>
              <ToolAction
                meta={meta}
                info={info}
                running={running}
                locked={!desktop || check !== "ready" || tools.busy !== null}
                onInstall={() => void tools.install(meta.name)}
                onCancel={tools.cancel}
              />
            </div>
            {running ? (
              <InstallProgress tool={meta.name} progress={tools.progress} />
            ) : null}
            {offered && !running ? (
              <ConsentNote meta={meta} info={info} />
            ) : null}
          </div>
        )
      })}
      <MediaToolsFooter desktop={desktop} binDir={tools.status?.binDir} />
      <MediaToolsMessages
        notice={tools.notice}
        error={tools.error ?? autoError(tools.auto)}
        onRetry={check === "failed" ? tools.retry : null}
      />
    </div>
  )
}

/** Tự tải lỗi (vd mất mạng) mà chưa có công cụ: nói rõ app sẽ tự thử lại. */
function autoError(auto: MediaToolsAuto | null): string | null {
  if (!auto?.enabled || auto.paused || !auto.error || !auto.pending.length) {
    return null
  }
  return t("toolsAuto.errorRetry", { error: commandErrorText(auto.error) })
}

/** Thông báo xong, lỗi, và nút Thử lại khi chưa hỏi được trạng thái công cụ. */
function MediaToolsMessages({
  notice,
  error,
  onRetry,
}: {
  notice: string | null
  error: string | null
  onRetry: (() => void) | null
}) {
  return (
    <>
      <p role="status" className="text-sm text-emerald-700 empty:hidden">
        {notice ?? ""}
      </p>
      {error ? (
        <p
          role="alert"
          className="rounded-lg bg-destructive/5 px-3 py-2 text-sm text-destructive"
        >
          {error}
        </p>
      ) : null}
      {onRetry ? (
        <Button type="button" variant="outline" onClick={onRetry}>
          <RefreshCw />
          {t("app.retry")}
        </Button>
      ) : null}
    </>
  )
}

function MediaToolsFooter({
  desktop,
  binDir,
}: {
  desktop: boolean
  binDir?: string
}) {
  if (!desktop) {
    return (
      <p className="flex items-center gap-1.5 text-sm text-muted-foreground">
        <Info className="size-3.5 shrink-0" />
        {t("tools.desktopOnly")}
      </p>
    )
  }
  if (!binDir) return null
  return (
    <p
      className="flex items-center gap-1.5 text-xs text-muted-foreground"
      title={binDir}
    >
      <span className="shrink-0">{t("tools.folder")}</span>
      <code className="truncate font-mono">{binDir}</code>
    </p>
  )
}
