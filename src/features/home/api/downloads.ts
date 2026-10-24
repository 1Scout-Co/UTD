import { invoke, isTauri } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { open as openDialog } from "@tauri-apps/plugin-dialog"

import type {
  AddBatchPayload,
  AddDownloadPayload,
  AddMediaPayload,
  BatchResult,
  BrowserIntegrationStatus,
  EngineProgress,
  EngineTask,
  MediaProbe,
  QueueSettings,
  RemoteFile,
  StorageInfo,
} from "../model/types"

export function isDesktopApp(): boolean {
  return isTauri()
}

export async function listDownloads(): Promise<EngineTask[]> {
  return invoke<EngineTask[]>("list_downloads")
}

export async function addDownload(
  payload: AddDownloadPayload,
): Promise<EngineTask> {
  return invoke<EngineTask>("add_download", { input: payload })
}

export async function pauseDownload(taskId: string): Promise<void> {
  await invoke("pause_download", { taskId })
}

export async function resumeDownload(taskId: string): Promise<void> {
  await invoke("resume_download", { taskId })
}

export async function removeDownload(taskId: string): Promise<void> {
  await invoke("remove_download", { taskId, deleteFiles: false })
}

export async function defaultDownloadDir(): Promise<string> {
  return invoke<string>("default_download_dir")
}

export async function diskSpace(path?: string): Promise<StorageInfo> {
  return invoke<StorageInfo>("disk_space", { path: path ?? null })
}

export interface BridgeError {
  taskId: string
  message: string
}

export interface EngineSubscriptions {
  onTask: (task: EngineTask) => void
  onProgress: (progress: EngineProgress) => void
  onError: (error: BridgeError) => void
}

export async function subscribeEngine(
  subscriptions: EngineSubscriptions,
): Promise<UnlistenFn> {
  const unlisteners = await Promise.all([
    listen<EngineTask>("utd://task", (event) =>
      subscriptions.onTask(event.payload),
    ),
    listen<EngineProgress>("utd://progress", (event) =>
      subscriptions.onProgress(event.payload),
    ),
    listen<BridgeError>("utd://error", (event) =>
      subscriptions.onError(event.payload),
    ),
  ])
  return () => {
    for (const unlisten of unlisteners) unlisten()
  }
}

export async function browserIntegrationStatus(): Promise<BrowserIntegrationStatus> {
  return invoke<BrowserIntegrationStatus>("browser_integration_status")
}

export async function installBrowserIntegration(): Promise<BrowserIntegrationStatus> {
  return invoke<BrowserIntegrationStatus>("install_browser_integration")
}

export async function openExtensionFolder(): Promise<string> {
  return invoke<string>("open_extension_folder")
}

export async function mediaProbe(url: string): Promise<MediaProbe> {
  return invoke<MediaProbe>("media_probe", { url })
}

export async function addMediaDownload(
  payload: AddMediaPayload,
): Promise<EngineTask> {
  return invoke<EngineTask>("add_media_download", { input: payload })
}

/** Lưu thư mục tải mặc định; trả đường dẫn đã chuẩn hoá. Lỗi là câu tiếng Việt ngắn. */
export async function setDefaultDownloadDir(path: string): Promise<string> {
  return invoke<string>("set_default_download_dir", { path })
}

/** Quay về Downloads\UTD; trả thư mục sẽ dùng. */
export async function resetDefaultDownloadDir(): Promise<string> {
  return invoke<string>("reset_default_download_dir")
}

/** Hộp chọn thư mục của Windows; null khi người dùng huỷ. */
export async function pickFolder(defaultPath?: string): Promise<string | null> {
  const picked = await openDialog({
    directory: true,
    multiple: false,
    title: "Chọn thư mục tải",
    defaultPath: defaultPath || undefined,
  })
  return typeof picked === "string" ? picked : null
}

/** Mã đầu lỗi của `add_download` khi URL là trang video (src-tauri/src/media_pages.rs). */
const MEDIA_PAGE_ERROR = "media_page:"

/** `add_download` từ chối vì URL là trang video: mở hộp Tải media thay vì báo lỗi. */
export function isMediaPageError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error)
  return message.startsWith(MEDIA_PAGE_ERROR)
}

/** Mã đầu lỗi khi URL là trang kênh: `channel_page: <link kênh>` (add_download, media_probe). */
const CHANNEL_PAGE_ERROR = "channel_page:"

/** Lỗi báo link là kênh (kể cả link rút gọn trỏ tới kênh): trả link kênh để mở hộp Tải cả kênh. */
export function channelPageUrl(error: unknown): string | null {
  const message = error instanceof Error ? error.message : String(error)
  if (!message.startsWith(CHANNEL_PAGE_ERROR)) return null
  const url = message.slice(CHANNEL_PAGE_ERROR.length).trim()
  return /^https?:\/\//i.test(url) ? url : null
}

/** Link trang video gửi từ trình duyệt (chuột phải / popup của extension). */
interface MediaRequest {
  url: string
  pageUrl?: string
}

export async function subscribeMediaRequest(
  onRequest: (request: MediaRequest) => void,
): Promise<UnlistenFn> {
  return listen<MediaRequest>("utd://media-request", (event) =>
    onRequest(event.payload),
  )
}

/** Link trình duyệt gửi tới lúc giao diện chưa kịp nghe (UTD vừa mở); gọi một lần sau khi đã nghe. */
export async function takeMediaRequest(): Promise<MediaRequest | null> {
  return invoke<MediaRequest | null>("take_media_request")
}

/** Tạo cả lô rồi xếp hàng đợi một lần (giữ thứ tự); link trang video dùng chất lượng chung. */
export async function addBatchDownloads(
  payload: AddBatchPayload,
): Promise<BatchResult> {
  return invoke<BatchResult>("add_batch_downloads", { input: payload })
}

/** Tên + kích thước một link, chưa tạo task (bảng xem trước của hộp Thêm hàng loạt). */
export async function inspectLink(url: string): Promise<RemoteFile> {
  return invoke<RemoteFile>("inspect_link", { url })
}

export async function queueSettings(): Promise<QueueSettings> {
  return invoke<QueueSettings>("queue_settings")
}

export async function setQueueLimit(limit: number): Promise<QueueSettings> {
  return invoke<QueueSettings>("set_queue_limit", { limit })
}

/** Lô link gửi từ trình duyệt ("Tải tất cả link bằng UTD"). */
export interface BatchRequest {
  urls: string[]
  pageUrl?: string
}

export async function subscribeBatchRequest(
  onRequest: (request: BatchRequest) => void,
): Promise<UnlistenFn> {
  return listen<BatchRequest>("utd://batch-request", (event) =>
    onRequest(event.payload),
  )
}

/** Lô trình duyệt gửi tới lúc giao diện chưa kịp nghe; gọi một lần sau khi đã nghe. */
export async function takeBatchRequest(): Promise<BatchRequest | null> {
  return invoke<BatchRequest | null>("take_batch_request")
}
