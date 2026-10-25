import type { ReactNode } from "react"

import {
  AlertCircle,
  CheckCircle2,
  ExternalLink,
  FileQuestion,
  FolderOpen,
  ListX,
  Pause,
  Play,
  RefreshCw,
  ShieldQuestion,
  Trash2,
  Wrench,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  formatBytes,
  formatDate,
  formatDuration,
  formatTime,
} from "@/shared/i18n/format"
import { t } from "@/shared/i18n/i18n"

import { downloadLog } from "../api/detail"
import {
  failures,
  hostOfTask,
  pauseReason,
  segmentCounts,
  tookMs,
  type DownloadDetail,
  type LiveNumbers,
} from "../model/detail"
import { diagnose } from "../model/doctor"
import type { DownloadItem } from "../model/types"
import { BigProgress, CopyButton, Rows, SegmentMap, Stats } from "./detail-parts"
import { CATEGORY_STYLE } from "./doctor-dialog"

/** Việc màn Chi tiết gọi ra ngoài (màn chính đã có sẵn các thao tác này). */
export interface DetailActions {
  desktop: boolean
  onPause: (id: string) => void
  onResume: (id: string) => void
  onRemove: (id: string) => void
  onTrash: (id: string) => void
  onOpen: (id: string) => void
  onReveal: (id: string) => void
  onSettings: () => void
  onClose: () => void
  /**
   * Chỗ chừa ở S-21 cho nút thêm của luồng khôi phục (vd "Cập nhật link" khi link hết hạn), đứng trước
   * nút Thử lại. Trả null khi không có gì.
   */
  failedExtra?: (detail: DownloadDetail) => ReactNode
}

export interface StateProps {
  detail: DownloadDetail
  item: DownloadItem | undefined
  live: LiveNumbers
  actions: DetailActions
}

const percentOf = (live: LiveNumbers, item?: DownloadItem) =>
  live.total ? Math.min(100, (live.downloaded / live.total) * 100) : (item?.progress ?? 0)

const bytesOf = (live: LiveNumbers) =>
  live.total !== null
    ? `${formatBytes(live.downloaded)} / ${formatBytes(live.total)}`
    : formatBytes(live.downloaded)

/** Ô "đã tải": số lớn là phần đã có, tổng nằm ở phần phụ. */
const bytesStat = (label: string, live: LiveNumbers) => ({
  label,
  value: formatBytes(live.downloaded),
  hint: live.total !== null ? `/ ${formatBytes(live.total)}` : undefined,
})

function Footer({ start, children }: { start?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-2 pt-1">
      <div className="flex flex-wrap gap-2">{start}</div>
      <div className="flex flex-wrap gap-2">{children}</div>
    </div>
  )
}

function RevealButton({ id, actions }: { id: string; actions: DetailActions }) {
  return (
    <Button type="button" variant="outline" disabled={!actions.desktop} onClick={() => actions.onReveal(id)}>
      <FolderOpen />
      {t("menu.reveal")}
    </Button>
  )
}

/** Câu ngắn thay ô Tốc độ khi task chưa thật sự nhận byte. */
function phaseOf(detail: DownloadDetail, item?: DownloadItem): string | null {
  const { status, kind } = detail.task
  if (status === "queued") return t("desc.queued")
  if (status === "resolving") return kind === "media" ? t("desc.analyzing") : t("desc.resolving")
  if (status === "verifying") return item?.description ?? t("desc.verifyingHash")
  return null
}

/** S-15: đang tải — tốc độ, còn lại, đã tải, kết nối, bản đồ đoạn. Giới hạn tốc độ: chưa có (engine chưa làm). */
export function ActiveState({ detail, item, live, actions }: StateProps) {
  const phase = phaseOf(detail, item)
  const connections =
    live.connections !== null && live.maxConnections !== null
      ? `${live.connections}/${live.maxConnections}`
      : "—"
  return (
    <div data-s="S-15" className="space-y-4">
      <BigProgress value={percentOf(live, item)} />
      <Stats
        items={[
          { label: t("detail.speed"), value: phase ?? item?.speed ?? "—" },
          { label: t("detail.left"), value: phase ? "—" : (item?.eta ?? "—") },
          bytesStat(t("detail.downloaded"), live),
          { label: t("detail.connections"), value: connections },
        ]}
      />
      <SegmentMap parts={detail.parts} live={detail.task.status === "downloading"} />
      <Footer start={<RevealButton id={detail.task.id} actions={actions} />}>
        <Button data-primary type="button" disabled={!actions.desktop} onClick={() => actions.onPause(detail.task.id)}>
          <Pause />
          {t("menu.pause")}
        </Button>
      </Footer>
    </div>
  )
}

/** S-19: tạm dừng — dừng ở đâu, đoạn đã xong, lý do. */
export function PausedState({ detail, item, live, actions }: StateProps) {
  const counts = segmentCounts(detail.parts, false)
  const reason = pauseReason(detail.events)
  const id = detail.task.id
  return (
    <div data-s="S-19" className="space-y-4">
      <BigProgress value={percentOf(live, item)} tone="var(--muted-foreground)" />
      <Stats
        items={[
          bytesStat(t("detail.stoppedAt"), live),
          {
            label: t("detail.partsDone"),
            value: counts.total ? `${counts.done}/${counts.total}` : "—",
          },
          {
            label: t("detail.reason"),
            value: reason ? t(`detail.reason.${reason}`) : t("status.paused"),
          },
          {
            label: t("detail.remaining"),
            value: live.total !== null ? formatBytes(Math.max(0, live.total - live.downloaded)) : "—",
          },
        ]}
      />
      <SegmentMap parts={detail.parts} live={false} />
      <Footer
        start={
          <>
            {/* text-red-700: chữ đỏ mặc định của nút này chỉ đạt 4,15:1 (WCAG AA cần 4,5:1). */}
            <Button type="button" variant="destructive" className="text-red-700" disabled={!actions.desktop} onClick={() => actions.onTrash(id)}>
              <Trash2 />
              {t("detail.cancel")}
            </Button>
            <RevealButton id={id} actions={actions} />
          </>
        }
      >
        <Button data-primary type="button" disabled={!actions.desktop} onClick={() => actions.onResume(id)}>
          <Play />
          {t("menu.resume")}
        </Button>
      </Footer>
    </div>
  )
}

function HashLine({ detail }: { detail: DownloadDetail }) {
  const verified = detail.task.hashState === "verified"
  const Icon = verified ? CheckCircle2 : ShieldQuestion
  return (
    <div className={`flex items-center gap-3 rounded-xl p-3 ${verified ? "bg-emerald-50 text-emerald-800" : "bg-muted"}`}>
      <Icon className="size-5 shrink-0" />
      <div className="min-w-0">
        <div className="text-sm font-semibold">
          {verified ? t("detail.hash.verified") : t("detail.hash.unchecked")}
        </div>
        <div className={`text-xs ${verified ? "text-emerald-800" : "text-muted-foreground"}`}>
          {verified ? t("detail.hash.verifiedHint") : t("detail.hash.uncheckedHint")}
        </div>
      </div>
    </div>
  )
}

/** S-20: hoàn tất — kết quả kiểm hash, đường dẫn, lúc xong, thời gian tải, dung lượng. */
export function DoneState({ detail, live, actions }: StateProps) {
  const { task } = detail
  const took = tookMs(detail)
  const finished = task.completedAt
  const gone = detail.fileExists === false
  return (
    <div data-s="S-20" className="space-y-4">
      <HashLine detail={detail} />
      <Rows
        rows={[
          {
            label: t("detail.path"),
            value: detail.targetPath,
            raw: true,
            action: (
              <CopyButton
                small
                label={t("detail.copyPath")}
                done={t("detail.pathCopied")}
                copy={() => navigator.clipboard.writeText(detail.targetPath)}
              />
            ),
          },
          { label: t("detail.size"), value: formatBytes(live.total ?? live.downloaded) },
          {
            label: t("detail.finishedAt"),
            value: finished ? `${formatTime(finished)} · ${formatDate(finished)}` : "—",
          },
          { label: t("detail.took"), value: took === null ? "—" : formatDuration(took) },
        ]}
      />
      {gone ? (
        <p role="status" className="flex items-center gap-2 rounded-lg bg-amber-50 px-3 py-2 text-sm text-amber-800">
          <FileQuestion className="size-4 shrink-0" />
          {t("detail.fileGone")}
        </p>
      ) : null}
      <Footer
        start={
          <Button type="button" variant="ghost" disabled={!actions.desktop} onClick={() => actions.onRemove(task.id)}>
            <ListX />
            {t("menu.remove")}
          </Button>
        }
      >
        <RevealButton id={task.id} actions={actions} />
        <Button data-primary type="button" disabled={!actions.desktop || gone} onClick={() => actions.onOpen(task.id)}>
          <ExternalLink />
          {t("menu.open")}
        </Button>
      </Footer>
    </div>
  )
}

/** Bằng chứng của Doctor: mã, lúc lỗi, đã tải tới đâu, bao nhiêu lần, máy chủ. */
function evidenceRows(detail: DownloadDetail, live: LiveNumbers, code: string) {
  const failed = failures(detail.events)
  const last = failed[0]
  const earlier = failed
    .slice(1, 4)
    .map((event) => `${formatTime(event.at)} ${event.code}`)
    .join(" · ")
  return [
    { label: t("detail.errorCode"), value: code, raw: true },
    {
      label: t("detail.failedAt"),
      value: last ? `${formatTime(last.at)} · ${formatDate(last.at)}` : "—",
    },
    { label: t("detail.downloaded"), value: bytesOf(live) },
    {
      label: t("detail.failures"),
      value: earlier ? `${failed.length} (${earlier})` : String(Math.max(1, failed.length)),
    },
    { label: t("detail.server"), value: hostOfTask(detail.task) || "—", raw: true },
  ]
}

/** S-21: lỗi — Doctor (nguyên nhân, bằng chứng, cách sửa, mã lỗi), thử lại, sao chép log. */
export function FailedState({ detail, item, live, actions }: StateProps) {
  const { task } = detail
  const report = diagnose(
    item ?? {
      id: task.id,
      name: task.fileName,
      status: task.status === "quarantined" ? "quarantined" : "failed",
      errorCode: task.error?.code,
      note: task.error?.message,
      hashState: task.hashState,
    } as DownloadItem
  )
  const style = CATEGORY_STYLE[report.category]
  const Icon = style.icon
  const code = task.error?.code ?? "unknown"
  const quarantined = task.status === "quarantined"
  return (
    <div data-s="S-21" className="space-y-4">
      <div className="flex items-start gap-3 rounded-xl bg-muted p-3">
        <div className={`grid size-10 shrink-0 place-items-center rounded-lg ${style.box}`}>
          <Icon className="size-4" />
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="text-md font-semibold">{report.title}</h3>
          <p className="mt-0.5 text-sm text-muted-foreground">{report.summary}</p>
        </div>
      </div>
      <section className="space-y-2">
        <h3 className="text-sm font-medium">{t("detail.evidence")}</h3>
        <Rows rows={evidenceRows(detail, live, code)} />
      </section>
      <section className="rounded-xl bg-muted/60 p-3.5">
        <h3 className="text-sm font-medium">{t("doctor.steps")}</h3>
        <ol className="mt-2 space-y-1.5 ps-4 text-sm text-muted-foreground">
          {report.steps.map((step) => (
            <li key={step} className="list-decimal">
              {step}
            </li>
          ))}
        </ol>
      </section>
      {/* Chưa chia đoạn nào (lỗi ngay từ đầu) hay file đã tải đủ rồi bị cách ly: bản đồ chỉ thêm chữ. */}
      {!quarantined && detail.parts.some((part) => part.segments.length > 0) ? (
        <SegmentMap parts={detail.parts} live={false} />
      ) : null}
      <Footer
        start={
          <>
            <CopyButton
              label={t("detail.copyLog")}
              done={t("detail.logCopied")}
              copy={async () => navigator.clipboard.writeText(await downloadLog(task.id))}
            />
            {quarantined ? null : <RevealButton id={task.id} actions={actions} />}
          </>
        }
      >
        {report.category === "tool" ? (
          <Button type="button" variant="outline" onClick={actions.onSettings}>
            <Wrench />
            {t("tools.title")}
          </Button>
        ) : null}
        {actions.failedExtra?.(detail)}
        {report.retryable ? (
          <Button data-primary type="button" disabled={!actions.desktop} onClick={() => actions.onResume(task.id)}>
            <RefreshCw />
            {t("menu.retry")}
          </Button>
        ) : quarantined ? (
          // File bị cách ly: việc chính là xem nó trong thư mục cách ly (không mở/chạy, AC-10).
          <Button data-primary type="button" disabled={!actions.desktop} onClick={() => actions.onReveal(task.id)}>
            <FolderOpen />
            {t("menu.reveal")}
          </Button>
        ) : (
          // Thử lại vô ích (nguồn đã gỡ, cần quyền, DRM…): làm theo cách sửa ở trên rồi đóng.
          <Button data-primary type="button" onClick={actions.onClose}>
            {t("app.close")}
          </Button>
        )}
      </Footer>
    </div>
  )
}

/** S-17: đang đọc chi tiết. */
export function LoadingState() {
  return (
    <div data-s="S-17" role="status" className="space-y-3">
      <div className="text-xs font-medium text-muted-foreground">{t("detail.loading")}</div>
      <div className="h-2 animate-pulse rounded-full bg-muted" />
      <div className="grid grid-cols-4 gap-2">
        {[0, 1, 2, 3].map((index) => (
          <div key={index} className="h-14 animate-pulse rounded-xl bg-muted" />
        ))}
      </div>
      <div className="h-3.5 animate-pulse rounded-md bg-muted" />
    </div>
  )
}

function Notice({
  s,
  icon: Icon,
  tone,
  title,
  body,
  children,
}: {
  s: string
  icon: typeof AlertCircle
  tone: string
  title: string
  body: ReactNode
  children: ReactNode
}) {
  return (
    <div data-s={s} className="flex flex-col items-center gap-3 py-6 text-center">
      <div className={`grid size-11 place-items-center rounded-xl ${tone}`}>
        <Icon className="size-5" />
      </div>
      <h3 className="text-md font-semibold">{title}</h3>
      <div className="max-w-sm text-sm text-muted-foreground">{body}</div>
      <div className="mt-1 flex gap-2">{children}</div>
    </div>
  )
}

/** S-16: download không còn (đã xoá khỏi danh sách). */
export function GoneState({ onClose }: { onClose: () => void }) {
  return (
    <Notice s="S-16" icon={FileQuestion} tone="bg-muted text-muted-foreground" title={t("detail.gone.title")} body={t("detail.gone.body")}>
      <Button data-primary type="button" onClick={onClose}>
        {t("detail.back")}
      </Button>
    </Notice>
  )
}

/** S-18: không đọc được dữ liệu download (journal lỗi). */
export function ReadErrorState({
  error,
  onRetry,
  onClose,
}: {
  error: string | null
  onRetry: () => void
  onClose: () => void
}) {
  return (
    <Notice
      s="S-18"
      icon={AlertCircle}
      tone="bg-destructive/10 text-destructive"
      title={t("detail.readFailed.title")}
      body={
        <>
          {t("detail.readFailed.body")}
          {error ? <code className="mt-2 block font-mono text-xs break-all">{error}</code> : null}
        </>
      }
    >
      <Button type="button" variant="ghost" onClick={onClose}>
        {t("detail.back")}
      </Button>
      <Button data-primary type="button" onClick={onRetry}>
        <RefreshCw />
        {t("app.retry")}
      </Button>
    </Notice>
  )
}
