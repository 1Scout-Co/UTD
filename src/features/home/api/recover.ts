import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

import type { EngineTask } from "../model/types"

// Cứu download lỗi (src-tauri/src/recover.rs): cập nhật link giữ phần đã tải, tải lại từ đầu.

/** Dán link mới cho download lỗi/đang dừng; app chỉ nhận khi cùng file. Lỗi: "[recover.…] câu". */
export async function updateDownloadLink(
  taskId: string,
  url: string,
): Promise<EngineTask> {
  return invoke<EngineTask>("update_download_link", { taskId, url })
}

/** Hộp "Cập nhật link" mở (id) / đóng (null): trong lúc mở, link tải từ trình duyệt gắn vào task này. */
export async function awaitDownloadLink(taskId: string | null): Promise<void> {
  await invoke("await_download_link", { taskId })
}

/** Bỏ phần đã tải rồi tải lại từ đầu (file trên máy chủ đã đổi, `.part` bị xoá). */
export async function restartDownload(taskId: string): Promise<void> {
  await invoke("restart_download", { taskId })
}

/** Kết quả link gửi từ trình duyệt cho hộp "Cập nhật link": `error` có mã `[recover.…]`. */
export interface LinkUpdate {
  taskId: string
  error: string | null
}

export async function onLinkUpdated(
  handler: (update: LinkUpdate) => void,
): Promise<UnlistenFn> {
  return listen<LinkUpdate>("utd://link-updated", (event) =>
    handler(event.payload),
  )
}
