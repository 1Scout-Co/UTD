import { invoke } from "@tauri-apps/api/core"

import type { EngineTask } from "../model/types"

// Thao tác trên file đã tải (menu chuột phải, giống IDM). Mọi lệnh chỉ chạm đúng file của task đó;
// lỗi trả về là câu tiếng Việt ngắn để hiện thẳng cho người dùng.

/** Mở file bằng ứng dụng mặc định của Windows. Chỉ cho task đã hoàn tất (không mở file bị cách ly). */
export async function openDownload(taskId: string): Promise<void> {
  await invoke("open_download", { taskId })
}

/** Mở Explorer ở thư mục chứa file và chọn sẵn file đó (file mất ⇒ mở thư mục; bị cách ly ⇒ mở thư mục cách ly). */
export async function revealDownload(taskId: string): Promise<void> {
  await invoke("reveal_download", { taskId })
}

/**
 * Đổi tên file (cả file đang dở). Không ghi đè file khác; chặn tên giả đuôi (`.pdf.exe`, ký tự đảo chiều).
 * Video/nhạc tải qua yt-dlp phải đợi tải xong. Trả task đã cập nhật.
 */
export async function renameDownload(
  taskId: string,
  newName: string
): Promise<EngineTask> {
  return invoke<EngineTask>("rename_download", { taskId, newName })
}

/**
 * Chuyển file sang thư mục khác (người dùng chọn bằng hộp chọn thư mục; kiểm như Cài đặt).
 * File đang dở thì chuyển cả file .part. Trả task đã cập nhật.
 */
export async function moveDownload(
  taskId: string,
  destDir: string
): Promise<EngineTask> {
  return invoke<EngineTask>("move_download", { taskId, destDir })
}

/**
 * Bỏ khỏi danh sách và chuyển file (kể cả file .part) vào Thùng rác — khôi phục được.
 * Ổ không có Thùng rác (USB, ổ mạng) ⇒ báo lỗi, không xoá gì, task vẫn còn.
 */
export async function deleteDownloadFile(taskId: string): Promise<void> {
  await invoke("delete_download_file", { taskId })
}

/**
 * Icon thật của file như Windows Explorer (icon riêng của .exe, icon theo loại file),
 * dạng `data:image/png;base64,…`, ảnh vuông 32/48/64 px theo tỉ lệ màn hình (vẽ ở 32 px CSS
 * là nét). Task chưa xong lấy icon theo đuôi file.
 * Task không có icon thì không có trong kết quả — giao diện tự dùng icon mặc định.
 */
export async function fileIcons(
  taskIds: string[]
): Promise<Record<string, string>> {
  if (taskIds.length === 0) return {}
  return invoke<Record<string, string>>("file_icons", { taskIds })
}
