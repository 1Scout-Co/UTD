import {
  Check,
  ClipboardPaste,
  Film,
  Globe2,
  Link2,
  Loader2,
  Sparkles,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { formatBitrate, formatBytes } from "@/shared/i18n/format"
import { t } from "@/shared/i18n/i18n"
import { cn } from "cn"

import type { EngineMediaFormat, MediaProbe } from "../model/types"
import { MEDIA_SOURCES, mediaSourceOf, SourceLogo } from "./source-logos"

function formatDuration(seconds: number | null): string {
  if (seconds === null || seconds <= 0) return "—"
  const hours = Math.floor(seconds / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  const rest = seconds % 60
  const pad = (value: number) => String(value).padStart(2, "0")
  return hours > 0
    ? `${hours}:${pad(minutes)}:${pad(rest)}`
    : `${minutes}:${pad(rest)}`
}

function hostOf(value: string): string {
  try {
    return new URL(value).hostname.replace(/^www\./, "")
  } catch {
    return value
  }
}

function formatMeta(format: EngineMediaFormat): string {
  const size = format.sizeBytes ? formatBytes(format.sizeBytes) : "—"
  const bitrate = format.bitrateKbps ? formatBitrate(format.bitrateKbps) : "—"
  return `${size} · ${bitrate}`
}

/** Nhãn của lựa chọn do Rust (media/probe.rs) đặt theo `id` ổn định: dịch theo id; id lạ thì giữ chữ gốc. */
function formatText(format: EngineMediaFormat, formats: EngineMediaFormat[]) {
  const { id } = format
  if (id === "best") {
    const heights = formats
      .map((item) => Number(item.id))
      .filter(Number.isInteger)
    const top = heights.length > 0 ? String(Math.max(...heights)) : null
    return {
      label: t("format.best"),
      detail: top
        ? t("format.videoAudioMax", { height: top })
        : t("format.videoAudio"),
      badge: t("format.recommended"),
    }
  }
  if (/^\d+$/.test(id)) {
    const badge = format.badge ? t("format.popular") : null
    return { label: format.label, detail: t("format.videoAudio"), badge }
  }
  if (id === "audio-m4a") {
    return {
      label: t("format.m4a"),
      detail: t("format.m4aDetail"),
      badge: null,
    }
  }
  if (id === "audio-mp3") {
    return {
      label: t("format.mp3"),
      detail: t("format.mp3Detail"),
      badge: null,
    }
  }
  return { label: format.label, detail: format.detail, badge: format.badge }
}

/** Ô dán link lớn: trống thì nút "Dán" (dán rồi phân tích luôn), có link thì nút "Phân tích". */
export function UrlField({
  url,
  disabled,
  loading,
  onChange,
  onAnalyze,
  onPaste,
}: {
  url: string
  disabled: boolean
  loading: boolean
  onChange: (value: string) => void
  onAnalyze: () => void
  onPaste: () => void
}) {
  return (
    <div className="flex h-12 items-center gap-2 rounded-xl bg-muted ps-3.5 pe-1.5 transition-shadow focus-within:bg-card focus-within:shadow-soft focus-within:ring-2 focus-within:ring-ring/40">
      <Link2 className="size-4 shrink-0 text-muted-foreground" />
      <Input
        aria-label={t("media.urlLabel")}
        value={url}
        autoFocus
        autoComplete="off"
        spellCheck={false}
        disabled={disabled}
        placeholder={t("media.urlPlaceholder")}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key !== "Enter" || !url.trim()) return
          event.preventDefault()
          onAnalyze()
        }}
        className="h-full border-0 bg-transparent px-0 shadow-none focus-visible:border-transparent focus-visible:ring-0 disabled:bg-transparent dark:bg-transparent"
      />
      {url.trim() ? (
        <Button
          type="button"
          variant="outline"
          disabled={disabled}
          onClick={onAnalyze}
        >
          {loading ? <Loader2 className="animate-spin" /> : <Sparkles />}
          {t("media.analyze")}
        </Button>
      ) : (
        <Button
          type="button"
          variant="outline"
          disabled={disabled}
          onClick={onPaste}
        >
          <ClipboardPaste />
          {t("app.paste")}
        </Button>
      )}
    </div>
  )
}

/** Hàng logo các trang hỗ trợ (thay cho dòng mô tả dài). */
export function SupportedSites() {
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      <span className="me-1 text-xs text-muted-foreground">
        {t("media.supported")}
      </span>
      {MEDIA_SOURCES.map((source) => (
        <span
          key={source.name}
          title={source.name}
          className="grid size-7 place-items-center rounded-lg bg-card shadow-soft ring-1 ring-foreground/5"
        >
          <SourceLogo
            source={source}
            label={source.name}
            className="size-3.5"
          />
        </span>
      ))}
      <span className="ms-1 text-xs text-muted-foreground">
        {t("media.alsoDirect")}
      </span>
    </div>
  )
}

/** Khung chờ trong lúc yt-dlp phân tích link. */
export function ProbeSkeleton() {
  return (
    <div role="status" className="space-y-2">
      <span className="sr-only">{t("media.analyzing")}</span>
      <div className="flex gap-3 rounded-xl bg-muted p-2.5">
        <div className="aspect-video w-32 shrink-0 animate-pulse rounded-lg bg-card" />
        <div className="flex-1 space-y-2 self-center">
          <div className="h-3.5 w-3/4 animate-pulse rounded bg-card" />
          <div className="h-3 w-1/3 animate-pulse rounded bg-card" />
        </div>
      </div>
      {[0, 1, 2].map((index) => (
        <div key={index} className="h-14 animate-pulse rounded-lg bg-muted" />
      ))}
    </div>
  )
}

export function ProbeCard({ probe }: { probe: MediaProbe }) {
  const host = hostOf(probe.webpageUrl)
  const source = mediaSourceOf(host)
  return (
    <div className="flex gap-3 rounded-xl bg-muted p-2.5">
      <div className="relative aspect-video w-32 shrink-0 overflow-hidden rounded-lg bg-card">
        {probe.thumbnail ? (
          <img
            src={probe.thumbnail}
            alt=""
            referrerPolicy="no-referrer"
            className="size-full object-cover"
          />
        ) : (
          <div className="grid size-full place-items-center">
            <Film className="size-6 text-primary/60" />
          </div>
        )}
      </div>
      <div className="min-w-0 flex-1 self-center">
        <div
          dir="auto"
          className="line-clamp-2 [text-align:match-parent] text-sm font-semibold break-words"
          title={probe.title}
        >
          {probe.title}
        </div>
        <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
          <span className="inline-flex items-center gap-1.5">
            {source ? (
              <SourceLogo source={source} className="size-3.5" />
            ) : (
              <Globe2 className="size-3.5" />
            )}
            {host}
          </span>
          <span>·</span>
          <span>{formatDuration(probe.durationSeconds)}</span>
          {probe.uploader ? (
            <>
              <span>·</span>
              <span className="max-w-56 truncate">{probe.uploader}</span>
            </>
          ) : null}
        </div>
      </div>
    </div>
  )
}

/** Danh sách chất lượng gọn: mỗi dòng một lựa chọn, dòng đang chọn nổi lên bằng bóng + viền xanh. */
export function FormatList({
  formats,
  selectedId,
  onSelect,
}: {
  formats: EngineMediaFormat[]
  selectedId: string | null
  onSelect: (id: string) => void
}) {
  return (
    <div className="-mx-1 max-h-64 space-y-1.5 overflow-y-auto px-1 py-0.5">
      {formats.map((format) => {
        const active = selectedId === format.id
        const text = formatText(format, formats)
        return (
          <button
            key={format.id}
            type="button"
            onClick={() => onSelect(format.id)}
            aria-pressed={active}
            className={cn(
              "flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-start transition-colors",
              active
                ? "bg-card shadow-soft ring-2 ring-primary"
                : "bg-muted/60 hover:bg-muted"
            )}
          >
            <span
              className={cn(
                "grid size-4 shrink-0 place-items-center rounded-full border",
                active
                  ? "border-primary bg-primary text-primary-foreground"
                  : "border-muted-foreground/40"
              )}
            >
              {active ? <Check className="size-2.5" /> : null}
            </span>
            <span className="min-w-0 flex-1">
              <span className="flex items-center gap-2 text-sm font-semibold">
                {text.label}
                {text.badge ? (
                  <span className="rounded-full bg-accent px-2 py-0.5 text-xs font-medium text-accent-foreground">
                    {text.badge}
                  </span>
                ) : null}
              </span>
              <span className="mt-0.5 block truncate text-xs text-muted-foreground">
                {text.detail}
              </span>
            </span>
            <span className="shrink-0 text-end text-xs text-muted-foreground tabular-nums">
              {formatMeta(format)}
            </span>
          </button>
        )
      })}
    </div>
  )
}
