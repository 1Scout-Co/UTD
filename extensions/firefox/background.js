const HOST = "app.utd.desktop"
const MENU_LINK = "utd-download"
const MENU_MEDIA = "utd-download-media"
const api = globalThis.browser ?? globalThis.chrome

// Chữ theo ngôn ngữ của trình duyệt (_locales/<mã>/messages.json, mặc định en).
const t = (key) => api.i18n.getMessage(key) || key

// Native host chưa gửi mã lỗi, chỉ gửi câu tiếng Việt cố định (src-tauri/src/native_host.rs, ipc.rs,
// media_pages.rs): đổi các câu đó sang chữ của tiện ích; câu khác hiện nguyên văn.
const HOST_TEXT = {
  "UTD chưa chạy. Mở UTD rồi thử lại.": "notRunning",
  "UTD chưa chạy hoặc chưa sẵn sàng nhận link. Mở UTD rồi thử lại.": "notRunning",
  "UTD không phản hồi.": "noResponse",
  "Không gửi được yêu cầu sang UTD.": "sendFailed",
  "Chỉ hỗ trợ URL HTTP/HTTPS.": "notHttp",
  "Đã mở UTD để chọn chất lượng": "openedForQuality",
}
const fromHost = (text) => (HOST_TEXT[text] ? t(HOST_TEXT[text]) : text)

async function reportFailure(message) {
  await api.action.setBadgeText({ text: "!" })
  if (api.action.setBadgeBackgroundColor) {
    await api.action.setBadgeBackgroundColor({ color: "#d93025" })
  }
  await api.storage.local.set({ lastResult: message, lastAt: Date.now() })
  return { ok: false, message }
}

async function sendToUtd(payload) {
  let response
  try {
    response = await api.runtime.sendNativeMessage(HOST, payload)
  } catch {
    // Chưa đăng ký native host hoặc trình duyệt chặn: lỗi của trình duyệt khó hiểu, báo câu chung.
    return reportFailure(t("notConnected"))
  }
  if (!response || response.ok !== true) {
    return reportFailure(response?.message ? fromHost(response.message) : t("noResponse"))
  }
  await api.action.setBadgeText({ text: "" })
  // UTD có thể gửi kèm câu báo, vd. link video thì "Đã mở UTD để chọn chất lượng".
  const note = typeof response.message === "string" ? fromHost(response.message) : null
  await api.storage.local.set({ lastResult: "ok", lastNote: note, lastAt: Date.now() })
  return { ok: true, taskId: response.taskId, message: note }
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
      title: t("menuDownload"),
      contexts: ["link", "video", "audio", "image", "page"],
    },
    {
      id: MENU_MEDIA,
      title: t("menuMedia"),
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
      sendResponse({ ok: false, message: t("notHttp") })
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
