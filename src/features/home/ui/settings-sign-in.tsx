import { useEffect, useState } from "react"

import { Trash2 } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { commandErrorText, t } from "@/shared/i18n/i18n"

import { authCommand, type AuthStatus } from "../api/auth"
import { SettingsSection } from "./settings-section"

/**
 * S-73: Cài đặt › Đăng nhập của trình duyệt. Công tắc chung (mặc định bật; trang nào dùng đăng nhập vẫn do
 * người dùng cho phép trong popup tiện ích), số trang đang giữ cookie trong RAM, Quên cookie.
 */
export function SettingsSignIn({ open, desktop }: { open: boolean; desktop: boolean }) {
  const [status, setStatus] = useState<AuthStatus | null>(null)
  const [error, setError] = useState<string | null>(null)

  const fail = (reason: unknown) => setError(commandErrorText(reason))
  const run = (...command: Parameters<typeof authCommand>) => {
    setError(null)
    authCommand(...command).then(setStatus, fail)
  }

  useEffect(() => {
    if (!open || !desktop) return
    authCommand("auth_status").then(setStatus, (reason: unknown) => setError(commandErrorText(reason)))
  }, [open, desktop])

  const count = status?.sites.length ?? 0
  return (
    <SettingsSection
      title={t("auth.settings.title")}
      description={t("auth.settings.note")}
      aside={
        <Switch
          aria-label={t("auth.settings.toggle")}
          checked={status?.enabled ?? true}
          disabled={!status}
          onCheckedChange={(enabled) => run("set_auth_enabled", { enabled })}
        />
      }
    >
      <div data-s="S-73" className="flex items-center justify-between gap-3 text-sm text-muted-foreground">
        {count > 0 ? t("auth.settings.held", { count }) : t("auth.settings.none")}
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={count === 0}
          onClick={() => run("forget_auth_cookies")}
        >
          <Trash2 />
          {t("auth.settings.forget")}
        </Button>
      </div>
      {error ? (
        <p role="alert" className="mt-2 rounded-lg bg-destructive/5 px-3 py-2 text-sm text-red-700">
          {error}
        </p>
      ) : null}
    </SettingsSection>
  )
}
