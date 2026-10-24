import { Channel, invoke } from "@tauri-apps/api/core"

import type { ListingEvent } from "../model/channel"

/**
 * Bắt đầu liệt kê video của kênh (src-tauri/src/media/channel): trả ngay, video tới dần qua `onEvent`,
 * sự kiện cuối luôn là `done`. Lỗi trả ngay khi link không phải kênh hoặc thiếu yt-dlp.
 */
export async function listChannel(
  url: string,
  listingId: string,
  onEvent: (event: ListingEvent) => void,
): Promise<void> {
  const channel = new Channel<ListingEvent>(onEvent)
  await invoke("channel_list", { url, listingId, onEvent: channel })
}

/** Dừng lượt liệt kê (nút Dừng, đóng hộp); video đã nhận vẫn dùng được. */
export async function cancelChannelListing(listingId: string): Promise<void> {
  await invoke("channel_list_cancel", { listingId })
}
