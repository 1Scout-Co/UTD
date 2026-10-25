import { useEffect, useState } from "react"

import { Search } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuTrigger,
} from "@/components/ui/context-menu"
import { formatPercent } from "@/shared/i18n/format"
import { t } from "@/shared/i18n/i18n"
import { cn } from "cn"

import type { DownloadFilter, DownloadItem } from "../../model/types"
import { Meter } from "../parts"
import type { ActionMessage } from "./soniox-file-actions"
import { ItemIcon, useFileIcons } from "./soniox-item-icon"
import {
  RowMenuItems,
  SonioxRowActions,
  rowCommands,
  type RowCommand,
  type RowContext,
} from "./soniox-row-actions"
import {
  PlainHeader,
  SortHeader,
  StatusPill,
  progressTone,
  useDownloadRows,
  type SortDir,
  type SortKey,
} from "./soniox-table-parts"

function TableHead({
  sortKey,
  sortDir,
  onSort,
}: {
  sortKey: SortKey
  sortDir: SortDir
  onSort: (key: SortKey) => void
}) {
  const sort = { current: sortKey, dir: sortDir, onSort }
  // Cột phụ ẩn bớt khi bảng hẹp (container query) để 1080px vẫn đọc được, không cuộn ngang.
  return (
    <thead className="sticky top-0 z-10">
      <tr className="*:bg-muted *:first:rounded-s-lg *:last:rounded-e-lg">
        <SortHeader
          label={t("table.name")}
          sortKey="name"
          className="ps-4"
          {...sort}
        />
        <PlainHeader
          label={t("table.description")}
          className="hidden @7xl:table-cell"
        />
        <SortHeader
          label={t("table.size")}
          sortKey="size"
          align="end"
          className="w-28"
          {...sort}
        />
        <SortHeader
          label={t("table.status")}
          sortKey="status"
          className="w-32 px-2"
          {...sort}
        />
        <PlainHeader label={t("table.progress")} className="w-36" />
        <SortHeader
          label={t("table.speed")}
          sortKey="speed"
          align="end"
          className="w-24"
          {...sort}
        />
        <SortHeader
          label={t("table.eta")}
          sortKey="eta"
          align="end"
          className="hidden w-24 @5xl:table-cell"
          {...sort}
        />
        <SortHeader
          label={t("table.date")}
          sortKey="date"
          className="hidden w-28 @4xl:table-cell"
          {...sort}
        />
        <th scope="col" className="w-12">
          <span className="sr-only">{t("table.options")}</span>
        </th>
      </tr>
    </thead>
  )
}

// Nền khi rê chuột / đang chọn là một khối bo tròn thụt 4px khỏi mép hàng. Vẽ bằng ::after của ô
// đầu tiên, định vị theo <tr> (relative) để trải hết hàng; đặt ::after trên chính <tr> thì Chrome
// coi là một ô bảng thừa và làm lệch cột. `isolate` trên <tr> giữ nền nằm sau chữ.
const ROW_BG = cn(
  "after:pointer-events-none after:absolute after:inset-x-1 after:inset-y-0.5 after:-z-10 after:rounded-lg after:transition-colors",
  "group-hover/row:after:bg-secondary group-data-selected/row:after:bg-accent group-data-selected/row:after:ring-1 group-data-selected/row:after:ring-primary/15"
)

// Tên file, mô tả là chữ của người dùng/máy chủ (thường Latin): dir="auto" giữ đúng thứ tự chữ và cắt "…"
// ở cuối tên cả khi giao diện tiếng Ả Rập; match-parent vẫn căn lề theo hướng của bảng.
const USER_TEXT = "[text-align:match-parent]"

function NameCell({ item, icon }: { item: DownloadItem; icon?: string }) {
  return (
    <td className={cn("py-2.5 ps-4 pe-3", ROW_BG)}>
      <div className="flex min-w-0 items-center gap-3">
        <ItemIcon item={item} icon={icon} />
        <div className="min-w-0">
          <div
            dir="auto"
            className={cn("truncate text-sm font-semibold", USER_TEXT)}
            title={item.name}
          >
            {item.name}
          </div>
          <div
            dir="auto"
            className={cn(
              "mt-0.5 truncate text-xs text-muted-foreground",
              USER_TEXT
            )}
            title={item.description}
          >
            {item.subtitle}
          </div>
        </div>
      </div>
    </td>
  )
}

function ProgressCell({ item }: { item: DownloadItem }) {
  return (
    <td className="px-3 py-2.5">
      <div className="flex items-center gap-2.5">
        <Meter
          label={t("table.progressOf", { name: item.name })}
          value={item.progress}
          tone={progressTone(item)}
          height={4}
          className="flex-1"
        />
        <span className="w-10 shrink-0 text-end text-xs whitespace-nowrap text-muted-foreground">
          {formatPercent(item.progress)}
        </span>
      </div>
    </td>
  )
}

/** Tốc độ; bảng hẹp (ẩn cột Còn lại) thì ghi thêm thời gian còn lại ngay dưới. */
function SpeedCell({ item }: { item: DownloadItem }) {
  return (
    <td className="px-3 py-2.5 text-end">
      <div
        className={cn(
          "whitespace-nowrap",
          !item.speed && "text-muted-foreground"
        )}
      >
        {item.speed ?? "—"}
      </div>
      {item.eta ? (
        <div className="text-xs whitespace-nowrap text-muted-foreground @5xl:hidden">
          {/* Đang chờ tự cứu: "Thử lại 14:32" / "Chờ mạng" là giờ hẹn, không phải thời gian còn lại. */}
          {item.waiting ? item.eta : t("table.etaLeft", { eta: item.eta })}
        </div>
      ) : null}
    </td>
  )
}

// Hàng chọn đậm hơn hàng đang rê (xem ROW_BG).
const ROW = "group/row relative isolate cursor-default text-sm tabular-nums"

function SonioxRow({
  item,
  icon,
  selected,
  groups,
  onSelect,
}: {
  item: DownloadItem
  icon?: string
  selected: boolean
  groups: RowCommand[][]
  onSelect: (id: string) => void
}) {
  const eta =
    item.eta ?? (item.status === "queued" ? t("table.waiting") : undefined)
  const open = groups[0]?.find((command) => command.key === "open")
  return (
    <tr
      data-row-id={item.id}
      data-selected={selected || undefined}
      onClick={() => onSelect(item.id)}
      onDoubleClick={() => (open && !open.disabled ? open.run() : undefined)}
      className={ROW}
    >
      <NameCell item={item} icon={icon} />
      <td className="hidden px-3 py-2.5 @7xl:table-cell">
        <div
          dir="auto"
          className={cn("truncate text-muted-foreground", USER_TEXT)}
          title={item.description}
        >
          {item.description}
        </div>
      </td>
      <td className="px-3 py-2.5 text-end font-medium">{item.size}</td>
      <td className="px-2 py-2.5">
        <StatusPill status={item.status} />
      </td>
      <ProgressCell item={item} />
      <SpeedCell item={item} />
      <td className="hidden px-3 py-2.5 text-end text-muted-foreground @5xl:table-cell">
        {eta ?? "—"}
      </td>
      <td className="hidden px-3 py-2.5 @4xl:table-cell">
        <div>{item.date}</div>
        <div className="text-xs text-muted-foreground">{item.time}</div>
      </td>
      {/* data-toolbar: cột hành động dày đặc, nút ⋯ để hình trơn kèm title (AI_RULES / ui-check). */}
      <td data-toolbar className="py-2.5 ps-1 pe-3 text-end">
        <SonioxRowActions item={item} groups={groups} />
      </td>
    </tr>
  )
}

function EmptyResults({ onClear }: { onClear: () => void }) {
  return (
    <div className="mx-auto flex max-w-xs flex-col items-center py-16 text-center">
      <div className="grid size-10 place-items-center rounded-xl bg-accent text-accent-foreground">
        <Search className="size-4" />
      </div>
      <div className="mt-3 text-sm font-semibold">{t("table.noResults")}</div>
      <div className="mt-1 text-xs text-muted-foreground">
        {t("table.noResultsHint")}
      </div>
      <Button variant="outline" className="mt-4 text-sm" onClick={onClear}>
        {t("table.clearFilters")}
      </Button>
    </div>
  )
}

/** F2 đổi tên hàng đang chọn (như Explorer/IDM), trừ khi đang gõ trong ô nhập hay đang mở hộp thoại/menu. */
function useRenameShortcut(item: DownloadItem | undefined, ctx: RowContext) {
  useEffect(() => {
    if (!item) return
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "F2" || event.defaultPrevented) return
      const target = event.target instanceof Element ? event.target : null
      if (
        target?.closest(
          "input, textarea, [contenteditable], [role=dialog], [role=menu]"
        )
      )
        return
      const rename = rowCommands(item, ctx)[0]?.find(
        (command) => command.key === "rename"
      )
      if (!rename || rename.disabled) return
      event.preventDefault()
      rename.run()
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [item, ctx])
}

/** Thân bảng + menu chuột phải dùng chung: chuột phải vào hàng nào thì chọn hàng đó rồi mở menu của nó. */
function RowsBody({
  rows,
  ctx,
  selectedId,
  onSelect,
}: {
  rows: DownloadItem[]
  ctx: RowContext
  selectedId: string | null
  onSelect: (id: string) => void
}) {
  const icons = useFileIcons(rows)
  const [menuId, setMenuId] = useState<string | null>(null)
  const menuItem = rows.find((row) => row.id === menuId)

  return (
    <ContextMenu>
      <ContextMenuTrigger
        render={<tbody />}
        onContextMenu={(event) => {
          const row =
            event.target instanceof Element ? event.target.closest("tr") : null
          const id = row?.getAttribute("data-row-id") ?? null
          setMenuId(id)
          if (id) onSelect(id)
        }}
      >
        {rows.map((item) => (
          <SonioxRow
            key={item.id}
            item={item}
            icon={icons[item.id]}
            selected={item.id === selectedId}
            groups={rowCommands(item, ctx)}
            onSelect={onSelect}
          />
        ))}
      </ContextMenuTrigger>
      <ContextMenuContent className="w-60">
        {menuItem ? (
          <RowMenuItems groups={rowCommands(menuItem, ctx)} kind="context" />
        ) : null}
      </ContextMenuContent>
    </ContextMenu>
  )
}

function TableFooter({
  count,
  query,
  message,
}: {
  count: number
  query: string
  message: ActionMessage | null
}) {
  return (
    <footer className="flex shrink-0 items-center justify-between gap-3 px-4 pt-3 text-xs text-muted-foreground">
      {message ? (
        <span
          role={message.tone === "error" ? "alert" : "status"}
          className={cn(
            "truncate font-medium",
            message.tone === "error" ? "text-destructive" : "text-emerald-700"
          )}
        >
          {message.text}
        </span>
      ) : (
        <span>
          {query
            ? t("table.countFor", { count, query })
            : t("table.count", { count })}
        </span>
      )}
      <span className="inline-flex shrink-0 items-center gap-1.5">
        <span className="size-1.5 rounded-full bg-emerald-500" />
        {t("table.engine")}
      </span>
    </footer>
  )
}

export function SonioxTable({
  items,
  filter,
  query,
  onClear,
  ctx,
  message,
}: {
  items: DownloadItem[]
  filter: DownloadFilter
  query: string
  onClear: () => void
  ctx: RowContext
  message: ActionMessage | null
}) {
  const { rows, sortKey, sortDir, toggleSort } = useDownloadRows(
    items,
    filter,
    query
  )
  const [selectedId, setSelectedId] = useState<string | null>(null)
  useRenameShortcut(
    rows.find((row) => row.id === selectedId),
    ctx
  )

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="@container min-h-0 flex-1 overflow-auto">
        <table className="w-full table-fixed border-separate border-spacing-0 text-start">
          <TableHead sortKey={sortKey} sortDir={sortDir} onSort={toggleSort} />
          <RowsBody
            rows={rows}
            ctx={ctx}
            selectedId={selectedId}
            onSelect={setSelectedId}
          />
        </table>
        {rows.length === 0 ? <EmptyResults onClear={onClear} /> : null}
      </div>
      <TableFooter count={rows.length} query={query} message={message} />
    </div>
  )
}
