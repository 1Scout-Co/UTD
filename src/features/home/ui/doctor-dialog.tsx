import { useState } from "react"

import {
  AlertTriangle,
  Check,
  Copy,
  Link2,
  RefreshCw,
  RotateCcw,
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
import { t } from "@/shared/i18n/i18n"

import { diagnose, type DoctorAction, type DoctorCategory } from "../model/doctor"
import type { DownloadItem } from "../model/types"
import { OpenPageButton } from "./sign-in-notice"

// Nhãn nhóm: t(`doctor.cat.${category}`). Màn Chi tiết (S-21) dùng chung.
export const CATEGORY_STYLE: Record<
  DoctorCategory,
  { icon: typeof WifiOff; box: string }
> = {
  network: {
    icon: WifiOff,
    box: "bg-amber-500/10 text-amber-700",
  },
  access: {
    icon: AlertTriangle,
    box: "bg-sky-500/10 text-sky-700",
  },
  security: {
    icon: ShieldAlert,
    box: "bg-destructive/10 text-destructive",
  },
  tool: {
    icon: Wrench,
    box: "bg-violet-500/10 text-violet-700",
  },
  disk: {
    icon: AlertTriangle,
    box: "bg-orange-500/10 text-orange-700",
  },
  unknown: {
    icon: Stethoscope,
    box: "bg-primary/10 text-primary",
  },
}

// Nút chính theo cách sửa: nhãn + icon.
const ACTION_BUTTON: Record<
  DoctorAction,
  { icon: typeof RefreshCw; label: () => string }
> = {
  resume: { icon: RefreshCw, label: () => t("menu.resume") },
  refreshLink: { icon: Link2, label: () => t("recover.menu") },
  restart: { icon: RotateCcw, label: () => t("recover.restart") },
}

export function DoctorDialog({
  item,
  open,
  onOpenChange,
  onRetry,
  onRefreshLink,
  onRestart,
}: {
  item: DownloadItem | null
  open: boolean
  onOpenChange: (open: boolean) => void
  onRetry: (id: string) => void
  /** Có ⇒ link hết hạn hiện nút "Cập nhật link…". */
  onRefreshLink?: (id: string) => void
  /** Có ⇒ file đã đổi / `.part` mất hiện nút "Tải lại từ đầu". */
  onRestart?: (id: string) => void
}) {
  const [copied, setCopied] = useState(false)
  if (!item) return null
  const report = diagnose(item)
  const style = CATEGORY_STYLE[report.category]
  const Icon = style.icon
  const code = item.errorCode ?? "unknown"
  const handlers: Record<DoctorAction, ((id: string) => void) | undefined> = {
    resume: onRetry,
    refreshLink: onRefreshLink,
    restart: onRestart,
  }
  const run = report.action ? handlers[report.action] : undefined
  const button = report.action ? ACTION_BUTTON[report.action] : null
  const ButtonIcon = button?.icon

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
          <DialogTitle>{t("doctor.title")}</DialogTitle>
          <DialogDescription>{t("doctor.description")}</DialogDescription>
        </DialogHeader>

        <div className="flex items-start gap-3 rounded-xl bg-muted p-3">
          <div className={`grid size-10 shrink-0 place-items-center rounded-lg ${style.box}`}>
            <Icon className="size-4" />
          </div>
          <div className="min-w-0 flex-1">
            <div dir="auto" className="truncate text-sm font-semibold [text-align:match-parent]" title={item.name}>
              {item.name}
            </div>
            <div className="mt-0.5 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
              <span>{t(`status.${item.status}`)}</span>
              <span>·</span>
              <span>{t(`doctor.cat.${report.category}`)}</span>
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
          <div className="text-sm font-medium">{t("doctor.steps")}</div>
          <ol className="mt-2 space-y-1.5 ps-4 text-sm text-muted-foreground">
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
            {copied ? t("doctor.copied") : t("doctor.copyCode")}
          </Button>
          <div className="flex gap-2">
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
            >
              {t("app.close")}
            </Button>
            {report.signIn && item.url ? (
              <OpenPageButton url={item.url} outline={Boolean(run)} />
            ) : null}
            {run && button && ButtonIcon ? (
              <Button
                type="button"
                onClick={() => {
                  run(item.id)
                  onOpenChange(false)
                }}
              >
                <ButtonIcon />
                {button.label()}
              </Button>
            ) : null}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
