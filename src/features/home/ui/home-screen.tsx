import { useCallback, useEffect, useState } from "react"

import type { UnlistenFn } from "@tauri-apps/api/event"

import {
  addDownload,
  addMediaDownload,
  channelPageUrl,
  defaultDownloadDir,
  diskSpace,
  isDesktopApp,
  isMediaPageError,
  listDownloads,
  pauseDownload,
  removeDownload,
  resumeDownload,
  subscribeEngine,
  subscribeMediaRequest,
  takeMediaRequest,
  type MediaRequest,
} from "../api/downloads"
import { formatBytes } from "@/shared/i18n/format"
import { t } from "@/shared/i18n/i18n"

import { mapEngineTask } from "../model/download-mapper"
import { restartDownload } from "../api/recover"
import { isChannelPage } from "../model/media-pages"
import { demoDownloads, demoStorage } from "../model/mock-data"
import type {
  AddDownloadPayload,
  AddMediaPayload,
  EngineProgress,
  EngineTask,
  ScreenState,
  StorageInfo,
} from "../model/types"
import { AddDownloadDialog } from "./add-download-dialog"
import { BatchDialog } from "./batch-dialog"
import { ChannelFlow, useChannelFlow } from "./channel-flow"
import { DetailFlow } from "./detail-flow"
import { DoctorDialog } from "./doctor-dialog"
import { MediaPickerDialog } from "./media-picker-dialog"
import { RefreshLinkDialog } from "./refresh-link-dialog"
import { SettingsDialog } from "./settings-dialog"
import { TakenOverNotice } from "./taken-over-notice"
import { ToolsAutoCard } from "./tools-auto"
import { useBatchFlow } from "./use-batch-flow"
import { useAppEvent } from "./use-app-event"
import { useUpdateAvailable } from "./use-update-available"
import { Soniox } from "./variants/soniox"

/**
 * Task vừa thêm lên đầu danh sách. Lệnh thêm trả trạng thái lúc tạo (đang chờ); file nhỏ có thể đã chạy xong
 * (`utd://task`) trước khi lệnh trả về — bản đã có trong danh sách mới hơn thì giữ, không đè lại "đang chờ".
 */
function putFirst(previous: EngineTask[], added: EngineTask[]): EngineTask[] {
  const known = new Map(previous.map((task) => [task.id, task]))
  const ids = new Set(added.map((task) => task.id))
  return [
    ...added.map((task) => known.get(task.id) ?? task),
    ...previous.filter((task) => !ids.has(task.id)),
  ]
}

export function HomeScreen() {
  const desktop = isDesktopApp()
  const [addOpen, setAddOpen] = useState(false)
  const [mediaOpen, setMediaOpen] = useState(false)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [settingsFocus, setSettingsFocus] = useState<"updates" | null>(null)
  const update = useUpdateAvailable(desktop)
  const [mediaBusy, setMediaBusy] = useState(false)
  const [mediaError, setMediaError] = useState<string | null>(null)
  // Link video chuyển từ hộp Thêm hoặc trình duyệt; `key` đổi ⇒ hộp Tải media dựng lại và tự phân tích.
  const [mediaRequest, setMediaRequest] = useState<{
    url: string
    key: number
    authGrant?: string
  } | null>(null)
  const [diagnoseId, setDiagnoseId] = useState<string | null>(null)
  const [detailId, setDetailId] = useState<string | null>(null)
  // Hộp "Cập nhật link" (menu chuột phải hoặc Doctor).
  const [relinkId, setRelinkId] = useState<string | null>(null)
  const [tasks, setTasks] = useState<EngineTask[]>([])
  const [progress, setProgress] = useState<Record<string, EngineProgress>>({})
  const [storage, setStorage] = useState<StorageInfo | null>(null)
  const [defaultDir, setDefaultDir] = useState("Downloads\\UTD")
  const [screenState, setScreenState] = useState<ScreenState>(
    desktop ? "loading" : "data",
  )
  const [addBusy, setAddBusy] = useState(false)
  const [addError, setAddError] = useState<string | null>(null)

  const refreshStorage = useCallback(() => {
    if (!desktop) return
    void diskSpace()
      .then(setStorage)
      .catch(() => undefined)
  }, [desktop])

  useEffect(() => {
    if (!desktop) return
    let disposed = false
    let unlisten: UnlistenFn | undefined

    const start = async () => {
      try {
        const off = await subscribeEngine({
          onTask: (task) => {
            if (disposed) return
            setTasks((previous) =>
              previous.some((item) => item.id === task.id)
                ? previous.map((item) => (item.id === task.id ? task : item))
                : [task, ...previous],
            )
            setScreenState((previous) =>
              previous === "empty" ? "data" : previous,
            )
            if (task.status === "completed") refreshStorage()
          },
          onProgress: (update) => {
            if (!disposed) {
              setProgress((previous) => ({
                ...previous,
                [update.taskId]: update,
              }))
            }
          },
          onError: (error) => {
            console.error("UTD bridge:", error.message)
          },
        })
        if (disposed) {
          off()
          return
        }
        unlisten = off
        const [list, directory, space] = await Promise.all([
          listDownloads(),
          defaultDownloadDir(),
          diskSpace(),
        ])
        if (disposed) return
        setTasks(list)
        setDefaultDir(directory)
        setStorage(space)
        setScreenState(list.length > 0 ? "data" : "empty")
      } catch (error) {
        console.error("UTD startup:", error)
        if (!disposed) setScreenState("error")
      }
    }

    void start()
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [desktop, refreshStorage])

  // Tính lại mỗi lần vẽ (tiến độ đổi liên tục): đổi ngôn ngữ thì App vẽ lại cả cây nên chữ, số, ngày đổi theo.
  const items = desktop
    ? tasks.map((task) => mapEngineTask(task, progress[task.id]))
    : demoDownloads()
  const diagnoseItem = diagnoseId
    ? (items.find((item) => item.id === diagnoseId) ?? null)
    : null
  const relinkItem = relinkId
    ? (items.find((item) => item.id === relinkId) ?? null)
    : null
  const closeRelink = useCallback(() => setRelinkId(null), [])

  const storageSummary = desktop
    ? storage && storage.totalBytes > 0
      ? {
          label: storage.label,
          free: formatBytes(storage.freeBytes),
          usedPercent: Math.round(
            (1 - storage.freeBytes / storage.totalBytes) * 100,
          ),
        }
      : undefined
    : {
        label: demoStorage.label,
        free: formatBytes(demoStorage.freeBytes),
        usedPercent: demoStorage.usedPercent,
      }

  // Task mới (thêm từ hộp Tải cả kênh) lên đầu danh sách.
  const showTask = useCallback((task: EngineTask) => {
    setTasks((previous) => putFirst(previous, [task]))
    setScreenState("data")
  }, [])
  const channel = useChannelFlow(showTask)
  const { openChannel, setOpen: setChannelOpen } = channel

  // Mỗi lúc một hộp: link/lô từ trình duyệt tới liền nhau thì hộp mới thay hộp cũ, không chồng lên.
  const closeDialogs = useCallback(() => {
    setAddOpen(false)
    setMediaOpen(false)
    setSettingsOpen(false)
    setChannelOpen(false)
  }, [setChannelOpen])
  const batch = useBatchFlow(desktop, closeDialogs)
  const { setOpen: setBatchOpen } = batch
  // Task vừa thêm theo lô: lên đầu danh sách (thứ tự chạy do hàng đợi ở app quyết).
  const addTasks = (added: EngineTask[]) => {
    setTasks((previous) => putFirst(previous, added))
    if (added.length) setScreenState("data")
    refreshStorage()
  }

  // Mã `channel_page:` của app (kể cả link rút gọn trỏ tới kênh) mở thẳng hộp Tải cả kênh.
  const openChannelWith = useCallback(
    (url: string, authGrant?: string) => {
      setAddOpen(false)
      setSettingsOpen(false)
      setMediaOpen(false)
      setBatchOpen(false)
      openChannel(url, authGrant)
    },
    [openChannel, setBatchOpen],
  )

  // Link trang video ⇒ hộp Tải media; link kênh (TikTok @user, kênh/playlist YouTube) ⇒ hộp Tải cả kênh.
  const openMediaWith = useCallback(
    (url: string, authGrant?: string) => {
      if (isChannelPage(url)) {
        openChannelWith(url, authGrant)
        return
      }
      setAddOpen(false)
      setSettingsOpen(false)
      setBatchOpen(false)
      setChannelOpen(false)
      setMediaError(null)
      setMediaRequest((previous) => ({ url, key: (previous?.key ?? 0) + 1, authGrant }))
      setMediaOpen(true)
    },
    [openChannelWith, setBatchOpen, setChannelOpen],
  )
  // Link trang video từ trình duyệt (chuột phải / popup).
  const onMediaRequest = useCallback(
    (request: MediaRequest) => openMediaWith(request.url, request.authGrant),
    [openMediaWith],
  )
  useAppEvent(desktop, subscribeMediaRequest, onMediaRequest, takeMediaRequest)

  const openAdd = () => {
    setAddError(null)
    setAddOpen(true)
  }

  const refresh = async () => {
    if (!desktop) return
    setScreenState("loading")
    try {
      const list = await listDownloads()
      setTasks(list)
      setScreenState(list.length > 0 ? "data" : "empty")
    } catch (error) {
      console.error("UTD refresh:", error)
      setScreenState("error")
    }
  }

  const submitAdd = async (payload: AddDownloadPayload) => {
    if (!desktop) {
      setAddOpen(false)
      return
    }
    setAddBusy(true)
    setAddError(null)
    try {
      const task = await addDownload(payload)
      setTasks((previous) => putFirst(previous, [task]))
      setScreenState("data")
      setAddOpen(false)
      refreshStorage()
    } catch (error) {
      const channelUrl = channelPageUrl(error)
      if (channelUrl) openChannelWith(channelUrl)
      else if (isMediaPageError(error)) openMediaWith(payload.url)
      else setAddError(error instanceof Error ? error.message : String(error))
    } finally {
      setAddBusy(false)
    }
  }

  const openMedia = () => {
    setMediaError(null)
    setMediaOpen(true)
  }

  const submitMedia = async (payload: AddMediaPayload) => {
    if (!desktop) {
      setMediaOpen(false)
      return
    }
    setMediaBusy(true)
    setMediaError(null)
    try {
      const task = await addMediaDownload(payload)
      setTasks((previous) => putFirst(previous, [task]))
      setScreenState("data")
      setMediaOpen(false)
      refreshStorage()
    } catch (error) {
      setMediaError(error instanceof Error ? error.message : String(error))
    } finally {
      setMediaBusy(false)
    }
  }

  const pause = (taskId: string) => {
    if (!desktop) return
    void pauseDownload(taskId).catch((error) => console.error("UTD pause:", error))
  }

  const resume = (taskId: string) => {
    if (!desktop) return
    void resumeDownload(taskId).catch((error) =>
      console.error("UTD resume:", error),
    )
  }

  // File trên máy chủ đã đổi / `.part` bị xoá: bỏ phần đã tải, tải lại từ đầu.
  const restart = (taskId: string) => {
    if (!desktop) return
    void restartDownload(taskId).catch((error) =>
      console.error("UTD restart:", error),
    )
  }

  const remove = (taskId: string) => {
    if (!desktop) return
    if (
      !window.confirm(t("home.confirmRemove"))
    ) {
      return
    }
    void removeDownload(taskId)
      .then(() => dropTask(taskId))
      .catch((error) => console.error("UTD remove:", error))
  }

  // Bỏ task khỏi danh sách (sau "Xoá khỏi danh sách" hoặc "Xoá file" ở menu chuột phải).
  const dropTask = (taskId: string) => {
    setTasks((previous) => previous.filter((item) => item.id !== taskId))
    setProgress((previous) => {
      const next = { ...previous }
      delete next[taskId]
      return next
    })
  }

  // Rỗng/có dữ liệu suy từ danh sách thật: xoá nhanh nhiều mục hay có download mới chen vào thì không lệch.
  const shownState: ScreenState =
    screenState === "data" && tasks.length === 0
      ? "empty"
      : screenState === "empty" && tasks.length > 0
        ? "data"
        : screenState

  return (
    <>
      <Soniox
        state={desktop ? shownState : "data"}
        items={items}
        storage={storageSummary}
        sideStatus={<ToolsAutoCard desktop={desktop} />}
        onAdd={openAdd}
        onMedia={openMedia}
        onRetry={() => void refresh()}
        onPause={pause}
        onResume={resume}
        onRemove={remove}
        onDiagnose={setDiagnoseId}
        onDetails={setDetailId}
        onRefreshLink={setRelinkId}
        onSettings={() => {
          setSettingsFocus(null)
          setSettingsOpen(true)
        }}
        updateVersion={update?.version}
        onUpdates={() => {
          setSettingsFocus("updates")
          setSettingsOpen(true)
        }}
        linkOf={(id) => tasks.find((task) => task.id === id)?.url}
        onTaskUpdate={(task) =>
          setTasks((previous) =>
            previous.map((item) => (item.id === task.id ? task : item)),
          )
        }
        onTaskRemoved={dropTask}
        onBatch={batch.openEmpty}
      />
      {/* Hai hộp cùng cấp: key có tiền tố riêng để không trùng nhau (cùng bắt đầu từ 0). */}
      <BatchDialog
        key={`batch-${batch.key}`}
        open={batch.open}
        onOpenChange={batch.setOpen}
        desktop={desktop}
        defaultDir={defaultDir}
        initialUrls={batch.urls}
        fromBrowser={batch.fromBrowser}
        authGrant={batch.authGrant}
        onAdded={addTasks}
      />
      <AddDownloadDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        defaultDir={defaultDir}
        desktop={desktop}
        busy={addBusy}
        error={addError}
        onSubmit={submitAdd}
        onMedia={openMediaWith}
        onBatch={batch.openWith}
      />
      <MediaPickerDialog
        key={`media-${mediaRequest?.key ?? 0}`}
        initialUrl={mediaRequest?.url}
        authGrant={mediaRequest?.authGrant}
        open={mediaOpen}
        onOpenChange={setMediaOpen}
        desktop={desktop}
        busy={mediaBusy}
        error={mediaError}
        onSubmit={submitMedia}
        onChannel={openChannelWith}
      />
      <ChannelFlow desktop={desktop} flow={channel} />
      <TakenOverNotice desktop={desktop} />
      <DetailFlow
        taskId={detailId}
        onClose={() => setDetailId(null)}
        desktop={desktop}
        items={items}
        tasks={tasks}
        progress={progress}
        onDropped={dropTask}
        onRefreshLink={(id) => {
          setDetailId(null)
          setRelinkId(id)
        }}
        onRestart={restart}
        onSettings={() => {
          setSettingsFocus(null)
          setSettingsOpen(true)
        }}
      />
      <DoctorDialog
        item={diagnoseItem}
        open={diagnoseItem !== null}
        onOpenChange={(open) => {
          if (!open) setDiagnoseId(null)
        }}
        onRetry={(id) => {
          resume(id)
          setDiagnoseId(null)
        }}
        onRefreshLink={(id) => {
          setDiagnoseId(null)
          setRelinkId(id)
        }}
        onRestart={(id) => {
          restart(id)
          setDiagnoseId(null)
        }}
      />
      <RefreshLinkDialog
        item={relinkItem}
        desktop={desktop}
        onClose={closeRelink}
      />
      <SettingsDialog
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        focus={settingsFocus}
        desktop={desktop}
        defaultDir={defaultDir}
        onDefaultDirChange={(directory) => {
          setDefaultDir(directory)
          refreshStorage()
        }}
      />
    </>
  )
}
