import { useMemo, useState } from "react"

import { ArrowDown, ArrowUp, ChevronsUpDown } from "lucide-react"

import { compareText } from "@/shared/i18n/format"
import { t } from "@/shared/i18n/i18n"
import { cn } from "cn"

import type {
  DownloadFilter,
  DownloadItem,
  DownloadStatus,
} from "../../model/types"

export type SortKey = "name" | "size" | "status" | "speed" | "eta" | "date"
export type SortDir = "asc" | "desc"

// Nhãn: t(`filter.${id}`).
export const TABS: DownloadFilter[] = ["all", "active", "completed", "error"]

// Nhãn: t(`status.${status}`).
export const STATUS_META: Record<
  DownloadStatus,
  { dot: string; pill: string; order: number }
> = {
  downloading: {
    dot: "bg-chart-1",
    pill: "bg-accent text-accent-foreground",
    order: 0,
  },
  paused: {
    dot: "bg-slate-400",
    pill: "bg-slate-100 text-slate-600",
    order: 1,
  },
  queued: {
    dot: "bg-slate-300",
    pill: "bg-slate-50 text-slate-500",
    order: 2,
  },
  verifying: {
    dot: "bg-sky-500",
    pill: "bg-sky-50 text-sky-700",
    order: 3,
  },
  completed: {
    dot: "bg-emerald-500",
    pill: "bg-emerald-50 text-emerald-700",
    order: 4,
  },
  failed: {
    dot: "bg-red-500",
    pill: "bg-red-50 text-red-700",
    order: 5,
  },
  quarantined: {
    dot: "bg-amber-500",
    pill: "bg-amber-50 text-amber-700",
    order: 6,
  },
}

export function matchesFilter(item: DownloadItem, filter: DownloadFilter) {
  if (filter === "all") return true
  if (filter === "completed") return item.status === "completed"
  if (filter === "error") {
    return item.status === "failed" || item.status === "quarantined"
  }
  return (
    item.status === "downloading" ||
    item.status === "paused" ||
    item.status === "queued" ||
    item.status === "verifying"
  )
}

export function countByFilter(items: DownloadItem[]) {
  return Object.fromEntries(
    TABS.map((tab) => [
      tab,
      items.filter((item) => matchesFilter(item, tab)).length,
    ])
  ) as Record<DownloadFilter, number>
}

export function progressTone(item: DownloadItem) {
  if (item.status === "failed") return "var(--destructive)"
  if (item.status === "quarantined") return "#f79009"
  if (item.status === "completed") return "var(--chart-2)"
  if (item.status === "paused") return "#94a3b8"
  if (item.status === "queued") return "#cbd5e1"
  return "var(--chart-1)"
}

function sortValue(item: DownloadItem, key: SortKey) {
  if (key === "name") return item.name
  if (key === "size") return item.sizeBytes
  if (key === "status") return STATUS_META[item.status].order
  if (key === "speed") return item.speedValue ?? -1
  if (key === "eta") return item.etaMinutes ?? Number.POSITIVE_INFINITY
  return item.sortTime
}

// Lọc theo tab + từ khoá, rồi sắp theo cột đang chọn.
export function useDownloadRows(
  items: DownloadItem[],
  filter: DownloadFilter,
  query: string
) {
  const [sortKey, setSortKey] = useState<SortKey>("date")
  const [sortDir, setSortDir] = useState<SortDir>("desc")

  const rows = useMemo(() => {
    const needle = query.trim().toLowerCase()
    const filtered = items.filter(
      (item) =>
        matchesFilter(item, filter) &&
        (!needle ||
          `${item.name} ${item.description} ${item.subtitle}`
            .toLowerCase()
            .includes(needle))
    )
    return filtered.sort((a, b) => {
      const va = sortValue(a, sortKey)
      const vb = sortValue(b, sortKey)
      const result =
        typeof va === "string" && typeof vb === "string"
          ? compareText(va, vb)
          : Number(va) - Number(vb)
      return sortDir === "asc" ? result : -result
    })
  }, [items, filter, query, sortKey, sortDir])

  const toggleSort = (key: SortKey) => {
    if (sortKey === key) {
      setSortDir((current) => (current === "asc" ? "desc" : "asc"))
      return
    }
    setSortKey(key)
    setSortDir(key === "name" || key === "status" ? "asc" : "desc")
  }

  return { rows, sortKey, sortDir, toggleSort }
}

// Tiêu đề cột chữ thường (không IN HOA): tiếng Việt có dấu đọc dễ hơn.
const HEAD_TEXT = "text-xs font-medium whitespace-nowrap text-muted-foreground"

export function PlainHeader({
  label,
  className,
}: {
  label: string
  className?: string
}) {
  return (
    <th
      scope="col"
      className={cn("h-10 px-3 text-start", HEAD_TEXT, className)}
    >
      {label}
    </th>
  )
}

export function SortHeader({
  label,
  sortKey,
  current,
  dir,
  onSort,
  align = "start",
  className,
}: {
  label: string
  sortKey: SortKey
  current: SortKey
  dir: SortDir
  onSort: (key: SortKey) => void
  align?: "start" | "end"
  className?: string
}) {
  const active = current === sortKey
  const Icon = !active ? ChevronsUpDown : dir === "asc" ? ArrowUp : ArrowDown
  return (
    <th
      scope="col"
      aria-sort={active ? (dir === "asc" ? "ascending" : "descending") : "none"}
      className={cn(
        "h-10 px-3",
        HEAD_TEXT,
        align === "end" ? "text-end" : "text-start",
        className
      )}
    >
      <button
        type="button"
        onClick={() => onSort(sortKey)}
        className={cn(
          "inline-flex items-center gap-1 rounded-sm align-middle transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none",
          HEAD_TEXT,
          active && "text-foreground",
          align === "end" && "flex-row-reverse"
        )}
      >
        {label}
        <Icon className={cn("size-3", !active && "opacity-50")} />
      </button>
    </th>
  )
}

/** `signIn`: task lỗi vì cần đăng nhập (S-72): nhãn "Cần đăng nhập" màu hổ phách thay cho "Lỗi". */
export function StatusPill({ status, signIn = false }: { status: DownloadStatus; signIn?: boolean }) {
  const meta = STATUS_META[status]
  return (
    <span
      data-s={signIn ? "S-72" : undefined}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-xs font-semibold whitespace-nowrap",
        signIn ? "bg-amber-50 text-amber-800" : meta.pill
      )}
    >
      <span className={cn("size-1.5 rounded-full", signIn ? "bg-amber-500" : meta.dot)} />
      {signIn ? t("auth.needed") : t(`status.${status}`)}
    </span>
  )
}
