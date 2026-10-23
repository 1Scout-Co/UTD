import { invoke, isTauri } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { open as openDialog } from "@tauri-apps/plugin-dialog"

import type {
  AddDownloadPayload,
  AddMediaPayload,
  BrowserIntegrationStatus,
  EngineProgress,
  EngineTask,
  MediaProbe,
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
