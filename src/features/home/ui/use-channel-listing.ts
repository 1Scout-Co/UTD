import { useCallback, useEffect, useRef, useState } from "react"

import { cancelChannelListing, listChannel } from "../api/channel"
import {
  applyListingEvent,
  LISTING_START,
  type ListingEvent,
  type ListingState,
} from "../model/channel"

let nextListing = 0

/**
 * Liệt kê video của kênh khi hộp mở: video tới dần, `stop` dừng lượt (giữ phần đã nhận), đóng hộp thì
 * tự dừng. `retry` chạy lại từ đầu.
 */
export function useChannelListing(url: string, enabled: boolean, authGrant: string | null = null) {
  const [state, setState] = useState<ListingState>(LISTING_START)
  const [run, setRun] = useState(0)
  const current = useRef<string | null>(null)

  useEffect(() => {
    if (!enabled || !url) return
    const id = `channel-${Date.now().toString(36)}-${nextListing++}`
    current.current = id
    let live = true
    const onEvent = (event: ListingEvent) => {
      // Lượt cũ (hộp đã đóng, StrictMode chạy effect hai lần) vẫn gửi tới: dừng hẳn nó.
      if (!live) {
        void cancelChannelListing(id).catch(() => undefined)
        return
      }
      setState((previous) => applyListingEvent(previous, event))
    }
    listChannel(url, id, onEvent, authGrant).catch((reason: unknown) => {
      if (!live) return
      const message = reason instanceof Error ? reason.message : String(reason)
      setState((previous) => ({ ...previous, status: "error", error: message }))
    })
    return () => {
      live = false
      void cancelChannelListing(id).catch(() => undefined)
    }
  }, [url, enabled, run, authGrant])

  const stop = useCallback(() => {
    if (current.current) void cancelChannelListing(current.current).catch(() => undefined)
  }, [])

  const retry = useCallback(() => {
    setState(LISTING_START)
    setRun((value) => value + 1)
  }, [])

  return { ...state, stop, retry }
}
