import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

import type {
  MediaToolName,
  MediaToolProgress,
  MediaToolsStatus,
} from "../model/types"

/** yt-dlp/ffmpeg đang dùng: bản của UTD hoặc bản có sẵn trên máy (PATH, WinGet…). */
export async function mediaToolsStatus(): Promise<MediaToolsStatus> {
  return invoke<MediaToolsStatus>("media_tools_status")
}

/**
 * Tải bản chính chủ từ GitHub vào thư mục công cụ của UTD, kiểm SHA-256 trước khi dùng.
 * Chỉ gọi khi người dùng bấm nút (bấm = đồng ý). Lỗi là câu tiếng Việt ngắn.
 */
export async function installMediaTool(
  tool: MediaToolName
): Promise<MediaToolsStatus> {
  return invoke<MediaToolsStatus>("install_media_tool", { tool })
}

export async function cancelMediaToolInstall(): Promise<boolean> {
  return invoke<boolean>("cancel_media_tool_install")
}

export async function subscribeToolProgress(
  onProgress: (progress: MediaToolProgress) => void
): Promise<UnlistenFn> {
  return listen<MediaToolProgress>("utd://tool-progress", (event) =>
    onProgress(event.payload)
  )
}
