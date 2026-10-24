import type { ComponentType } from "react"

import {
  AlertCircle,
  Box,
  Cpu,
  Disc3,
  Download,
  FileText,
  Film,
  FolderOpen,
  Gamepad2,
  Package,
  Plus,
  RefreshCw,
  ShieldAlert,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import { t } from "@/shared/i18n/i18n"
import { cn } from "cn"

import type { DownloadKind, ScreenState } from "../model/types"

type IconComponent = ComponentType<{ className?: string; strokeWidth?: number }>

const KIND_ICON: Record<DownloadKind, IconComponent> = {
  game: Gamepad2,
  model: Cpu,
  iso: Disc3,
  video: Film,
  archive: Package,
  document: FileText,
  app: Box,
}

export function KindIcon({
  kind,
  tone,
  size = "md",
}: {
  kind: DownloadKind
  tone: string
  size?: "sm" | "md" | "lg"
}) {
  const Icon = KIND_ICON[kind]
  const box =
    size === "sm"
      ? "size-7 rounded-md"
      : size === "lg"
        ? "size-11 rounded-xl"
        : "size-9 rounded-lg"
  const glyph = size === "sm" ? "size-3.5" : size === "lg" ? "size-5" : "size-4"

  return (
    <div
      className={cn("grid shrink-0 place-items-center", box)}
      style={{ color: tone, backgroundColor: `${tone}1a` }}
    >
      <Icon className={glyph} strokeWidth={1.8} />
    </div>
  )
}

export function Meter({
  value,
  tone,
  className,
  height = 6,
  label = t("table.progress"),
}: {
  value: number
  tone?: string
  className?: string
  height?: number
  label?: string
}) {
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(value)}
      className={cn(
        "relative w-full overflow-hidden rounded-full bg-muted",
        className
      )}
      style={{ height }}
    >
      <div
        className="h-full rounded-full transition-[width] duration-500"
        style={{
          width: `${Math.max(0, Math.min(100, value))}%`,
          backgroundColor: tone ?? "var(--primary)",
        }}
      />
    </div>
  )
}

export function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string
  checked: boolean
  onChange: (checked: boolean) => void
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className="flex w-full items-center justify-between gap-4 text-start"
    >
      <span className="text-sm">{label}</span>
      <span
        className={cn(
          "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full border transition-colors",
          checked ? "border-primary bg-primary" : "border-border bg-muted"
        )}
      >
        <span
          className={cn(
            "block size-3.5 rounded-full bg-background shadow-sm transition-transform",
            checked ? "translate-x-[18px]" : "translate-x-[3px]"
          )}
        />
      </span>
    </button>
  )
}

const STATE_CODE: Record<Exclude<ScreenState, "data">, string> = {
  empty: "S-02",
  loading: "S-03",
  error: "S-04",
}

type StateProps = {
  onAdd: () => void
  onRetry: () => void
  onSettings?: () => void
  className?: string
  soft: boolean
}

function LoadingState({ className, soft }: StateProps) {
  return (
    <div
      data-s={STATE_CODE.loading}
      className={cn(
        "flex min-h-[420px] flex-col gap-3 rounded-xl p-4",
        soft ? "bg-muted/60" : "border border-border/70",
        className
      )}
    >
      <div className="text-xs font-semibold tracking-wider text-muted-foreground uppercase">
        {t("state.loading")}
      </div>
      {Array.from({ length: 5 }, (_, index) => (
        <div
          key={index}
          className="flex animate-pulse items-center gap-3 rounded-lg bg-card p-3 shadow-soft"
        >
          <div className="size-9 rounded-lg bg-muted" />
          <div className="min-w-0 flex-1 space-y-2">
            <div className="h-2.5 w-1/3 rounded bg-muted" />
            <div className="h-2 w-2/3 rounded bg-muted" />
          </div>
          <div className="h-2 w-20 rounded bg-muted" />
        </div>
      ))}
    </div>
  )
}

function ErrorState({ onRetry, className, soft }: StateProps) {
  return (
    <div
      data-s={STATE_CODE.error}
      className={cn(
        "flex min-h-[420px] flex-col items-center justify-center gap-3 rounded-xl bg-destructive/5 p-8 text-center",
        !soft && "border border-dashed border-destructive/30",
        className
      )}
    >
      <div className="grid size-11 place-items-center rounded-xl bg-destructive/10 text-destructive">
        <AlertCircle className="size-5" />
      </div>
      <h2 className="text-lg font-semibold">{t("state.errorTitle")}</h2>
      <p className="max-w-sm text-sm leading-6 text-muted-foreground">
        {t("state.errorBody")}
      </p>
      <div className="mt-1 flex gap-2">
        <Button data-primary onClick={onRetry}>
          <RefreshCw />
          {t("app.retry")}
        </Button>
        <Button variant="outline">
          <FolderOpen />
          {t("state.openLog")}
        </Button>
      </div>
    </div>
  )
}

function EmptyState({ onAdd, onSettings, className, soft }: StateProps) {
  return (
    <div
      data-s={STATE_CODE.empty}
      className={cn(
        "flex min-h-[420px] flex-col items-center justify-center gap-3 rounded-xl p-8 text-center",
        soft ? "bg-muted/60" : "border border-dashed border-border bg-muted/20",
        className
      )}
    >
      <div className="grid size-12 place-items-center rounded-2xl bg-primary/10 text-primary">
        <Download className="size-6" />
      </div>
      <h2 className="text-lg font-semibold">{t("state.emptyTitle")}</h2>
      <p className="max-w-sm text-sm leading-6 text-muted-foreground">
        {t("state.emptyBody")}
      </p>
      <div className="mt-1 flex gap-2">
        <Button data-primary onClick={onAdd}>
          <Plus />
          {t("toolbar.add")}
        </Button>
        <Button variant="outline" onClick={onSettings}>
          <ShieldAlert />
          {t("browser.install")}
        </Button>
      </div>
    </div>
  )
}

export function StateView({
  state,
  soft = false,
  ...props
}: Omit<StateProps, "soft"> & {
  state: Exclude<ScreenState, "data">
  soft?: boolean
}) {
  if (state === "loading") return <LoadingState soft={soft} {...props} />
  if (state === "error") return <ErrorState soft={soft} {...props} />
  return <EmptyState soft={soft} {...props} />
}
