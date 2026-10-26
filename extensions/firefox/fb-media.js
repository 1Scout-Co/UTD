// Facebook: ghi nhớ link file video có trong dữ liệu JSON của trang để nút "Tải video này" tải đúng video đang xem
// (notes/fb-media.md). Chạy trong thế giới của trang (MAIN) từ document_start, chỉ trên facebook.com (nền đăng ký
// lúc chạy). Dữ liệu Tin/video tới bằng JSON: khối `<script type="application/json" data-sjs>` lúc mở trang và câu
// trả lời `/api/graphql/` khi chuyển trang trong app — cả hai đều qua `JSON.parse`, nên bọc `JSON.parse` (như
// HushFacebookWeb) là bắt được hết. Chỉ đọc, không đổi kết quả, không gửi gì đi; trang thấy nhưng không dùng.
;(() => {
  const KEY = Symbol.for("utd.fb-media")
  if (window[KEY]) return
  // Lọc rẻ trước khi duyệt: chỉ chuỗi JSON có dữ liệu video/Tin.
  const MARKERS = /progressive_url|playable_url|browser_native_|dash_manifest|all_video_dash_prefetch|story_card_info/
  const MAX_VIDEOS = 400
  const MAX_URL = 4096
  const videos = new Map() // mã video ⇒ { hd, sd, manifest, reps, seconds }
  const cards = new Map() // mã thẻ Tin (phần cuối địa chỉ /stories/<bucket>/<thẻ>) ⇒ { media, owner }
  const web = (value) => typeof value === "string" && value.length <= MAX_URL && /^https:\/\//i.test(value)
  const first = (...values) => values.find(web) ?? null

  function video(id) {
    let entry = videos.get(id)
    if (entry) {
      videos.delete(id)
    } else {
      entry = { hd: null, sd: null, manifest: null, reps: [], seconds: null }
      if (videos.size >= MAX_VIDEOS) videos.delete(videos.keys().next().value)
    }
    videos.set(id, entry)
    return entry
  }

  /** Một object media video (GraphQL `Video`, dữ liệu trình phát cũ): file trọn có tiếng HD/SD, manifest DASH. */
  function noteVideo(node, id) {
    const result = node.videoDeliveryResponseFragment?.videoDeliveryResponseResult
    const legacy = node.videoDeliveryLegacyFields ?? {}
    const progressive = (quality) =>
      (Array.isArray(result?.progressive_urls) ? result.progressive_urls : []).find(
        (item) => !item?.failure_reason && String(item?.metadata?.quality).toUpperCase() === quality,
      )?.progressive_url
    const hd = first(progressive("HD"), node.playable_url_quality_hd, legacy.playable_url_quality_hd, node.browser_native_hd_url, legacy.browser_native_hd_url, node.hd_src_no_ratelimit, node.hd_src, node.hdSrc)
    const sd = first(progressive("SD"), node.playable_url, legacy.playable_url, node.browser_native_sd_url, legacy.browser_native_sd_url, node.sd_src_no_ratelimit, node.sd_src, node.sdSrc)
    const manifests = Array.isArray(result?.dash_manifests) ? result.dash_manifests : []
    const manifest = [manifests[0]?.manifest_xml, node.dash_manifest, node.manifest_xml, node.dash_manifest_xml_string, legacy.dash_manifest]
      .find((text) => typeof text === "string" && text.includes("<MPD"))
    const seconds = Number(node.length_in_second) || Number(node.playable_duration_in_ms) / 1000 || null
    if (!hd && !sd && !manifest) return
    const entry = video(id)
    entry.hd = hd ?? entry.hd
    entry.sd = sd ?? entry.sd
    entry.manifest = manifest ?? entry.manifest
    entry.seconds = seconds ?? entry.seconds
  }

  /** Duyệt (giới hạn) một kết quả JSON, ghi video, thẻ Tin (thẻ ⇒ video) và chủ Tin (để đặt tên file). */
  function index(root) {
    let budget = 60000
    const stack = [[root, null]]
    while (stack.length > 0 && budget > 0) {
      const [node, owner] = stack.pop()
      budget -= 1
      if (!node || typeof node !== "object") continue
      if (Array.isArray(node)) {
        for (const item of node) if (item && typeof item === "object") stack.push([item, owner])
        continue
      }
      const id = typeof node.id === "string" || typeof node.id === "number" ? String(node.id) : null
      const name = node.story_bucket_owner?.name ?? node.owner?.name
      const holder = typeof name === "string" ? name.slice(0, 120) : owner
      if (id && /^\d+$/.test(id)) noteVideo(node, id)
      // Thẻ Tin: `id` là mã base64 trong địa chỉ, media nằm ở attachments[].media.
      if (id && (node.story_card_info || /^UzpfSVNDO/.test(id)) && Array.isArray(node.attachments)) {
        const media = node.attachments.map((item) => item?.media).find((item) => item?.id)
        if (media) cards.set(id, { media: String(media.id), owner: holder ?? null })
      }
      // Danh sách luồng DASH tách rời (hình riêng, tiếng riêng) của nhiều video, khoá theo video_id.
      if (Array.isArray(node.all_video_dash_prefetch_representations)) {
        for (const item of node.all_video_dash_prefetch_representations) {
          if (!item?.video_id || !Array.isArray(item.representations)) continue
          const reps = item.representations
            .filter((rep) => web(rep?.base_url) && typeof rep.mime_type === "string")
            .map((rep) => ({ url: rep.base_url, mime: rep.mime_type, codecs: String(rep.codecs ?? ""), height: Number(rep.height) || 0, bandwidth: Number(rep.bandwidth) || 0 }))
          if (reps.length > 0) video(String(item.video_id)).reps = reps
        }
      }
      for (const key in node) {
        const child = node[key]
        if (child && typeof child === "object") stack.push([child, holder])
      }
    }
  }

  const parse = JSON.parse
  const wrapped = {
    parse(text, reviver) {
      const value = parse.call(this, text, reviver)
      // Duyệt sau, ngoài lượt của trang: không làm chậm trang.
      if (typeof text === "string" && text.length > 200 && MARKERS.test(text)) {
        setTimeout(() => {
          try {
            index(value)
          } catch {
            // Ghi nhớ hỏng thì thôi, trang vẫn chạy như thường.
          }
        }, 0)
      }
      return value
    },
  }.parse
  Object.defineProperty(wrapped, "toString", { value: () => parse.toString() })
  JSON.parse = wrapped

  // Yêu cầu đoạn luồng (`bytestart=`) gần đây cho đường dự phòng kiểu IDM của page-media.js: bộ đệm Resource Timing
  // của trình duyệt chỉ giữ 250 mục, đầy là thôi ghi; bộ quan sát vẫn được báo.
  const MAX_REQUESTS = 600
  const requests = []
  try {
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        if (!/[?&]bytestart=/.test(entry.name)) continue
        requests.push({ name: entry.name, initiatorType: entry.initiatorType, startTime: entry.startTime })
      }
      if (requests.length > MAX_REQUESTS) requests.splice(0, requests.length - MAX_REQUESTS)
    }).observe({ type: "resource", buffered: true })
  } catch {
    // Trình duyệt không có bộ quan sát: page-media.js chỉ đọc bộ đệm.
  }

  let scanned = false
  Object.defineProperty(window, KEY, {
    value: Object.freeze({
      /** Các yêu cầu đoạn luồng đã thấy (cũ trước). */
      requests() {
        return requests.slice()
      },
      /** Đọc lại các khối JSON có sẵn trong trang (tiện ích nạp sau khi trang đã mở). */
      scanScripts() {
        if (scanned) return
        scanned = true
        for (const script of document.querySelectorAll('script[type="application/json"]')) {
          const text = script.textContent ?? ""
          if (!MARKERS.test(text)) continue
          try {
            index(parse(text))
          } catch {
            // Khối hỏng: bỏ qua.
          }
        }
      },
      /** Video của thẻ Tin `card` hoặc của một trong các mã `ids` (gần trước); không có ⇒ null. */
      find(card, ids) {
        const story = card ? cards.get(card) : null
        for (const id of [story?.media, ...(ids ?? [])]) {
          const entry = id ? videos.get(String(id)) : null
          if (entry) return { id: String(id), owner: story?.owner ?? null, ...entry, reps: [...entry.reps] }
        }
        return null
      },
    }),
  })
})()
