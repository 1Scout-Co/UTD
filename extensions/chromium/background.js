const HOST = "app.utd.desktop"
const MENU_LINK = "utd-download"
const MENU_MEDIA = "utd-download-media"
const MENU_ALL = "utd-download-all"
const MENU_SOCIAL = "utd-social-bulk"
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
  "social.use_bulk": "socialUseBulk",
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

// Trang gốc của từng host (UTD tính theo danh sách hậu tố, tiện ích không có): nhớ 5 phút.
const SITE_TTL = 5 * 60_000
const sites = new Map()

/**
 * Trang gốc của `url` nếu được dùng đăng nhập (Q11, notes/auth-plan.md GĐ3): công tắc chung trong UTD bật, không
 * phải trang nhạy cảm (những trang đó mỗi lượt phải bấm trong popup), người dùng không tắt riêng trang này trong
 * popup. Chỉ hỏi UTD, không đọc cookie.
 */
async function signInSite(url) {
  const host = hostOf(url)
  if (!host || !api.cookies) return null
  let info = sites.get(host)
  if (!info || Date.now() - info.at > SITE_TTL) {
    const reply = await sendNative({ version: 2, type: "auth-pending", url })
    if (!reply?.ok || !reply.auth?.site) return null
    info = { at: Date.now(), site: reply.auth.site, sensitive: reply.auth.sensitive, enabled: reply.auth.enabled }
    sites.set(host, info)
  }
  if (!info.enabled || info.sensitive) return null
  const key = `signin-off:${info.site}`
  if ((await api.storage.local.get(key))[key]) return null
  return info.site
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
 * Phần `auth` của tin nhắn tải trong cú bấm của người dùng, khi trang của link được dùng đăng nhập (`signInSite`).
 * Không được ⇒ `null`, không đọc cookie nào.
 */
async function authFor(targetUrl, pageUrl, tab) {
  const site = await signInSite(targetUrl)
  if (!site) return null
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
    // Tải hàng loạt Facebook / Instagram / Threads (social-bulk.js): chỉ hiện trên các trang đó.
    {
      id: MENU_SOCIAL,
      title: t("menuSocial"),
      contexts: ["page", "link", "image", "video"],
      documentUrlPatterns: ["*://*.facebook.com/*", "*://*.instagram.com/*", "*://*.threads.net/*", "*://*.threads.com/*"],
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
  // Chuột phải lên chính trang cá nhân / fanpage / nhóm (social-bulk.js): nhắc dùng Tải hàng loạt ngay trên trang.
  if (url === tab?.url && typeof socialPlan === "function" && socialPlan(url)) {
    await socialHint(tab)
    return
  }
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

/** Mở bảng chọn định dạng ngay trên trang (content.js). `false`: khung đó không có content script ⇒ làm như cũ. */
async function openFormats(info, tab, url) {
  if (!tab?.id) return false
  try {
    const reply = await api.tabs.sendMessage(tab.id, { type: "utd.open-formats", url }, { frameId: info.frameId ?? 0 })
    return reply?.ok === true
  } catch {
    return false
  }
}

/** Chuột phải lên video, hoặc link/trang xem video quen ⇒ bảng định dạng trên trang; còn lại gửi link cho UTD. */
async function downloadOrPick(info, tab) {
  const media = info.menuItemId === MENU_MEDIA || info.mediaType === "video" || info.mediaType === "audio"
  const url = media ? (info.pageUrl ?? tab?.url) : pickUrl(info, tab)
  const video = media || (typeof url === "string" && isVideoPage(cleanLink(url) ?? new URL("about:blank")))
  if (url && video && (await openFormats(info, tab, url))) return
  await download(info, tab)
}

api.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId === MENU_ALL) void downloadAll(info, tab)
  else if (info.menuItemId === MENU_SOCIAL) void socialStart(tab)
  else void downloadOrPick(info, tab)
})

/** Popup bấm "Dùng đăng nhập" / "Tiếp tục" (cú bấm của người dùng, cả trang nhạy cảm): đọc cookie cho tab và việc chờ. */
async function renew({ tabId, site, urls }) {
  const tab = await api.tabs.get(tabId)
  if (!tab?.url || !api.cookies || !within(hostOf(tab.url), site)) return { ok: false, message: "auth.denied" }
  const links = [tab.url, ...(Array.isArray(urls) ? urls : [])].filter(
    (url) => typeof url === "string" && within(hostOf(url), site),
  )
  const cookies = await readCookies(site, [...new Set(links)].slice(0, 21), tab, false)
  const auth = { site, cookies, userAgent: navigator.userAgent }
  return (await sendNative({ version: 2, type: "auth-renew", url: tab.url, auth })) ?? { ok: false }
}

// --- Nút "Tải video này" trên trang (content.js; notes/auth-plan.md GĐ3) ---
// Kết quả phân tích nhớ 10 phút theo (link, có đăng nhập): bảng mở lại tức thì, đọc sẵn không chạy hai lần.
const FORMATS_TTL = 10 * 60_000
const formatReplies = new Map()

const codeOf = (message) => /^\[([a-z][a-z0-9_]*(?:\.[a-z0-9_]+)+)\] /.exec(message ?? "")?.[1] ?? null

/** Định dạng cho bảng trên trang. `signIn`: cú bấm thật mà nguồn đòi đăng nhập ⇒ kèm cookie nếu trang cho dùng. */
function videoFormats({ url, src, signIn }, tab) {
  const key = `${signIn ? 1 : 0} ${url}`
  const known = formatReplies.get(key)
  if (known && Date.now() - known.at < FORMATS_TTL) return known.reply
  const reply = (async () => {
    const auth = signIn ? await authFor(url, tab?.url, tab).catch(() => null) : null
    const payload = { version: 2, type: "media-formats", url, src: src ?? null, pageUrl: tab?.url ?? null }
    const response = await sendNative(withAuth(payload, auth))
    if (response === null) return { ok: false, message: t("notConnected") }
    if (response.ok !== true) {
      return { ok: false, code: codeOf(response.message), message: response.message ? fromHost(response.message) : t("noResponse") }
    }
    return { ok: true, media: response.media }
  })()
  formatReplies.set(key, { at: Date.now(), reply })
  void reply.then((answer) => {
    if (!answer.ok) formatReplies.delete(key)
  })
  return reply
}

/** Dòng định dạng người dùng bấm: UTD tải luôn, không mở hộp Tải media. */
async function videoDownload({ url, title, selector, audioFormat, signIn }, tab) {
  const auth = signIn ? await authFor(url, tab?.url, tab).catch(() => null) : null
  const payload = { version: 2, type: "media-download", url, pageUrl: tab?.url ?? null, title, selector, audioFormat }
  return sendToUtd(withAuth(payload, auth))
}

// --- Luồng đọc thẳng từ trang (page-media.js; notes/fb-media.md): Tin Facebook, video trong bảng tin… ---
const STREAM_KINDS = new Set(["video", "audio", "both"])

/** Một luồng hợp lệ (link https, loại đã biết) hoặc `null`. */
function cleanStream(stream) {
  if (!stream || !webUrl(stream.url) || !/^https:/i.test(stream.url) || !STREAM_KINDS.has(stream.kind)) return null
  const codecs = typeof stream.codecs === "string" && /^[\w.-]{1,64}$/.test(stream.codecs) ? stream.codecs : null
  const sizeBytes = Number.isSafeInteger(stream.sizeBytes) && stream.sizeBytes > 0 ? stream.sizeBytes : null
  return { url: stream.url, kind: stream.kind, codecs, sizeBytes }
}

/** 1 luồng có hình, hoặc hình + tiếng; khác thì `null`. */
function cleanStreams(list) {
  if (!Array.isArray(list) || list.length < 1 || list.length > 2) return null
  const streams = list.map(cleanStream)
  if (streams.some((stream) => !stream)) return null
  const kinds = streams.map((stream) => stream.kind).sort().join("+")
  return ["both", "video", "audio+video"].includes(kinds) ? streams : null
}

/** Kết quả của page-media.js (chạy trong trang, không tin hẳn): giữ đúng hình dạng mong đợi. */
function cleanPageMedia(found) {
  const formats = (Array.isArray(found?.formats) ? found.formats : []).slice(0, 8).flatMap((format) => {
    const streams = cleanStreams(format?.streams)
    const id = typeof format?.id === "string" && /^[\w-]{1,16}$/.test(format.id) ? format.id : null
    if (!streams || !id) return []
    const sizeBytes = Number.isSafeInteger(format.sizeBytes) && format.sizeBytes > 0 ? format.sizeBytes : null
    return [{ id, sizeBytes, streams }]
  })
  const links = (list) => (Array.isArray(list) ? list.filter(webUrl).slice(0, 4) : [])
  const title = typeof found?.title === "string" ? found.title.slice(0, 200) : null
  return { title, formats, manifests: links(found?.manifests), files: links(found?.files) }
}

/**
 * Luồng của video ở điểm (x, y) đọc thẳng trong trang. Thế giới MAIN để thấy dữ liệu trình phát của trang; trình
 * duyệt không cho (Firefox < 128) thì chạy như content script, hàm tự đọc qua `wrappedJSObject`.
 */
async function pageMedia({ x, y }, tab) {
  const target = { tabId: tab.id, frameIds: [0] }
  const args = [Number.isFinite(x) ? x : null, Number.isFinite(y) ? y : null]
  for (const world of ["MAIN", null]) {
    try {
      const options = world ? { target, world, func: utdFindPageMedia, args } : { target, func: utdFindPageMedia, args }
      const [result] = await api.scripting.executeScript(options)
      return { ok: true, media: cleanPageMedia(result?.result) }
    } catch {
      // Thử cách sau.
    }
  }
  return { ok: false }
}

/** Dòng của bảng lấy từ trang: UTD tải các luồng (hình + tiếng thì ghép), không cookie — link đã có chữ ký. */
async function videoStreams({ title, streams }, tab) {
  const payload = { version: 2, type: "media-streams", url: tab.url, pageUrl: tab.url, title, streams }
  return sendToUtd(payload)
}

/** Dòng "Tải file video": tải thẳng nguồn của thẻ `<video>` như chuột phải › Tải bằng UTD. */
async function videoFile({ url }, tab) {
  const auth = await authFor(url, tab?.url, tab).catch(() => null)
  return sendToUtd(withAuth({ version: 1, type: "download", url, pageUrl: tab?.url ?? null, suggestedName: null }, auth))
}

const webUrl = (value) => typeof value === "string" && value.length <= MAX_LINK_CHARS && /^https?:\/\//i.test(value)

// Content script của chính tiện ích ở khung chính một trang web (trang không gửi tin cho tiện ích được).
const fromContent = (sender) =>
  sender?.id === api.runtime.id && Boolean(sender.tab) && (sender.frameId ?? 0) === 0 && webUrl(sender.url)

// Lệnh từ popup của chính tiện ích: không từ trang web, content script hay tiện ích khác (lệnh có thể kèm cookie,
// notes/auth-plan.md §4.3).
const fromPopup = (sender) =>
  sender?.id === api.runtime.id && !sender.tab && sender.url === api.runtime.getURL("popup.html")

// Tự bắt link tải (takeover.js). Chromium (service worker cổ điển) nạp thêm ở đây; Firefox nạp theo manifest.
if (typeof importScripts === "function") importScripts("takeover.js")
// Hàm đọc luồng trong trang (page-media.js): Chromium nạp ở đây; Firefox nạp theo manifest.
if (typeof importScripts === "function") importScripts("page-media.js")
// Tải hàng loạt Facebook / Instagram / Threads (social-bulk.js): Chromium nạp ở đây; Firefox nạp theo manifest.
if (typeof importScripts === "function") importScripts("social-bulk.js")

// Facebook: fb-media.js ghi nhớ link file video trong dữ liệu JSON của trang (thế giới MAIN, từ document_start) để
// nút "Tải video này" tìm đúng video đang xem. Đăng ký lúc chạy như takeover-keys.js; Firefox < 128 không cho thế
// giới MAIN ⇒ bỏ qua, page-media.js vẫn đọc trình phát và yêu cầu mạng.
const FB_MEDIA = "utd-fb-media"
async function registerFacebookMedia() {
  if (!api.scripting?.registerContentScripts) return
  const known = await api.scripting.getRegisteredContentScripts({ ids: [FB_MEDIA] }).catch(() => [])
  if (known.length > 0) return
  await api.scripting
    .registerContentScripts([
      {
        id: FB_MEDIA,
        js: ["fb-media.js"],
        matches: ["https://*.facebook.com/*"],
        runAt: "document_start",
        world: "MAIN",
      },
    ])
    .catch(() => undefined)
}
void registerFacebookMedia()

api.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (fromContent(sender)) {
    const tab = sender.tab
    const answer = (work) => {
      work.catch(() => ({ ok: false, message: t("noResponse") })).then(sendResponse)
      return true
    }
    if (message?.type === "utd.video-formats" && webUrl(message.url)) {
      return answer(videoFormats({ url: message.url, src: webUrl(message.src) ? message.src : null, signIn: message.signIn === true }, tab))
    }
    if (message?.type === "utd.video-download" && webUrl(message.url) && typeof message.selector === "string") {
      const title = typeof message.title === "string" ? message.title.slice(0, 300) : null
      const audioFormat = typeof message.audioFormat === "string" ? message.audioFormat : null
      const picked = { url: message.url, title, selector: message.selector, audioFormat, signIn: message.signIn === true }
      return answer(videoDownload(picked, tab))
    }
    if (message?.type === "utd.video-file" && webUrl(message.url)) {
      return answer(videoFile({ url: message.url }, tab))
    }
    if (message?.type === "utd.page-media") {
      return answer(pageMedia({ x: message.x, y: message.y }, tab))
    }
    if (message?.type === "utd.video-streams") {
      const streams = cleanStreams(message.streams)
      if (!streams) return false
      const title = typeof message.title === "string" ? message.title.slice(0, 300) : null
      return answer(videoStreams({ title, streams }, tab))
    }
    return false
  }
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
