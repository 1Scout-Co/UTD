import { useState } from "react"

import { ListPlus, Play } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { commandErrorText, t } from "@/shared/i18n/i18n"

import { checkLinks } from "../api/detail"
import { addBatchDownloads, pickFolder } from "../api/downloads"
import { groupOf, type BatchRow } from "../model/batch-links"
import type { BatchQuality, BatchResult, EngineTask } from "../model/types"
import { warnedLinks, type WarnedLink } from "../model/warnings"
import {
  BatchSource,
  BatchTable,
  FolderButton,
  GroupChips,
  QualityPicker,
  type SourceMode,
} from "./batch-dialog-parts"
import { groupsOf, summaryOf, useBatchRows, useSourceLinks } from "./use-batch-rows"
import { BatchWarning } from "./warning-panel"

export interface BatchDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  desktop: boolean
  defaultDir: string
  /** Link mở sẵn (từ trình duyệt). */
  initialUrls?: string[]
  /** Link gom trên trang: link không rõ là file thì bỏ tick sẵn. */
  fromBrowser?: boolean
  /** Trang gửi lô đã được cho phép đăng nhập: chỉ link cùng trang kèm cookie (app kiểm). */
  authGrant?: string
  onAdded: (tasks: EngineTask[]) => void
}

/** Gửi lô; còn link lỗi thì giữ hộp mở với đúng các link đó để sửa hoặc thử lại. */
function useSubmit(props: BatchDialogProps, setText: (text: string) => void, setMode: (mode: SourceMode) => void) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const submit = async (urls: string[], quality: BatchQuality, destDir: string | null, later: boolean) => {
    if (!props.desktop) {
      props.onOpenChange(false)
      return
    }
    setBusy(true)
    setError(null)
    try {
      const result: BatchResult = await addBatchDownloads({
        urls,
        quality,
        later,
        destDir: destDir ?? undefined,
        authGrant: props.authGrant,
      })
      props.onAdded(result.tasks)
      if (result.failed.length === 0) {
        props.onOpenChange(false)
        return
      }
      setMode("list")
      setText(result.failed.map((item) => item.url).join("\n"))
      const added = result.tasks.length
        ? `${t("batch.added", { count: result.tasks.length })} `
        : ""
      const failed = t("batch.failed", {
        count: result.failed.length,
        error: commandErrorText(result.failed[0].message),
      })
      setError(`${added}${failed}`)
    } catch (reason) {
      setError(commandErrorText(reason))
    } finally {
      setBusy(false)
    }
  }
  return { busy, error, setError, submit }
}

/**
 * S-22 cho cả lô: trước khi gửi, kiểm mọi link file đã chọn một lần (luật của Download Guard, dùng kết quả
 * dò tên/kiểu nội dung nếu có). Có link cảnh báo thì giữ lô lại, hỏi một lần cho cả lô.
 */
function useBatchWarning(desktop: boolean) {
  const [held, setHeld] = useState<{ warned: WarnedLink[]; later: boolean } | null>(null)
  const check = async (rows: BatchRow[], later: boolean) => {
    if (!desktop) return false
    const files = rows.filter((row) => row.kind !== "media")
    const facts = files.map((row) => ({
      url: row.url,
      finalUrl: row.finalUrl,
      fileName: row.name,
      contentType: row.contentType ?? undefined,
    }))
    const found = await checkLinks(facts).catch(() => [])
    const warned = warnedLinks(facts, found)
    if (warned.length) setHeld({ warned, later })
    return warned.length > 0
  }
  return { held, check, clear: () => setHeld(null) }
}

/** Bộ lọc nhanh + bảng + dòng tổng của các link đang có. */
function BatchRows({
  rows,
  probing,
  setMany,
}: {
  rows: BatchRow[]
  probing: boolean
  setMany: (urls: string[], checked: boolean) => void
}) {
  return (
    <div className="space-y-2">
      <GroupChips
        groups={groupsOf(rows)}
        onToggle={(name, checked) =>
          setMany(rows.filter((row) => groupOf(row) === name).map((row) => row.url), checked)
        }
      />
      <BatchTable
        rows={rows}
        probing={probing}
        onToggle={(url, checked) => setMany([url], checked)}
        onToggleAll={(checked) => setMany(rows.map((row) => row.url), checked)}
      />
      <p className="text-sm text-muted-foreground tabular-nums">{summaryOf(rows)}</p>
    </div>
  )
}

/** Chân hộp: thư mục lưu, "Thêm vào hàng đợi" (cuối hàng), "Bắt đầu ngay" (đầu hàng, nút chính). */
function BatchActions({
  dir,
  desktop,
  busy,
  ready,
  onPick,
  onSend,
}: {
  dir: string
  desktop: boolean
  busy: boolean
  ready: boolean
  onPick: () => void
  onSend: (later: boolean) => void
}) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <FolderButton dir={dir} desktop={desktop} disabled={busy} onPick={onPick} />
      <Button variant="outline" type="button" disabled={busy || !ready} onClick={() => onSend(true)}>
        <ListPlus />
        {t("batch.queue")}
      </Button>
      <Button data-primary type="button" disabled={busy || !ready} onClick={() => onSend(false)}>
        <Play />
        {busy ? t("app.adding") : t("batch.startNow")}
      </Button>
    </div>
  )
}

/** Ô nguồn: danh sách (mở sẵn link từ trình duyệt) hoặc mẫu; dán / nhập .txt thêm vào cuối danh sách. */
function useSourceState(initialUrls: string[] | undefined) {
  const [mode, setMode] = useState<SourceMode>("list")
  const [text, setText] = useState(() => (initialUrls ?? []).join("\n"))
  const [pattern, setPattern] = useState("")
  const append = (more: string) => {
    setMode("list")
    setText((previous) => (previous.trim() ? `${previous.trimEnd()}\n${more}` : more))
  }
  return { mode, setMode, text, setText, pattern, setPattern, append }
}

/** S-05 (trống) / S-07 (có link): thêm nhiều link một lần, kiểu "Add batch download" của IDM. */
export function BatchDialog(props: BatchDialogProps) {
  const { open, onOpenChange, desktop, defaultDir, fromBrowser = false } = props
  const source = useSourceState(props.initialUrls)
  const [quality, setQuality] = useState<BatchQuality>("best")
  const [destDir, setDestDir] = useState<string | null>(null)
  const links = useSourceLinks(source)
  const { rows, probing, setMany } = useBatchRows(links.links, fromBrowser, desktop, open)
  const { busy, error, setError, submit } = useSubmit(props, source.setText, source.setMode)
  const warning = useBatchWarning(desktop)
  const videos = rows.filter((row) => row.kind === "media").length
  const chosenRows = rows.filter((row) => row.checked)
  const chosen = chosenRows.map((row) => row.url)
  const send = async (urls: string[], later: boolean) => {
    warning.clear()
    await submit(urls, quality, destDir, later)
  }
  const pick = () =>
    void pickFolder(destDir ?? defaultDir)
      .then((picked) => picked && setDestDir(picked))
      .catch((reason: unknown) => setError(commandErrorText(reason)))

  return (
    <Dialog open={open} onOpenChange={(next) => !busy && onOpenChange(next)}>
      <DialogContent
        data-s={rows.length ? "S-07" : "S-05"}
        data-variant="soniox"
        // Cửa sổ thấp nhất (680 px) không đủ chỗ cho cả bảng: hộp tự cuộn thay vì tràn ra ngoài.
        className="max-h-[calc(100svh-2rem)] gap-4 overflow-y-auto sm:max-w-[720px]"
      >
        <DialogHeader>
          <DialogTitle>{t("batch.title")}</DialogTitle>
          <DialogDescription className="sr-only">
            {t("batch.description")}
          </DialogDescription>
        </DialogHeader>
        <BatchSource
          mode={source.mode}
          text={source.text}
          pattern={source.pattern}
          note={links.note}
          busy={busy}
          onMode={source.setMode}
          onText={source.setText}
          onPattern={source.setPattern}
          onAppend={source.append}
          onError={setError}
        />
        {rows.length ? <BatchRows rows={rows} probing={probing} setMany={setMany} /> : null}
        {videos ? <QualityPicker value={quality} count={videos} onChange={setQuality} disabled={busy} /> : null}
        {error ? (
          <p role="alert" className="rounded-lg bg-destructive/5 px-3 py-2 text-sm text-red-700">
            {error}
          </p>
        ) : null}
        {warning.held ? (
          <BatchWarning
            warned={warning.held.warned}
            total={chosen.length}
            busy={busy}
            onCancel={warning.clear}
            onProceed={() => void send(chosen, warning.held?.later ?? false)}
            onSkip={() => {
              const skipped = new Set(warning.held?.warned.map((link) => link.url))
              void send(chosen.filter((url) => !skipped.has(url)), warning.held?.later ?? false)
            }}
          />
        ) : (
          <BatchActions
            dir={destDir ?? defaultDir}
            desktop={desktop}
            busy={busy}
            ready={chosen.length > 0}
            onPick={pick}
            onSend={(later) =>
              void warning.check(chosenRows, later).then((held) => (held ? undefined : send(chosen, later)))
            }
          />
        )}
      </DialogContent>
    </Dialog>
  )
}
