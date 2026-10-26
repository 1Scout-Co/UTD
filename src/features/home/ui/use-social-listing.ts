import { useCallback, useEffect, useState } from "react"

import { controlSocial, watchSocial } from "../api/social"
import { applyListingEvent, LISTING_START, type ListingState } from "../model/channel"

/**
 * Danh sách tải hàng loạt tiện ích đang gửi (cùng dạng với liệt kê kênh): mục tới dần, `stop` = Dừng (tiện ích
 * thôi lấy ở đợt sau, giữ phần đã có). Không có "thử lại": danh sách do trang trên trình duyệt gửi.
 */
export function useSocialListing(listingId: string | null, enabled: boolean) {
  const [state, setState] = useState<ListingState>(LISTING_START)

  useEffect(() => {
    if (!enabled || !listingId) return
    let live = true
    watchSocial(listingId, (event) => {
      // Lượt cũ (StrictMode chạy effect hai lần) vẫn gửi tới: bỏ qua.
      if (live) setState((previous) => applyListingEvent(previous, event))
    }).catch((reason: unknown) => {
      if (!live) return
      const message = reason instanceof Error ? reason.message : String(reason)
      setState((previous) => ({ ...previous, status: "error", error: message }))
    })
    return () => {
      live = false
    }
  }, [listingId, enabled])

  const stop = useCallback(() => {
    if (listingId) void controlSocial(listingId, { go: false, sources: null, since: null }).catch(() => undefined)
  }, [listingId])

  return { ...state, stop, retry: () => undefined }
}
