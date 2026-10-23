import { useEffect, useState } from "react"

import { CheckCircle2, Info, Loader2, Puzzle } from "lucide-react"

import { Button } from "@/components/ui/button"
import { cn } from "cn"

import {
  browserIntegrationStatus,
  installBrowserIntegration,
  openExtensionFolder,
} from "../api/downloads"
import type { BrowserIntegrationStatus } from "../model/types"
import { MoreInfo, SettingsSection } from "./settings-section"

function messageOf(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason)
}

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
        if (!disposed) setError(messageOf(reason))
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
      setNotice("Đã cài. Thư mục tiện ích đang mở, nạp nó vào trình duyệt.")
    } catch (reason) {
      setError(messageOf(reason))
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
      {status === null ? "Đang kiểm tra…" : installed ? "Đã cài" : "Chưa cài"}
    </span>
  )
}

function Guide({ status }: { status: BrowserIntegrationStatus | null }) {
  return (
    <MoreInfo label="Xem hướng dẫn" className="mt-3">
      <ol className="list-decimal space-y-1 pl-4">
        <li>
          Bấm “Cài extension”: UTD đăng ký native host và mở thư mục tiện ích.
        </li>
        <li>
          Chrome/Edge: mở <code>chrome://extensions</code>, bật Developer mode,
          chọn “Load unpacked” rồi trỏ vào thư mục <code>chromium</code>.
        </li>
        <li>
          Firefox: <code>about:debugging</code> → This Firefox → “Load Temporary
          Add-on”, chọn <code>firefox/manifest.json</code>.
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
          <dt>Thư mục</dt>
          <dd
            className="truncate font-mono text-xs"
            title={status.extensionDir}
          >
            {status.extensionDir}
          </dd>
        </dl>
      ) : null}
      <p>
        Tiện ích chưa có trên Chrome Web Store nên nạp thủ công. UTD không gửi
        cookie hay header đăng nhập từ trình duyệt.
      </p>
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
      title="Tích hợp trình duyệt"
      description="Chuột phải vào link hoặc video để tải bằng UTD."
      aside={desktop ? <StatusBadge status={status} /> : undefined}
    >
      <div className="flex flex-wrap items-center gap-3">
        <Button
          type="button"
          disabled={!desktop || busy}
          onClick={() => void install()}
        >
          {busy ? <Loader2 className="animate-spin" /> : <Puzzle />}
          {status?.installed ? "Cài lại" : "Cài extension"}
        </Button>
        <span className="flex min-w-0 flex-1 items-center gap-1.5 text-sm text-muted-foreground">
          {desktop ? null : <Info className="size-3.5 shrink-0" />}
          {desktop
            ? "Rồi chọn “Load unpacked” trong trang tiện ích."
            : "Bản xem trước trên trình duyệt không cài được. Mở app UTD để cài."}
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
