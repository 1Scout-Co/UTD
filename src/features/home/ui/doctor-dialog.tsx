import { useState } from "react"

import {
  AlertTriangle,
  Check,
  Copy,
  RefreshCw,
  ShieldAlert,
  Stethoscope,
  WifiOff,
  Wrench,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

import { diagnose, type DoctorCategory } from "../model/doctor"
import type { DownloadItem } from "../model/types"

const CATEGORY_STYLE: Record<
  DoctorCategory,
  { icon: typeof WifiOff; box: string; text: string }
> = {
  network: {
    icon: WifiOff,
    box: "bg-amber-500/10 text-amber-700",
    text: "Mạng",
  },
  access: {
    icon: AlertTriangle,
    box: "bg-sky-500/10 text-sky-700",
    text: "Quyền truy cập",
  },
  security: {
    icon: ShieldAlert,
    box: "bg-destructive/10 text-destructive",
    text: "An toàn",
  },
  tool: {
    icon: Wrench,
    box: "bg-violet-500/10 text-violet-700",
    text: "Công cụ",
  },
  disk: {
    icon: AlertTriangle,
    box: "bg-orange-500/10 text-orange-700",
    text: "Ổ đĩa",
  },
  unknown: {
    icon: Stethoscope,
    box: "bg-primary/10 text-primary",
    text: "Chưa rõ",
  },
}

const STATUS_LABEL: Record<string, string> = {
  failed: "Có lỗi",
  quarantined: "Đã cách ly",
  paused: "Tạm dừng",
}

export function DoctorDialog({
  item,
  open,
  onOpenChange,
  onRetry,
}: {
  item: DownloadItem | null
  open: boolean
  onOpenChange: (open: boolean) => void
  onRetry: (id: string) => void
}) {
  const [copied, setCopied] = useState(false)
  if (!item) return null
  const report = diagnose(item)
  const style = CATEGORY_STYLE[report.category]
  const Icon = style.icon
  const code = item.errorCode ?? "unknown"

  const copyCode = async () => {
    try {
      await navigator.clipboard.writeText(code)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1500)
    } catch {
      // Clipboard có thể bị chặn; mã vẫn hiển thị để chọn tay.
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent data-variant="soniox" className="sm:max-w-[560px]">
        <DialogHeader>
          <DialogTitle>Chẩn đoán download</DialogTitle>
          <DialogDescription>
            Dựa trên lỗi engine ghi trong journal cục bộ.
          </DialogDescription>
        </DialogHeader>

        <div className="flex items-start gap-3 rounded-xl bg-muted p-3">
          <div className={`grid size-10 shrink-0 place-items-center rounded-lg ${style.box}`}>
            <Icon className="size-4" />
          </div>
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-semibold" title={item.name}>
              {item.name}
            </div>
            <div className="mt-0.5 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
              <span>{STATUS_LABEL[item.status] ?? item.status}</span>
              <span>·</span>
              <span>{style.text}</span>
            </div>
          </div>
          <span className="shrink-0 rounded-full bg-card px-2 py-0.5 font-mono text-xs text-muted-foreground shadow-soft">
            {code}
          </span>
        </div>

        <div className="space-y-2">
          <h3 className="text-md font-semibold">{report.title}</h3>
          <p className="text-sm text-muted-foreground">
            {report.summary}
          </p>
        </div>

        <div className="rounded-xl bg-muted p-3.5">
          <div className="text-sm font-medium">
            Cách xử lý
          </div>
          <ol className="mt-2 space-y-1.5 pl-4 text-sm text-muted-foreground">
            {report.steps.map((step) => (
              <li key={step} className="list-decimal">
                {step}
              </li>
            ))}
          </ol>
        </div>

        <div className="flex items-center justify-between gap-3">
          <Button
            type="button"
            variant="outline"
            onClick={() => void copyCode()}
          >
            {copied ? <Check /> : <Copy />}
            {copied ? "Đã chép mã" : `Chép mã lỗi`}
          </Button>
          <div className="flex gap-2">
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
            >
              Đóng
            </Button>
            {report.retryable ? (
              <Button
                type="button"
                onClick={() => {
                  onRetry(item.id)
                  onOpenChange(false)
                }}
              >
                <RefreshCw />
                Tiếp tục
              </Button>
            ) : null}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
