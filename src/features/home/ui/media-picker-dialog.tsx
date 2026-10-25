import { useCallback, useEffect, useRef, useState, type FormEvent } from "react"

import { AlertCircle, Download } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { commandErrorText, t } from "@/shared/i18n/i18n"

import {
  channelPageUrl,
  mediaProbe,
  subscribeMediaRequest,
  takeMediaRequest,
} from "../api/downloads"
import { isChannelPage } from "../model/media-pages"
import { commandCode, needsSignIn } from "../model/sign-in"
import type { AddMediaPayload, MediaProbe } from "../model/types"
import {
  FormatList,
  ProbeCard,
  ProbeSkeleton,
  SupportedSites,
  UrlField,
} from "./media-picker-parts"
import { SignInNotice, useAuthGranted } from "./sign-in-notice"
import { ToolsAutoNote } from "./tools-auto"

/**
 * Link trang video từ trình duyệt (chuột phải / popup): `onUrl` cần ổn định (useCallback). Link tới
 * trước khi kịp nghe (UTD vừa mở) được app giữ lại: lấy một lần ngay sau khi nghe.
 */
export function useMediaRequests(
  desktop: boolean,
  onUrl: (url: string, authGrant?: string) => void,
) {
  useEffect(() => {
    if (!desktop) return
    let disposed = false
    let unlisten: (() => void) | undefined
    subscribeMediaRequest((request) => {
      if (!disposed) onUrl(request.url, request.authGrant)
    })
      .then(async (off) => {
        if (disposed) {
          off()
          return
        }
        unlisten = off
        const early = await takeMediaRequest()
        if (early && !disposed) onUrl(early.url, early.authGrant)
      })
      .catch((reason: unknown) => console.error("UTD media request:", reason))
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [desktop, onUrl])
}

/**
 * `initialUrl`: mở sẵn link này và tự phân tích (từ hộp Thêm hoặc trình duyệt). Đổi link thì đổi `key`
 * để dựng lại hộp với trạng thái mới.
 */
export function MediaPickerDialog({
  open,
  onOpenChange,
  desktop,
  busy,
  error,
  initialUrl,
  authGrant,
  onSubmit,
  onChannel,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  desktop: boolean
  busy: boolean
  error: string | null
  initialUrl?: string | null
  /** Trang đã được cho phép đăng nhập (cú bấm trên trình duyệt): mã lần cho phép, không phải cookie. */
  authGrant?: string | null
  onSubmit: (payload: AddMediaPayload) => void | Promise<void>
  /** Link là kênh (dán vào đây, hoặc link rút gọn app thấy trỏ tới kênh): mở hộp Tải cả kênh. */
  onChannel: (url: string, authGrant?: string) => void
}) {
  const autoAnalyze = Boolean(initialUrl) && desktop
  const [url, setUrl] = useState(initialUrl ?? "")
  const [probe, setProbe] = useState<MediaProbe | null>(null)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [loading, setLoading] = useState(autoAnalyze)
  const [localError, setLocalError] = useState<string | null>(
    initialUrl && !desktop ? t("media.desktopOnly") : null,
  )
  const autoStarted = useRef(false)
  // S-14: lần cho phép hiện dùng (từ trình duyệt, hoặc popup vừa cho phép) và trạng thái "cần đăng nhập".
  const [grant, setGrant] = useState<string | null>(authGrant ?? null)
  const [signIn, setSignIn] = useState(false)

  const selected =
    probe?.formats.find((format) => format.id === selectedId) ??
    probe?.formats[0] ??
    null

  const updateUrl = (value: string) => {
    setUrl(value)
    setProbe(null)
    setSelectedId(null)
    setLocalError(null)
    setSignIn(false)
  }

  const close = () => {
    if (!busy) {
      setLocalError(null)
      onOpenChange(false)
    }
  }

  const paste = () => {
    void navigator.clipboard
      ?.readText()
      .then((text) => {
        updateUrl(text.trim())
        if (text.trim()) analyze(text)
      })
      .catch(() => undefined)
  }

  // Chỉ đổi state sau `await`: gọi được từ effect tự phân tích bên dưới.
  const runProbe = useCallback(async (value: string, withGrant: string | null) => {
    try {
      const result = await mediaProbe(value, withGrant)
      setProbe(result)
      setSelectedId(
        result.formats.find((format) => format.recommended)?.id ??
          result.formats[0]?.id ??
          null,
      )
    } catch (reason) {
      const channel = channelPageUrl(reason)
      if (channel) onChannel(channel, withGrant ?? undefined)
      else {
        setSignIn(needsSignIn(commandCode(reason)))
        setLocalError(commandErrorText(reason))
      }
    } finally {
      setLoading(false)
    }
  }, [onChannel])

  // Mở với link sẵn ⇒ phân tích ngay (ref chặn chạy hai lần ở StrictMode).
  useEffect(() => {
    if (!autoAnalyze || !initialUrl || autoStarted.current) return
    autoStarted.current = true
    void runProbe(initialUrl.trim(), authGrant ?? null)
  }, [autoAnalyze, initialUrl, authGrant, runProbe])

  // Popup tiện ích vừa cho phép trang này: phân tích lại với đăng nhập.
  const regrant = useCallback(
    (fresh: string) => {
      setGrant(fresh)
      setSignIn(false)
      setLocalError(null)
      setLoading(true)
      void runProbe(url.trim(), fresh)
    },
    [runProbe, url],
  )
  useAuthGranted(open && signIn, url.trim(), regrant)

  const analyze = (value = url) => {
    if (!desktop) {
      setLocalError(t("media.desktopOnly"))
      return
    }
    if (!/^https?:\/\//i.test(value.trim())) {
      setLocalError(t("media.needUrl"))
      return
    }
    if (isChannelPage(value)) {
      onChannel(value.trim())
      return
    }
    setLoading(true)
    setLocalError(null)
    setSignIn(false)
    setProbe(null)
    setSelectedId(null)
    void runProbe(value.trim(), grant)
  }

  const submit = (event: FormEvent) => {
    event.preventDefault()
    if (!probe || !selected) {
      setLocalError(t("media.analyzeFirst"))
      return
    }
    setLocalError(null)
    void onSubmit({
      url: probe.webpageUrl || url.trim(),
      title: probe.title,
      selector: selected.selector,
      audioFormat: selected.audioFormat ?? undefined,
      pageUrl: probe.webpageUrl || undefined,
      authGrant: grant ?? undefined,
    })
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent data-variant="soniox" className="sm:max-w-[600px]">
        <DialogHeader>
          <DialogTitle>{t("media.title")}</DialogTitle>
          <DialogDescription className="sr-only">
            {t("media.description")}
          </DialogDescription>
        </DialogHeader>

        <form className="space-y-4" onSubmit={submit}>
          <UrlField
            url={url}
            disabled={busy || loading}
            loading={loading}
            onChange={updateUrl}
            onAnalyze={() => analyze()}
            onPaste={paste}
          />

          {loading ? <ToolsAutoNote desktop={desktop} /> : null}
          {loading ? <ProbeSkeleton /> : null}
          {!loading && !probe ? <SupportedSites /> : null}

          {probe ? (
            <>
              <ProbeCard probe={probe} />
              <FormatList
                formats={probe.formats}
                selectedId={selected?.id ?? null}
                onSelect={setSelectedId}
              />
              {!probe.ffmpegAvailable ? (
                <div className="flex items-start gap-2 rounded-lg bg-amber-50 px-3 py-2 text-sm text-amber-800">
                  <AlertCircle className="mt-0.5 size-4 shrink-0" />
                  {t("media.noFfmpeg")}
                </div>
              ) : null}
            </>
          ) : null}
          {signIn ? <SignInNotice url={url.trim()} detail={localError} /> : null}
          {/* text-red-700: text-destructive trên nền này chỉ đạt 4,49:1, dưới mức 4,5:1 của WCAG AA. */}
          {(localError && !signIn) || error ? (
            <p
              role="alert"
              className="rounded-lg bg-destructive/5 px-3 py-2 text-sm text-red-700"
            >
              {signIn ? error : (localError ?? error)}
            </p>
          ) : null}

          <div className="flex items-center justify-end gap-2">
            <Button
              variant="ghost"
              type="button"
              onClick={close}
              disabled={busy}
            >
              {t("app.cancel")}
            </Button>
            <Button
              data-primary
              type="submit"
              disabled={busy || loading || !probe || !selected}
            >
              <Download />
              {busy ? t("app.adding") : t("media.download")}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}
