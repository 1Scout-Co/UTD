import { useEffect, useRef } from "react"

import { ListPlus, Plus, Search, X } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Kbd, KbdGroup } from "@/components/ui/kbd"
import { formatNumber } from "@/shared/i18n/format"
import { t } from "@/shared/i18n/i18n"
import { cn } from "cn"
import { isMac } from "@/shared/platform"

import type { DownloadFilter } from "../../model/types"
import { TABS } from "./soniox-table-parts"

/** Phím tắt hiện như hệ điều hành quen: ⌘ trên Mac, Ctrl trên Windows (bắt phím thì nhận cả hai). */
const MOD_KEY = isMac ? "⌘" : "Ctrl"

function SearchField({
  query,
  onQueryChange,
}: {
  query: string
  onQueryChange: (query: string) => void
}) {
  const searchRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault()
        searchRef.current?.focus()
        searchRef.current?.select()
      }
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [])

  return (
    // Cửa sổ hẹp (<1280px) ô tìm gọn lại, bỏ gợi ý Ctrl K: nhãn lọc tiếng Pháp/Indonesia… dài hơn tiếng Việt.
    <div className="relative w-44 xl:w-60 2xl:w-72">
      <Search className="pointer-events-none absolute start-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
      <Input
        ref={searchRef}
        value={query}
        onChange={(event) => onQueryChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key !== "Escape") return
          if (query) onQueryChange("")
          else event.currentTarget.blur()
        }}
        aria-label={t("toolbar.search")}
        placeholder={t("toolbar.searchPlaceholder")}
        className="h-9 rounded-lg border-transparent bg-muted ps-9 pe-3 text-sm focus-visible:bg-card xl:pe-20"
      />
      {query ? (
        <Button
          variant="ghost"
          size="icon-xs"
          aria-label={t("toolbar.clearSearch")}
          title={t("toolbar.clearSearch")}
          onClick={() => onQueryChange("")}
          className="absolute end-1.5 top-1/2 -translate-y-1/2 text-muted-foreground"
        >
          <X />
        </Button>
      ) : (
        <KbdGroup className="absolute end-2 top-1/2 hidden -translate-y-1/2 xl:flex">
          <Kbd className="bg-card text-xs shadow-soft">{MOD_KEY}</Kbd>
          <Kbd className="bg-card text-xs shadow-soft">K</Kbd>
        </KbdGroup>
      )}
    </div>
  )
}

// Nút bật/tắt thay cho Tabs của Base UI: cùng dáng segmented nhưng nhẹ hơn ~4 kB gzip.
function FilterBar({
  filter,
  counts,
  onFilterChange,
}: {
  filter: DownloadFilter
  counts: Record<DownloadFilter, number>
  onFilterChange: (filter: DownloadFilter) => void
}) {
  return (
    <div
      role="group"
      aria-label={t("toolbar.filter")}
      className="inline-flex h-9 items-center gap-0.5 rounded-lg bg-muted p-0.5"
    >
      {TABS.map((tab) => {
        const active = filter === tab
        return (
          <Button
            key={tab}
            variant="ghost"
            aria-pressed={active}
            onClick={() => onFilterChange(tab)}
            className={cn(
              "h-full gap-1.5 rounded-md px-2.5 text-sm",
              active
                ? "bg-card text-foreground shadow-soft hover:bg-card"
                : "text-muted-foreground hover:bg-card/60"
            )}
          >
            {t(`filter.${tab}`)}
            <span
              className={cn(
                "min-w-5 rounded-full px-1.5 text-xs tabular-nums",
                active
                  ? "bg-accent text-accent-foreground"
                  : "bg-card/70 text-muted-foreground"
              )}
            >
              {formatNumber(counts[tab], 0)}
            </span>
          </Button>
        )
      })}
    </div>
  )
}

// Thanh công cụ nằm trên bảng, ngoài khung bảng: lọc bên trái, tìm + nút chính bên phải.
export function SonioxToolbar({
  filter,
  counts,
  query,
  onFilterChange,
  onQueryChange,
  onAdd,
  onBatch,
}: {
  filter: DownloadFilter
  counts: Record<DownloadFilter, number>
  query: string
  onFilterChange: (filter: DownloadFilter) => void
  onQueryChange: (query: string) => void
  onAdd: () => void
  /** Hộp Thêm hàng loạt (Ctrl+B). */
  onBatch?: () => void
}) {
  return (
    <div data-toolbar className="flex flex-wrap items-center gap-3">
      <FilterBar
        filter={filter}
        counts={counts}
        onFilterChange={onFilterChange}
      />
      <div className="ms-auto flex items-center gap-2">
        <SearchField query={query} onQueryChange={onQueryChange} />
        {onBatch ? (
          <Button
            variant="outline"
            className="h-9 px-3 text-sm"
            title={t("batch.shortcut", { keys: `${MOD_KEY}+B` })}
            onClick={onBatch}
          >
            <ListPlus />
            {t("batch.button")}
          </Button>
        ) : null}
        <Button
          data-primary
          className="h-9 px-3.5 text-sm shadow-md shadow-primary/25"
          onClick={onAdd}
        >
          <Plus />
          {t("toolbar.add")}
        </Button>
      </div>
    </div>
  )
}
