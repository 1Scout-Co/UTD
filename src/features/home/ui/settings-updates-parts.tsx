import { useId } from "react"

import { Download, Info, Loader2, RefreshCw, ShieldCheck } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { formatBytes, formatDate } from "@/shared/i18n/format"
import { t, tx } from "@/shared/i18n/i18n"

import type { UpdateInfo, UpdateProgress } from "../model/types"
import { percentOf, StageProgress } from "./stage-progress"

// Các mảnh hiển thị của mục "Cập nhật" trong Cài đặt (logic nằm ở settings-updates.tsx).

/** Ngày phát hành Rust gửi dạng dd/mm/yyyy → định dạng theo ngôn ngữ; dạng lạ thì giữ nguyên. */
function releaseDate(value: string): string {
  const [day, month, year] = value.split("/").map(Number)
  const date = new Date(year, month - 1, day)
  return Number.isNaN(date.getTime()) ? value : formatDate(date.getTime())
}

function InstallProgress({ progress }: { progress: UpdateProgress | null }) {
  const stage = progress?.stage ?? "download"
  const total = progress?.totalBytes ?? null
  const done = progress?.downloadedBytes ?? 0
  const sized = stage === "download" && total !== null && total > 0
  // Tải xong, đang cài: thanh giữ đầy chứ không tụt về rỗng.
  const value = stage === "install" ? 100 : percentOf(done, total)
  return (
    <div className="space-y-1">
      <StageProgress percent={value} label={t("updates.progress")} />
      <p className="text-xs text-muted-foreground tabular-nums">
        {stage === "install"
          ? t("updates.installing")
          : `${t("app.downloading")}${sized ? ` ${formatBytes(done)} / ${formatBytes(total)}` : ""}`}
      </p>
    </div>
  )
}

export function UpdateCard({
  update,
  installing,
  progress,
  locked,
  onInstall,
}: {
  update: UpdateInfo
  installing: boolean
  progress: UpdateProgress | null
  locked: boolean
  onInstall: () => void
}) {
  return (
    <div className="space-y-2.5 rounded-xl bg-accent/60 p-3.5 ring-1 ring-primary/15">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="text-sm font-semibold">
            {t("nav.updateAvailable", { version: update.version })}
            {update.date ? (
              <span className="font-normal text-muted-foreground">
                {" "}
                · {releaseDate(update.date)}
              </span>
            ) : null}
          </p>
          {update.notes ? (
            <p className="mt-1 line-clamp-4 text-xs whitespace-pre-line text-muted-foreground">
              {update.notes}
            </p>
          ) : null}
        </div>
        {installing ? null : (
          <Button
            type="button"
            variant="outline"
            disabled={locked}
            onClick={onInstall}
          >
            <Download />
            {t("updates.install")}
          </Button>
        )}
      </div>
      {installing ? (
        <InstallProgress progress={progress} />
      ) : (
        <p className="flex items-start gap-1.5 text-xs leading-relaxed text-muted-foreground">
          <ShieldCheck className="mt-0.5 size-3.5 shrink-0" />
          {t("updates.signatureNote")}
        </p>
      )}
    </div>
  )
}

export function AutoCheck({
  enabled,
  disabled,
  onChange,
}: {
  enabled: boolean
  disabled: boolean
  onChange: (enabled: boolean) => void
}) {
  const id = useId()
  return (
    <div className="flex items-center justify-between gap-3 rounded-xl bg-muted p-3.5">
      <div className="min-w-0">
        <p id={`${id}-label`} className="text-sm font-semibold">
          {t("updates.auto")}
        </p>
        <p id={`${id}-hint`} className="text-xs text-muted-foreground">
          {t("updates.autoHint")}
        </p>
      </div>
      <Switch
        aria-labelledby={`${id}-label`}
        aria-describedby={`${id}-hint`}
        checked={enabled}
        disabled={disabled}
        onCheckedChange={(checked) => onChange(checked)}
      />
    </div>
  )
}

function Hint({ text }: { text: string }) {
  return (
    <p className="flex items-start gap-1.5 text-sm text-muted-foreground">
      <Info className="mt-0.5 size-3.5 shrink-0" />
      {text}
    </p>
  )
}

export function VersionRow({
  current,
  checking,
  locked,
  onCheck,
}: {
  current: string
  checking: boolean
  locked: boolean
  onCheck: () => void
}) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <p className="text-sm">
        {tx("updates.current", {
          version: <span className="font-medium tabular-nums">{current}</span>,
        })}
      </p>
      <Button
        type="button"
        variant="outline"
        disabled={locked}
        onClick={onCheck}
      >
        {checking ? <Loader2 className="animate-spin" /> : <RefreshCw />}
        {t("updates.check")}
      </Button>
    </div>
  )
}

/** Dòng nhắc (bản xem trước / bản chưa bật tự cập nhật), thông báo xong và lỗi của mục Cập nhật. */
export function UpdateMessages({
  desktop,
  notReady,
  notice,
  error,
}: {
  desktop: boolean
  notReady: boolean
  notice: string | null
  error: string | null
}) {
  return (
    <>
      {!desktop ? <Hint text={t("updates.desktopOnly")} /> : null}
      {notReady ? <Hint text={t("updates.notReady")} /> : null}
      <p role="status" className="text-sm text-emerald-700 empty:hidden">
        {notice ?? ""}
      </p>
      {error ? (
        <p
          role="alert"
          className="rounded-lg bg-destructive/5 px-3 py-2 text-sm text-destructive"
        >
          {error}
        </p>
      ) : null}
    </>
  )
}
