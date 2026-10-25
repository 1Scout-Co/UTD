import { useState } from "react"

import { t } from "@/shared/i18n/i18n"

import { pauseDownload, removeDownload, resumeDownload } from "../api/downloads"
import { deleteDownloadFile, openDownload, revealDownload } from "../api/files"
import type { DownloadItem, EngineProgress, EngineTask } from "../model/types"
import { DetailDialog } from "./detail-dialog"

/** Màn chính đưa vào hộp Chi tiết: task đang mở, dữ liệu sống, cách bỏ task khỏi danh sách. */
export interface DetailFlowProps {
  taskId: string | null
  onClose: () => void
  desktop: boolean
  items: DownloadItem[]
  tasks: EngineTask[]
  progress: Record<string, EngineProgress>
  /** Bỏ task khỏi danh sách đang hiện (sau Xoá khỏi danh sách / Huỷ tải). */
  onDropped: (id: string) => void
  onSettings: () => void
}

/** Hộp Chi tiết nối với thao tác thật của app; xoá/huỷ xong thì đóng hộp. */
export function DetailFlow({
  taskId,
  onClose,
  desktop,
  items,
  tasks,
  progress,
  onDropped,
  onSettings,
}: DetailFlowProps) {
  // Đóng hộp vẫn vẽ task vừa xem cho tới khi hộp mờ hẳn (không nháy sang màn "không còn").
  const [lastId, setLastId] = useState(taskId)
  if (taskId !== null && taskId !== lastId) setLastId(taskId)
  const shownId = taskId ?? lastId
  const dropped = (id: string) => {
    onDropped(id)
    onClose()
  }
  return (
    <DetailDialog
      open={taskId !== null}
      taskId={shownId}
      item={items.find((item) => item.id === shownId)}
      task={tasks.find((task) => task.id === shownId)}
      progress={shownId ? progress[shownId] : undefined}
      desktop={desktop}
      onClose={onClose}
      handlers={{
        onPause: pauseDownload,
        onResume: resumeDownload,
        onOpen: openDownload,
        onReveal: revealDownload,
        // Thùng rác: khôi phục được, như "Xoá file" ở menu chuột phải.
        onTrash: (id) => deleteDownloadFile(id).then(() => dropped(id)),
        onRemove: (id) =>
          window.confirm(t("home.confirmRemove"))
            ? removeDownload(id).then(() => dropped(id))
            : undefined,
        onSettings: () => {
          onClose()
          onSettings()
        },
      }}
    />
  )
}
