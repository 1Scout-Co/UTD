import { useId } from "react"

import {
  CheckCircle2,
  Download,
  Loader2,
  RefreshCw,
  ShieldCheck,
  X,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { formatBytes } from "@/shared/i18n/format"
import { t, tx } from "@/shared/i18n/i18n"
import { isMac } from "@/shared/platform"

import type {
  MediaToolInfo,
  MediaToolName,
  MediaToolProgress,
  MediaToolSource,
  MediaToolStage,
} from "../model/types"
import { percentOf, StageProgress } from "./stage-progress"

// Các mảnh hiển thị của mục "Công cụ media" trong Cài đặt (logic nằm ở settings-media-tools.tsx).

// role/license là hàm: chữ theo ngôn ngữ đang chọn lúc vẽ.
export interface ToolMeta {
  name: MediaToolName
  role: () => string
  /** Nơi tải: mặc định GitHub (`repo` là tên kho). */
  site?: string
  repo: string
  license: () => string
  size: string
}

/**
 * Thông tin hiện trước khi tải: bấm nút = đồng ý tải từ nguồn này. Theo bảng nguồn của app
 * (`src-tauri/src/media/sources.rs`): Windows dùng bản `.exe`/zip, macOS bản `yt-dlp_macos`, zip deno của
 * Apple và ffmpeg + ffprobe của Martin Riedl. Kích thước đo ngày 2026-10-24.
 */
export const TOOLS: ToolMeta[] = [
  {
    name: "yt-dlp",
    role: () => t("tools.ytdlpRole"),
    repo: "yt-dlp/yt-dlp",
    license: () =>
      isMac ? t("tools.ytdlpLicenseMac") : t("tools.ytdlpLicense"),
    size: isMac ? "~35 MB" : "~17 MB",
  },
  {
    name: "deno",
    role: () => t("tools.denoRole"),
    repo: "denoland/deno",
    license: () => "MIT",
    size: isMac ? "~40 MB" : "~43 MB",
  },
  isMac
    ? {
        name: "ffmpeg",
        role: () => t("tools.ffmpegRole"),
        site: "Martin Riedl",
        repo: "ffmpeg.martin-riedl.de",
        license: () => t("tools.ffmpegLicense"),
        size: "~60 MB",
      }
    : {
        name: "ffmpeg",
        role: () => t("tools.ffmpegRole"),
        repo: "yt-dlp/FFmpeg-Builds",
        license: () => t("tools.ffmpegLicense"),
        size: "~191 MB",
      },
]

// Bản của UTD: t("tools.sourceUtd"); còn lại là tên riêng.
const SOURCE_LABEL: Record<Exclude<MediaToolSource, "utd">, string> = {
  path: "PATH",
  winget: "WinGet",
  scoop: "Scoop",
  choco: "Chocolatey",
}

/** loading: đang hỏi app; failed: hỏi lỗi (chưa biết có công cụ hay không). */
export type ToolCheck = "loading" | "failed" | "ready"

export function ToolState({
  info,
  desktop,
  check,
}: {
  info: MediaToolInfo | null
  desktop: boolean
  check: ToolCheck
}) {
  if (!desktop) {
    return (
      <p className="text-xs text-muted-foreground">{t("tools.notChecked")}</p>
    )
  }
  if (check === "loading") {
    return (
      <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
        <Loader2 className="size-3.5 animate-spin" />
        {t("app.checking")}
      </p>
    )
  }
  if (check === "failed") {
    return (
      <p className="text-xs text-muted-foreground">{t("tools.checkFailed")}</p>
    )
  }
  if (!info) {
    return <p className="text-xs text-amber-700">{t("tools.missing")}</p>
  }
  const source = info.source === "utd" ? null : SOURCE_LABEL[info.source]
  return (
    <p
      className="flex min-w-0 items-center gap-1.5 text-xs text-emerald-700"
      title={info.path}
    >
      <CheckCircle2 className="size-3.5 shrink-0" />
      <span className="truncate">
        {info.version} ·{" "}
        {source ? t("tools.usingLocal", { source }) : t("tools.sourceUtd")}
      </span>
    </p>
  )
}

export function ConsentNote({
  meta,
  info,
}: {
  meta: ToolMeta
  info: MediaToolInfo | null
}) {
  const lead = !info
    ? null
    : info.source === "utd"
      ? t("tools.keepBackup")
      : t("tools.separateCopy")
  return (
    <p className="flex items-start gap-1.5 text-xs leading-relaxed text-muted-foreground">
      <ShieldCheck className="mt-0.5 size-3.5 shrink-0" />
      <span>
        {lead ? `${lead} ` : null}
        {tx("tools.consent", {
          site: meta.site ?? "GitHub",
          repo: <code>{meta.repo}</code>,
          license: meta.license(),
          size: <span className="whitespace-nowrap">{meta.size}</span>,
          sha: <span className="whitespace-nowrap">SHA-256</span>,
        })}
      </span>
    </p>
  )
}

/** Tải xong (giải nén, chạy thử, cài) thì thanh giữ đầy, không tụt về rỗng. */
function percentAt(
  stage: MediaToolStage,
  done: number,
  total: number | null
): number | null {
  if (stage === "checksum") return null
  if (stage === "download") return percentOf(done, total)
  return 100
}

export function InstallProgress({
  tool,
  progress,
}: {
  tool: MediaToolName
  progress: MediaToolProgress | null
}) {
  const current = progress?.tool === tool ? progress : null
  const stage = current?.stage ?? "checksum"
  const total = current?.totalBytes ?? null
  const done = current?.downloadedBytes ?? 0
  const sized = stage === "download" && percentOf(done, total) !== null
  return (
    <div className="space-y-1">
      <StageProgress
        percent={percentAt(stage, done, total)}
        label={t("tools.progress", { tool })}
      />
      <p className="text-xs text-muted-foreground tabular-nums">
        {t(`tools.stage.${stage}`)}
        {sized && total !== null
          ? ` ${formatBytes(done)} / ${formatBytes(total)}`
          : ""}
      </p>
    </div>
  )
}

export function ToolAction({
  meta,
  info,
  running,
  locked,
  onInstall,
  onCancel,
}: {
  meta: ToolMeta
  info: MediaToolInfo | null
  running: boolean
  locked: boolean
  onInstall: () => void
  onCancel: () => void
}) {
  if (running) {
    return (
      <Button type="button" variant="ghost" onClick={onCancel}>
        <X />
        {t("app.cancel")}
      </Button>
    )
  }
  if (info && meta.name !== "yt-dlp") return null
  const Icon = info ? RefreshCw : Download
  // Ghi rõ tên công cụ: cùng hộp thoại còn nút "Cập nhật" của chính UTD.
  return (
    <Button
      type="button"
      variant="outline"
      disabled={locked}
      onClick={onInstall}
    >
      <Icon />
      {info
        ? t("tools.update", { tool: meta.name })
        : t("tools.download", { tool: meta.name })}
    </Button>
  )
}

/** Công tắc tự tải công cụ còn thiếu và tự cập nhật yt-dlp/deno của UTD (mặc định bật). */
export function AutoTools({
  enabled,
  disabled,
  onChange,
}: {
  enabled: boolean
  disabled: boolean
  onChange: (enabled: boolean) => void
}) {
  const id = useId()
  return (
    <div className="flex items-center justify-between gap-3 rounded-xl bg-muted p-3.5">
      <div className="min-w-0">
        <p id={`${id}-label`} className="text-sm font-semibold">
          {t("tools.auto")}
        </p>
        <p id={`${id}-hint`} className="text-xs text-muted-foreground">
          {t("tools.autoHint")}
        </p>
      </div>
      <Switch
        aria-labelledby={`${id}-label`}
        aria-describedby={`${id}-hint`}
        checked={enabled}
        disabled={disabled}
        onCheckedChange={(checked) => onChange(checked)}
      />
    </div>
  )
}
