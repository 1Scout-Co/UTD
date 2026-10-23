const HOST = "app.utd.desktop"
const MENU_LINK = "utd-download"
const MENU_MEDIA = "utd-download-media"
const api = globalThis.browser ?? globalThis.chrome

async function sendToUtd(payload) {
  try {
    const response = await api.runtime.sendNativeMessage(HOST, payload)
    if (!response || response.ok !== true) {
      throw new Error(response?.message || "UTD không phản hồi.")
    }
    await api.action.setBadgeText({ text: "" })
    // UTD có thể gửi kèm câu báo, vd. link video thì "Đã mở UTD để chọn chất lượng".
    const note = typeof response.message === "string" ? response.message : null
    await api.storage.local.set({ lastResult: "ok", lastNote: note, lastAt: Date.now() })
    return { ok: true, taskId: response.taskId, message: note }
  } catch (error) {
    const message =
      error?.message ||
      "Không kết nối được UTD. Kiểm tra UTD đang chạy và đã bật tích hợp trình duyệt."
    await api.action.setBadgeText({ text: "!" })
    if (api.action.setBadgeBackgroundColor) {
      await api.action.setBadgeBackgroundColor({ color: "#d93025" })
    }
    await api.storage.local.set({ lastResult: message, lastAt: Date.now() })
    return { ok: false, message }
  }
}

async function installMenus() {
  try {
    await api.contextMenus.removeAll()
  } catch {
    // Menu có thể chưa tồn tại.
  }
  for (const item of [
    {
      id: MENU_LINK,
      title: "Tải bằng UTD",
      contexts: ["link", "video", "audio", "image", "page"],
    },
    {
      id: MENU_MEDIA,
      title: "Tải video/âm thanh bằng UTD",
      contexts: ["video", "audio"],
    },
  ]) {
    try {
      await api.contextMenus.create(item)
    } catch {
      // Menu đã tồn tại sau khi service worker khởi động lại.
    }
  }
}

function pickUrl(info, tab) {
  const candidates = [info.linkUrl, info.srcUrl, info.pageUrl, tab?.url]
  return (
    candidates.find((value) => typeof value === "string" && /^https?:\/\//i.test(value)) ??
    null
  )
}

function download(info, tab) {
  const url = pickUrl(info, tab)
  if (!url) return
  void sendToUtd({
    version: 1,
    type: "download",
    url,
    pageUrl: tab?.url ?? null,
    suggestedName: null,
  })
}

api.runtime.onInstalled.addListener(() => void installMenus())
api.runtime.onStartup.addListener(() => void installMenus())
void installMenus()

api.contextMenus.onClicked.addListener((info, tab) => {
  download(info, tab)
})

api.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type === "utd.send-page") {
    const url = message.url
    if (typeof url !== "string" || !/^https?:\/\//i.test(url)) {
      sendResponse({ ok: false, message: "Trang này không phải HTTP/HTTPS." })
      return false
    }
    sendToUtd({
      version: 1,
      type: "download",
      url,
      pageUrl: url,
      suggestedName: null,
    }).then(sendResponse)
    return true
  }
  return false
})
