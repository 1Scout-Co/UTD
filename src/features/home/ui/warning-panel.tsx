import { useState } from "react"

import {
  ChevronDown,
  Download,
  FileWarning,
  Lock,
  ShieldAlert,
  SkipForward,
  type LucideIcon,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import { t } from "@/shared/i18n/i18n"
import { cn } from "cn"

import {
  countByKind,
  hostOfLink,
  visibleName,
  type WarnedLink,
  type WarningKind,
} from "../model/warnings"

// Nút đỏ nhạt: chữ đỏ mặc định chỉ đạt 4,15:1 trên nền này (WCAG AA cần 4,5:1).
const DANGER = "text-red-700"

const KIND_ICON: Record<WarningKind, LucideIcon> = {
  hidden_chars: FileWarning,
  spoofed_name: FileWarning,
  dangerous_type: ShieldAlert,
  mime_mismatch: FileWarning,
  insecure_http: Lock,
}

function Heading({ children }: { children?: React.ReactNode }) {
  return (
    <div className="flex items-start gap-3">
      <div className="grid size-10 shrink-0 place-items-center rounded-lg bg-amber-100 text-amber-800">
        <ShieldAlert className="size-5" />
      </div>
      <div className="min-w-0 flex-1">
        <h3 className="text-md font-semibold">{t("warn.title")}</h3>
        {children}
      </div>
    </div>
  )
}

/** Tên file (ký tự ẩn hiện thành mã) + máy chủ. */
function LinkName({ link }: { link: WarnedLink }) {
  return (
    <div className="min-w-0">
      <div dir="auto" className="truncate text-sm font-medium [text-align:match-parent]" title={link.url}>
        {visibleName(link.fileName, link.warnings) || hostOfLink(link.url)}
      </div>
      <div dir="auto" className="truncate text-xs text-muted-foreground [text-align:match-parent]">
        {hostOfLink(link.url)}
      </div>
    </div>
  )
}

/** Mỗi cảnh báo một dòng ngắn; mở chi tiết thì thêm câu giải thích. */
function WarningLines({ link, explain }: { link: WarnedLink; explain: boolean }) {
  return (
    <ul className="space-y-1.5">
      {link.warnings.map((warning) => {
        const Icon = KIND_ICON[warning.kind]
        return (
          <li key={`${warning.kind}:${warning.detail}`} className="flex items-start gap-2 text-sm">
            <Icon className="mt-0.5 size-4 shrink-0 text-amber-700" />
            <div className="min-w-0">
              <span className="font-medium">{t(`warn.kind.${warning.kind}`)}</span>
              <code dir="ltr" className="ms-2 rounded bg-muted px-1.5 py-0.5 font-mono text-xs text-muted-foreground">
                {warning.detail}
              </code>
              {explain ? (
                <p className="mt-0.5 text-xs text-muted-foreground">
                  {t(`warn.why.${warning.kind}`, { detail: warning.detail })}
                </p>
              ) : null}
            </div>
          </li>
        )
      })}
    </ul>
  )
}

function DetailsToggle({ open, onToggle }: { open: boolean; onToggle: () => void }) {
  return (
    <Button type="button" variant="ghost" aria-expanded={open} onClick={onToggle}>
      <ChevronDown className={cn("transition-transform", open && "rotate-180")} />
      {open ? t("warn.hideDetails") : t("warn.details")}
    </Button>
  )
}

/** S-22 cho một link: vẫn tải (có ý thức), huỷ (nút chính, an toàn), xem chi tiết. */
export function SingleWarning({
  link,
  busy,
  onProceed,
  onCancel,
}: {
  link: WarnedLink
  busy: boolean
  onProceed: () => void
  onCancel: () => void
}) {
  const [open, setOpen] = useState(false)
  return (
    <section data-s="S-22" aria-label={t("warn.title")} className="space-y-4">
      <Heading>
        <LinkName link={link} />
      </Heading>
      <div className="rounded-xl bg-amber-50/70 p-3 ring-1 ring-amber-200">
        <WarningLines link={link} explain={open} />
      </div>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <DetailsToggle open={open} onToggle={() => setOpen((value) => !value)} />
        <div className="flex gap-2">
          <Button type="button" variant="destructive" className={DANGER} disabled={busy} onClick={onProceed}>
            <Download />
            {busy ? t("app.adding") : t("warn.proceed")}
          </Button>
          <Button data-primary type="button" disabled={busy} onClick={onCancel}>
            {t("app.cancel")}
          </Button>
        </div>
      </div>
    </section>
  )
}

/** S-22 cho cả lô (hàng loạt, lô từ trình duyệt): một lần hỏi, đếm theo loại, chi tiết từng link khi mở. */
export function BatchWarning({
  warned,
  total,
  busy,
  onSkip,
  onProceed,
  onCancel,
}: {
  warned: WarnedLink[]
  total: number
  busy: boolean
  /** Bỏ các link có cảnh báo, tải phần còn lại (nút chính khi còn link sạch). */
  onSkip: () => void
  onProceed: () => void
  onCancel: () => void
}) {
  const [open, setOpen] = useState(false)
  const rest = total - warned.length
  return (
    <section data-s="S-22" aria-label={t("warn.title")} className="space-y-3">
      <Heading>
        <p className="text-sm text-muted-foreground tabular-nums">
          {t("warn.batchCount", { count: warned.length, total })}
        </p>
      </Heading>
      <ul className="flex flex-wrap gap-2">
        {countByKind(warned).map(({ kind, count }) => {
          const Icon = KIND_ICON[kind]
          return (
            <li key={kind} className="inline-flex items-center gap-1.5 rounded-full bg-amber-50 px-2.5 py-1 text-xs font-medium text-amber-800 ring-1 ring-amber-200">
              <Icon className="size-3.5" />
              {t(`warn.kind.${kind}`)}
              <span className="tabular-nums">{count}</span>
            </li>
          )
        })}
      </ul>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <DetailsToggle open={open} onToggle={() => setOpen((value) => !value)} />
        <div className="flex flex-wrap gap-2">
          <Button type="button" variant={rest > 0 ? "ghost" : "default"} data-primary={rest > 0 ? undefined : true} disabled={busy} onClick={onCancel}>
            {t("app.cancel")}
          </Button>
          <Button type="button" variant="destructive" className={DANGER} disabled={busy} onClick={onProceed}>
            <Download />
            {t("warn.proceedAll")}
          </Button>
          {rest > 0 ? (
            <Button data-primary type="button" disabled={busy} onClick={onSkip}>
              <SkipForward />
              {t("warn.skip")}
            </Button>
          ) : null}
        </div>
      </div>
      {/* Danh sách dưới hàng nút: lô dài mở chi tiết thì nút vẫn ở chỗ cũ. */}
      {open ? (
        <ul className="max-h-56 space-y-2 overflow-y-auto rounded-xl bg-muted/60 p-3">
          {warned.map((link) => (
            <li key={link.url} className="space-y-1.5 border-b border-border/60 pb-2 last:border-0 last:pb-0">
              <LinkName link={link} />
              <WarningLines link={link} explain />
            </li>
          ))}
        </ul>
      ) : null}
    </section>
  )
}
