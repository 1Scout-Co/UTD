import { useCallback, useEffect, useMemo, useState } from "react"

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
} from "../api/downloads"
import { formatBytes, mapEngineTask } from "../model/download-mapper"
import { isChannelPage } from "../model/media-pages"
import { downloads as demoDownloads, stats as demoStats } from "../model/mock-data"
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
import { DoctorDialog } from "./doctor-dialog"
import { MediaPickerDialog, useMediaRequests } from "./media-picker-dialog"
import { SettingsDialog } from "./settings-dialog"
import { ToolsAutoCard } from "./tools-auto"
import { useBatchFlow } from "./use-batch-flow"
import { useUpdateAvailable } from "./use-update-available"
import { Soniox } from "./variants/soniox"

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
  } | null>(null)
  const [diagnoseId, setDiagnoseId] = useState<string | null>(null)
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

  const liveItems = useMemo(
    () =>
      tasks.map((task) => mapEngineTask(task, progress[task.id])),
    [tasks, progress],
  )
  const items = desktop ? liveItems : demoDownloads
  const diagnoseItem = diagnoseId
    ? (items.find((item) => item.id === diagnoseId) ?? null)
    : null

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
        label: "Ổ D:",
        free: demoStats.storageFree,
        usedPercent: demoStats.storageUsed,
      }

  const closeDialogs = useCallback(() => {
    setAddOpen(false)
    setMediaOpen(false)
    setSettingsOpen(false)
  }, [])
  const batch = useBatchFlow(desktop, closeDialogs)
  // Task vừa thêm theo lô: lên đầu danh sách (thứ tự chạy do hàng đợi ở app quyết).
  const addTasks = (added: EngineTask[]) => {
    const ids = new Set(added.map((task) => task.id))
    setTasks((previous) => [...added, ...previous.filter((task) => !ids.has(task.id))])
    if (added.length) setScreenState("data")
    refreshStorage()
  }

  // Task mới (thêm từ hộp Tải cả kênh) lên đầu danh sách.
  const showTask = useCallback((task: EngineTask) => {
    setTasks((previous) => [task, ...previous.filter((item) => item.id !== task.id)])
    setScreenState("data")
  }, [])
  const channel = useChannelFlow(tasks, showTask)
  const { openChannel } = channel

  // Mã `channel_page:` của app (kể cả link rút gọn trỏ tới kênh) mở thẳng hộp Tải cả kênh.
  const openChannelWith = useCallback(
    (url: string) => {
      setAddOpen(false)
      setSettingsOpen(false)
      setMediaOpen(false)
      openChannel(url)
    },
    [openChannel],
  )

  // Link trang video ⇒ hộp Tải media; link kênh (TikTok @user, kênh/playlist YouTube) ⇒ hộp Tải cả kênh.
  const openMediaWith = useCallback(
    (url: string) => {
      if (isChannelPage(url)) {
        openChannelWith(url)
        return
      }
      setAddOpen(false)
      setSettingsOpen(false)
      setMediaError(null)
      setMediaRequest((previous) => ({ url, key: (previous?.key ?? 0) + 1 }))
      setMediaOpen(true)
    },
    [openChannelWith],
  )
  useMediaRequests(desktop, openMediaWith)

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
      setTasks((previous) => [
        task,
        ...previous.filter((item) => item.id !== task.id),
      ])
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
      setTasks((previous) => [
        task,
        ...previous.filter((item) => item.id !== task.id),
      ])
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

  const remove = (taskId: string) => {
    if (!desktop) return
    if (
      !window.confirm(
        "Bỏ mục này khỏi danh sách? File đã tải vẫn được giữ trên đĩa.",
      )
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
      <BatchDialog
        key={batch.key}
        open={batch.open}
        onOpenChange={batch.setOpen}
        desktop={desktop}
        defaultDir={defaultDir}
        initialUrls={batch.urls}
        fromBrowser={batch.fromBrowser}
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
        key={mediaRequest?.key ?? 0}
        initialUrl={mediaRequest?.url}
        open={mediaOpen}
        onOpenChange={setMediaOpen}
        desktop={desktop}
        busy={mediaBusy}
        error={mediaError}
        onSubmit={submitMedia}
        onChannel={openChannelWith}
      />
      <ChannelFlow desktop={desktop} flow={channel} />
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
