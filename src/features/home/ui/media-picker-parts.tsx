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
import { cn } from "cn"

import { formatBytes, formatNumber } from "../model/download-mapper"
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
  const bitrate = format.bitrateKbps
    ? `${formatNumber(format.bitrateKbps, 0)} kbps`
    : "—"
  return `${size} · ${bitrate}`
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
    <div className="flex h-12 items-center gap-2 rounded-xl bg-muted pr-1.5 pl-3.5 transition-shadow focus-within:bg-card focus-within:shadow-soft focus-within:ring-2 focus-within:ring-ring/40">
      <Link2 className="size-4 shrink-0 text-muted-foreground" />
      <Input
        aria-label="URL media"
        value={url}
        autoFocus
        autoComplete="off"
        spellCheck={false}
        disabled={disabled}
        placeholder="Dán link video hoặc luồng media…"
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
          Phân tích
        </Button>
      ) : (
        <Button
          type="button"
          variant="outline"
          disabled={disabled}
          onClick={onPaste}
        >
          <ClipboardPaste />
          Dán
        </Button>
      )}
    </div>
  )
}

/** Hàng logo các trang hỗ trợ (thay cho dòng mô tả dài). */
export function SupportedSites() {
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      <span className="mr-1 text-xs text-muted-foreground">Hỗ trợ</span>
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
      <span className="ml-1 text-xs text-muted-foreground">
        · HLS/DASH · link trực tiếp
      </span>
    </div>
  )
}

/** Khung chờ trong lúc yt-dlp phân tích link. */
export function ProbeSkeleton() {
  return (
    <div role="status" className="space-y-2">
      <span className="sr-only">Đang phân tích…</span>
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
          className="line-clamp-2 text-sm font-semibold break-words"
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
        return (
          <button
            key={format.id}
            type="button"
            onClick={() => onSelect(format.id)}
            aria-pressed={active}
            className={cn(
              "flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left transition-colors",
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
                {format.label}
                {format.badge ? (
                  <span className="rounded-full bg-accent px-2 py-0.5 text-xs font-medium text-accent-foreground">
                    {format.badge}
                  </span>
                ) : null}
              </span>
              <span className="mt-0.5 block truncate text-xs text-muted-foreground">
                {format.detail}
              </span>
            </span>
            <span className="shrink-0 text-right text-xs text-muted-foreground tabular-nums">
              {formatMeta(format)}
            </span>
          </button>
        )
      })}
    </div>
  )
}
