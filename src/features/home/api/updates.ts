import { getVersion } from "@tauri-apps/api/app"
import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

import type {
  UpdateCheckState,
  UpdateInfo,
  UpdateProgress,
} from "../model/types"

// Tự cập nhật: app chỉ đọc latest.json trên GitHub Releases của UTD (không gửi dữ liệu gì khác).
// Lỗi trả về là câu tiếng Việt ngắn để hiện thẳng cho người dùng.

/** Số phiên bản đang chạy (từ tauri.conf.json). */
export async function appVersion(): Promise<string> {
  return getVersion()
}

/** null = đang dùng bản mới nhất. */
export async function checkForUpdate(): Promise<UpdateInfo | null> {
  return invoke<UpdateInfo | null>("check_for_update")
}

/**
 * Tải gói (kiểm chữ ký), dừng download đang chạy rồi chạy bộ cài; app tự đóng và mở lại.
 * Chỉ gọi khi người dùng bấm "Cập nhật". Trả về chỉ khi lỗi (bản cũ giữ nguyên).
 */
export async function installUpdate(): Promise<void> {
  await invoke("install_update")
}

export async function getUpdateCheck(): Promise<UpdateCheckState> {
  return invoke<UpdateCheckState>("get_update_check")
}

export async function setUpdateCheck(enabled: boolean): Promise<boolean> {
  return invoke<boolean>("set_update_check", { enabled })
}

export async function subscribeUpdateProgress(
  onProgress: (progress: UpdateProgress) => void
): Promise<UnlistenFn> {
  return listen<UpdateProgress>("utd://update-progress", (event) =>
    onProgress(event.payload)
  )
}

/** Lần tự kiểm tra lúc mở app tìm thấy bản mới. */
export async function subscribeUpdateAvailable(
  onAvailable: (update: UpdateInfo) => void
): Promise<UnlistenFn> {
  return listen<UpdateInfo>("utd://update-available", (event) =>
    onAvailable(event.payload)
  )
}
