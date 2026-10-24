import { useEffect, useMemo, useRef, useState } from "react"

import { isDesktopApp } from "../../api/downloads"
import { fileIcons } from "../../api/files"
import type { DownloadItem } from "../../model/types"
import { KindIcon } from "../parts"
import { mediaSourceOf, SourceLogo } from "../source-logos"

/** Tên miền nguồn: dòng phụ của mỗi hàng có dạng "youtube.com · Video". */
function hostOf(item: DownloadItem): string {
  return item.subtitle.split(" · ")[0] ?? ""
}

/**
 * Icon thật của file (như Explorer) cho các hàng đang hiện, chỉ trong app desktop.
 * Gọi theo lô, nhớ kết quả; gọi lại một lần khi task chuyển sang "completed"
 * (file .exe hoàn tất mới có icon riêng).
 */
export function useFileIcons(rows: DownloadItem[]) {
  const desktop = isDesktopApp()
  const [icons, setIcons] = useState<Record<string, string>>({})
  const asked = useRef(new Map<string, boolean>())
  const key = useMemo(
    () =>
      rows.map((row) => `${row.id}:${row.status === "completed"}`).join("|"),
    [rows]
  )

  useEffect(() => {
    if (!desktop) return
    const need = key
      .split("|")
      .filter(Boolean)
      .map((part) => part.split(":"))
      .filter(([id, done]) => {
        const before = asked.current.get(id)
        return before === undefined || (done === "true" && !before)
      })
    if (need.length === 0) return
    for (const [id, done] of need) asked.current.set(id, done === "true")
    void fileIcons(need.map(([id]) => id))
      .then((result) => setIcons((previous) => ({ ...previous, ...result })))
      .catch(() => undefined)
  }, [desktop, key])

  return icons
}

/** Thứ tự như IDM: media → logo trang nguồn (kèm icon file nhỏ ở góc); file → icon thật; còn lại icon theo loại. */
export function ItemIcon({
  item,
  icon,
}: {
  item: DownloadItem
  icon?: string
}) {
  const source = mediaSourceOf(hostOf(item))
  if (source) {
    return (
      <div className="relative grid size-9 shrink-0 place-items-center rounded-lg bg-card shadow-soft ring-1 ring-foreground/5">
        <SourceLogo source={source} className="size-5" />
        {icon ? (
          <img
            src={icon}
            alt=""
            draggable={false}
            className="absolute -end-1 -bottom-1 size-4 rounded-sm bg-card ring-2 ring-card"
          />
        ) : null}
      </div>
    )
  }
  if (icon) {
    return (
      <div className="grid size-9 shrink-0 place-items-center">
        <img src={icon} alt="" draggable={false} className="size-8" />
      </div>
    )
  }
  return <KindIcon kind={item.kind} tone="#1881ff" size="md" />
}
