import { invoke, type InvokeArgs } from "@tauri-apps/api/core"

/** Cài đặt › Tích hợp › Đăng nhập (S-73): chỉ tên trang, không bao giờ có giá trị cookie. */
export interface AuthStatus {
  enabled: boolean
  sites: string[]
}

/** Đọc trạng thái, bật/tắt (`{ enabled }`), quên cookie: lệnh nào cũng trả trạng thái mới. */
export async function authCommand(
  command: "auth_status" | "set_auth_enabled" | "forget_auth_cookies",
  args?: InvokeArgs,
): Promise<AuthStatus> {
  return invoke<AuthStatus>(command, args)
}
