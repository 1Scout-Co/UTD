import { useRef, useState, type ReactNode } from "react"

import { Copy, Minus, Square, X } from "lucide-react"
import { cn } from "cn"

import { runCaption, type CaptionButton } from "./api"
import { desktopPlatform } from "./platform"
import {
  useCaptionReport,
  useCaptionState,
  useCaptionTip,
  useTitleBarMouse,
  useWindowFrame,
  useWindowMenuKey,
} from "./use-window-frame"

const CONTROLS: {
  button: CaptionButton
  label: (maximized: boolean) => string
}[] = [
  { button: "minimize", label: () => "Thu nhỏ" },
  {
    button: "maximize",
    label: (maximized) => (maximized ? "Khôi phục" : "Phóng to"),
  },
  { button: "close", label: () => "Đóng" },
]

function CaptionIcon({
  button,
  maximized,
}: {
  button: CaptionButton
  maximized: boolean
}) {
  const props = { className: "relative size-4", strokeWidth: 1.5 }
  if (button === "minimize") return <Minus {...props} />
  if (button === "close") return <X {...props} />
  // Khôi phục: hai ô chồng nhau như nút gốc Windows 11 (lật ngang để ô sau nằm góc trên phải).
  if (maximized)
    return <Copy {...props} className="relative size-4 -scale-x-100" />
  return <Square {...props} className="relative size-3.5" />
}

/**
 * Một nút cửa sổ. `hover`/`pressed`: theo Rust (Windows); `tip`: đang hiện chú thích dưới nút. Không dùng
 * `title`: chuột trên nút đi vào cửa sổ con của Rust nên webview không bao giờ hiện `title`.
 */
function CaptionControl({
  button,
  label,
  maximized,
  hover,
  pressed,
  tip,
  onPointer,
}: {
  button: CaptionButton
  label: string
  maximized: boolean
  hover: boolean
  pressed: boolean
  tip: boolean
  onPointer: (button: CaptionButton | null) => void
}) {
  return (
    <button
      type="button"
      tabIndex={-1}
      data-caption={button}
      data-hover={hover || undefined}
      data-pressed={pressed || undefined}
      aria-label={label}
      onMouseEnter={() => onPointer(button)}
      onMouseLeave={() => onPointer(null)}
      onClick={() => void runCaption(button).catch(() => undefined)}
      className={cn(
        "group/caption relative grid h-10 w-11.5 place-items-center text-foreground/80 outline-none group-data-[focused=false]/titlebar:text-foreground/40",
        button === "close" &&
          "hover:text-white data-hover:text-white data-pressed:text-white"
      )}
    >
      <span
        aria-hidden
        className={cn(
          "absolute inset-1 rounded-lg transition-colors duration-100",
          button === "close"
            ? "group-hover/caption:bg-destructive group-data-hover/caption:bg-destructive group-data-pressed/caption:bg-destructive/85"
            : "group-hover/caption:bg-foreground/6 group-data-hover/caption:bg-foreground/6 group-data-pressed/caption:bg-foreground/10"
        )}
      />
      <CaptionIcon button={button} maximized={maximized} />
      {tip ? (
        <span
          aria-hidden
          data-caption-tip
          className="pointer-events-none absolute top-full right-1 mt-1 rounded-md bg-foreground px-2 py-1 text-xs whitespace-nowrap text-background shadow-md"
        >
          {label}
        </span>
      ) : null}
    </button>
  )
}

/**
 * Thu nhỏ · Phóng to/Khôi phục · Đóng kiểu Windows 11 theo style app: ô 46×40 sát mép phải (rê ra góc
 * vẫn trúng Đóng), nền rê là ô bo góc bên trong, Đóng rê thành đỏ, rê yên thì hiện chú thích. Chuột trên
 * nút do cửa sổ con của Rust nhận (Snap Layouts, bấm được cả khi có hộp thoại) nên trạng thái rê/nhấn lấy từ
 * `useCaptionState`; chuột của webview và `onClick` là đường lui khi không có cửa sổ con đó (bản thử e2e).
 * Như nút gốc, không nằm trong thứ tự Tab: bàn phím dùng Alt+Space, Alt+F4, Win+↑/↓.
 */
function WindowControls({ maximized }: { maximized: boolean }) {
  const root = useRef<HTMLDivElement>(null)
  const caption = useCaptionState()
  const [pointer, setPointer] = useState<CaptionButton | null>(null)
  const tip = useCaptionTip(caption.hover ?? pointer, caption.pressed)
  useCaptionReport(root)
  useWindowMenuKey()
  return (
    <div
      ref={root}
      role="group"
      aria-label="Điều khiển cửa sổ"
      className="flex self-stretch no-drag-region"
    >
      {CONTROLS.map(({ button, label }) => (
        <CaptionControl
          key={button}
          button={button}
          label={label(maximized)}
          maximized={maximized}
          hover={caption.hover === button}
          pressed={caption.pressed === button}
          tip={tip === button}
          onPointer={setPointer}
        />
      ))}
    </div>
  )
}

/**
 * Thanh tiêu đề riêng, hoà vào nền app, cao 40px (`TITLE_BAR_HEIGHT`). Kéo để di chuyển, nhấp đúp để phóng to:
 * Windows dùng `app-region: drag` của WebView2 (như thanh gốc: Aero Snap, chuột phải ra menu hệ thống;
 * `data-tauri-drag-region` là đường lui), macOS do `useTitleBarMouse` (nhấp đúp theo cài đặt hệ thống).
 * macOS giữ đèn giao thông gốc nên chừa chỗ bên trái (trừ khi toàn màn hình: đèn ẩn, không kéo).
 * Nằm trên lớp phủ hộp thoại để vẫn kéo được khi đang mở hộp thoại; menu/tooltip không vào dải này
 * (`POPUP_COLLISION_PADDING`).
 */
export function TitleBar({
  className,
  children,
}: {
  className?: string
  children?: ReactNode
}) {
  const [platform] = useState(desktopPlatform)
  const frame = useWindowFrame(platform)
  const lights = platform === "macos" && !frame.fullscreen
  const mouse = useTitleBarMouse(lights)
  const windows = platform === "windows"
  return (
    <div
      data-titlebar={platform ?? "web"}
      data-focused={frame.focused}
      data-maximized={frame.maximized}
      data-tauri-drag-region={windows ? "deep" : undefined}
      onMouseDown={mouse.onMouseDown}
      onMouseUp={mouse.onMouseUp}
      className={cn(
        "group/titlebar relative z-60 flex h-10 shrink-0 items-center select-none",
        windows && "drag-region",
        lights ? "pl-21" : "pl-5.5",
        className
      )}
    >
      <div className="flex min-w-0 flex-1 items-center transition-opacity group-data-[focused=false]/titlebar:opacity-50">
        {children}
      </div>
      {windows ? <WindowControls maximized={frame.maximized} /> : null}
    </div>
  )
}
