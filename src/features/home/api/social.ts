import { Channel, invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

import type { ListingEvent } from "../model/channel"
import type { SocialRequest, SocialSource } from "../model/social"
import type { EngineTask } from "../model/types"

/** Danh sách mới tiện ích gửi tới (Tải hàng loạt bằng UTD): mở hộp với mã đó. */
export async function subscribeSocialRequest(onRequest: (request: SocialRequest) => void): Promise<UnlistenFn> {
  return listen<SocialRequest>("utd://social-request", (event) => onRequest(event.payload))
}

/** Danh sách tới lúc giao diện chưa kịp nghe (UTD vừa mở); gọi một lần sau khi đã nghe. */
export async function takeSocialRequest(): Promise<SocialRequest | null> {
  return invoke<SocialRequest | null>("take_social_request")
}

/** Nhận danh sách (src-tauri/src/social): phần đã có gửi lại ngay, phần sau tới dần; sự kiện cuối là `done`. */
export async function watchSocial(listingId: string, onEvent: (event: ListingEvent) => void): Promise<void> {
  const channel = new Channel<ListingEvent>(onEvent)
  await invoke("social_watch", { listingId, onEvent: channel })
}

/** Báo tiện ích (ở đợt gửi sau): đi tiếp hay Dừng, nhóm nào còn cần lấy, thôi lấy mục cũ hơn `since`. */
export async function controlSocial(
  listingId: string,
  control: { go: boolean; sources: SocialSource[] | null; since: number | null },
): Promise<void> {
  await invoke("social_control", { listingId, ...control })
}

/** Hộp đóng: app bỏ danh sách, tiện ích dừng. */
export async function closeSocial(listingId: string): Promise<void> {
  await invoke("social_close", { listingId })
}

/** Đưa các mục đã chọn (theo id) vào hàng đợi chung. */
export async function addSocialDownloads(input: {
  listingId: string
  ids: string[]
  subfolder?: string
  later?: boolean
}): Promise<{ tasks: EngineTask[]; failed: number }> {
  return invoke<{ tasks: EngineTask[]; failed: number }>("add_social_downloads", { input })
}
