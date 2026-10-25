import { useEffect } from "react"

import { ExternalLink, LogIn } from "lucide-react"

import { Button } from "@/components/ui/button"
import { t } from "@/shared/i18n/i18n"

import { openWebPage, subscribeAuthGranted } from "../api/downloads"
import { sameSite } from "../model/sign-in"

function hostOf(url: string): string {
  try {
    return new URL(url).hostname
  } catch {
    return ""
  }
}

/**
 * S-14 / S-63: trang cần đăng nhập. Cho phép xảy ra trong popup tiện ích (app không tự lấy cookie); khi
 * người dùng cho phép, `useAuthGranted` báo để hộp phân tích lại.
 */
export function SignInNotice({
  url,
  detail,
  screen = "S-14",
}: {
  url: string
  detail?: string | null
  /** Mã trạng thái màn; hộp Tải cả kênh đã gắn S-63 ở gốc nên truyền `null`. */
  screen?: "S-14" | null
}) {
  const host = hostOf(url)
  return (
    <div
      data-s={screen ?? undefined}
      role="alert"
      className="space-y-2 rounded-lg bg-amber-50 px-3 py-2.5 text-sm text-amber-900"
    >
      <div className="flex items-center gap-2 font-semibold">
        <LogIn className="size-4 shrink-0" />
        <span dir="auto">{host ? `${t("auth.needed")} · ${host}` : t("auth.needed")}</span>
      </div>
      {detail ? <p>{detail}</p> : null}
      <p>{t("auth.hint")}</p>
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={() => {
          void openWebPage(url).catch((reason: unknown) => console.error("UTD open page:", reason))
        }}
      >
        <ExternalLink />
        {t("auth.openPage")}
      </Button>
    </div>
  )
}

/** Popup tiện ích vừa cho phép trang của `url`: gọi `onGranted` với mã lần cho phép mới. */
export function useAuthGranted(
  active: boolean,
  url: string,
  onGranted: (authGrant: string) => void,
) {
  useEffect(() => {
    if (!active) return
    let disposed = false
    let unlisten: (() => void) | undefined
    subscribeAuthGranted((granted) => {
      if (!disposed && sameSite(url, granted.site)) onGranted(granted.authGrant)
    })
      .then((off) => {
        if (disposed) off()
        else unlisten = off
      })
      .catch((reason: unknown) => console.error("UTD auth granted:", reason))
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [active, url, onGranted])
}
