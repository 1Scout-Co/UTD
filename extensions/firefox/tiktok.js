// TikTok: link riêng của video đang gắn nút "Tải video này" (content.js hỏi qua `__utdVideoSites`). Feed (For You,
// Đang theo dõi, Khám phá…) và lưới video trên trang hồ sơ có địa chỉ trang không phải link video nên yt-dlp không
// đọc được (feed) hoặc nhận nhầm link kênh (lưới). Chỉ đọc DOM của trang, chạy cùng content script (manifest, chỉ
// tiktok.com). Không khai báo tên ở phạm vi ngoài: dùng chung phạm vi với content.js.
// Học từ mediago, sixem/tiktok-download-buttons, vot.js… (notes/tiktok-button.md): feed không có link tới trang của
// video, mã nằm ở khung trình phát `xgwrapper-<số>-<mã>`; ô lưới bọc/chứa link `/@kênh/video/<mã>`.
;(globalThis.__utdVideoSites ??= []).push({
  hosts: /(^|\.)tiktok\.com$/,
  videoUrl(video) {
    // /@<kênh>/video/<mã> hoặc /@<kênh>/photo/<mã> (bài ảnh); kênh có thể trống.
    const POST = /^\/@([\w.-]*)\/(?:video|photo)\/(\d{8,25})(?:[/?#]|$)/
    const LINKS = 'a[href*="/video/"], a[href*="/photo/"]'
    const postOf = (href) => {
      try {
        const url = new URL(href ?? "", location.href)
        const match = /(^|\.)tiktok\.com$/.test(url.hostname) ? POST.exec(url.pathname) : null
        return match ? { user: match[1], id: match[2] } : null
      } catch {
        return null
      }
    }
    // yt-dlp chỉ nhận www.tiktok.com/@<kênh>/video/<mã> (kênh trống ⇒ `_` như yt-dlp tự dựng); bài ảnh cũng gửi
    // dạng này: yt-dlp không tải ảnh, chỉ ra nhạc của bài.
    const linkOf = (post) => `https://www.tiktok.com/@${post.user || "_"}/video/${post.id}`

    // 1. Ô của lưới (hồ sơ, tìm kiếm, khám phá): video nằm trong link của ô, hoặc cạnh link trong cùng ô.
    const tile = postOf(video.closest(LINKS)?.getAttribute("href")) ??
      postOf(video.closest('[data-e2e="user-post-item"], [data-e2e="explore-item"], [data-e2e="search_top-item"]')?.querySelector(LINKS)?.getAttribute("href"))
    if (tile) return linkOf(tile)

    // 2. Feed: mã ở khung trình phát `xgwrapper-<số>-<mã>`, kênh ở link tác giả của cùng mục. Thẻ quảng cáo thì thôi.
    const item = video.closest('[data-e2e="recommend-list-item-container"], article')
    if (item?.querySelector('[data-e2e="ad-card"], [data-e2e="feed-ad"], [data-e2e="promoted-label"]')) return null
    const player = video.closest('[id^="xgwrapper-"]') ?? item?.querySelector('[id^="xgwrapper-"]')
    const id = /^xgwrapper-\d+-(\d{8,25})$/.exec(player?.id ?? "")?.[1]
    const page = postOf(location.href)
    if (id) {
      if (page?.id === id) return linkOf(page)
      const author =
        item?.querySelector('a[data-e2e="video-author-avatar"]')?.getAttribute("href") ??
        item?.querySelector('a[href^="/@"]')?.getAttribute("href")
      return linkOf({ user: /^\/@([\w.-]+)\/?$/.exec(author ?? "")?.[1] ?? "", id })
    }

    // 3. Trang xem một video (/@kênh/video/<mã>): chính trang.
    if (page) return linkOf(page)

    // 4. Link bài đăng trong tổ tiên gần nhất có link như vậy; nhiều bài khác nhau (cả lưới) ⇒ không đoán.
    for (let element = video.parentElement, hops = 0; element && hops < 12; element = element.parentElement, hops += 1) {
      const posts = [...element.querySelectorAll(LINKS)].map((link) => postOf(link.getAttribute("href"))).filter(Boolean)
      if (posts.length === 0) continue
      return new Set(posts.map((post) => post.id)).size === 1 ? linkOf(posts[0]) : null
    }
    return null
  },
})
