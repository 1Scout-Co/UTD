import { useEffect, useState } from "react"

import { open as openDialog } from "@tauri-apps/plugin-dialog"

import { commandErrorText, t } from "@/shared/i18n/i18n"

import { isDesktopApp } from "../../api/downloads"
import {
  deleteDownloadFile,
  moveDownload,
  openDownload,
  renameDownload,
  revealDownload,
} from "../../api/files"
import type { DownloadItem, EngineTask } from "../../model/types"

export type FileActionHooks = {
  /** Link gốc của task (để "Sao chép link"); bản xem trước không có. */
  linkOf?: (id: string) => string | undefined
  /** Engine trả task mới sau khi đổi tên / di chuyển. */
  onTaskUpdate?: (task: EngineTask) => void
  /** Task đã bỏ khỏi danh sách (sau "Xoá file"). */
  onTaskRemoved?: (id: string) => void
}

export type ActionMessage = { tone: "ok" | "error"; text: string }

/** Chạy một thao tác; trả chuỗi = báo thành công, ném lỗi = báo lỗi. Thông báo tự tắt sau 6 giây. */
function useActionMessage() {
  const [message, setMessage] = useState<ActionMessage | null>(null)

  useEffect(() => {
    if (!message) return
    const timer = window.setTimeout(() => setMessage(null), 6000)
    return () => window.clearTimeout(timer)
  }, [message])

  const guard = (task: () => Promise<string | void>) => {
    setMessage(null)
    void task()
      .then((ok) => {
        if (ok) setMessage({ tone: "ok", text: ok })
      })
      .catch((reason: unknown) =>
        setMessage({ tone: "error", text: commandErrorText(reason) })
      )
  }

  return { message, guard }
}

/** Chọn thư mục đích bằng hộp chọn thư mục của Windows rồi chuyển file; huỷ chọn thì thôi. */
async function pickAndMove(id: string): Promise<EngineTask | null> {
  const dir = await openDialog({
    directory: true,
    multiple: false,
    title: t("file.moveTitle"),
  })
  return typeof dir === "string" ? moveDownload(id, dir) : null
}

/** Thao tác file của menu chuột phải (giống IDM). Lỗi/thông báo ngắn hiện ở chân bảng rồi tự tắt. */
export function useFileActions({
  linkOf,
  onTaskUpdate,
  onTaskRemoved,
}: FileActionHooks) {
  const desktop = isDesktopApp()
  const { message, guard } = useActionMessage()
  const [renaming, setRenaming] = useState<DownloadItem | null>(null)

  const move = (id: string) =>
    guard(async () => {
      const task = await pickAndMove(id)
      if (!task) return
      onTaskUpdate?.(task)
      return t("file.moved")
    })

  return {
    desktop,
    message,
    renaming,
    setRenaming,
    hasLink: (id: string) => Boolean(linkOf?.(id)),
    open: (id: string) => guard(() => openDownload(id)),
    reveal: (id: string) => guard(() => revealDownload(id)),
    move,
    copyLink: (id: string) =>
      guard(async () => {
        await navigator.clipboard.writeText(linkOf?.(id) ?? "")
        return t("file.linkCopied")
      }),
    trash: (id: string) =>
      guard(async () => {
        await deleteDownloadFile(id)
        onTaskRemoved?.(id)
        return t("file.trashed")
      }),
    /** Hộp Đổi tên gọi; lỗi ném lại để hộp hiện ngay dưới ô nhập. */
    rename: async (id: string, name: string) => {
      onTaskUpdate?.(await renameDownload(id, name))
    },
  }
}
