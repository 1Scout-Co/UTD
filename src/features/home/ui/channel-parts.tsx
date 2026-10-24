import { Film, Loader2, Lock, Square } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { formatCompact, formatDate } from "@/shared/i18n/format"
import { codeText, t } from "@/shared/i18n/i18n"
import { cn } from "cn"

import {
  CHANNEL_QUALITIES,
  isSelected,
  type ChannelEntry,
  type ChannelFilter,
  type ChannelHead,
  type ChannelProfile,
  type ChannelRange,
  type ChannelSelection,
} from "../model/channel"

function formatDuration(seconds: number): string {
  const minutes = Math.floor(seconds / 60)
  const rest = String(Math.round(seconds % 60)).padStart(2, "0")
  return minutes >= 60
    ? `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, "0")}:${rest}`
    : `${minutes}:${rest}`
}

function entryMeta(entry: ChannelEntry): string {
  const parts = [
    entry.timestamp === null ? null : formatDate(entry.timestamp * 1000),
    entry.durationSeconds ? formatDuration(entry.durationSeconds) : null,
    entry.viewCount === null
      ? null
      : t("channel.views", {
          count: entry.viewCount,
          views: formatCompact(entry.viewCount),
        }),
  ]
  return parts.filter(Boolean).join(" · ") || "—"
}

/** Ảnh đại diện, tên kênh, số video (đếm dần), nút Dừng khi đang liệt kê. */
export function ChannelCard({
  head,
  profile,
  count,
  loading,
  onStop,
}: {
  head: ChannelHead | null
  profile: ChannelProfile | null
  count: number
  loading: boolean
  onStop: () => void
}) {
  const name = profile?.name ?? head?.name ?? null
  const total = profile?.videoCount
  const details = [
    head?.handle,
    total && total > count
      ? t("channel.videoCountOf", { done: count, total })
      : t("channel.videoCount", { count }),
    profile?.followers
      ? t("channel.followers", {
          count: profile.followers,
          followers: formatCompact(profile.followers),
        })
      : null,
  ].filter(Boolean)
  return (
    <div className="flex items-center gap-3 rounded-xl bg-muted p-2.5">
      <div className="grid size-12 shrink-0 place-items-center overflow-hidden rounded-full bg-card text-lg font-semibold text-primary">
        {profile?.avatar ? (
          <img src={profile.avatar} alt="" referrerPolicy="no-referrer" className="size-full object-cover" />
        ) : (
          (name?.trim().charAt(0).toUpperCase() ?? <Film className="size-5 text-primary/60" />)
        )}
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-semibold" title={name ?? undefined}>
          {name ?? t("channel.reading")}
        </div>
        <div className="mt-0.5 flex items-center gap-1.5 text-xs text-muted-foreground tabular-nums">
          {loading ? <Loader2 className="size-3 animate-spin" /> : null}
          <span className="truncate">{details.join(" · ")}</span>
        </div>
      </div>
      {loading ? (
        <Button type="button" variant="outline" size="sm" onClick={onStop}>
          <Square />
          {t("channel.stop")}
        </Button>
      ) : null}
    </div>
  )
}

// Nhãn: t(`channel.range.${id}`).
const RANGES: ChannelRange[] = ["all", "latest", "dates"]

/** Lọc: tất cả / N video mới nhất / khoảng ngày; ẩn video đã tải. */
export function FilterBar({
  filter,
  downloaded,
  onChange,
}: {
  filter: ChannelFilter
  downloaded: number
  onChange: (filter: ChannelFilter) => void
}) {
  const set = (patch: Partial<ChannelFilter>) => onChange({ ...filter, ...patch })
  return (
    <div className="flex flex-wrap items-center gap-2">
      <div className="flex rounded-lg bg-muted p-0.5">
        {RANGES.map((range) => (
          <button
            key={range}
            type="button"
            aria-pressed={filter.range === range}
            onClick={() => set({ range })}
            className={cn(
              "rounded-md px-2.5 py-1 text-xs font-medium transition-colors",
              filter.range === range
                ? "bg-card shadow-soft"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {t(`channel.range.${range}`)}
          </button>
        ))}
      </div>
      {filter.range === "latest" ? (
        <Input
          type="number"
          aria-label={t("channel.latestCount")}
          min={1}
          value={filter.latest}
          onChange={(event) => set({ latest: Number(event.target.value) })}
          className="h-8 w-20"
        />
      ) : null}
      {filter.range === "dates" ? (
        <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <Input
            type="date"
            aria-label={t("channel.fromDate")}
            value={filter.from}
            onChange={(event) => set({ from: event.target.value })}
            className="h-8 w-36"
          />
          –
          <Input
            type="date"
            aria-label={t("channel.toDate")}
            value={filter.to}
            onChange={(event) => set({ to: event.target.value })}
            className="h-8 w-36"
          />
        </span>
      ) : null}
      {downloaded > 0 ? (
        <label className="ms-auto flex items-center gap-2 text-xs text-muted-foreground">
          <Switch
            size="sm"
            checked={filter.onlyNew}
            onCheckedChange={(checked) => set({ onlyNew: checked })}
          />
          {t("channel.onlyNew")}
          <span className="tabular-nums">
            {t("channel.downloadedCount", { count: downloaded })}
          </span>
        </label>
      ) : null}
    </div>
  )
}

/** Danh sách video có ô chọn; dòng ngoài màn hình không vẽ (`content-visibility`) nên kênh dài vẫn mượt. */
export function EntryList({
  entries,
  selection,
  onToggle,
}: {
  entries: ChannelEntry[]
  selection: ChannelSelection
  onToggle: (id: string) => void
}) {
  return (
    <ul aria-label={t("channel.videos")} className="-mx-1 max-h-80 space-y-0.5 overflow-y-auto px-1">
      {entries.map((entry) => (
        <li
          key={entry.id}
          className="[contain-intrinsic-size:auto_56px] [content-visibility:auto]"
        >
          <label
            className={cn(
              "flex items-center gap-3 rounded-lg px-2 py-1.5",
              entry.blocked ? "opacity-60" : "cursor-pointer hover:bg-muted",
            )}
          >
            <Checkbox
              aria-label={entry.title}
              checked={isSelected(entry, selection)}
              disabled={Boolean(entry.blocked)}
              onCheckedChange={() => onToggle(entry.id)}
            />
            <span className="h-11 w-16 shrink-0 overflow-hidden rounded-md bg-muted">
              {entry.thumbnail ? (
                <img
                  src={entry.thumbnail}
                  alt=""
                  loading="lazy"
                  decoding="async"
                  referrerPolicy="no-referrer"
                  className="size-full object-cover"
                />
              ) : null}
            </span>
            <span className="min-w-0 flex-1">
              <span className="block truncate text-sm" title={entry.title}>
                {entry.title}
              </span>
              <span className="mt-0.5 block truncate text-xs text-muted-foreground tabular-nums">
                {entryMeta(entry)}
              </span>
            </span>
            {entry.blocked ? (
              <span className="flex shrink-0 items-center gap-1 text-xs text-muted-foreground">
                <Lock className="size-3" />
                {codeText("channel.blocked", entry.blocked)}
              </span>
            ) : entry.downloaded ? (
              <span className="shrink-0 rounded-full bg-accent px-2 py-0.5 text-xs font-medium text-accent-foreground">
                {t("channel.downloaded")}
              </span>
            ) : null}
          </label>
        </li>
      ))}
    </ul>
  )
}

/** Một chất lượng chung cho cả loạt. */
export function QualityPicker({
  value,
  onChange,
}: {
  value: string
  onChange: (id: string) => void
}) {
  return (
    <div role="group" aria-label={t("channel.quality.label")} className="flex flex-wrap items-center gap-1.5">
      {CHANNEL_QUALITIES.map((quality) => (
        <button
          key={quality.id}
          type="button"
          title={t(quality.detail)}
          aria-pressed={value === quality.id}
          onClick={() => onChange(quality.id)}
          className={cn(
            "rounded-lg px-2.5 py-1 text-xs font-medium transition-colors",
            value === quality.id
              ? "bg-card shadow-soft ring-2 ring-primary"
              : "bg-muted text-muted-foreground hover:text-foreground",
          )}
        >
          {quality.labelKey ? t(quality.labelKey) : quality.label}
        </button>
      ))}
    </div>
  )
}
