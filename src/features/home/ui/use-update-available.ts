import { useEffect, useState } from "react"

import { getUpdateCheck, subscribeUpdateAvailable } from "../api/updates"
import type { UpdateInfo } from "../model/types"

/**
 * Bản mới tìm thấy (để hiện chấm "Mới" cạnh Cài đặt). App tự kiểm tra ~15 s sau khi mở rồi phát
 * `utd://update-available`; đọc thêm trạng thái đã lưu để không lỡ sự kiện phát trước khi nghe.
 * Bản xem trước trên trình duyệt: luôn null.
 */
export function useUpdateAvailable(desktop: boolean): UpdateInfo | null {
  const [update, setUpdate] = useState<UpdateInfo | null>(null)

  useEffect(() => {
    if (!desktop) return
    let disposed = false
    let stop: (() => void) | undefined
    void subscribeUpdateAvailable((value) => {
      if (!disposed) setUpdate(value)
    })
      .then((unlisten) => {
        if (disposed) unlisten()
        else stop = unlisten
      })
      .catch(() => undefined)
    void getUpdateCheck()
      .then((state) => {
        if (!disposed && state.available) setUpdate(state.available)
      })
      .catch(() => undefined)
    return () => {
      disposed = true
      stop?.()
    }
  }, [desktop])

  return update
}
