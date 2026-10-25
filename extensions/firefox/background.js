const HOST = "app.utd.desktop"
const MENU_LINK = "utd-download"
const MENU_MEDIA = "utd-download-media"
const MENU_ALL = "utd-download-all"
const api = globalThis.browser ?? globalThis.chrome

// Chữ theo ngôn ngữ của trình duyệt (_locales/<mã>/messages.json, mặc định en).
const t = (key) => api.i18n.getMessage(key) || key

// Native host chưa gửi mã lỗi, chỉ gửi câu tiếng Việt cố định (src-tauri/src/native_host.rs, ipc.rs,
// media_pages.rs, batch.rs): đổi các câu đó sang chữ của tiện ích; câu khác hiện nguyên văn.
const HOST_TEXT = {
  "UTD chưa chạy. Mở UTD rồi thử lại.": "notRunning",
  "UTD chưa chạy hoặc chưa sẵn sàng nhận link. Mở UTD rồi thử lại.": "notRunning",
  "UTD không phản hồi.": "noResponse",
  "Không gửi được yêu cầu sang UTD.": "sendFailed",
  "Chỉ hỗ trợ URL HTTP/HTTPS.": "notHttp",
  "Trang này không có link tải được.": "noLinks",
  "Quá nhiều link trong một lần gửi.": "tooManyLinks",
  "Đã mở UTD để chọn chất lượng": "openedForQuality",
  "Đã mở UTD để tải cả kênh": "openedForChannel",
  "Đã mở UTD để chọn link": "openedForLinks",
}
// Câu có mã (`[mã] câu`, src-tauri/src/coded.rs): mã đã biết ⇒ chữ của tiện ích; mã khác ⇒ bỏ mã, giữ câu.
const HOST_CODES = {
  "link.drive_folder": "driveFolder",
}
const fromHost = (text) => {
  if (HOST_TEXT[text]) return t(HOST_TEXT[text])
  const coded = /^\[([a-z][a-z0-9_]*(?:\.[a-z0-9_]+)+)\] ([\s\S]*)$/.exec(text)
  if (!coded) return text
  return HOST_CODES[coded[1]] ? t(HOST_CODES[coded[1]]) : coded[2]
}

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

// --- Đăng nhập của trình duyệt (docs/specs/2026-10-23-utd-v1/notes/auth-plan.md §4) ---
// Cookie chỉ được đọc ở đây, bên trong cú bấm của người dùng: menu chuột phải (trang đã cho phép) hoặc nút
// trong popup. Tin nhắn từ app không bao giờ dẫn tới việc đọc cookie. Không lưu cookie ở đâu cả.
const MAX_COOKIES = 300
const MAX_COOKIE_BYTES = 64 * 1024

const hostOf = (url) => {
  try {
    return new URL(url).hostname.toLowerCase()
  } catch {
    return ""
  }
}

const within = (host, site) => host === site || host.endsWith(`.${site}`)

// Trang file Google Drive (`/file/d/<id>/…`, `open?id=`, `uc?id=`) và máy chủ tải thật của nó.
const DRIVE_FILE_PAGE = /^https:\/\/(drive|docs)\.google\.com\/(u\/\d+\/)?(file\/d\/|open\?|uc\?)/
const DRIVE_DOWNLOAD = "https://drive.usercontent.google.com/download"

/** Trang (tên miền gốc) đã được cho phép phủ `host`: quyền `*://*.google.com/*` phủ drive.google.com. */
async function grantedSite(host) {
  if (!host || !api.cookies) return null
  const { origins = [] } = await api.permissions.getAll()
  for (const origin of origins) {
    const match = /^\*:\/\/(\*\.)?([^/*]+)\/\*$/.exec(origin)
    if (match && (match[2] === host || (match[1] && within(host, match[2])))) return match[2]
  }
  return null
}

/** Kho cookie của tab: container/cửa sổ riêng tư (Firefox), ẩn danh (Chrome). */
async function storeOf(tab) {
  if (tab?.cookieStoreId) return tab.cookieStoreId
  const stores = await api.cookies.getAllCookieStores()
  return stores.find((store) => store.tabIds.includes(tab?.id))?.id
}

/**
 * Cookie áp cho các link `urls` (không trùng, trần 300 cookie / 64 KiB). Chéo trang (`crossSite`) thì bỏ
 * cookie SameSite=Strict như trình duyệt khi điều hướng. Không nêu `partitionKey` ⇒ chỉ cookie của trang
 * chính, không phải cookie nhúng.
 */
async function readCookies(site, urls, tab, crossSite) {
  const storeId = await storeOf(tab)
  const picked = new Map()
  let bytes = 0
  for (const url of urls) {
    const query = storeId ? { url, storeId } : { url }
    let cookies = []
    try {
      cookies = await api.cookies.getAll(query)
    } catch {
      // Firefox bật cách ly bên thứ nhất (privacy.firstparty.isolate) thì phải nêu firstPartyDomain.
      cookies = await api.cookies.getAll({ ...query, firstPartyDomain: site }).catch(() => [])
    }
    for (const cookie of cookies) {
      const key = `${cookie.name}\t${cookie.domain}\t${cookie.path}`
      if ((crossSite && cookie.sameSite === "strict") || picked.has(key)) continue
      bytes += cookie.name.length + cookie.value.length
      if (picked.size >= MAX_COOKIES || bytes > MAX_COOKIE_BYTES) return [...picked.values()]
      picked.set(key, {
        name: cookie.name,
        value: cookie.value,
        domain: cookie.domain,
        hostOnly: cookie.hostOnly,
        path: cookie.path,
        secure: cookie.secure,
        expires: cookie.session ? null : Math.floor(cookie.expirationDate ?? 0) || null,
      })
    }
  }
  return [...picked.values()]
}

/**
 * Phần `auth` của tin nhắn tải khi trang của link đã được cho phép (và không phải trang nhạy cảm: những
 * trang đó mỗi lượt phải bấm trong popup). Chưa cho phép ⇒ `null`, không đọc cookie nào.
 */
async function authFor(targetUrl, pageUrl, tab) {
  const site = await grantedSite(hostOf(targetUrl))
  if (!site) return null
  const key = `sensitive:${site}`
  if ((await api.storage.local.get(key))[key]) return null
  const sameSite = within(hostOf(pageUrl ?? ""), site)
  const urls = sameSite && pageUrl && pageUrl !== targetUrl ? [targetUrl, pageUrl] : [targetUrl]
  // Trang file Google Drive: UTD tải bằng link tải thật ở máy khác (src-tauri/src/media_pages.rs).
  if (DRIVE_FILE_PAGE.test(targetUrl)) urls.push(DRIVE_DOWNLOAD)
  const cookies = await readCookies(site, urls, tab, !sameSite)
  return cookies.length > 0 ? { site, cookies, userAgent: navigator.userAgent } : null
}

/** Tin nhắn bản 1 như cũ; có cookie thì bản 2 (UTD cũ không hiểu bản 2 nên chỉ dùng khi cần). */
function withAuth(payload, auth) {
  return auth ? { ...payload, version: 2, auth } : payload
}

/** Gửi một tin sang native host, trả nguyên trả lời (hoặc `null` khi không nối được). */
async function sendNative(payload) {
  try {
    return await api.runtime.sendNativeMessage(HOST, payload)
  } catch {
    return null
  }
}

async function sendToUtd(payload) {
  const response = await sendNative(payload)
  if (response === null) {
    // Chưa đăng ký native host hoặc trình duyệt chặn: lỗi của trình duyệt khó hiểu, báo câu chung.
    return fail(t("notConnected"))
  }
  if (!response || response.ok !== true) {
    return fail(response?.message ? fromHost(response.message) : t("noResponse"))
  }
  await api.action.setBadgeText({ text: "" })
  // UTD có thể gửi kèm câu báo, vd. link video thì "Đã mở UTD để chọn chất lượng".
  const note = typeof response.message === "string" ? fromHost(response.message) : null
  await api.storage.local.set({ lastResult: "ok", lastNote: note, lastAt: Date.now() })
  return { ok: true, taskId: response.taskId, message: note }
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
      title: t("menuDownload"),
      contexts: ["link", "video", "audio", "image", "page"],
    },
    {
      id: MENU_MEDIA,
      title: t("menuMedia"),
      contexts: ["video", "audio"],
    },
    {
      id: MENU_ALL,
      title: t("menuAll"),
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

async function download(info, tab) {
  const url = pickUrl(info, tab)
  if (!url) return
  const auth = await authFor(url, tab?.url, tab).catch(() => null)
  await sendToUtd(
    withAuth({ version: 1, type: "download", url, pageUrl: tab?.url ?? null, suggestedName: null }, auth),
  )
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
    await fail(t("notHttp"))
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
    await fail(t("readLinksFailed"))
    return
  }
  const urls = pickLinks(found ?? { anchors: [], media: [] })
  if (urls.length === 0) {
    await fail(t("noLinks"))
    return
  }
  // Cookie của trang đang mở; UTD chỉ dùng cho link cùng trang.
  const auth = await authFor(tab.url, tab.url, tab).catch(() => null)
  await sendToUtd(withAuth({ version: 1, type: "batch", urls, pageUrl: tab.url }, auth))
}

api.runtime.onInstalled.addListener(() => void installMenus())
api.runtime.onStartup.addListener(() => void installMenus())
void installMenus()

api.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId === MENU_ALL) void downloadAll(info, tab)
  else void download(info, tab)
})

/** Popup bấm "Dùng đăng nhập" / "Tiếp tục": quyền đã có (popup vừa xin); đọc cookie cho tab và việc chờ. */
async function renew({ tabId, site, urls }) {
  const tab = await api.tabs.get(tabId)
  if (!tab?.url || !(await grantedSite(hostOf(tab.url)))) return { ok: false, message: "auth.denied" }
  const links = [tab.url, ...(Array.isArray(urls) ? urls : [])].filter(
    (url) => typeof url === "string" && within(hostOf(url), site),
  )
  const cookies = await readCookies(site, [...new Set(links)].slice(0, 21), tab, false)
  const auth = { site, cookies, userAgent: navigator.userAgent }
  return (await sendNative({ version: 2, type: "auth-renew", url: tab.url, auth })) ?? { ok: false }
}

// Chỉ nhận lệnh từ popup của chính tiện ích: không từ trang web, content script hay tiện ích khác (sau này
// lệnh có thể kèm cookie, notes/auth-plan.md §4.3).
const fromPopup = (sender) =>
  sender?.id === api.runtime.id && !sender.tab && sender.url === api.runtime.getURL("popup.html")

api.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (!fromPopup(sender)) return false
  if (message?.type === "utd.send-page") {
    const url = message.url
    if (typeof url !== "string" || !/^https?:\/\//i.test(url)) {
      sendResponse({ ok: false, message: t("notHttp") })
      return false
    }
    api.tabs
      .get(message.tabId)
      .catch(() => null)
      .then((tab) => authFor(url, url, tab).catch(() => null))
      .then((auth) =>
        sendToUtd(withAuth({ version: 1, type: "download", url, pageUrl: url, suggestedName: null }, auth)),
      )
      .then(sendResponse)
    return true
  }
  // Popup vừa mở: tab có việc chờ đăng nhập không (không đọc cookie).
  if (message?.type === "utd.auth-pending" && typeof message.url === "string") {
    sendNative({ version: 2, type: "auth-pending", url: message.url }).then(sendResponse)
    return true
  }
  if (message?.type === "utd.auth-renew" && typeof message.site === "string") {
    renew(message)
      .catch(() => ({ ok: false }))
      .then(sendResponse)
    return true
  }
  return false
})
