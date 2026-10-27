// Douyin: link riêng của video đang gắn nút "Tải video này" (content.js hỏi qua `__utdVideoSites`, như tiktok.js).
// Feed (推荐/精选), trang mở video kiểu khung (`?modal_id=`) và lưới hồ sơ có địa chỉ trang không phải link video.
// Chỉ đọc DOM, chạy cùng content script (manifest, chỉ douyin.com). Học từ mediago, douyin-dl-user-js, các tiện ích
// tải Douyin (notes/douyin.md): mục feed đang phát là `[data-e2e="feed-active-video"]` mang `data-e2e-vid`; ô lưới
// chứa link `/video/<mã>` hoặc `/note/<mã>`. Không khai báo tên ở phạm vi ngoài: dùng chung phạm vi với content.js.
;(globalThis.__utdVideoSites ??= []).push({
  hosts: /(^|\.)douyin\.com$/,
  videoUrl(video) {
    const ID = /^\d{8,25}$/
    const POST = /\/(?:video|note|slides)\/(\d{8,25})(?:[/?#]|$)/
    const LINKS = 'a[href*="/video/"], a[href*="/note/"]'
    const idOf = (href) => {
      try {
        const url = new URL(href ?? "", location.href)
        return /(^|\.)douyin\.com$/.test(url.hostname) ? (POST.exec(url.pathname)?.[1] ?? null) : null
      } catch {
        return null
      }
    }
    const linkOf = (id) => `https://www.douyin.com/video/${id}`

    // 1. Ô lưới (hồ sơ, tìm kiếm): video nằm trong link của ô, hoặc cạnh link trong cùng ô.
    const tile = idOf(video.closest(LINKS)?.getAttribute("href")) ??
      idOf(video.closest('[data-e2e="user-post-list"] li, li')?.querySelector(LINKS)?.getAttribute("href"))
    if (tile) return linkOf(tile)

    // 2. Feed: mục đang phát mang mã video (mục cũ của thẻ khác còn sót trong trang nên chỉ lấy tổ tiên của video).
    const marked = video.closest('[data-e2e="feed-active-video"][data-e2e-vid], [data-e2e-vid], [data-aweme-id]')
    const vid = marked?.getAttribute("data-e2e-vid") ?? marked?.getAttribute("data-aweme-id")
    if (vid && ID.test(vid)) return linkOf(vid)

    // 3. Video mở kiểu khung (`?modal_id=<mã>`) hoặc trang xem một video (/video/<mã>, /note/<mã>).
    const modal = new URL(location.href).searchParams.get("modal_id")
    if (modal && ID.test(modal)) return linkOf(modal)
    const page = idOf(location.href)
    if (page) return linkOf(page)

    // 4. Link bài đăng trong tổ tiên gần nhất có link như vậy; nhiều bài khác nhau (cả lưới) ⇒ không đoán.
    for (let element = video.parentElement, hops = 0; element && hops < 12; element = element.parentElement, hops += 1) {
      const ids = [...element.querySelectorAll(LINKS)].map((link) => idOf(link.getAttribute("href"))).filter(Boolean)
      if (ids.length === 0) continue
      return new Set(ids).size === 1 ? linkOf(ids[0]) : null
    }
    return null
  },
})
