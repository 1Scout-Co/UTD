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
const douyinRunning = new Set()

/** Gọi `window[Symbol.for("utd.douyin")][method](...args)` trong trang (nạp douyin-page.js nếu chưa có). */
async function douyinCall(tab, method, args) {
  const target = { tabId: tab.id, frameIds: [0] }
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
 * bấm của người dùng (menu chuột phải, nút trong popup). Một hồ sơ chỉ chạy một lượt cùng lúc.
 */
async function douyinChannel(url, tab) {
  const secUid = DOUYIN_USER.exec(url ?? "")?.[1]
  if (!secUid || !tab?.id || !DOUYIN_HOSTS.test(hostOf(tab.url ?? ""))) return false
  if (douyinRunning.has(secUid)) return true
  douyinRunning.add(secUid)
  const channelUrl = `https://www.douyin.com/user/${secUid}`
  const push = (part) => sendNative({ version: 2, type: "channel-entries", url: channelUrl, entries: [], done: false, ...part })
  try {
    // Báo UTD có lượt mới trước, rồi mới mở hộp: hộp mở ra là đã có chỗ nhận danh sách.
    const started = await push({})
    if (!started?.ok) {
      await fail(started?.message ? fromHost(started.message) : t("notConnected"))
      return true
    }
    await sendToUtd({ version: 1, type: "download", url: channelUrl, pageUrl: tab.url, suggestedName: null })
    let cursor = "0"
    let total = 0
    for (;;) {
      const page = await douyinCall(tab, "posts", [secUid, cursor]).catch(() => null)
      if (!page || page.error) {
        await push({ done: true, error: page?.error === "login" ? "login" : "blocked" })
        return true
      }
      const entries = douyinEntries(page.entries)
      total += entries.length
      // Hết khi: không còn trang, con trỏ không đổi, hoặc trang rỗng (Douyin có khi vẫn báo còn).
      const next = typeof page.cursor === "string" ? page.cursor : ""
      const end = !page.hasMore || !next || next === cursor || entries.length === 0 || total >= DOUYIN_MAX_ENTRIES
      const author = typeof page.author?.name === "string" ? { name: page.author.name.slice(0, 120) } : null
      await push({ entries, author, done: end })
      if (end) return true
      cursor = next
      await new Promise((resolve) => setTimeout(resolve, DOUYIN_PAGE_DELAY))
    }
  } finally {
    douyinRunning.delete(secUid)
  }
}
