// Tải hàng loạt Facebook / Instagram / Threads (notes/social-bulk.md): đọc ảnh/video trong câu trả lời GraphQL mà
// social-bulk.js lấy ngay trong trang (`read`), giao cho nền (`take`). Nền nạp file này vào trang khi người dùng bấm
// "Tải hàng loạt bằng UTD". Không đụng dữ liệu của trang, không gửi gì đi.
;(() => {
  const KEY = Symbol.for("utd.social-harvest")
  if (window[KEY]) return
  const MAX_ITEMS = 25000
  const MAX_URL = 4096
  const CDN = /^https:\/\/[^/]*(fbcdn\.net|cdninstagram\.com|fbsbx\.com)\//i
  // id ⇒ mục của các câu trả lời đọc từ lần giao trước. Giao xong là xoá: bấm Tải hàng loạt lần nữa trên cùng tab vẫn
  // nhận đủ mục (nền tự bỏ trùng trong một lượt).
  const items = new Map()
  let parsed = 0
  const cdn = (value) => typeof value === "string" && value.length <= MAX_URL && CDN.test(value)
  const text = (value, max) => (typeof value === "string" ? value.replace(/\s+/g, " ").trim().slice(0, max) : "")
  const seconds = (value) => {
    const number = Number(value)
    return Number.isFinite(number) && number > 1e9 ? Math.floor(number > 1e11 ? number / 1000 : number) : null
  }
  const area = (item) => (item?.width ?? 0) * (item?.height ?? 0)

  function put(item) {
    if (!item.id || (!item.url && !item.page)) return
    const known = items.get(item.id)
    if (known) {
      // Cùng mục thấy lại: chỉ thay khi có link file mà trước chưa có, video có bản HD mà trước chỉ SD, hoặc cùng mức mà lớn hơn.
      const better = (!known.url && item.url) || (item.url && item.hd && !known.hd) || (item.url && Boolean(item.hd) === Boolean(known.hd) && area(item) > area(known))
      if (better) items.set(item.id, { ...known, ...item, story: known.story || item.story })
    } else if (items.size < MAX_ITEMS) {
      items.set(item.id, item)
    }
  }

  // --- Instagram / Threads: bài viết dạng REST (`image_versions2`, `video_versions`, `carousel_media`) ---

  /** Ảnh lớn nhất và ảnh nhỏ (~320px) trong `candidates`. */
  function images(node) {
    const list = (Array.isArray(node?.image_versions2?.candidates) ? node.image_versions2.candidates : []).filter((item) => cdn(item?.url))
    if (list.length === 0) return { best: null, small: null }
    const sized = list.map((item) => ({ url: item.url, width: Number(item.width) || 0, height: Number(item.height) || 0 }))
    const best = sized.reduce((a, b) => (area(b) > area(a) ? b : a))
    const small = sized.reduce((a, b) => (Math.abs(b.width - 320) < Math.abs(a.width - 320) ? b : a))
    return { best, small }
  }

  function bestVideo(node) {
    const list = (Array.isArray(node?.video_versions) ? node.video_versions : []).filter((item) => cdn(item?.url))
    if (list.length === 0) return null
    return list
      .map((item) => ({ url: item.url, width: Number(item.width) || 0, height: Number(item.height) || 0 }))
      .reduce((a, b) => (area(b) > area(a) ? b : a))
  }

  function igPost(node) {
    const owner = node.user ?? node.owner ?? {}
    const username = typeof owner.username === "string" ? owner.username : null
    const code = typeof node.code === "string" && /^[\w-]{1,64}$/.test(node.code) ? node.code : null
    const threads = /(^|\.)threads\.(net|com)$/.test(location.hostname)
    const reel = node.product_type === "clips"
    let page = null
    if (code && threads && username) page = `https://www.threads.com/@${username}/post/${code}`
    else if (code && !threads) page = `https://www.instagram.com/${reel ? "reel" : "p"}/${code}/`
    const children = Array.isArray(node.carousel_media) && node.carousel_media.length > 0 ? node.carousel_media : [node]
    const caption = text(node.caption?.text, 300)
    const timestamp = seconds(node.taken_at ?? node.taken_at_timestamp)
    const base = String(node.pk ?? node.id ?? code ?? "")
    children.forEach((child, index) => {
      const { best, small } = images(child)
      const video = bestVideo(child)
      const id = String(child.pk ?? child.id ?? `${base}_${index}`).replace(/[^\w:.-]/g, "_")
      // `media_type: 2` = video; danh sách có lúc thiếu link video ⇒ không lấy ảnh bìa thay (UTD tải từ trang bài).
      const isVideo = Boolean(video) || child.media_type === 2
      const file = isVideo ? video : best
      put({
        id,
        kind: isVideo ? "video" : "photo",
        url: file?.url ?? null,
        width: file?.width ?? 0,
        height: file?.height ?? 0,
        thumb: small?.url ?? null,
        timestamp: seconds(child.taken_at) ?? timestamp,
        title: caption,
        page,
        owner: username,
        product: reel ? "reel" : "post",
        durationSeconds: Number(child.video_duration) || null,
      })
    })
  }

  // --- Facebook: object GraphQL `Photo`, `Video`, Tin (bucket có `unified_stories*`) ---

  const IMAGE_KEYS = ["image", "viewer_image", "photo_image", "large_image", "largest_image", "full_image", "massive_image", "image_high_res", "previewImage", "preview_image"]

  function fbPhoto(node, story) {
    const candidates = IMAGE_KEYS.map((key) => node[key])
      .filter((image) => image && cdn(image.uri))
      .map((image) => ({ url: image.uri, width: Number(image.width) || 0, height: Number(image.height) || 0 }))
    if (candidates.length === 0) return
    const best = candidates.reduce((a, b) => (area(b) > area(a) ? b : a))
    const small = candidates.reduce((a, b) => (area(b) > 0 && area(b) < area(a) ? b : a))
    const id = String(node.id)
    const page = typeof node.url === "string" && /^https:\/\/(www|web|m)\.facebook\.com\//.test(node.url) ? node.url : `https://www.facebook.com/photo/?fbid=${id}`
    put({
      id,
      kind: "photo",
      url: best.url,
      width: best.width,
      height: best.height,
      thumb: small.url,
      timestamp: seconds(node.created_time ?? node.publish_time ?? node.creation_story?.creation_time),
      title: text(node.message?.text ?? node.title?.text, 300),
      page,
      owner: text(node.owner?.name, 120) || null,
      story: Boolean(story),
    })
  }

  function fbVideo(node, story) {
    const result = node.videoDeliveryResponseFragment?.videoDeliveryResponseResult
    const legacy = node.videoDeliveryLegacyFields ?? {}
    const progressive = (Array.isArray(result?.progressive_urls) ? result.progressive_urls : []).filter((item) => !item?.failure_reason && cdn(item?.progressive_url))
    const high = (item) => String(item?.metadata?.quality).toUpperCase() === "HD"
    // Bản HD trước (progressive HD, *_hd_url), rồi SD.
    const hd = [...progressive.filter(high).map((item) => item.progressive_url), node.playable_url_quality_hd, legacy.playable_url_quality_hd, node.browser_native_hd_url, legacy.browser_native_hd_url].find(cdn)
    const sd = [...progressive.filter((item) => !high(item)).map((item) => item.progressive_url), node.playable_url, legacy.playable_url, node.browser_native_sd_url, legacy.browser_native_sd_url].find(cdn)
    const url = hd ?? sd ?? null
    const thumb = [node.preferred_thumbnail?.image?.uri, node.thumbnailImage?.uri, node.image?.uri, node.previewImage?.uri, node.first_frame_thumbnail].find(cdn) ?? null
    const id = String(node.id)
    const link = [node.permalink_url, node.url].find((value) => typeof value === "string" && /^https:\/\/(www|web|m)\.facebook\.com\//.test(value))
    if (!url && !thumb && !link) return
    put({
      id,
      kind: "video",
      url,
      hd: Boolean(hd),
      width: Number(node.width) || 0,
      height: Number(node.height) || 0,
      thumb,
      timestamp: seconds(node.publish_time ?? node.created_time ?? node.creation_time),
      title: text(node.title?.text ?? node.message?.text ?? node.name, 300),
      page: link ?? `https://www.facebook.com/watch/?v=${id}`,
      owner: text(node.owner?.name, 120) || null,
      durationSeconds: Number(node.length_in_second) || Number(node.playable_duration_in_ms) / 1000 || null,
      story: Boolean(story),
    })
  }

  function fbMedia(node, story) {
    if (!node || typeof node !== "object" || (typeof node.id !== "string" && typeof node.id !== "number")) return
    if (!/^\d{3,}$/.test(String(node.id))) return
    const type = node.__typename ?? node.__isMedia
    if (type === "Photo") fbPhoto(node, story)
    else if (type === "Video") fbVideo(node, story)
  }

  /** Tin: thẻ của một bucket (Tin của một người / một tin nổi bật). */
  function fbBucket(node) {
    const owner = text(node.story_bucket_owner?.name ?? node.owner?.name, 120) || null
    for (const key of ["unified_stories_with_notes", "unified_stories"]) {
      const edges = Array.isArray(node[key]?.edges) ? node[key].edges : []
      for (const edge of edges) {
        const card = edge?.node
        for (const attachment of Array.isArray(card?.attachments) ? card.attachments : []) {
          const media = attachment?.media
          if (!media) continue
          fbMedia(media, true)
          const item = items.get(String(media.id))
          if (item) {
            item.bucket = String(node.id)
            item.owner = item.owner ?? owner
            item.timestamp = item.timestamp ?? seconds(card.creation_time)
            item.story = true
          }
        }
      }
    }
  }

  /** Duyệt (giới hạn) một kết quả JSON. */
  function index(root) {
    parsed += 1
    let budget = 80000
    const stack = [root]
    while (stack.length > 0 && budget > 0) {
      const node = stack.pop()
      budget -= 1
      if (!node || typeof node !== "object") continue
      // Đẩy ngược để lấy ra đúng thứ tự trong trang (mới trước).
      if (Array.isArray(node)) {
        for (let at = node.length - 1; at >= 0; at -= 1) if (node[at] && typeof node[at] === "object") stack.push(node[at])
        continue
      }
      if ((node.image_versions2 || node.video_versions || node.carousel_media) && (node.pk || node.id || node.code)) {
        igPost(node)
        // Mục con của carousel đã đọc cùng bài: không duyệt vào nữa (khỏi thành bài riêng).
        continue
      }
      if (node.unified_stories_with_notes || node.unified_stories) fbBucket(node)
      else fbMedia(node, false)
      const keys = Object.keys(node)
      for (let at = keys.length - 1; at >= 0; at -= 1) {
        const child = node[keys[at]]
        if (child && typeof child === "object") stack.push(child)
      }
    }
  }

  Object.defineProperty(window, KEY, {
    value: Object.freeze({
      /** Mục đọc được từ lần giao trước (rồi xoá); `parsed` = số câu trả lời đã đọc. */
      take() {
        const list = Array.from(items.values(), (item) => ({ ...item }))
        items.clear()
        return { items: list, parsed }
      },
      /** Đọc một câu trả lời JSON tiện ích vừa lấy. */
      read(value) {
        index(value)
      },
    }),
  })
})()
