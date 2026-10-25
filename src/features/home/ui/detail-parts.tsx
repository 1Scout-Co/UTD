import { useState, type ReactNode } from "react"

import { Check, ChevronDown, Copy } from "lucide-react"

import { Button } from "@/components/ui/button"
import { formatBytes, formatPercent } from "@/shared/i18n/format"
import { t } from "@/shared/i18n/i18n"
import { cn } from "cn"

import {
  byteLength,
  mapBlocks,
  segmentCounts,
  type BlockState,
  type DetailPart,
} from "../model/detail"
import { Meter } from "./parts"

// Màu từng trạng thái đoạn (kiểu thanh đoạn của IDM). Nhãn: t(`detail.legend.${state}`).
const BLOCK_TONE: Record<BlockState, string> = {
  done: "bg-primary",
  active: "bg-chart-1",
  partial: "bg-primary/45",
  pending: "bg-transparent",
  failed: "bg-destructive",
}

/** Một luồng: mỗi đoạn một khối rộng theo số byte, tô phần đã tải. */
function SegmentBar({ part, live }: { part: DetailPart; live: boolean }) {
  return (
    <div className="flex h-3.5 min-w-0 flex-1 gap-px overflow-hidden rounded-md bg-card ring-1 ring-foreground/10">
      {mapBlocks(part, live).map((block) => (
        <div
          key={block.key}
          data-state={block.state}
          className="relative h-full min-w-px bg-muted"
          style={{ flex: `${block.width} 1 0%` }}
        >
          <div
            className={cn("absolute inset-y-0 start-0", BLOCK_TONE[block.state])}
            style={{ width: `${block.fill}%` }}
          />
        </div>
      ))}
    </div>
  )
}

function Legend({ states }: { states: BlockState[] }) {
  return (
    <ul className="flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted-foreground">
      {states.map((state) => (
        <li key={state} className="inline-flex items-center gap-1.5">
          <span
            className={cn(
              "size-2.5 rounded-sm ring-1 ring-foreground/10",
              state === "pending" ? "bg-muted" : BLOCK_TONE[state]
            )}
          />
          {t(`detail.legend.${state}`)}
        </li>
      ))}
    </ul>
  )
}

const LEGEND_ORDER: BlockState[] = ["done", "active", "partial", "pending", "failed"]

/** Bảng từng đoạn (như bảng kết nối trong cửa sổ tiến độ của IDM), gập sẵn. */
function SegmentTable({ parts, live }: { parts: DetailPart[]; live: boolean }) {
  const labelled = parts.length > 1
  return (
    <details className="group rounded-xl bg-muted/60">
      <summary className="flex cursor-pointer list-none items-center gap-1.5 rounded-xl px-3 py-2 text-xs font-medium text-muted-foreground outline-none select-none hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50">
        <ChevronDown className="size-3.5 transition-transform group-open:rotate-180" />
        {t("detail.segments")}
      </summary>
      <div className="max-h-48 overflow-auto px-3 pb-2">
        <table className="w-full text-xs tabular-nums">
          <thead className="text-muted-foreground">
            <tr className="*:py-1 *:font-medium">
              <th scope="col" className="text-start">#</th>
              <th scope="col" className="text-end">{t("detail.col.start")}</th>
              <th scope="col" className="text-end">{t("detail.col.done")}</th>
              <th scope="col" className="ps-3 text-start">{t("detail.col.state")}</th>
            </tr>
          </thead>
          <tbody>
            {parts.flatMap((part) => {
              const blocks = mapBlocks(part, live)
              return part.segments.map((segment, index) => (
                <tr key={`${part.id}:${segment.index}`} className="*:py-0.5">
                  <td>
                    {labelled ? `${t(`detail.role.${part.role}`)} ` : ""}
                    {segment.index + 1}
                  </td>
                  <td className="text-end">{formatBytes(segment.start)}</td>
                  <td className="text-end">
                    {formatBytes(segment.downloaded)} / {formatBytes(byteLength(segment))}
                  </td>
                  <td className="ps-3">{t(`detail.legend.${blocks[index].state}`)}</td>
                </tr>
              ))
            })}
          </tbody>
        </table>
      </div>
    </details>
  )
}

/** Bản đồ đoạn: một thanh mỗi luồng (media: Hình, Tiếng), chú thích màu, bảng từng đoạn. */
export function SegmentMap({ parts, live }: { parts: DetailPart[]; live: boolean }) {
  if (parts.length === 0) {
    return <p className="text-xs text-muted-foreground">{t("detail.mapNone")}</p>
  }
  const counts = segmentCounts(parts, live)
  const states = new Set(parts.flatMap((part) => mapBlocks(part, live).map((block) => block.state)))
  const label = counts.total
    ? t("detail.mapSummary", { done: counts.done, total: counts.total })
    : t("detail.mapSingle")
  return (
    <section className="space-y-2">
      <div className="flex items-baseline justify-between gap-3">
        <h3 className="text-sm font-medium">{t("detail.map")}</h3>
        <span className="text-xs text-muted-foreground tabular-nums">{label}</span>
      </div>
      <div role="img" aria-label={`${t("detail.map")}: ${label}`} className="space-y-1.5">
        {parts.map((part) => (
          <div key={part.id} className="flex items-center gap-2">
            {part.role !== "file" ? (
              <span className="w-12 shrink-0 text-xs text-muted-foreground">
                {t(`detail.role.${part.role}`)}
              </span>
            ) : null}
            <SegmentBar part={part} live={live} />
          </div>
        ))}
      </div>
      <Legend states={LEGEND_ORDER.filter((state) => states.has(state))} />
      {counts.total > 0 ? <SegmentTable parts={parts} live={live} /> : null}
    </section>
  )
}

/** Thanh tiến độ lớn + %. */
export function BigProgress({ value, tone }: { value: number; tone?: string }) {
  return (
    <div className="flex items-center gap-3">
      <Meter label={t("table.progress")} value={value} tone={tone} height={8} className="flex-1" />
      <span className="w-12 shrink-0 text-end text-sm font-semibold tabular-nums">
        {formatPercent(value)}
      </span>
    </div>
  )
}

export interface Stat {
  label: string
  value: string
  /** Phần phụ nhỏ sau giá trị (vd "/ 19,4 GB"); ô hẹp thì xuống dòng thay vì cắt. */
  hint?: string
}

/** Ô số liệu ngắn (tốc độ, còn lại, đã tải, kết nối…). */
export function Stats({ items }: { items: Stat[] }) {
  return (
    <dl className="grid grid-cols-2 gap-2 sm:grid-cols-4">
      {items.map((item) => (
        <div key={item.label} className="min-w-0 rounded-xl bg-muted px-3 py-2.5">
          <dt className="truncate text-xs text-muted-foreground">{item.label}</dt>
          <dd className="mt-0.5 text-sm font-semibold tabular-nums">
            <span>{item.value}</span>
            {item.hint ? (
              <span className="ms-1 text-xs font-normal whitespace-nowrap text-muted-foreground">
                {item.hint}
              </span>
            ) : null}
          </dd>
        </div>
      ))}
    </dl>
  )
}

export interface Row {
  label: string
  value: ReactNode
  /** Chữ của người dùng/máy (đường dẫn, mã): giữ hướng chữ gốc kể cả giao diện tiếng Ả Rập. */
  raw?: boolean
  /** Nút nhỏ cuối hàng (vd sao chép đường dẫn). */
  action?: ReactNode
}

/** Danh sách nhãn — giá trị (đường dẫn, thời gian, mã lỗi…). */
export function Rows({ rows }: { rows: Row[] }) {
  return (
    <dl className="divide-y divide-border/60 rounded-xl bg-muted/60 px-3">
      {rows.map((row) => (
        <div key={row.label} className="flex items-baseline gap-3 py-2 text-sm">
          <dt className="w-28 shrink-0 text-xs text-muted-foreground">{row.label}</dt>
          {/* Nút của hàng nằm trong <dd>: <dl> chỉ cho dt/dd làm con. */}
          <dd className="flex min-w-0 flex-1 items-baseline gap-2">
            <span
              dir={row.raw ? "auto" : undefined}
              className={cn("min-w-0 flex-1 break-all", row.raw && "font-mono text-xs [text-align:match-parent]")}
            >
              {row.value}
            </span>
            {row.action}
          </dd>
        </div>
      ))}
    </dl>
  )
}

/** Nút sao chép: chạy `copy`, đổi nhãn thành "Đã chép" một lúc. */
export function CopyButton({
  label,
  done,
  copy,
  small = false,
}: {
  label: string
  done: string
  copy: () => Promise<void>
  /** Nút nhỏ nằm trong một hàng thông tin. */
  small?: boolean
}) {
  const [copied, setCopied] = useState(false)
  return (
    <Button
      type="button"
      variant={small ? "ghost" : "outline"}
      size={small ? "xs" : "default"}
      className={small ? "-my-1 shrink-0 self-center" : undefined}
      onClick={() =>
        void copy()
          .then(() => {
            setCopied(true)
            window.setTimeout(() => setCopied(false), 1500)
          })
          .catch(() => undefined)
      }
    >
      {copied ? <Check /> : <Copy />}
      {copied ? done : label}
    </Button>
  )
}
