import { Film, Link2, ListVideo, Users } from "lucide-react"

import { t } from "@/shared/i18n/i18n"

/**
 * Thẻ trên ô URL: gợi ý dán, tên file + máy chủ, hoặc báo đây là link video (chuyển sang Tải media) / link
 * kênh (chuyển sang Tải cả kênh).
 */
export function UrlPreview({
  url,
  host,
  name,
  mediaPage,
  channelPage,
  socialPage,
}: {
  url: string
  host: string
  name: string
  mediaPage: boolean
  channelPage: boolean
  /** Trang cá nhân / fanpage / nhóm: chỉ tải cả trang bằng Tải hàng loạt trên trình duyệt. */
  socialPage: boolean
}) {
  if (socialPage) {
    return (
      <div role="status" className="flex items-center gap-3 rounded-xl bg-accent p-3 text-accent-foreground">
        <div className="grid size-10 place-items-center rounded-lg bg-card text-primary shadow-soft">
          <Users className="size-4" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-sm font-semibold">{t("add.socialLink")}</div>
          <div className="mt-0.5 text-xs">{t("add.socialLinkHint")}</div>
        </div>
      </div>
    )
  }
  if (mediaPage || channelPage) {
    const Icon = channelPage ? ListVideo : Film
    return (
      <div
        role="status"
        className="flex items-center gap-3 rounded-xl bg-accent p-3 text-accent-foreground"
      >
        <div className="grid size-10 place-items-center rounded-lg bg-card text-primary shadow-soft">
          <Icon className="size-4" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-sm font-semibold">
            {channelPage ? t("add.channelLink") : t("add.videoLink")}
          </div>
          <div className="mt-0.5 truncate text-xs">
            {channelPage
              ? t("add.channelLinkHint", { host })
              : t("add.videoLinkHint", { host })}
          </div>
        </div>
      </div>
    )
  }
  if (!url.trim()) {
    return (
      <div className="flex items-center gap-3 rounded-xl bg-muted p-3 text-muted-foreground">
        <div className="grid size-10 place-items-center rounded-lg bg-card shadow-soft">
          <Link2 className="size-4" />
        </div>
        <div className="text-sm">{t("add.previewHint")}</div>
      </div>
    )
  }
  return (
    <div className="flex items-center gap-3 rounded-xl bg-muted p-3">
      <div className="grid size-10 place-items-center rounded-lg bg-card text-primary shadow-soft">
        <Link2 className="size-4" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-semibold">
          {name || t("add.serverName")}
        </div>
        <div className="mt-0.5 truncate text-xs text-muted-foreground">
          {host || t("add.invalidUrl")}
        </div>
      </div>
    </div>
  )
}
