import { useEffect, useId, useState } from "react"

import { Trash2 } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { commandErrorText, t } from "@/shared/i18n/i18n"

import { authStatus, forgetAuthCookies, setAuthEnabled, type AuthStatus } from "../api/auth"
import { SettingsSection } from "./settings-section"

/**
 * S-73: Cài đặt › Đăng nhập của trình duyệt. Công tắc chung (mặc định bật; trang nào dùng đăng nhập vẫn do
 * người dùng cho phép trong popup tiện ích), số trang đang giữ cookie trong RAM, Quên cookie.
 */
export function SettingsSignIn({ open, desktop }: { open: boolean; desktop: boolean }) {
  const id = useId()
  const [status, setStatus] = useState<AuthStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open || !desktop) return
    let disposed = false
    void authStatus()
      .then((value) => {
        if (!disposed) setStatus(value)
      })
      .catch((reason: unknown) => {
        if (!disposed) setError(commandErrorText(reason))
      })
    return () => {
      disposed = true
    }
  }, [open, desktop])

  const run = async (work: () => Promise<AuthStatus>) => {
    setBusy(true)
    setError(null)
    try {
      setStatus(await work())
    } catch (reason) {
      setError(commandErrorText(reason))
    } finally {
      setBusy(false)
    }
  }

  const sites = status?.sites ?? []
  return (
    <SettingsSection title={t("auth.settings.title")} description={t("auth.settings.note")}>
      <div data-s="S-73" className="space-y-3">
        <div className="flex items-center justify-between gap-3 text-sm">
          <span id={`${id}-label`}>{t("auth.settings.toggle")}</span>
          <Switch
            aria-labelledby={`${id}-label`}
            checked={status?.enabled ?? true}
            disabled={!desktop || !status || busy}
            onCheckedChange={(enabled) => void run(() => setAuthEnabled(enabled))}
          />
        </div>
        <div className="flex flex-wrap items-center justify-between gap-3 text-sm text-muted-foreground">
          <span dir="auto" className="min-w-0 flex-1 truncate" title={sites.join(", ")}>
            {sites.length > 0
              ? `${t("auth.settings.held", { count: sites.length })} · ${sites.join(", ")}`
              : t("auth.settings.none")}
          </span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={!desktop || busy || sites.length === 0}
            onClick={() => void run(forgetAuthCookies)}
          >
            <Trash2 />
            {t("auth.settings.forget")}
          </Button>
        </div>
        {error ? (
          <p role="alert" className="rounded-lg bg-destructive/5 px-3 py-2 text-sm text-red-700">
            {error}
          </p>
        ) : null}
      </div>
    </SettingsSection>
  )
}
