import { useEffect, useMemo, useRef, useState } from "react"

import { inspectLink } from "../api/downloads"
import {
  expandPattern,
  groupOf,
  parseLinks,
  patternCount,
  toRow,
  type BatchRow,
} from "../model/batch-links"
import { formatBytes } from "../model/download-mapper"
import type { RemoteFile } from "../model/types"
import type { SourceMode } from "./batch-dialog-parts"

/** Dò kích thước tối đa chừng này link mỗi lần mở hộp, mỗi lúc chừng này link (không dồn một máy chủ). */
const MAX_PROBES = 500
const PARALLEL_PROBES = 6

/**
 * Dò tên + kích thước các link file theo thứ tự, `PARALLEL_PROBES` link một lúc, không chặn giao diện.
 * Link đã dò (hoặc đang dò) không dò lại khi danh sách đổi; `null` = dò lỗi.
 */
function useLinkSizes(urls: string[], enabled: boolean) {
  const [found, setFound] = useState<ReadonlyMap<string, RemoteFile | null>>(new Map())
  const [pending, setPending] = useState(0)
  const started = useRef(new Set<string>())

  useEffect(() => {
    if (!enabled) return
    const room = Math.max(0, MAX_PROBES - started.current.size)
    const todo = urls.filter((url) => !started.current.has(url)).slice(0, room)
    if (todo.length === 0) return
    let stopped = false
    const worker = async () => {
      while (!stopped && todo.length > 0) {
        const url = todo.shift() as string
        started.current.add(url)
        setPending((count) => count + 1)
        const result = await inspectLink(url).catch(() => null)
        setFound((previous) => new Map(previous).set(url, result))
        setPending((count) => count - 1)
      }
    }
    for (let index = 0; index < PARALLEL_PROBES; index += 1) void worker()
    return () => {
      stopped = true
    }
  }, [urls, enabled])

  return { found, probing: pending > 0 }
}

/** Kết quả dò ghép vào dòng: tên server đề xuất, kích thước, trang web ⇒ bỏ tick (nếu chưa tự tick). */
function withProbe(row: BatchRow, remote: RemoteFile | null | undefined, fromBrowser: boolean): BatchRow {
  if (row.kind === "media" || remote === undefined) return row
  if (remote === null) return { ...row, size: null }
  const page = remote.page
  return {
    ...row,
    name: remote.fileName || row.name,
    size: remote.totalBytes,
    kind: page ? "page" : "file",
    checked: page ? false : row.checked || fromBrowser,
  }
}

interface Source {
  mode: SourceMode
  text: string
  pattern: string
}

/** Danh sách link từ nguồn đang chọn + dòng ghi chú (số link mẫu, link trùng/rác đã bỏ). */
export function useSourceLinks({ mode, text, pattern }: Source) {
  return useMemo(() => {
    if (mode === "pattern") {
      if (!pattern.trim()) return { links: [] as string[], note: null }
      const expanded = expandPattern(pattern)
      if (!expanded.ok) return { links: [] as string[], note: { text: expanded.error, error: true } }
      const count = patternCount(pattern).toLocaleString("vi-VN")
      return { links: expanded.links, note: { text: `Mẫu tạo ${count} link.`, error: false } }
    }
    const parsed = parseLinks(text)
    const dropped = [
      parsed.duplicates ? `${parsed.duplicates} trùng` : "",
      parsed.junk ? `${parsed.junk} dòng không phải link` : "",
      parsed.over ? `${parsed.over} vượt 10.000` : "",
    ].filter(Boolean)
    const note = dropped.length ? { text: `Đã bỏ ${dropped.join(", ")}.`, error: false } : null
    return { links: parsed.links, note }
  }, [mode, text, pattern])
}

/** Dòng bảng = link + kết quả dò + tick của người dùng (giữ theo URL khi danh sách đổi). */
export function useBatchRows(links: string[], fromBrowser: boolean, desktop: boolean, open: boolean) {
  const [ticks, setTicks] = useState<ReadonlyMap<string, boolean>>(new Map())
  const fileLinks = useMemo(() => links.filter((url) => toRow(url).kind === "file"), [links])
  const { found, probing } = useLinkSizes(fileLinks, desktop && open)
  const rows = useMemo(
    () =>
      links.map((url) => {
        const row = withProbe(toRow(url, fromBrowser), found.get(url), fromBrowser)
        const tick = ticks.get(url)
        return tick === undefined ? row : { ...row, checked: tick }
      }),
    [links, found, ticks, fromBrowser],
  )
  const setMany = (urls: string[], checked: boolean) =>
    setTicks((previous) => {
      const next = new Map(previous)
      for (const url of urls) next.set(url, checked)
      return next
    })
  return { rows, probing, setMany }
}

export function groupsOf(rows: BatchRow[]) {
  const groups = new Map<string, { name: string; total: number; checked: number }>()
  for (const row of rows) {
    const name = groupOf(row)
    const group = groups.get(name) ?? { name, total: 0, checked: 0 }
    group.total += 1
    if (row.checked) group.checked += 1
    groups.set(name, group)
  }
  return [...groups.values()].sort((left, right) => right.total - left.total)
}

export function summaryOf(rows: BatchRow[]): string {
  const chosen = rows.filter((row) => row.checked)
  const bytes = chosen.reduce((total, row) => total + (row.size ?? 0), 0)
  const size = bytes > 0 ? ` · ${formatBytes(bytes)}` : ""
  return `Đã chọn ${chosen.length}/${rows.length}${size}`
}
