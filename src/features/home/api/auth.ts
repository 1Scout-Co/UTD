import { invoke } from "@tauri-apps/api/core"

/** Cài đặt › Tích hợp › Đăng nhập (S-73): chỉ tên trang, không bao giờ có giá trị cookie. */
export interface AuthStatus {
  enabled: boolean
  sites: string[]
}

export async function authStatus(): Promise<AuthStatus> {
  return invoke<AuthStatus>("auth_status")
}

export async function setAuthEnabled(enabled: boolean): Promise<AuthStatus> {
  return invoke<AuthStatus>("set_auth_enabled", { enabled })
}

export async function forgetAuthCookies(): Promise<AuthStatus> {
  return invoke<AuthStatus>("forget_auth_cookies")
}
