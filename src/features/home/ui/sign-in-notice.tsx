import { useCallback } from "react"

import { AlertCircle, ExternalLink } from "lucide-react"

import { Button } from "@/components/ui/button"
import { t } from "@/shared/i18n/i18n"

import { openWebPage, subscribeAuthGranted, type AuthGranted } from "../api/downloads"
import { sameSite } from "../model/sign-in"
import { useAppEvent } from "./use-app-event"

function hostOf(url: string): string {
  try {
    return new URL(url).hostname
  } catch {
    return ""
  }
}

/** "Mở trang" (S-14, S-72): trang mở trên trình duyệt đã đăng nhập, người dùng cho phép trong popup UTD. */
export function OpenPageButton({ url, outline = true }: { url: string; outline?: boolean }) {
  return (
    <Button
      type="button"
      variant={outline ? "outline" : "default"}
      size={outline ? "sm" : "default"}
      onClick={() => {
        void openWebPage(url).catch((reason: unknown) => console.error("UTD open page:", reason))
      }}
    >
      <ExternalLink />
      {t("auth.openPage")}
    </Button>
  )
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
        <AlertCircle className="size-4 shrink-0" />
        <span dir="auto">{host ? `${t("auth.needed")} · ${host}` : t("auth.needed")}</span>
      </div>
      {detail ? <p>{detail}</p> : null}
      <p>{t("auth.hint")}</p>
      <OpenPageButton url={url} />
    </div>
  )
}

/** Popup tiện ích vừa cho phép trang của `url`: gọi `onGranted` với mã lần cho phép mới. */
export function useAuthGranted(
  active: boolean,
  url: string,
  onGranted: (authGrant: string) => void,
) {
  const onRequest = useCallback(
    (granted: AuthGranted) => {
      if (sameSite(url, granted.site)) onGranted(granted.authGrant)
    },
    [url, onGranted],
  )
  useAppEvent(active, subscribeAuthGranted, onRequest)
}
