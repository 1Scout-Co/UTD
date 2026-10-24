import { cn } from "cn"

// Cờ SVG 3:2 chép từ country-flag-icons 1.6.20 (MIT, flags/LICENSE): Windows không vẽ emoji cờ
// (Segoe UI Emoji ra hai chữ cái). Mỗi cờ là một tệp riêng trong dist/assets, chỉ tải khi hiện.
const FLAGS = import.meta.glob<string>("./flags/*.svg", {
  eager: true,
  query: "?url&no-inline",
  import: "default",
})

export function Flag({
  code,
  className,
}: {
  code: string
  className?: string
}) {
  return (
    <img
      src={FLAGS[`./flags/${code}.svg`]}
      alt=""
      draggable={false}
      className={cn(
        "h-3.5 w-5 shrink-0 rounded-xs object-cover ring-1 ring-foreground/10",
        className
      )}
    />
  )
}
