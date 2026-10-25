import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

import type { DownloadDetail, PartsUpdate } from "../model/detail"
import { fileNameFromUrl } from "../model/download-mapper"
import type { LinkFacts, LinkWarning, WarnedLink } from "../model/warnings"
import { inspectLink } from "./downloads"

/** Dò link chờ tối đa chừng này trước khi tự kiểm theo link + tên (mạng chậm không giữ nút Bắt đầu lâu). */
const PROBE_MS = 3000

/** Một lần đọc cho màn Chi tiết; null = download không còn (S-16). */
export async function downloadDetail(
  taskId: string
): Promise<DownloadDetail | null> {
  return invoke<DownloadDetail | null>("download_detail", { taskId })
}

/** Báo app màn Chi tiết đang mở task nào (null = đã đóng): chỉ task đó được đẩy bản đồ đoạn. */
export async function watchDownload(taskId: string | null): Promise<void> {
  await invoke("watch_download", { taskId })
}

/** Bản đồ đoạn mới của task đang mở (app đẩy theo nhịp tiến độ, tối đa ~1 lần/giây). */
export async function subscribeSegments(
  onUpdate: (update: PartsUpdate) => void
): Promise<UnlistenFn> {
  return listen<PartsUpdate>("utd://segments", (event) =>
    onUpdate(event.payload)
  )
}

/** Nhật ký một task (link đã bỏ token, thư mục người dùng thành ~) để dán vào báo lỗi. */
export async function downloadLog(taskId: string): Promise<string> {
  return invoke<string>("download_log", { taskId })
}

/** Cảnh báo trước khi tải (S-22) của từng link, cùng thứ tự; không mở mạng. */
export async function checkLinks(links: LinkFacts[]): Promise<LinkWarning[][]> {
  if (links.length === 0) return []
  const found = await invoke<LinkWarning[][] | null>("check_links", { links })
  return Array.isArray(found) ? found : []
}

/**
 * Một link trước khi bắt đầu: dò tên server đặt + kiểu nội dung (tối đa `PROBE_MS`), rồi kiểm.
 * `fileName` = tên người dùng tự đặt (nếu có). Không có gì đáng ngại hoặc không kiểm được ⇒ null (engine
 * vẫn chạy Download Guard sau khi tải).
 */
export async function preflightLink(
  url: string,
  fileName?: string
): Promise<WarnedLink | null> {
  const probe = new Promise<null>((resolve) => window.setTimeout(() => resolve(null), PROBE_MS))
  const remote = await Promise.race([inspectLink(url).catch(() => null), probe])
  const name = fileName || remote?.fileName || fileNameFromUrl(url)
  const facts: LinkFacts = {
    url,
    finalUrl: remote?.url,
    fileName: name,
    contentType: remote?.contentType ?? undefined,
  }
  const found = await checkLinks([facts]).catch(() => [])
  const warnings = found[0] ?? []
  return warnings.length ? { url, fileName: name, warnings } : null
}
