// Douyin trong nền (notes/douyin.md): chạy douyin-page.js trong trang (thế giới MAIN, API được trang tự ký), và tải
// cả hồ sơ: chuột phải trên trang `douyin.com/user/<sec_uid>` › Tải bằng UTD ⇒ mở hộp Tải cả kênh của UTD rồi lật
// từng trang danh sách ngay trong tab, đẩy dần sang UTD (tin `channel-entries`). yt-dlp không liệt kê được hồ sơ
// Douyin và từ 2026-09 không đọc được cả video Douyin nên UTD tải thẳng link MP4 trong danh sách. Không đọc cookie.
// Chromium nạp bằng importScripts trong background.js; Firefox nạp theo manifest. Tên dùng chung phạm vi với
// background.js nên đều có tiền tố douyin/DOUYIN.

const DOUYIN_USER = /^https:\/\/(?:www\.)?douyin\.com\/user\/([\w-]{8,200})(?:[/?#]|$)/
const DOUYIN_HOSTS = /(^|\.)douyin\.com$/
// Nghỉ giữa hai trang danh sách như trình duyệt người thật (dYm: 5 s; jiji262: 2–5 s).
const DOUYIN_PAGE_DELAY = 2500
const DOUYIN_MAX_ENTRIES = 5000
// Lượt tải cả hồ sơ đang chạy theo sec_uid: bấm lại thì lượt mới thay lượt cũ, vòng cũ thấy mình không còn là lượt hiện tại.
const douyinRuns = new Map()

/**
 * Tài liệu đang mở ở khung chính của tab, nếu là trang Douyin (https); không phải ⇒ ném lỗi, không chạy gì trong đó.
 * Đọc ở thế giới ISOLATED (không đặt `world`): trang không sửa được hàm gốc ở đó. Ở thế giới MAIN thì trang tự định
 * nghĩa `window[Symbol.for("utd.douyin")]` trả gì cũng được (douyin-page.js giữ nguyên khi đã có), nên host phải
 * kiểm trước MỖI lời gọi, không chỉ lúc bấm: người dùng có thể mở website khác trong cùng tab. Trả `documentId`
 * (Chromium ≥ 106; Firefox có thể không có ⇒ `null`) để ghim lời gọi sau vào đúng tài liệu này.
 */
async function douyinDocument(tab) {
  const [probe] = await api.scripting.executeScript({
    target: { tabId: tab.id, frameIds: [0] },
    func: () => (location.protocol === "https:" ? location.hostname : ""),
  })
  if (!DOUYIN_HOSTS.test(String(probe?.result ?? ""))) throw new Error("the tab is not on a Douyin page")
  return typeof probe.documentId === "string" ? probe.documentId : null
}

/**
 * Gọi `window[Symbol.for("utd.douyin")][method](...args)` trong trang (nạp douyin-page.js nếu chưa có). Chỉ chạy khi
 * tab đang ở trang Douyin và nhắm đúng tài liệu vừa kiểm (`documentIds`; không có thì khung chính như cũ, còn một
 * khe hở ngắn giữa lúc kiểm và lúc chạy). `run` = các lời gọi cùng một lượt: tài liệu đổi giữa chừng (điều hướng)
 * ⇒ ném lỗi, lượt đó dừng.
 */
async function douyinCall(tab, method, args, run = {}) {
  const documentId = await douyinDocument(tab)
  if ("documentId" in run && run.documentId !== documentId) throw new Error("the tab moved to another page")
  run.documentId = documentId
  const target = documentId ? { tabId: tab.id, documentIds: [documentId] } : { tabId: tab.id, frameIds: [0] }
  await api.scripting.executeScript({ target, world: "MAIN", files: ["douyin-page.js"] })
  const [result] = await api.scripting.executeScript({
    target,
    world: "MAIN",
    func: (name, values) => window[Symbol.for("utd.douyin")]?.[name](...values) ?? null,
    args: [method, args],
  })
  return result?.result ?? null
}

/** Mã video trong danh sách: số; link `/video|note/<mã>` của douyin.com; tối đa 4 bản MP4 hợp lệ. */
function douyinEntries(list) {
  return (Array.isArray(list) ? list : []).slice(0, 100).flatMap((entry) => {
    const id = typeof entry?.id === "string" && /^\d{8,25}$/.test(entry.id) ? entry.id : null
    if (!id || !/^https:\/\/www\.douyin\.com\/(video|note)\/\d+$/.test(entry.url ?? "")) return []
    const variants = (Array.isArray(entry.variants) ? entry.variants : []).slice(0, 4).flatMap((variant) => {
      if (!webUrl(variant?.url) || !/^https:/i.test(variant.url)) return []
      const height = Number.isSafeInteger(variant.height) && variant.height >= 0 ? variant.height : 0
      const sizeBytes = Number.isSafeInteger(variant.sizeBytes) && variant.sizeBytes > 0 ? variant.sizeBytes : null
      return [{ url: variant.url, height, sizeBytes, h265: variant.h265 === true }]
    })
    const number = (value) => (Number.isSafeInteger(value) && value >= 0 ? value : null)
    return [
      {
        id,
        url: entry.url,
        title: typeof entry.title === "string" ? entry.title.slice(0, 300) : id,
        timestamp: number(entry.timestamp),
        durationSeconds: number(entry.durationSeconds),
        thumbnail: webUrl(entry.thumbnail) ? entry.thumbnail : null,
        variants,
      },
    ]
  })
}

/**
 * Trang hồ sơ Douyin ⇒ mở hộp Tải cả kênh và đẩy danh sách dần sang UTD; trả `true` khi đã nhận việc. Gọi trong cú
 * bấm của người dùng (menu chuột phải, nút trong popup). Một hồ sơ chỉ có một lượt chạy: bấm lại thì lượt cũ dừng
 * (không đẩy thêm gì) và lượt mới chạy như lần đầu, nên hộp luôn mở lại. Lượt cũng dừng khi UTD không nhận nữa
 * (người dùng bấm Dừng hoặc đóng hộp, hoặc UTD đã tắt), khi tab sang trang khác (`douyinCall`), hoặc khi hết danh sách.
 */
async function douyinChannel(url, tab) {
  const secUid = DOUYIN_USER.exec(url ?? "")?.[1]
  if (!secUid || !tab?.id || !DOUYIN_HOSTS.test(hostOf(tab.url ?? ""))) return false
  const run = {}
  douyinRuns.set(secUid, run)
  const current = () => douyinRuns.get(secUid) === run
  const channelUrl = `https://www.douyin.com/user/${secUid}`
  const push = (part) => sendNative({ version: 2, type: "channel-entries", url: channelUrl, entries: [], done: false, ...part })
  try {
    // Báo UTD có lượt mới trước, rồi mới mở hộp: hộp mở ra là đã có chỗ nhận danh sách.
    const started = await push({})
    if (!current()) return true
    if (!started?.ok) {
      await fail(started?.message ? fromHost(started.message) : t("notConnected"))
      return true
    }
    await sendToUtd({ version: 1, type: "download", url: channelUrl, pageUrl: tab.url, suggestedName: null })
    let cursor = "0"
    let total = 0
    for (;;) {
      const page = await douyinCall(tab, "posts", [secUid, cursor], run).catch((error) => {
        console.warn("[UTD] Douyin posts", error?.message ?? error)
        return null
      })
      if (!current()) return true
      if (!page || page.error) {
        console.warn("[UTD] Douyin posts", page ? page.error : "no result from the page")
        await push({ done: true, error: page?.error === "login" ? "login" : "blocked" })
        return true
      }
      const entries = douyinEntries(page.entries)
      total += entries.length
      // Hết khi: không còn trang, con trỏ không đổi, hoặc trang rỗng (Douyin có khi vẫn báo còn).
      const next = typeof page.cursor === "string" ? page.cursor : ""
      const end = !page.hasMore || !next || next === cursor || entries.length === 0 || total >= DOUYIN_MAX_ENTRIES
      const author = typeof page.author?.name === "string" ? { name: page.author.name.slice(0, 120) } : null
      // Trang trước báo còn mà trang này rỗng: chưa đăng nhập Douyin chỉ cho xem trang đầu (~18 bài, ô "登录").
      const wall = cursor !== "0" && entries.length === 0
      const pushed = await push({ entries, author, done: end, ...(wall ? { error: "login" } : {}) })
      // UTD không nhận nữa (đã dừng/đóng hộp, hoặc đã tắt) ⇒ thôi lật.
      if (end || !pushed?.ok || !current()) return true
      cursor = next
      await new Promise((resolve) => setTimeout(resolve, DOUYIN_PAGE_DELAY))
      if (!current()) return true
    }
  } finally {
    if (current()) douyinRuns.delete(secUid)
  }
}
