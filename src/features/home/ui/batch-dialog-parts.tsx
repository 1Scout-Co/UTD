import { useRef, useState, type ChangeEvent } from "react"

import { ClipboardPaste, FileText, FolderOpen } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "cn"

import { formatBytes } from "../model/download-mapper"
import type { BatchRow } from "../model/batch-links"
import type { BatchQuality } from "../model/types"

export type SourceMode = "list" | "pattern"

/** Nút bật/tắt dạng segmented (như bộ lọc ở màn chính), có `aria-pressed`. */
export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
  disabled,
}: {
  label: string
  value: T
  options: { id: T; label: string }[]
  onChange: (value: T) => void
  disabled?: boolean
}) {
  return (
    <div
      role="group"
      aria-label={label}
      className="inline-flex h-9 items-center gap-0.5 rounded-lg bg-muted p-0.5"
    >
      {options.map((option) => (
        <Button
          key={option.id}
          type="button"
          variant="ghost"
          disabled={disabled}
          aria-pressed={value === option.id}
          onClick={() => onChange(option.id)}
          className={cn(
            "h-full rounded-md px-2.5 text-sm",
            value === option.id
              ? "bg-card text-foreground shadow-soft hover:bg-card"
              : "text-muted-foreground hover:bg-card/60",
          )}
        >
          {option.label}
        </Button>
      ))}
    </div>
  )
}

const MODES: { id: SourceMode; label: string }[] = [
  { id: "list", label: "Danh sách" },
  { id: "pattern", label: "Mẫu" },
]

/** File .txt lớn hơn chừng này thì không đọc (10.000 link dài vẫn dưới 5 MB). */
const MAX_TEXT_FILE = 5 * 1024 * 1024

/** Nguồn link: dán danh sách (mỗi dòng một link), nhập .txt, hoặc mẫu `[001-120]`. */
export function BatchSource({
  mode,
  text,
  pattern,
  note,
  busy,
  onMode,
  onText,
  onPattern,
  onAppend,
  onError,
}: {
  mode: SourceMode
  text: string
  pattern: string
  /** Dòng nhỏ dưới ô nhập: số link mẫu tạo, link trùng/rác đã bỏ. */
  note: { text: string; error: boolean } | null
  busy: boolean
  onMode: (mode: SourceMode) => void
  onText: (text: string) => void
  onPattern: (pattern: string) => void
  /** Thêm chữ (dán, đọc file) vào cuối danh sách. */
  onAppend: (text: string) => void
  onError: (message: string) => void
}) {
  const fileRef = useRef<HTMLInputElement>(null)
  const readFile = (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0]
    event.target.value = ""
    if (!file) return
    if (file.size > MAX_TEXT_FILE) {
      onError("File quá lớn (tối đa 5 MB).")
      return
    }
    void file
      .text()
      .then(onAppend)
      .catch(() => onError("Không đọc được file."))
  }
  const paste = () => {
    void navigator.clipboard
      ?.readText()
      .then(onAppend)
      .catch(() => undefined)
  }

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <Segmented label="Nguồn link" value={mode} options={MODES} onChange={onMode} disabled={busy} />
        {mode === "list" ? (
          <div className="ml-auto flex gap-1.5">
            <Button type="button" variant="outline" disabled={busy} onClick={paste}>
              <ClipboardPaste />
              Dán
            </Button>
            <Button
              type="button"
              variant="outline"
              disabled={busy}
              onClick={() => fileRef.current?.click()}
            >
              <FileText />
              Nhập .txt
            </Button>
            <input
              ref={fileRef}
              type="file"
              accept=".txt,text/plain"
              className="hidden"
              aria-hidden
              tabIndex={-1}
              onChange={readFile}
            />
          </div>
        ) : null}
      </div>
      {mode === "list" ? (
        <Textarea
          aria-label="Danh sách link"
          value={text}
          onChange={(event) => onText(event.target.value)}
          placeholder={"Mỗi dòng một link\nhttps://example.com/file-1.zip"}
          disabled={busy}
          spellCheck={false}
          className="max-h-28 min-h-20 font-mono text-sm"
        />
      ) : (
        <Input
          aria-label="Mẫu link"
          value={pattern}
          onChange={(event) => onPattern(event.target.value)}
          placeholder="https://example.com/anh[001-120].jpg"
          disabled={busy}
          spellCheck={false}
          autoComplete="off"
          className="font-mono"
        />
      )}
      <p
        role={note?.error ? "alert" : "status"}
        className={cn(
          "min-h-5 text-sm empty:hidden",
          note?.error ? "text-red-700" : "text-muted-foreground",
        )}
      >
        {note?.text ?? ""}
      </p>
    </div>
  )
}

const ROW_HEIGHT = 36
const VISIBLE_ROWS = 6
const OVERSCAN = 6

function SizeCell({ row, probing }: { row: BatchRow; probing: boolean }) {
  if (row.kind === "media") return <span className="text-muted-foreground">—</span>
  if (row.size === undefined) {
    return <span className="text-muted-foreground">{probing ? "…" : "—"}</span>
  }
  return <span>{row.size === null ? "—" : formatBytes(row.size)}</span>
}

const KIND_LABEL: Record<BatchRow["kind"], string> = {
  file: "File",
  media: "Video",
  page: "Trang web",
}

/**
 * Bảng xem trước: tick từng dòng hoặc tất cả. Chỉ vẽ các dòng đang thấy (mẫu có thể ra 10.000 link);
 * cột số dòng cố định nên tính được vị trí từ `scrollTop`.
 */
export function BatchTable({
  rows,
  probing,
  onToggle,
  onToggleAll,
}: {
  rows: BatchRow[]
  /** Đang dò kích thước: ô chưa có số hiện "…". */
  probing: boolean
  onToggle: (url: string, checked: boolean) => void
  onToggleAll: (checked: boolean) => void
}) {
  const [scrollTop, setScrollTop] = useState(0)
  const checked = rows.filter((row) => row.checked).length
  const first = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN)
  const last = Math.min(rows.length, first + VISIBLE_ROWS + OVERSCAN * 2)
  const shown = rows.slice(first, last)

  return (
    // max-h-63 = 7 dòng (tiêu đề + VISIBLE_ROWS) × ROW_HEIGHT.
    <div
      className="max-h-63 overflow-y-auto rounded-xl bg-muted/60"
      onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
    >
      <table className="w-full table-fixed text-sm">
        <thead className="sticky top-0 z-10 bg-muted text-left text-xs text-muted-foreground">
          <tr className="h-9">
            <th className="w-10 pl-3">
              <Checkbox
                aria-label="Chọn tất cả"
                checked={rows.length > 0 && checked === rows.length}
                indeterminate={checked > 0 && checked < rows.length}
                onCheckedChange={(value) => onToggleAll(value)}
              />
            </th>
            <th className="font-medium">Tên file</th>
            <th className="w-24 font-medium">Kích thước</th>
            <th className="w-24 pr-3 font-medium">Loại</th>
          </tr>
        </thead>
        <tbody>
          {first > 0 ? <tr style={{ height: first * ROW_HEIGHT }} aria-hidden /> : null}
          {shown.map((row) => (
            <tr key={row.url} className="h-9 border-t border-border/40" title={row.url}>
              <td className="pl-3">
                <Checkbox
                  aria-label={`Chọn ${row.name}`}
                  checked={row.checked}
                  onCheckedChange={(value) => onToggle(row.url, value)}
                />
              </td>
              <td className="truncate pr-2">{row.name}</td>
              <td className="tabular-nums">
                <SizeCell row={row} probing={probing} />
              </td>
              <td className="pr-3">
                <span
                  className={cn(
                    "rounded-full px-2 py-0.5 text-xs",
                    row.kind === "media"
                      ? "bg-accent text-accent-foreground"
                      : row.kind === "page"
                        ? "bg-card text-muted-foreground"
                        : "bg-card",
                  )}
                >
                  {KIND_LABEL[row.kind]}
                </span>
              </td>
            </tr>
          ))}
          {last < rows.length ? (
            <tr style={{ height: (rows.length - last) * ROW_HEIGHT }} aria-hidden />
          ) : null}
        </tbody>
      </table>
    </div>
  )
}

/** Lọc nhanh theo loại (đuôi file, Video, Trang web): bấm để tick/bỏ cả nhóm. */
export function GroupChips({
  groups,
  onToggle,
}: {
  groups: { name: string; total: number; checked: number }[]
  onToggle: (name: string, checked: boolean) => void
}) {
  if (groups.length < 2) return null
  return (
    <div role="group" aria-label="Lọc theo loại" className="flex flex-wrap gap-1.5">
      {groups.map((group) => {
        const all = group.checked === group.total
        return (
          <Button
            key={group.name}
            type="button"
            size="xs"
            variant="outline"
            aria-pressed={all}
            onClick={() => onToggle(group.name, !all)}
            className={cn("rounded-full", all ? "bg-accent text-accent-foreground" : "")}
          >
            {group.name}
            <span className="tabular-nums text-muted-foreground">{group.total}</span>
          </Button>
        )
      })}
    </div>
  )
}

const QUALITIES: { id: BatchQuality; label: string }[] = [
  { id: "best", label: "Tốt nhất" },
  { id: "1080", label: "1080p" },
  { id: "720", label: "720p" },
  { id: "audio-m4a", label: "M4A" },
  { id: "audio-mp3", label: "MP3" },
]

/** Một chất lượng cho mọi link video trong lô (không hỏi từng link). */
export function QualityPicker({
  value,
  count,
  onChange,
  disabled,
}: {
  value: BatchQuality
  count: number
  onChange: (value: BatchQuality) => void
  disabled: boolean
}) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <span className="text-sm font-medium">
        Video <span className="text-muted-foreground tabular-nums">({count})</span>
      </span>
      <Segmented
        label="Chất lượng video"
        value={value}
        options={QUALITIES}
        onChange={onChange}
        disabled={disabled}
      />
    </div>
  )
}

/** Nút chọn thư mục lưu (như hộp Thêm download). */
export function FolderButton({
  dir,
  desktop,
  disabled,
  onPick,
}: {
  dir: string
  desktop: boolean
  disabled: boolean
  onPick: () => void
}) {
  return (
    <button
      type="button"
      disabled={!desktop || disabled}
      title={desktop ? dir : "Chọn thư mục chỉ có trong app UTD"}
      aria-label={`Lưu tại ${dir || "Downloads\\UTD"}`}
      onClick={onPick}
      className="flex h-9 min-w-0 flex-1 items-center gap-2 rounded-lg border border-input px-3 text-left text-sm text-muted-foreground transition-colors hover:border-foreground/25 disabled:cursor-not-allowed disabled:opacity-70"
    >
      <FolderOpen className="size-4 shrink-0" />
      <span className="truncate">{dir || "Downloads\\UTD"}</span>
    </button>
  )
}
