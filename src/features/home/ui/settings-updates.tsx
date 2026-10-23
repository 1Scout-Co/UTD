import { useEffect, useRef, useState } from "react"

import {
  appVersion,
  checkForUpdate,
  getUpdateCheck,
  installUpdate,
  setUpdateCheck,
  subscribeUpdateAvailable,
  subscribeUpdateProgress,
} from "../api/updates"
import type {
  UpdateCheckState,
  UpdateInfo,
  UpdateProgress,
} from "../model/types"
import { SettingsSection } from "./settings-section"
import {
  AutoCheck,
  UpdateCard,
  UpdateMessages,
  VersionRow,
} from "./settings-updates-parts"

/** own = false: lần cài bắt đầu trước khi mở lại Cài đặt, theo dõi bằng cách hỏi lại. */
type Busy = { kind: "check" } | { kind: "install"; own: boolean }

function messageOf(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason)
}

/** Số phiên bản đang chạy; null khi chưa đọc được hoặc ở bản xem trước trên trình duyệt. */
export function useAppVersion(desktop: boolean): string | null {
  const [version, setVersion] = useState<string | null>(null)
  useEffect(() => {
    if (!desktop) return
    let disposed = false
    void appVersion()
      .then((value) => {
        if (!disposed) setVersion(value)
      })
      .catch(() => undefined)
    return () => {
      disposed = true
    }
  }, [desktop])
  return version
}

/** Lần cài bắt đầu trước khi mở lại Cài đặt: hỏi lại mỗi 1,5 s đến khi app báo cài xong. */
function useAdoptedInstall(adopted: boolean, onDone: () => void) {
  const done = useRef(onDone)
  useEffect(() => {
    done.current = onDone
  })
  useEffect(() => {
    if (!adopted) return
    const timer = window.setInterval(() => {
      void getUpdateCheck()
        .then((value) => {
          if (!value.installing) done.current()
        })
        .catch(() => undefined)
    }, 1500)
    return () => window.clearInterval(timer)
  }, [adopted])
}

/** Trạng thái đã lưu + sự kiện từ app (tiến độ cài, bản mới tìm thấy lúc mở app). */
function useUpdateState(active: boolean) {
  const [state, setState] = useState<UpdateCheckState | null>(null)
  const [available, setAvailable] = useState<UpdateInfo | null>(null)
  const [progress, setProgress] = useState<UpdateProgress | null>(null)
  const [busy, setBusy] = useState<Busy | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!active) return
    let disposed = false
    const stops: Array<() => void> = []
    const keep = (stop: () => void) => {
      if (disposed) stop()
      else stops.push(stop)
    }
    void subscribeUpdateProgress((value) => {
      if (!disposed) setProgress(value)
    })
      .then(keep)
      .catch(() => undefined)
    void subscribeUpdateAvailable((value) => {
      if (!disposed) setAvailable(value)
    })
      .then(keep)
      .catch(() => undefined)
    void getUpdateCheck()
      .then((value) => {
        if (disposed) return
        setState(value)
        if (value.available) setAvailable(value.available)
        if (value.installing) setBusy({ kind: "install", own: false })
      })
      .catch((reason: unknown) => {
        if (!disposed) setError(messageOf(reason))
      })
    return () => {
      disposed = true
      stops.forEach((stop) => stop())
    }
  }, [active])

  // Cài xong thì app tự đóng và mở lại: còn ở đây mà hết "đang cài" nghĩa là lần cài đó hỏng.
  useAdoptedInstall(busy?.kind === "install" && !busy.own, () => {
    setBusy(null)
    setProgress(null)
    setError("Cập nhật chưa xong. Bấm Cập nhật để thử lại.")
  })

  return {
    state,
    setState,
    available,
    setAvailable,
    progress,
    setProgress,
    busy,
    setBusy,
    error,
    setError,
  }
}

function useUpdates(active: boolean) {
  const updates = useUpdateState(active)
  const [notice, setNotice] = useState<string | null>(null)
  const { setBusy, setError, setProgress } = updates

  const check = async () => {
    setBusy({ kind: "check" })
    setError(null)
    setNotice(null)
    try {
      const found = await checkForUpdate()
      updates.setAvailable(found)
      if (!found) setNotice("Bạn đang dùng bản mới nhất.")
    } catch (reason) {
      setError(messageOf(reason))
    } finally {
      setBusy(null)
    }
  }

  // Thành công thì app tự đóng để bộ cài chạy, rồi mở lại bản mới: không quay về đây.
  const install = async () => {
    setBusy({ kind: "install", own: true })
    setError(null)
    setNotice(null)
    setProgress(null)
    try {
      await installUpdate()
    } catch (reason) {
      setError(messageOf(reason))
      setBusy(null)
      setProgress(null)
    }
  }

  const toggle = async (enabled: boolean) => {
    const before = updates.state
    if (!before) return
    updates.setState({ ...before, enabled })
    setError(null)
    try {
      await setUpdateCheck(enabled)
    } catch (reason) {
      updates.setState(before)
      setError(messageOf(reason))
    }
  }

  return { ...updates, notice, check, install, toggle }
}

/** Cuộn mục vào tầm nhìn khi mở thẳng tới nó; chờ hộp thoại đặt focus đầu tiên xong (nếu không sẽ bị cuộn ngược lên). */
function useScrollIntoView(active: boolean) {
  const ref = useRef<HTMLElement>(null)
  useEffect(() => {
    if (!active) return
    const timer = window.setTimeout(
      () => ref.current?.scrollIntoView({ block: "start", behavior: "smooth" }),
      150
    )
    return () => window.clearTimeout(timer)
  }, [active])
  return ref
}

/** Mục "Cập nhật" trong Cài đặt (S-27): bản đang dùng, kiểm tra, cài bản mới có chữ ký. */
export function SettingsUpdates({
  open,
  desktop,
  version,
  focused = false,
}: {
  open: boolean
  desktop: boolean
  version: string | null
  focused?: boolean
}) {
  const updates = useUpdates(open && desktop)
  const sectionRef = useScrollIntoView(open && focused)
  const ready = updates.state?.ready === true
  const installing = updates.busy?.kind === "install"
  const locked = !desktop || !ready || updates.busy !== null
  const current = version ?? (desktop ? "…" : "bản xem trước")

  return (
    <SettingsSection
      ref={sectionRef}
      title="Cập nhật"
      description="Chỉ hỏi GitHub có bản mới không, không gửi dữ liệu."
    >
      <div className="space-y-2.5" data-testid="settings-updates">
        <VersionRow
          current={current}
          checking={updates.busy?.kind === "check"}
          locked={locked}
          onCheck={() => void updates.check()}
        />
        {updates.available ? (
          <UpdateCard
            update={updates.available}
            installing={installing}
            progress={updates.progress}
            locked={locked}
            onInstall={() => void updates.install()}
          />
        ) : null}
        <AutoCheck
          enabled={updates.state?.enabled ?? true}
          disabled={!desktop || !ready}
          onChange={(enabled) => void updates.toggle(enabled)}
        />
        <UpdateMessages
          desktop={desktop}
          notReady={desktop && updates.state !== null && !ready}
          notice={updates.notice}
          error={updates.error}
        />
      </div>
    </SettingsSection>
  )
}
