import { useEffect, useState } from "react"

import { CheckCircle2, Info, Loader2, Puzzle } from "lucide-react"

import { Button } from "@/components/ui/button"
import { commandErrorText, t, tx } from "@/shared/i18n/i18n"
import { cn } from "cn"

import {
  browserIntegrationStatus,
  installBrowserIntegration,
  openExtensionFolder,
} from "../api/downloads"
import type { BrowserIntegrationStatus } from "../model/types"
import { MoreInfo, SettingsSection } from "./settings-section"

/** Trạng thái native host + một hành động: cài (đăng ký host) rồi mở thư mục tiện ích. */
function useBrowserIntegration(open: boolean, desktop: boolean) {
  const [status, setStatus] = useState<BrowserIntegrationStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  useEffect(() => {
    if (!open || !desktop) return
    let disposed = false
    void browserIntegrationStatus()
      .then((value) => {
        if (disposed) return
        setStatus(value)
        setError(null)
      })
      .catch((reason: unknown) => {
        if (!disposed) setError(commandErrorText(reason))
      })
    return () => {
      disposed = true
    }
  }, [open, desktop])

  const install = async () => {
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      setStatus(await installBrowserIntegration())
      await openExtensionFolder()
      setNotice(t("browser.installed"))
    } catch (reason) {
      setError(commandErrorText(reason))
    } finally {
      setBusy(false)
    }
  }

  return { status, busy, error, notice, install }
}

function StatusBadge({ status }: { status: BrowserIntegrationStatus | null }) {
  const installed = status?.installed === true
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium",
        installed
          ? "bg-emerald-50 text-emerald-700"
          : "bg-muted text-muted-foreground"
      )}
    >
      {installed ? (
        <CheckCircle2 className="size-3.5" />
      ) : (
        <span className="size-1.5 rounded-full bg-current" />
      )}
      {status === null
        ? t("app.checking")
        : installed
          ? t("browser.statusInstalled")
          : t("browser.statusMissing")}
    </span>
  )
}

function Guide({ status }: { status: BrowserIntegrationStatus | null }) {
  return (
    <MoreInfo label={t("browser.guide")} className="mt-3">
      <ol className="list-decimal space-y-1 ps-4">
        <li>{t("browser.step1", { button: t("browser.install") })}</li>
        <li>
          {tx("browser.step2", {
            page: <code>chrome://extensions</code>,
            folder: <code>chromium</code>,
          })}
        </li>
        <li>
          {tx("browser.step3", {
            page: <code>about:debugging</code>,
            file: <code>firefox/manifest.json</code>,
          })}
        </li>
      </ol>
      {status ? (
        <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
          <dt>Chrome / Edge</dt>
          <dd className="truncate font-mono text-xs">
            {status.chromiumExtensionId}
          </dd>
          <dt>Firefox</dt>
          <dd className="truncate font-mono text-xs">
            {status.firefoxExtensionId}
          </dd>
          <dt>{t("browser.folder")}</dt>
          <dd
            className="truncate font-mono text-xs"
            title={status.extensionDir}
          >
            {status.extensionDir}
          </dd>
        </dl>
      ) : null}
      <p>{t("browser.manualNote")}</p>
    </MoreInfo>
  )
}

/** Mục "Tích hợp trình duyệt" trong Cài đặt (S-27): một nút, hướng dẫn chi tiết thu gọn. */
export function SettingsBrowser({
  open,
  desktop,
}: {
  open: boolean
  desktop: boolean
}) {
  const { status, busy, error, notice, install } = useBrowserIntegration(
    open,
    desktop
  )

  return (
    <SettingsSection
      title={t("browser.title")}
      description={t("browser.description")}
      aside={desktop ? <StatusBadge status={status} /> : undefined}
    >
      <div className="flex flex-wrap items-center gap-3">
        <Button
          type="button"
          disabled={!desktop || busy}
          onClick={() => void install()}
        >
          {busy ? <Loader2 className="animate-spin" /> : <Puzzle />}
          {status?.installed ? t("browser.reinstall") : t("browser.install")}
        </Button>
        <span className="flex min-w-0 flex-1 items-center gap-1.5 text-sm text-muted-foreground">
          {desktop ? null : <Info className="size-3.5 shrink-0" />}
          {desktop ? t("browser.thenLoad") : t("browser.previewOnly")}
        </span>
      </div>
      <p role="status" className="mt-2 text-sm text-emerald-700 empty:hidden">
        {notice ?? ""}
      </p>
      {error ? (
        <p
          role="alert"
          className="mt-2 rounded-lg bg-destructive/5 px-3 py-2 text-sm text-destructive"
        >
          {error}
        </p>
      ) : null}
      <Guide status={status} />
    </SettingsSection>
  )
}
