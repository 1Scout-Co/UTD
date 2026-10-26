// --- Tự bắt link tải kiểu IDM (AC-15; docs/specs/2026-10-23-utd-v1/notes/auth-plan.md §4.4) ---
// Dùng chung `api`, `sendNative`, `authFor`, `withAuth` của background.js (Chromium nạp file này bằng
// importScripts, Firefox nạp sau background.js theo manifest). Tên ở đây đều có tiền tố takeover/TAKEOVER để không
// trùng tên của background.js.
//
// Trình duyệt bắt đầu tải ⇒ tạm dừng ⇒ hỏi UTD. UTD nhận (dò được file) ⇒ huỷ và xoá bản của trình duyệt. Còn lại
// — UTD chưa chạy, lỗi, quá giờ, UTD không dò được (POST, link dùng một lần, trang web) — trình duyệt tải tiếp như
// thường: không download nào bị mất.
//
// Cookie: chỉ đọc ở đây, trong lúc xử lý một download trình duyệt vừa bắt đầu (người dùng bấm tải trên trang), theo
// đúng luật của cú chuột phải (`authFor`): cookie của link đó, và của trang chứa nó khi cùng trang gốc; trang không
// dùng đăng nhập thì không đọc gì. Tin nhắn từ UTD không bao giờ dẫn tới việc đọc cookie.

const TAKEOVER_DEFAULTS = { takeover: true, takeoverMinMb: 1, takeoverSkip: "" }
// UTD dò file tối đa 12 s (src-tauri/src/takeover.rs); quá 20 s thì trình duyệt tải tiếp.
const TAKEOVER_WAIT_MS = 20_000
// Giữ Alt khi bấm (takeover-keys.js) ⇒ download bắt đầu trong chừng này để trình duyệt tải.
const TAKEOVER_ALT_MS = 5_000
const TAKEOVER_KEYS = "utd-takeover-keys"
let takeoverSkipUntil = 0

async function takeoverSettings() {
  const stored = await api.storage.local.get(Object.keys(TAKEOVER_DEFAULTS)).catch(() => ({}))
  return { ...TAKEOVER_DEFAULTS, ...stored }
}

/** Tên trình duyệt đặt cho file (Firefox: cả đường dẫn), `""` khi chưa có. */
const takeoverBrowserName = (item) => String(item.filename ?? "").split(/[\\/]/).pop()

/** Đuôi file: tên trình duyệt đặt, không thì phần cuối đường dẫn của link. */
function takeoverExtension(item, url) {
  let name = takeoverBrowserName(item)
  if (!name) {
    try {
      name = decodeURIComponent(new URL(url).pathname.split("/").pop() || "")
    } catch {
      name = ""
    }
  }
  const dot = name.lastIndexOf(".")
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : ""
}

/** "zip, .iso *.torrent" ⇒ {zip, iso, torrent}. */
const takeoverSkipList = (text) =>
  new Set(
    String(text ?? "")
      .toLowerCase()
      .split(/[\s,;]+/)
      .map((entry) => entry.replace(/^\*?\./, ""))
      .filter(Boolean),
  )

/** Download này UTD có nên thử nhận không (chưa tạm dừng, chưa đọc gì). */
function takeoverWanted(item, settings) {
  if (!settings.takeover) return false
  const url = item.finalUrl || item.url
  // blob:, data:, file:, ftp: … UTD không phát lại được.
  if (!/^https?:\/\//i.test(url ?? "")) return false
  if (item.state && item.state !== "in_progress") return false
  // Tiện ích (kể cả tiện ích khác) tạo bằng downloads.download: không phải cú bấm của người dùng trên trang.
  if (item.byExtensionId) return false
  const size = item.totalBytes > 0 ? item.totalBytes : -1
  const min = Number(settings.takeoverMinMb) * 1024 * 1024
  if (size >= 0 && Number.isFinite(min) && size < min) return false
  return !takeoverSkipList(settings.takeoverSkip).has(takeoverExtension(item, url))
}

const takeoverTimeout = (promise, ms) =>
  Promise.race([promise, new Promise((resolve) => setTimeout(() => resolve(null), ms))])

/** Hỏi UTD có nhận download này không: `true` chỉ khi UTD trả lời đã nhận. */
async function takeoverOffer(item) {
  const url = item.finalUrl || item.url
  const pageUrl = /^https?:\/\//i.test(item.referrer ?? "") ? item.referrer : null
  // Kho cookie của download: container/cửa sổ riêng tư (Firefox), ẩn danh (Chrome "1"), còn lại kho mặc định.
  const store = { cookieStoreId: item.cookieStoreId ?? (item.incognito ? "1" : undefined) }
  const auth = await authFor(url, pageUrl, store).catch(() => null)
  const payload = { version: 2, type: "takeover", url, pageUrl, suggestedName: takeoverBrowserName(item) || null }
  const reply = await takeoverTimeout(sendNative(withAuth(payload, auth)), TAKEOVER_WAIT_MS)
  return reply?.ok === true
}

async function takeOver(item) {
  const settings = await takeoverSettings()
  if (Date.now() < takeoverSkipUntil) {
    takeoverSkipUntil = 0
    return
  }
  if (!takeoverWanted(item, settings)) return
  try {
    await api.downloads.pause(item.id)
  } catch {
    // Đã xong hoặc đã hỏng: để nguyên cho trình duyệt.
    return
  }
  const taken = await takeoverOffer(item).catch(() => false)
  if (taken) {
    await api.downloads.cancel(item.id).catch(() => undefined)
    await api.downloads.erase({ id: item.id }).catch(() => undefined)
  } else {
    await api.downloads.resume(item.id).catch(() => undefined)
  }
}

if (api.downloads?.onCreated) api.downloads.onCreated.addListener((item) => void takeOver(item))

// Bấm kèm Alt trên trang (takeover-keys.js, đăng ký lúc chạy để không đụng content script trong manifest).
api.runtime.onMessage.addListener((message, sender) => {
  if (message?.type === "utd.takeover-skip" && sender?.id === api.runtime.id && sender.tab) {
    takeoverSkipUntil = Date.now() + TAKEOVER_ALT_MS
  }
  return false
})

async function registerTakeoverKeys() {
  if (!api.scripting?.registerContentScripts) return
  const known = await api.scripting.getRegisteredContentScripts({ ids: [TAKEOVER_KEYS] }).catch(() => [])
  if (known.length > 0) return
  await api.scripting
    .registerContentScripts([
      {
        id: TAKEOVER_KEYS,
        js: ["takeover-keys.js"],
        matches: ["http://*/*", "https://*/*"],
        allFrames: true,
        runAt: "document_start",
      },
    ])
    .catch(() => undefined)
}
void registerTakeoverKeys()
