import { useState } from "react"

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { commandErrorText, t } from "@/shared/i18n/i18n"

import {
  liveNumbers,
  screenOf,
  type DownloadDetail,
} from "../model/detail"
import type { DownloadItem, EngineProgress, EngineTask } from "../model/types"
import {
  ActiveState,
  DoneState,
  FailedState,
  GoneState,
  LoadingState,
  PausedState,
  ReadErrorState,
  type DetailActions,
} from "./detail-states"
import { useDownloadDetail } from "./use-download-detail"
import { ItemIcon, useFileIcons } from "./variants/soniox-item-icon"
import { StatusPill } from "./variants/soniox-table-parts"

/** Thao tác màn chính đưa vào; lỗi (câu có mã từ Rust) hiện ngay trong hộp. */
export type DetailHandlers = {
  [Key in "onPause" | "onResume" | "onRemove" | "onTrash" | "onOpen" | "onReveal"]: (
    id: string
  ) => void | Promise<unknown>
} & Pick<DetailActions, "onSettings" | "failedExtra">

export interface DetailDialogProps {
  open: boolean
  /** Task đang mở (hoặc vừa đóng, để vẽ nốt lúc hộp mờ dần). */
  taskId: string | null
  /** Dòng của task trong danh sách (tốc độ, còn lại, Doctor); không còn ⇒ có thể đã bị xoá. */
  item: DownloadItem | undefined
  task: EngineTask | undefined
  progress: EngineProgress | undefined
  desktop: boolean
  onClose: () => void
  handlers: DetailHandlers
}

function Header({ item, detail }: { item?: DownloadItem; detail: DownloadDetail | null }) {
  const icons = useFileIcons(item ? [item] : [])
  const name = item?.name ?? detail?.task.fileName ?? t("detail.title")
  return (
    <DialogHeader className="flex-row items-center gap-3">
      {item ? <ItemIcon item={item} icon={icons[item.id]} /> : null}
      <div className="min-w-0 flex-1">
        <DialogTitle dir="auto" className="truncate [text-align:match-parent]" title={name}>
          {name}
        </DialogTitle>
        <DialogDescription dir="auto" className="truncate [text-align:match-parent]">
          {item?.subtitle ?? t("detail.title")}
        </DialogDescription>
      </div>
      {item ? <StatusPill status={item.status} /> : null}
    </DialogHeader>
  )
}

/** Chạy một thao tác; ném lỗi ⇒ hiện câu lỗi dưới thân hộp. */
function useGuarded(handlers: DetailHandlers) {
  const [error, setError] = useState<string | null>(null)
  const guard =
    (run: (id: string) => void | Promise<unknown>) =>
    (id: string) => {
      setError(null)
      void Promise.resolve()
        .then(() => run(id))
        .catch((reason: unknown) => setError(commandErrorText(reason)))
    }
  return {
    error,
    clear: () => setError(null),
    actions: {
      onPause: guard(handlers.onPause),
      onResume: guard(handlers.onResume),
      onRemove: guard(handlers.onRemove),
      onTrash: guard(handlers.onTrash),
      onOpen: guard(handlers.onOpen),
      onReveal: guard(handlers.onReveal),
      onSettings: handlers.onSettings,
      failedExtra: handlers.failedExtra,
    },
  }
}

/**
 * Chi tiết download (S-15..S-21), mở bằng nhấp đúp một dòng, phím Enter hoặc "Chi tiết" ở menu chuột phải.
 * Mỗi trạng thái một màn; số liệu sống lấy từ tiến độ màn chính đã nhận, bản đồ đoạn từ `utd://segments`.
 */
export function DetailDialog({
  open,
  taskId,
  item,
  task,
  progress,
  desktop,
  onClose,
  handlers,
}: DetailDialogProps) {
  const version = `${task?.status ?? "-"}|${task?.error?.code ?? ""}|${item ? "listed" : "gone"}`
  const state = useDownloadDetail(taskId, open, desktop, version, item)
  const { error, clear, actions } = useGuarded(handlers)
  const detail = state.detail
  // Bản mới nhất (theo `updatedAt`): sự kiện của danh sách thường tới trước, journal đọc lại theo sau.
  const newest =
    task && detail && detail.task.updatedAt > task.updatedAt ? detail.task : (task ?? detail?.task)
  const status = desktop ? newest?.status : detail?.task.status
  const screen = screenOf(state.load, status)
  const live = liveNumbers(item, desktop ? newest : undefined, progress)
  const props = detail
    ? { detail, item, live, actions: { ...actions, desktop, onClose } }
    : null

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          clear()
          onClose()
        }
      }}
    >
      <DialogContent data-variant="soniox" className="gap-4 sm:max-w-[640px]">
        <Header item={item} detail={detail} />
        {screen === "S-17" ? <LoadingState /> : null}
        {screen === "S-16" ? <GoneState onClose={onClose} /> : null}
        {screen === "S-18" ? (
          <ReadErrorState error={state.error} onRetry={state.retry} onClose={onClose} />
        ) : null}
        {props && screen === "S-15" ? <ActiveState {...props} /> : null}
        {props && screen === "S-19" ? <PausedState {...props} /> : null}
        {props && screen === "S-20" ? <DoneState {...props} /> : null}
        {props && screen === "S-21" ? <FailedState {...props} /> : null}
        {/* text-red-700: text-destructive trên nền này chỉ đạt 4,49:1, dưới mức 4,5:1 của WCAG AA. */}
        {error ? (
          <p role="alert" className="rounded-lg bg-destructive/5 px-3 py-2 text-sm text-red-700">
            {error}
          </p>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}
