import { useCallback, useMemo, useState } from "react"

import { AlertCircle, Download, FolderOpen, RotateCw } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Switch } from "@/components/ui/switch"
import { commandErrorText, t } from "@/shared/i18n/i18n"

import {
  CHANNEL_QUALITIES,
  channelFolder,
  channelPayloads,
  DEFAULT_FILTER,
  DEFAULT_SELECTION,
  pickedEntries,
  SELECT_NONE,
  selectAll,
  toggleEntry,
  visibleEntries,
} from "../model/channel"
import { commandCode, needsSignIn } from "../model/sign-in"
import type { AddMediaPayload } from "../model/types"
import { ChannelCard, EntryList, FilterBar, QualityPicker } from "./channel-parts"
import { SignInNotice, useAuthGranted } from "./sign-in-notice"
import { useChannelListing } from "./use-channel-listing"

/**
 * Tải cả kênh (S-60 đang liệt kê, S-61 có danh sách, S-62 trống, S-63 lỗi): liệt kê dần video của kênh,
 * lọc/chọn, một chất lượng chung, rồi giao danh sách cho `onStart` (màn chính thêm lần lượt từng video).
 * Mỗi lần mở là một `key` mới: hộp dựng lại và liệt kê từ đầu.
 */
export function ChannelDialog({
  open,
  onOpenChange,
  desktop,
  url,
  authGrant,
  onStart,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  desktop: boolean
  url: string
  /** Kênh riêng tư (S-63): lần cho phép đăng nhập từ trình duyệt, không phải cookie. */
  authGrant?: string
  onStart: (payloads: AddMediaPayload[]) => void
}) {
  const [grant, setGrant] = useState<string | null>(authGrant ?? null)
  const listing = useChannelListing(url, open && desktop, grant)
  const errorCode = commandCode(listing.error)
  const signIn =
    Boolean(listing.profile?.private) ||
    errorCode === "channel.private_all" ||
    needsSignIn(errorCode)
  // Popup tiện ích vừa cho phép trang của kênh: liệt kê lại với đăng nhập.
  const regrant = useCallback((fresh: string) => setGrant(fresh), [])
  useAuthGranted(open && signIn, url, regrant)
  const [filter, setFilter] = useState(DEFAULT_FILTER)
  const [selection, setSelection] = useState(DEFAULT_SELECTION)
  const [qualityId, setQualityId] = useState(CHANNEL_QUALITIES[0].id)
  const [ownFolder, setOwnFolder] = useState(true)

  const { entries, status } = listing
  const visible = useMemo(() => visibleEntries(entries, filter), [entries, filter])
  const picked = useMemo(() => pickedEntries(visible, selection), [visible, selection])
  const downloaded = useMemo(() => entries.filter((entry) => entry.downloaded).length, [entries])
  const selectable = visible.filter((entry) => !entry.blocked).length
  const folder = channelFolder(listing.head, listing.profile)
  const loading = status === "loading"
  const code =
    status === "error" || !desktop
      ? "S-63"
      : entries.length > 0
        ? "S-61"
        : loading
          ? "S-60"
          : "S-62"

  const start = () => {
    const quality = CHANNEL_QUALITIES.find((item) => item.id === qualityId) ?? CHANNEL_QUALITIES[0]
    const payloads = channelPayloads(picked, quality, listing.head?.key ?? null, ownFolder ? folder : null)
    onStart(payloads.map((payload) => ({ ...payload, authGrant: grant ?? undefined })))
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent data-variant="soniox" className="sm:max-w-[680px]">
        <DialogHeader>
          <DialogTitle>{t("channel.title")}</DialogTitle>
          <DialogDescription className="sr-only">
            {t("channel.description")}
          </DialogDescription>
        </DialogHeader>

        <div data-s={code} className="min-w-0 space-y-3">
          {desktop ? (
            <ChannelCard
              head={listing.head}
              profile={listing.profile}
              count={entries.length}
              loading={loading}
              onStop={listing.stop}
            />
          ) : null}
          {signIn ? <SignInNotice url={url} screen={null} /> : null}

          {entries.length > 0 ? (
            <>
              <FilterBar filter={filter} downloaded={downloaded} onChange={setFilter} />
              <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
                <span className="tabular-nums">
                  {t("channel.picked", { done: picked.length, total: selectable })}
                </span>
                <span className="flex gap-1">
                  <Button type="button" variant="ghost" size="xs" onClick={() => setSelection(selectAll(visible))}>
                    {t("channel.selectAll")}
                  </Button>
                  <Button type="button" variant="ghost" size="xs" onClick={() => setSelection(SELECT_NONE)}>
                    {t("channel.selectNone")}
                  </Button>
                </span>
              </div>
              {visible.length > 0 ? (
                <EntryList
                  entries={visible}
                  selection={selection}
                  onToggle={(id) => setSelection((previous) => toggleEntry(previous, id))}
                />
              ) : (
                <p className="py-6 text-center text-sm text-muted-foreground">
                  {t("channel.noMatch")}
                </p>
              )}
            </>
          ) : loading && desktop ? (
            <ListSkeleton />
          ) : null}

          <StatusMessage
            desktop={desktop}
            status={status}
            error={listing.error ? commandErrorText(listing.error) : null}
            empty={entries.length === 0}
            onRetry={listing.retry}
          />

          {entries.length > 0 ? (
            <div className="flex flex-wrap items-center justify-between gap-2">
              <QualityPicker value={qualityId} onChange={setQualityId} />
              {folder ? (
                <label className="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
                  <Switch size="sm" checked={ownFolder} onCheckedChange={setOwnFolder} />
                  <FolderOpen className="size-3.5 shrink-0" />
                  <span className="max-w-48 truncate" title={folder}>
                    {t("channel.folder", { folder })}
                  </span>
                </label>
              ) : null}
            </div>
          ) : null}

          <div className="flex items-center justify-end gap-2">
            <Button variant="ghost" type="button" onClick={() => onOpenChange(false)}>
              {t("app.cancel")}
            </Button>
            <Button data-primary type="button" disabled={!desktop || picked.length === 0} onClick={start}>
              <Download />
              {t("channel.download", { count: picked.length })}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}

function Notice({ text }: { text: string }) {
  return (
    <div className="flex items-start gap-2 rounded-lg bg-amber-50 px-3 py-2 text-sm text-amber-800">
      <AlertCircle className="mt-0.5 size-4 shrink-0" />
      {text}
    </div>
  )
}

/** Lỗi (thử lại được), kênh trống, cảnh báo dừng giữa chừng, hoặc bản xem trước không chạy được yt-dlp. */
function StatusMessage({
  desktop,
  status,
  error,
  empty,
  onRetry,
}: {
  desktop: boolean
  status: string
  error: string | null
  empty: boolean
  onRetry: () => void
}) {
  if (!desktop) {
    return <Alert text={t("channel.desktopOnly")} />
  }
  if (status === "error") {
    return (
      <div className="space-y-2">
        <Alert text={error ?? t("channel.readFailed")} />
        <Button type="button" variant="outline" size="sm" onClick={onRetry}>
          <RotateCw />
          {t("app.retry")}
        </Button>
      </div>
    )
  }
  if (error) return <Notice text={error} />
  if (empty && status !== "loading") {
    return (
      <p className="py-6 text-center text-sm text-muted-foreground">
        {t("channel.empty")}
      </p>
    )
  }
  return null
}

// text-red-700: text-destructive trên nền này chỉ đạt 4,49:1, dưới mức 4,5:1 của WCAG AA.
function Alert({ text }: { text: string }) {
  return (
    <p role="alert" className="rounded-lg bg-destructive/5 px-3 py-2 text-sm text-red-700">
      {text}
    </p>
  )
}

function ListSkeleton() {
  return (
    <div role="status" className="space-y-1.5">
      <span className="sr-only">{t("channel.listing")}</span>
      {[0, 1, 2, 3].map((index) => (
        <div key={index} className="flex items-center gap-3 px-2 py-1.5">
          <div className="size-4 animate-pulse rounded bg-muted" />
          <div className="h-11 w-16 animate-pulse rounded-md bg-muted" />
          <div className="flex-1 space-y-1.5">
            <div className="h-3 w-3/4 animate-pulse rounded bg-muted" />
            <div className="h-2.5 w-1/3 animate-pulse rounded bg-muted" />
          </div>
        </div>
      ))}
    </div>
  )
}
