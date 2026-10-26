// Đọc link file thật của video đang xem ngay trong trang (notes/fb-media.md): Tin Facebook và video trong bảng tin
// phát bằng `blob:` (MSE) và yt-dlp không đọc được trang. Nền chạy hàm này bằng scripting.executeScript ở thế giới
// của trang (MAIN) để thấy chỗ fb-media.js ghi nhớ và dữ liệu trình phát React; Firefox cũ chạy như content script,
// đọc qua `wrappedJSObject`. Phải tự đủ: không dùng biến ngoài hàm. Không đọc cookie, không gửi gì đi.
// Thứ tự: (1) chỗ ghi nhớ của fb-media.js theo mã thẻ Tin trong địa chỉ / mã video quanh thẻ <video>; (2) link có
// sẵn trong props React của trình phát; (3) kiểu IDM: các yêu cầu mạng trang đã gửi (Resource Timing).
function utdFindPageMedia(x, y) {
  const MAX_FORMATS = 6
  const MAX_URL = 4096
  // File trọn có tiếng của Facebook thường tới 720p: luồng DASH cao hơn thế mới thêm dòng (hình + tiếng ghép).
  const PROGRESSIVE_HEIGHT = 720
  const page = (value) => (value && typeof value === "object" && value.wrappedJSObject) || value
  const web = (value) => typeof value === "string" && value.length <= MAX_URL && /^https:\/\//i.test(value)

  /** Video ở điểm (x, y) — video người dùng vừa bấm nút — không có thì video đang phát, lớn nhất trong khung nhìn. */
  function pickVideo() {
    const videos = [...document.getElementsByTagName("video")]
    const area = (video) => {
      const rect = video.getBoundingClientRect()
      const width = Math.max(0, Math.min(rect.right, innerWidth) - Math.max(rect.left, 0))
      const height = Math.max(0, Math.min(rect.bottom, innerHeight) - Math.max(rect.top, 0))
      return width * height
    }
    if (Number.isFinite(x) && Number.isFinite(y)) {
      const hit = videos.find((video) => {
        const rect = video.getBoundingClientRect()
        return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom && area(video) > 0
      })
      if (hit) return hit
    }
    const score = (video) => (video.paused ? 0 : 1e9) + video.readyState * 1e6 + area(video)
    return videos.filter((video) => area(video) > 0).sort((a, b) => score(b) - score(a))[0] ?? null
  }

  /** Props React của `node` và tổ tiên gần nhất có fiber (đi lên `.return`), gần trước. */
  function reactProps(node) {
    const found = []
    for (let element = node, hops = 0; element && hops < 12 && found.length === 0; element = element.parentElement, hops += 1) {
      const raw = page(element)
      const key = Object.keys(raw).find((name) => name.startsWith("__reactFiber$") || name.startsWith("__reactInternalInstance$"))
      for (let fiber = key ? raw[key] : null, depth = 0; fiber && depth < 80; fiber = fiber.return, depth += 1) {
        if (fiber.memoizedProps && typeof fiber.memoizedProps === "object") found.push(fiber.memoizedProps)
      }
    }
    return found
  }

  /** Mã thẻ Tin (địa chỉ /stories/<bucket>/<thẻ>) và các mã video có thể của `video`, chắc trước. */
  function targetOf(video, props) {
    let card = /^\/stories\/[^/]+\/([^/?#]+)/.exec(location.pathname)?.[1] ?? null
    try {
      if (card) card = decodeURIComponent(card)
    } catch {
      // Giữ nguyên.
    }
    const ids = []
    const add = (value) => {
      const id = String(value ?? "")
      if (/^\d{5,25}$/.test(id) && !ids.includes(id)) ids.push(id)
    }
    const fromLink = (href) => /\/(?:reel|videos)\/(\d+)/.exec(href)?.[1] ?? /[?&]v=(\d+)/.exec(href)?.[1]
    add(fromLink(location.href))
    for (const item of props.slice(0, 40)) {
      for (const value of [item.videoId, item.videoID, item.videoFBID, item.video_id, item.video?.id, item.videoData?.videoId]) add(value)
    }
    // Link /reel/<mã>, /videos/<mã> gần thẻ <video> nhất (bảng tin): dừng ở tổ tiên đầu tiên có link như vậy.
    for (let element = video?.parentElement, hops = 0; element && hops < 15; element = element.parentElement, hops += 1) {
      const links = element.querySelectorAll('a[href*="/reel/"], a[href*="/videos/"], a[href*="watch/?v="]')
      if (links.length === 0) continue
      for (const link of links) add(fromLink(link.href))
      break
    }
    return { card, ids }
  }

  /** Link có sẵn trong props trình phát (cách cũ: hdSrc/sdSrc/playable_url…), tìm sâu có giới hạn. */
  function fromProps(props) {
    const hit = { hd: null, sd: null, manifest: null, reps: [], seconds: null }
    const names = {
      hd: ["hdSrc", "hd_src", "hdSrcNoRatelimit", "hd_src_no_ratelimit", "playable_url_quality_hd", "browser_native_hd_url"],
      sd: ["sdSrc", "sd_src", "sdSrcNoRatelimit", "sd_src_no_ratelimit", "playable_url", "browser_native_sd_url"],
    }
    const seen = new WeakSet()
    let budget = 20000
    const visit = (value, depth) => {
      if (!value || typeof value !== "object" || depth > 6 || budget <= 0 || seen.has(value)) return
      if (typeof Node !== "undefined" && value instanceof Node) return
      seen.add(value)
      budget -= 1
      let keys
      try {
        keys = Object.keys(value)
      } catch {
        return
      }
      for (const quality of ["hd", "sd"]) {
        for (const name of names[quality]) if (!hit[quality] && web(value[name])) hit[quality] = value[name]
      }
      for (const name of ["dashManifest", "dash_manifest", "manifest_xml", "dash_manifest_xml_string"]) {
        if (!hit.manifest && typeof value[name] === "string" && value[name].includes("<MPD")) hit.manifest = value[name]
      }
      for (const key of keys.slice(0, 200)) {
        if (key === "return" || key === "child" || key === "sibling" || key === "_owner" || key === "stateNode") continue
        let child
        try {
          child = value[key]
        } catch {
          continue
        }
        if (child && typeof child === "object") visit(page(child), depth + 1)
      }
    }
    for (const root of props) {
      visit(page(root), 0)
      if (hit.hd || hit.sd || hit.manifest) return hit
    }
    return null
  }

  /** Các Representation của manifest DASH (Facebook: mỗi luồng một BaseURL trọn file, hình và tiếng tách rời). */
  function parseManifest(xml) {
    const list = []
    let doc
    try {
      doc = new DOMParser().parseFromString(xml, "application/xml")
    } catch {
      return { list, seconds: null }
    }
    const duration = /PT(?:(\d+)H)?(?:(\d+)M)?(?:([\d.]+)S)?/.exec(doc.documentElement?.getAttribute("mediaPresentationDuration") ?? "")
    const seconds = duration ? Number(duration[1] ?? 0) * 3600 + Number(duration[2] ?? 0) * 60 + Number(duration[3] ?? 0) : null
    for (const rep of doc.getElementsByTagName("Representation")) {
      const set = rep.parentElement
      const attr = (name) => rep.getAttribute(name) ?? set?.getAttribute(name) ?? ""
      const base = rep.getElementsByTagName("BaseURL")[0]?.textContent?.trim()
      if (!web(base)) continue
      list.push({
        url: base,
        mime: attr("mimeType"),
        codecs: attr("codecs"),
        height: Number(attr("height")) || 0,
        bandwidth: Number(attr("bandwidth")) || 0,
      })
    }
    return { list, seconds: seconds || null }
  }

  /**
   * Bảng định dạng của một video: luồng DASH cao hơn file trọn (hình + tiếng tốt nhất, UTD ghép), rồi file trọn HD/SD
   * có sẵn tiếng. Không có file trọn ⇒ mọi chiều cao DASH. Cùng chiều cao thì ưu tiên H.264 (máy nào cũng phát được).
   */
  function formatsOf(entry, seconds) {
    const parsed = entry.manifest ? parseManifest(entry.manifest) : { list: [], seconds: null }
    const reps = [...parsed.list, ...entry.reps]
    const length = parsed.seconds || entry.seconds || seconds
    const estimate = (rep) => (rep.bandwidth && length ? Math.round((rep.bandwidth * length) / 8) : null)
    const avc = (rep) => (/^avc/i.test(rep.codecs) ? 1 : 0)
    const audio = reps
      .filter((rep) => rep.mime.startsWith("audio/"))
      .sort((a, b) => /^mp4a/i.test(b.codecs) - /^mp4a/i.test(a.codecs) || b.bandwidth - a.bandwidth)[0]
    const video = reps
      .filter((rep) => rep.mime.startsWith("video/") && rep.height > 0)
      .sort((a, b) => b.height - a.height || avc(b) - avc(a) || b.bandwidth - a.bandwidth)
    const progressive = [entry.hd, entry.sd].filter(web)
    const floor = progressive.length > 0 ? PROGRESSIVE_HEIGHT : 0
    const formats = []
    for (const rep of video) {
      if (rep.height <= floor || formats.length >= MAX_FORMATS || formats.some((format) => format.id === String(rep.height))) continue
      const streams = [{ url: rep.url, kind: audio ? "video" : "both", codecs: rep.codecs || null, sizeBytes: estimate(rep) }]
      if (audio) streams.push({ url: audio.url, kind: "audio", codecs: audio.codecs || null, sizeBytes: estimate(audio) })
      const total = streams.reduce((sum, stream) => sum + (stream.sizeBytes ?? 0), 0)
      formats.push({ id: String(rep.height), sizeBytes: total || null, streams })
    }
    if (web(entry.hd)) formats.push({ id: "hd", sizeBytes: null, streams: [{ url: entry.hd, kind: "both" }] })
    if (web(entry.sd) && entry.sd !== entry.hd) formats.push({ id: "sd", sizeBytes: null, streams: [{ url: entry.sd, kind: "both" }] })
    return formats
  }

  /** `efg` (base64 JSON) của link fbcdn: `vencode_tag` (đuôi `audio` = tiếng, `.ad.`/`-ads` = quảng cáo), mã asset. */
  function efgOf(url) {
    try {
      const efg = url.searchParams.get("efg")
      if (!efg) return {}
      const base64 = efg.replace(/-/g, "+").replace(/_/g, "/")
      return JSON.parse(atob(base64 + "=".repeat((4 - (base64.length % 4)) % 4))) ?? {}
    } catch {
      return {}
    }
  }

  /**
   * Dự phòng kiểu IDM: các yêu cầu mạng trang đã gửi. Luồng fbcdn tải từng đoạn `bytestart`/`byteend` ⇒ bỏ hai tham
   * số đó là cả luồng; gộp theo asset (`xpv_asset_id`), lấy asset được tải nhiều đoạn nhất (video đang phát, không
   * phải video tải trước), trong đó luồng hình và luồng tiếng nhiều đoạn nhất. Trang khác: manifest HLS/DASH (yt-dlp
   * đọc) và file media thường.
   */
  function sniff(remembered) {
    const streams = new Map()
    const manifests = []
    const files = []
    const entries = []
    const seen = new Set()
    // Facebook: fb-media.js ghi thêm các yêu cầu đoạn luồng sau khi bộ đệm Resource Timing đầy.
    for (const read of [() => remembered?.requests() ?? [], () => performance.getEntriesByType("resource")]) {
      try {
        for (const entry of read()) {
          const key = `${entry.startTime} ${entry.name}`
          if (seen.has(key)) continue
          seen.add(key)
          entries.push(entry)
        }
      } catch {
        // Nguồn này không đọc được: dùng nguồn còn lại.
      }
    }
    for (const entry of entries) {
      let url
      try {
        url = new URL(entry.name)
      } catch {
        continue
      }
      if (url.protocol !== "https:" || entry.name.length > MAX_URL) continue
      if (url.searchParams.has("bytestart")) {
        // Cắt chuỗi thay vì dựng lại URL: giữ nguyên từng ký tự của link có chữ ký.
        const whole = entry.name
          .replace(/[?&](?:bytestart|byteend)=\d*/g, (match) => (match[0] === "?" ? "?" : ""))
          .replace("?&", "?")
          .replace(/\?$/, "")
        url = new URL(whole)
        const info = efgOf(url)
        const tag = String(info.vencode_tag ?? "")
        if (/\.ad\.|-ads/i.test(tag)) continue
        const key = whole
        const stream = streams.get(key) ?? {
          url: key,
          asset: String(info.xpv_asset_id ?? info.video_id ?? url.pathname),
          audio: /audio$/i.test(tag),
          count: 0,
          last: 0,
        }
        stream.count += 1
        stream.last = Math.max(stream.last, entry.startTime)
        streams.set(key, stream)
        continue
      }
      const path = url.pathname.toLowerCase()
      if (/\.(m3u8|mpd)$/.test(path)) manifests.push(entry.name)
      else if (/\.(mp4|webm|m4v|mov)$/.test(path) && entry.initiatorType !== "link") files.push(entry.name)
    }
    const assets = new Map()
    for (const stream of streams.values()) {
      const asset = assets.get(stream.asset) ?? { count: 0, last: 0, list: [] }
      asset.count += stream.count
      asset.last = Math.max(asset.last, stream.last)
      asset.list.push(stream)
      assets.set(stream.asset, asset)
    }
    const best = [...assets.values()].sort((a, b) => b.count - a.count || b.last - a.last)[0]
    const rank = (a, b) => b.count - a.count || b.last - a.last
    const picture = best?.list.filter((stream) => !stream.audio).sort(rank)[0]
    const sound = best?.list.filter((stream) => stream.audio).sort(rank)[0]
    const formats = []
    if (picture) {
      const list = [{ url: picture.url, kind: sound ? "video" : "both" }]
      if (sound) list.push({ url: sound.url, kind: "audio" })
      formats.push({ id: "stream", sizeBytes: null, streams: list })
    }
    const recent = (list) => [...new Set(list)].reverse().slice(0, 4)
    return { formats, manifests: recent(manifests), files: recent(files) }
  }

  const video = pickVideo()
  const seconds = video && Number.isFinite(video.duration) && video.duration > 0 ? video.duration : null
  const props = video ? reactProps(video) : []
  const target = targetOf(video, props)
  const remembered = window[Symbol.for("utd.fb-media")]
  let entry = null
  if (remembered) {
    remembered.scanScripts()
    entry = remembered.find(target.card, target.ids)
  }
  entry ??= props.length > 0 ? fromProps(props) : null
  const named = (document.title || "").replace(/\s*[|·-]\s*Facebook\s*$/i, "").trim()
  const title = (entry?.owner || named || "").slice(0, 200) || null
  const formats = entry ? formatsOf(entry, seconds) : []
  if (formats.length > 0) return { title, formats, manifests: [], files: [] }
  return { title, ...sniff(remembered) }
}
