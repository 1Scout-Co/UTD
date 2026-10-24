const HOST = "app.utd.desktop"
const MENU_LINK = "utd-download"
const MENU_MEDIA = "utd-download-media"
const MENU_ALL = "utd-download-all"
const api = globalThis.browser ?? globalThis.chrome

// Lô "Tải tất cả link": UTD nhận tối đa 2000 link (src-tauri/src/batch.rs), tin nhắn native ≤ 1 MiB.
const MAX_BATCH_LINKS = 2000
const MAX_BATCH_CHARS = 900_000
const MAX_LINK_CHARS = 4096

// Đuôi tệp tải được: link <a> khác đuôi (trang web) bị lọc bỏ.
const FILE_EXTENSIONS = new Set([
  "zip", "rar", "7z", "tar", "gz", "xz", "bz2", "iso", "img", "exe", "msi", "dmg", "pkg", "deb", "rpm",
  "apk", "appimage", "pdf", "epub", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "csv", "json",
  "mp4", "mkv", "webm", "mov", "avi", "m4v", "flv", "mp3", "m4a", "flac", "wav", "ogg", "opus", "aac",
  "jpg", "jpeg", "png", "gif", "webp", "avif", "bmp", "svg", "tif", "tiff", "heic", "psd",
  "gguf", "safetensors", "onnx", "bin", "torrent",
])

// Trang xem video (gần đúng luật của UTD, src-tauri/src/media_pages.rs; UTD kiểm lại). So với "host/đường dẫn?query".
const VIDEO_PAGES = [
  /^(www\.|m\.|music\.)?youtube\.com\/(watch\?(.*&)?v=[\w-]+|shorts\/[\w-]+|live\/[\w-]+)/,
  /^youtu\.be\/[\w-]+/,
  /^(www\.)?tiktok\.com\/@[^/]+\/video\/\d+/,
  /^(vm|vt)\.tiktok\.com\/\w+/,
  /^(www\.|m\.)?facebook\.com\/(.+\/videos\/\d+|watch\/?\?v=\d+|reel\/\d+)/,
  /^fb\.watch\/\w+/,
  /^(www\.)?instagram\.com\/(p|reel|reels|tv)\/[\w-]+/,
  /^(mobile\.)?(x|twitter)\.com\/[^/]+\/status\/\d+/,
  /^(www\.)?vimeo\.com\/\d+/,
  /^(www\.)?dailymotion\.com\/video\/\w+/,
  /^dai\.ly\/\w+/,
  /^(www\.)?twitch\.tv\/videos\/\d+/,
  /^clips\.twitch\.tv\/[\w-]+/,
  /^(www\.)?bilibili\.com\/video\/\w+/,
]

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
    return fail(
      error?.message ||
        "Không kết nối được UTD. Kiểm tra UTD đang chạy và đã bật tích hợp trình duyệt.",
    )
  }
}

async function fail(message) {
  await api.action.setBadgeText({ text: "!" })
  if (api.action.setBadgeBackgroundColor) {
    await api.action.setBadgeBackgroundColor({ color: "#d93025" })
  }
  await api.storage.local.set({ lastResult: message, lastAt: Date.now() })
  return { ok: false, message }
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
    {
      id: MENU_ALL,
      title: "Tải tất cả link bằng UTD",
      contexts: ["page", "link", "image", "selection"],
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

// Chạy trong trang (scripting.executeScript): phải tự đủ, không dùng biến ngoài.
// Có vùng chọn thì chỉ lấy link trong vùng chọn (như "Tải link đã chọn" của IDM).
function collectPageLinks(onlySelection) {
  const selection = onlySelection ? window.getSelection() : null
  const ranges = []
  if (selection && !selection.isCollapsed) {
    for (let index = 0; index < selection.rangeCount; index += 1) {
      ranges.push(selection.getRangeAt(index))
    }
  }
  const inside = (node) => ranges.length === 0 || ranges.some((range) => range.intersectsNode(node))
  const anchors = []
  const media = []
  for (const link of document.querySelectorAll("a[href]")) {
    if (inside(link)) anchors.push(link.href)
  }
  for (const element of document.querySelectorAll("img, video, audio, source")) {
    if (inside(element)) media.push(element.currentSrc || element.src || "")
  }
  return { anchors: anchors.slice(0, 20000), media: media.slice(0, 20000) }
}

function cleanLink(value) {
  if (typeof value !== "string" || value.length > MAX_LINK_CHARS) return null
  try {
    const url = new URL(value)
    if (url.protocol !== "http:" && url.protocol !== "https:") return null
    url.hash = ""
    return url
  } catch {
    return null
  }
}

function extensionOf(url) {
  const name = decodeURIComponent(url.pathname.split("/").pop() || "")
  const dot = name.lastIndexOf(".")
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : ""
}

function isVideoPage(url) {
  const short = `${url.hostname}${url.pathname}${url.search}`.toLowerCase()
  return VIDEO_PAGES.some((pattern) => pattern.test(short))
}

/** Lọc theo đuôi: link <a> giữ khi là tệp hoặc trang video; ảnh/video/âm thanh trên trang giữ hết. Bỏ trùng. */
function pickLinks({ anchors, media }) {
  const seen = new Set()
  const picked = []
  let size = 0
  const add = (url) => {
    const href = url.href
    if (seen.has(href) || picked.length >= MAX_BATCH_LINKS || size + href.length > MAX_BATCH_CHARS) return
    seen.add(href)
    picked.push(href)
    size += href.length + 3
  }
  for (const raw of anchors) {
    const url = cleanLink(raw)
    if (url && (FILE_EXTENSIONS.has(extensionOf(url)) || isVideoPage(url))) add(url)
  }
  for (const raw of media) {
    const url = cleanLink(raw)
    if (url) add(url)
  }
  return picked
}

async function downloadAll(info, tab) {
  if (!tab?.id || !/^https?:\/\//i.test(tab.url ?? "")) {
    await fail("Trang này không phải HTTP/HTTPS.")
    return
  }
  let found
  try {
    const [result] = await api.scripting.executeScript({
      target: { tabId: tab.id, frameIds: [info.frameId ?? 0] },
      func: collectPageLinks,
      args: [Boolean(info.selectionText)],
    })
    found = result?.result
  } catch {
    await fail("Không đọc được link trên trang này.")
    return
  }
  const urls = pickLinks(found ?? { anchors: [], media: [] })
  if (urls.length === 0) {
    await fail("Không thấy link tải được trên trang.")
    return
  }
  await sendToUtd({ version: 1, type: "batch", urls, pageUrl: tab.url })
}

api.runtime.onInstalled.addListener(() => void installMenus())
api.runtime.onStartup.addListener(() => void installMenus())
void installMenus()

api.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId === MENU_ALL) void downloadAll(info, tab)
  else download(info, tab)
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
