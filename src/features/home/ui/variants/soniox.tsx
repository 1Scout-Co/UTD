import { useMemo, useState, type ComponentType } from "react"

import {
  Clock3,
  Download,
  Film,
  Gauge,
  Settings2,
  ShieldCheck,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import { TitleBar } from "@/shared/window/title-bar"
import { cn } from "cn"

import { formatNumber, formatSpeed } from "@/shared/i18n/format"
import { t } from "@/shared/i18n/i18n"

import type {
  DownloadFilter,
  DownloadItem,
  HomeScreenProps,
  StorageSummary,
} from "../../model/types"
import { Meter, StateView } from "../parts"
import { RenameDialog } from "../rename-dialog"
import { useFileActions, type FileActionHooks } from "./soniox-file-actions"
import type { RowContext } from "./soniox-row-actions"
import { SonioxTable } from "./soniox-table"
import { countByFilter } from "./soniox-table-parts"
import { SonioxToolbar } from "./soniox-toolbar"

/** Props của màn chính + móc cho thao tác file (menu chuột phải); bản xem trước bỏ trống được. */
export type SonioxProps = HomeScreenProps &
  FileActionHooks & {
    /** Có bản mới (app tự kiểm tra lúc mở): hiện chấm "Mới" cạnh Cài đặt. */
    updateVersion?: string
    /** Mở thẳng mục Cập nhật trong Cài đặt. */
    onUpdates?: () => void
    /** Thẻ trạng thái nhỏ trên thẻ ổ đĩa (vd app đang tự tải công cụ media). */
    sideStatus?: React.ReactNode
    /** Mở hộp Thêm hàng loạt (nút "Hàng loạt" cạnh "Thêm URL"). */
    onBatch?: () => void
  }

/** Mục Cài đặt ở chân thanh bên; có bản mới thì kèm nhãn "Mới" và bấm vào mở thẳng mục Cập nhật. */
function SettingsNav({
  updateVersion,
  onSettings,
  onUpdates,
}: {
  updateVersion?: string
  onSettings: () => void
  onUpdates?: () => void
}) {
  const update = Boolean(updateVersion && onUpdates)
  return (
    <NavItem
      icon={Settings2}
      label={t("nav.settings")}
      title={
        update
          ? t("nav.updateAvailable", { version: updateVersion ?? "" })
          : undefined
      }
      onClick={update ? onUpdates : onSettings}
      trailing={
        update ? (
          <span className="inline-flex items-center gap-1 rounded-full bg-primary px-2 text-xs font-medium text-primary-foreground">
            <span className="size-1.5 rounded-full bg-primary-foreground" />
            {t("nav.new")}
          </span>
        ) : undefined
      }
    />
  )
}

// Tên app nằm trên thanh tiêu đề (thay chữ "UTD" của khung gốc), không lặp lại trong thanh bên.
function SonioxLogo() {
  return (
    // Tên sản phẩm luôn đọc trái→phải, kể cả giao diện tiếng Ả Rập.
    <div dir="ltr" className="flex items-baseline gap-1.5">
      <span className="text-xl font-extrabold tracking-tighter text-foreground">
        utd
      </span>
      <span className="text-xs font-semibold text-primary">desktop</span>
    </div>
  )
}

function NavItem({
  icon: Icon,
  label,
  active = false,
  trailing,
  ...props
}: {
  icon: ComponentType<{ className?: string; strokeWidth?: number }>
  label: string
  active?: boolean
  trailing?: React.ReactNode
} & React.ComponentProps<typeof Button>) {
  return (
    <Button
      variant="ghost"
      aria-current={active ? "page" : undefined}
      className={cn(
        "h-9 w-full justify-start gap-2.5 px-2.5 text-sm font-medium",
        active
          ? "bg-card text-foreground shadow-soft hover:bg-card"
          : "text-muted-foreground hover:bg-card/70 hover:text-foreground"
      )}
      {...props}
    >
      <Icon
        className={cn("size-4", active && "text-primary")}
        strokeWidth={1.8}
      />
      <span className="flex-1 text-start">{label}</span>
      {trailing}
    </Button>
  )
}

function StorageCard({ storage }: { storage: StorageSummary }) {
  return (
    <div className="rounded-xl bg-card p-3 shadow-soft ring-1 ring-foreground/5">
      {/* Tên ổ dài ("Macintosh HD", tên USB) thì dung lượng xuống dòng dưới; "C:" vẫn chung một dòng. */}
      <div className="flex flex-wrap items-center justify-between gap-x-2 gap-y-0.5 text-xs">
        <span
          className="max-w-full truncate text-muted-foreground"
          title={storage.label}
        >
          {t("nav.drive", { drive: storage.label })}
        </span>
        <span className="shrink-0 font-semibold whitespace-nowrap">
          {t("nav.free", { size: storage.free })}
        </span>
      </div>
      <Meter
        label={t("nav.diskUsage")}
        value={storage.usedPercent}
        height={4}
        tone="var(--chart-1)"
        className="mt-2"
      />
    </div>
  )
}

function SonioxSidebar({
  total,
  storage,
  sideStatus,
  updateVersion,
  onHome,
  onMedia,
  onSettings,
  onUpdates,
}: {
  total: number
  storage?: StorageSummary
  sideStatus?: React.ReactNode
  updateVersion?: string
  onHome: () => void
  onMedia: () => void
  onSettings: () => void
  onUpdates?: () => void
}) {
  return (
    <aside className="flex w-52 shrink-0 flex-col px-3 pt-6 pb-5">
      <nav aria-label={t("nav.main")} className="space-y-1">
        <NavItem
          icon={Download}
          label={t("nav.downloads")}
          active
          onClick={onHome}
          trailing={
            <span className="rounded-full bg-accent px-1.5 text-xs text-accent-foreground tabular-nums">
              {total}
            </span>
          }
        />
        <NavItem icon={Film} label={t("nav.media")} onClick={onMedia} />
        {/* Download Guard chưa có màn riêng: hiện để biết sắp có, không phải nút bấm. */}
        <div className="flex h-9 items-center gap-2.5 px-2.5 text-sm font-medium text-muted-foreground">
          <ShieldCheck className="size-4" strokeWidth={1.8} />
          <span className="flex-1">{t("nav.safety")}</span>
          <span className="rounded-full bg-card px-1.5 text-xs ring-1 ring-foreground/5">
            {t("nav.soon")}
          </span>
        </div>
      </nav>
      <div className="mt-auto space-y-2">
        {sideStatus}
        {storage ? <StorageCard storage={storage} /> : null}
        <SettingsNav
          updateVersion={updateVersion}
          onSettings={onSettings}
          onUpdates={onUpdates}
        />
      </div>
    </aside>
  )
}

function liveStats(items: DownloadItem[]) {
  const downloading = items.filter((item) => item.status === "downloading")
  const queued = items.filter((item) => item.status === "queued").length
  const speed = downloading.reduce(
    (total, item) => total + (item.speedValue ?? 0),
    0
  )
  return [
    {
      icon: Gauge,
      label: t("stats.speed"),
      value: downloading.length ? formatSpeed(speed) : "—",
      tone: "text-primary",
    },
    {
      icon: Download,
      label: t("stats.downloading"),
      value: formatNumber(downloading.length, 0),
      tone: "text-primary",
    },
    {
      icon: Clock3,
      label: t("stats.queued"),
      value: formatNumber(queued, 0),
      tone: "text-muted-foreground",
    },
  ]
}

// Số liệu sống của hàng đợi, đặt bên phải tiêu đề thay cho dòng chữ phụ.
function HeaderStats({ items }: { items: DownloadItem[] }) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      {liveStats(items).map((stat) => (
        <div
          key={stat.label}
          className="flex items-center gap-2.5 rounded-xl bg-muted py-1.5 ps-1.5 pe-3.5"
        >
          <span
            className={cn(
              "grid size-7 place-items-center rounded-lg bg-card shadow-soft",
              stat.tone
            )}
          >
            <stat.icon className="size-3.5" strokeWidth={2} />
          </span>
          <span className="flex flex-col leading-tight">
            <span className="text-xs text-muted-foreground">{stat.label}</span>
            <span className="text-sm font-semibold tabular-nums">
              {stat.value}
            </span>
          </span>
        </div>
      ))}
    </div>
  )
}

/** Gộp thao tác file (menu chuột phải) với thao tác task có sẵn thành ngữ cảnh cho từng hàng. */
function toRowContext(
  files: ReturnType<typeof useFileActions>,
  handlers: Pick<
    HomeScreenProps,
    "onPause" | "onResume" | "onRemove" | "onDiagnose" | "onDetails" | "onRefreshLink"
  >
): RowContext {
  return {
    ...handlers,
    desktop: files.desktop,
    hasLink: files.hasLink,
    onOpen: files.open,
    onReveal: files.reveal,
    onRename: files.setRenaming,
    onMove: files.move,
    onCopyLink: files.copyLink,
    onTrash: files.trash,
  }
}

// S-01: thứ bậc header → thanh công cụ (lọc · tìm · nút chính) → bảng.
function DownloadsView({
  items,
  filter,
  query,
  onFilterChange,
  onQueryChange,
  onClear,
  onAdd,
  onBatch,
  linkOf,
  onTaskUpdate,
  onTaskRemoved,
  ...handlers
}: {
  filter: DownloadFilter
  query: string
  onFilterChange: (filter: DownloadFilter) => void
  onQueryChange: (query: string) => void
  onClear: () => void
  onBatch?: () => void
} & Pick<
  HomeScreenProps,
  | "items"
  | "onAdd"
  | "onPause"
  | "onResume"
  | "onRemove"
  | "onDiagnose"
  | "onDetails"
  | "onRefreshLink"
> &
  FileActionHooks) {
  const counts = useMemo(() => countByFilter(items), [items])
  const files = useFileActions({ linkOf, onTaskUpdate, onTaskRemoved })
  const ctx = toRowContext(files, handlers)
  return (
    <div data-s="S-01" className="mt-5 flex min-h-0 flex-1 flex-col gap-3">
      <SonioxToolbar
        filter={filter}
        counts={counts}
        query={query}
        onFilterChange={onFilterChange}
        onQueryChange={onQueryChange}
        onAdd={onAdd}
        onBatch={onBatch}
      />
      <SonioxTable
        items={items}
        filter={filter}
        query={query}
        onClear={onClear}
        ctx={ctx}
        message={files.message}
      />
      <RenameDialog
        item={files.renaming}
        onClose={() => files.setRenaming(null)}
        onSubmit={files.rename}
      />
    </div>
  )
}

function PageHeader({ items }: { items?: DownloadItem[] }) {
  return (
    <header className="flex flex-wrap items-center justify-between gap-4">
      <h1 className="text-2xl font-bold tracking-tight">
        {t("nav.downloads")}
      </h1>
      {items ? <HeaderStats items={items} /> : null}
    </header>
  )
}

/** Bộ lọc + từ khoá của danh sách; `clear` đưa về "Tất cả", bỏ tìm. */
function useListControls() {
  const [filter, setFilter] = useState<DownloadFilter>("all")
  const [query, setQuery] = useState("")
  const clear = () => {
    setQuery("")
    setFilter("all")
  }
  return { filter, setFilter, query, setQuery, clear }
}

export function Soniox({
  state,
  items,
  storage,
  onAdd,
  onMedia,
  onRetry,
  onSettings,
  updateVersion,
  onUpdates,
  sideStatus,
  ...handlers
}: SonioxProps) {
  const { filter, setFilter, query, setQuery, clear } = useListControls()

  return (
    <div
      data-variant="soniox"
      className="relative flex h-svh w-full overflow-hidden bg-background bg-[radial-gradient(circle_at_14%_0%,#e8f1ff_0,transparent_36%),radial-gradient(circle_at_100%_100%,#eef4ff_0,transparent_34%)] pt-10 text-foreground"
    >
      <TitleBar className="absolute inset-x-0 top-0">
        <SonioxLogo />
      </TitleBar>
      <SonioxSidebar
        total={items.length}
        storage={storage}
        sideStatus={sideStatus}
        updateVersion={updateVersion}
        onHome={clear}
        onMedia={onMedia}
        onSettings={onSettings}
        onUpdates={onUpdates}
      />
      <main className="me-3 mb-3 flex min-w-0 flex-1 flex-col rounded-2xl bg-card p-6 shadow-panel ring-1 ring-foreground/5">
        <PageHeader items={state === "data" ? items : undefined} />
        {state === "data" ? (
          <DownloadsView
            items={items}
            filter={filter}
            query={query}
            onFilterChange={setFilter}
            onQueryChange={setQuery}
            onClear={clear}
            onAdd={onAdd}
            {...handlers}
          />
        ) : (
          <div className="mt-5 min-h-0 flex-1 overflow-auto">
            <StateView
              soft
              state={state}
              onAdd={onAdd}
              onRetry={onRetry}
              onSettings={onSettings}
            />
          </div>
        )}
      </main>
    </div>
  )
}
