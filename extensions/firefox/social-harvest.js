// Tải hàng loạt Facebook (notes/social-bulk.md): gom ảnh/video có trong dữ liệu JSON của trang trong lúc
// tiện ích tự cuộn. Nền chạy file này trong thế giới của trang (MAIN) khi người dùng bấm "Tải hàng loạt bằng UTD".
// Dữ liệu tới bằng JSON: khối `<script type="application/json" data-sjs>` lúc mở trang và câu trả lời `/api/graphql/`
// khi trang nạp thêm — cả hai đều qua `JSON.parse` (như fb-media.js). Không phụ thuộc mã truy vấn (doc_id) hay đổi:
// trang tự gọi bằng mã đang dùng, ở đây chỉ đọc theo. Không đổi kết quả, không gửi gì đi.
;(() => {
  const KEY = Symbol.for("utd.social-harvest")
  if (window[KEY]) return
  // Lọc rẻ trước khi duyệt: chỉ chuỗi JSON có dữ liệu ảnh/video.
  const MARKERS = /"Photo"|"Video"|progressive_url|playable_url|unified_stories/
  const MAX_ITEMS = 25000
  const MAX_URL = 4096
  const CDN = /^https:\/\/[^/]*(fbcdn\.net|cdninstagram\.com|fbsbx\.com)\//i
  const items = new Map() // id ⇒ mục
  let fresh = new Set() // id mới hoặc vừa có link tốt hơn, chưa giao cho nền
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
      // Cùng mục thấy lại: chỉ thay khi có link file mà trước chưa có, hoặc ảnh lớn hơn.
      const better = (!known.url && item.url) || (item.url && area(item) > area(known))
      if (!better) return
      items.set(item.id, { ...known, ...item, story: known.story || item.story })
    } else {
      if (items.size >= MAX_ITEMS) return
      items.set(item.id, item)
    }
    fresh.add(item.id)
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
      // Kích thước thật của ảnh (ảnh trong lưới thường là ảnh thu nhỏ): nền mở trang ảnh để lấy bản gốc.
      fullWidth: Number(node.viewer_image?.width) || Number(node.width) || 0,
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
    const progressive = (Array.isArray(result?.progressive_urls) ? result.progressive_urls : [])
      .filter((item) => !item?.failure_reason && cdn(item?.progressive_url))
      .sort((a, b) => (String(b?.metadata?.quality).toUpperCase() === "HD") - (String(a?.metadata?.quality).toUpperCase() === "HD"))
      .map((item) => item.progressive_url)
    const url = [...progressive, node.playable_url_quality_hd, legacy.playable_url_quality_hd, node.browser_native_hd_url, legacy.browser_native_hd_url, node.playable_url, legacy.playable_url, node.browser_native_sd_url, legacy.browser_native_sd_url].find(cdn) ?? null
    const thumb = [node.preferred_thumbnail?.image?.uri, node.thumbnailImage?.uri, node.image?.uri, node.previewImage?.uri, node.first_frame_thumbnail].find(cdn) ?? null
    const id = String(node.id)
    const link = [node.permalink_url, node.url].find((value) => typeof value === "string" && /^https:\/\/(www|web|m)\.facebook\.com\//.test(value))
    if (!url && !thumb && !link) return
    put({
      id,
      kind: "video",
      url,
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
      if (node.unified_stories_with_notes || node.unified_stories) fbBucket(node)
      else fbMedia(node, false)
      const keys = Object.keys(node)
      for (let at = keys.length - 1; at >= 0; at -= 1) {
        const child = node[keys[at]]
        if (child && typeof child === "object") stack.push(child)
      }
    }
  }

  const parse = JSON.parse
  const wrapped = {
    parse(raw, reviver) {
      const value = parse.call(this, raw, reviver)
      if (typeof raw === "string" && raw.length > 200 && MARKERS.test(raw)) {
        setTimeout(() => {
          try {
            index(value)
          } catch {
            // Hỏng thì thôi, trang vẫn chạy như thường.
          }
        }, 0)
      }
      return value
    },
  }.parse
  Object.defineProperty(wrapped, "toString", { value: () => parse.toString() })
  JSON.parse = wrapped

  /** Đọc các khối JSON có sẵn trong trang (dữ liệu lúc mở trang). */
  function scan() {
    for (const script of document.querySelectorAll('script[type="application/json"]')) {
      const raw = script.textContent ?? ""
      if (!MARKERS.test(raw)) continue
      try {
        index(parse(raw))
      } catch {
        // Khối hỏng: bỏ qua.
      }
    }
  }
  scan()

  Object.defineProperty(window, KEY, {
    value: Object.freeze({
      /** Mục mới (hoặc vừa có link tốt hơn) từ lần gọi trước; `parsed` = số khối JSON đã đọc (trang còn nạp không). */
      take() {
        const ids = [...fresh]
        fresh = new Set()
        return { items: ids.map((id) => ({ ...items.get(id) })), parsed, total: items.size }
      },
      /** Link Tin / tin nổi bật trên trang hồ sơ (vòng tròn ảnh): `{url, title}`. */
      storyLinks() {
        const seen = new Set()
        const links = []
        for (const anchor of document.querySelectorAll('a[href*="/stories/"]')) {
          const url = new URL(anchor.href, location.href)
          const match = /^\/stories\/(\d+)\/?/.exec(url.pathname)
          if (url.hostname !== location.hostname || !match || seen.has(match[1])) continue
          seen.add(match[1])
          const title = text(anchor.getAttribute("aria-label") ?? anchor.textContent, 100)
          links.push({ url: url.href, bucket: match[1], title, highlight: /highlight/i.test(url.search) })
          if (links.length >= 100) break
        }
        return links
      },
    }),
  })
})()
