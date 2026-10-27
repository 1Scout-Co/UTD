// Douyin: hỏi API web của chính Douyin ngay trong trang (thế giới MAIN, nền nạp bằng scripting.executeScript khi
// người dùng bấm). Từ 2026-09 Douyin chặn mọi lời gọi không có chữ ký của trang (ArgusSecurityPlugin); `fetch` trong
// trang được SDK của Douyin tự ký, nên UTD không tự làm a_bogus/X-Bogus và không cần đọc cookie: trình duyệt tự kèm
// (notes/douyin.md: dYm, social-media-copilot, jiji262/douyin-downloader). Không tự điền các tham số trang tự thêm
// (msToken, a_bogus, uifid, verifyFp…): trùng là bị từ chối "Sign Invalid".
// Kết quả: link MP4 không watermark có sẵn tiếng (`bit_rate[].play_addr`, bỏ bản `dash` chỉ có hình).
;(() => {
  const KEY = Symbol.for("utd.douyin")
  if (window[KEY]) return
  const COMMON = {
    device_platform: "webapp",
    aid: "6383",
    channel: "channel_pc_web",
    pc_client_type: "1",
    version_code: "290100",
    version_name: "29.1.0",
    cookie_enabled: "true",
    platform: "PC",
  }
  const MAX_URL = 4096
  const MAX_VARIANTS = 6
  const IMAGE_TYPES = new Set([2, 68, 150])
  const web = (value) => typeof value === "string" && value.length <= MAX_URL && /^https:\/\//i.test(value)
  const watermarked = (url) => /playwm|watermark=1|dy-water|owner_watermark/i.test(url)

  /** Gọi API bằng `fetch` của trang (được tự ký). `{ data }`, hoặc `{ error: "blocked" | "login" }`. */
  async function call(path, params) {
    // Lý do bị chặn ghi ra Console của trang (lọc "[UTD]"): không có cookie hay nội dung trả về, chỉ mã + độ dài.
    const blocked = (why) => {
      console.warn("[UTD] Douyin", path, why)
      return { error: "blocked" }
    }
    let response
    try {
      response = await fetch(`${path}?${new URLSearchParams({ ...COMMON, ...params })}`, { credentials: "include" })
    } catch (error) {
      return blocked(`fetch: ${error?.name ?? error}`)
    }
    const text = await response.text().catch(() => "")
    if (!response.ok || !text || /ArgusSecurityPlugin/.test(text)) {
      return blocked(`HTTP ${response.status}, ${text.length} chars${/ArgusSecurityPlugin/.test(text) ? ", Argus" : ""}`)
    }
    try {
      const data = JSON.parse(text)
      if (data?.status_code === 2483 || /请先登录|用户未登录/.test(String(data?.status_msg ?? ""))) return { error: "login" }
      return { data }
    } catch {
      return blocked(`not JSON, ${text.length} chars`)
    }
  }

  /** Link tốt nhất của một địa chỉ phát: máy chủ CDN (douyinvod…) trước, link /aweme/v1/play/ sau, bỏ bản watermark. */
  function bestUrl(address) {
    const list = Array.isArray(address)
      ? address.map((item) => item?.src) // dạng camelCase của trình phát: [{ src }]
      : (address?.url_list ?? address?.urlList ?? [])
    const usable = list.map((url) => (typeof url === "string" && url.startsWith("//") ? `https:${url}` : url)).filter(web)
    const clean = usable.filter((url) => !watermarked(url))
    return clean.find((url) => !/(^https:\/\/)(www\.)?douyin\.com\//.test(url)) ?? clean[0] ?? null
  }

  /** Các bản MP4 có tiếng của một bài (API snake_case hoặc dữ liệu trình phát camelCase), cạnh ngắn giảm dần. */
  function variantsOf(aweme) {
    const video = aweme?.video ?? {}
    const ladder = Array.isArray(video.bit_rate) ? video.bit_rate : Array.isArray(video.bitRateList) ? video.bitRateList : []
    const found = []
    for (const rate of ladder) {
      // API snake_case (`gear_name`, `is_bytevc1`) và dữ liệu trình phát camelCase (`gearName`, `isBytevc1`).
      const gear = String(rate?.gear_name ?? rate?.gearName ?? "")
      if (String(rate?.format ?? "").toLowerCase() === "dash" || /bytevc2/i.test(gear)) continue
      const address = rate.play_addr ?? rate.playAddr
      const url = bestUrl(address)
      if (!url) continue
      const width = Number(address?.width ?? rate.width) || 0
      const height = Number(address?.height ?? rate.height) || 0
      found.push({
        url,
        height: Math.min(width || height, height || width),
        sizeBytes: Number(address?.data_size ?? rate.dataSize) || null,
        h265: Boolean(rate.is_h265 || rate.is_bytevc1 || rate.isH265 || rate.isBytevc1),
      })
    }
    const base = bestUrl(video.play_addr ?? video.playAddr)
    if (base && found.length === 0) {
      const width = Number(video.width) || 0
      const height = Number(video.height) || 0
      found.push({ url: base, height: Math.min(width || height, height || width), sizeBytes: null, h265: false })
    }
    // Cùng cạnh ngắn: H.264 trước (máy nào cũng phát được), rồi file lớn hơn.
    found.sort((a, b) => b.height - a.height || a.h265 - b.h265 || (b.sizeBytes ?? 0) - (a.sizeBytes ?? 0))
    const unique = []
    for (const variant of found) {
      if (!unique.some((kept) => kept.height === variant.height)) unique.push(variant)
    }
    return unique.slice(0, MAX_VARIANTS)
  }

  const idOf = (aweme) => String(aweme?.aweme_id ?? aweme?.awemeId ?? "")
  const titleOf = (aweme) =>
    String(aweme?.desc ?? "").replace(/\s+/g, " ").trim().slice(0, 200) ||
    String(aweme?.author?.nickname ?? aweme?.authorInfo?.nickname ?? "").trim() ||
    `douyin ${idOf(aweme)}`

  /** Bài đang phát trong trình phát của trang (`window.player`, douyin-dl-user-js): dự phòng khi API bị chặn. */
  function fromPlayer(id) {
    const player = window.player
    const plugins = player?._config?._config?.pluginsConfig ?? {}
    for (const info of [...Object.values(plugins).map((plugin) => plugin?.awemeInfo), player?.config?.awemeInfo]) {
      if (info && idOf(info) === id) return info
    }
    return null
  }

  window[KEY] = Object.freeze({
    /** Bảng định dạng của video ở link `url` (`/video/<mã>`, `/note/<mã>`, `?modal_id=`). */
    async media(url) {
      let id = ""
      try {
        const parsed = new URL(url, location.href)
        id = /\/(?:video|note|slides)\/(\d{8,25})/.exec(parsed.pathname)?.[1] ?? parsed.searchParams.get("modal_id") ?? ""
      } catch {
        // Link hỏng.
      }
      if (!/^\d{8,25}$/.test(id)) return { title: null, formats: [], manifests: [], files: [] }
      let result = await call("/aweme/v1/web/aweme/detail/", { aweme_id: id })
      // Bài ảnh (note) có khi chỉ trả với aid=1128.
      if (!result.data?.aweme_detail && result.data?.filter_detail) {
        result = await call("/aweme/v1/web/aweme/detail/", { aweme_id: id, aid: "1128" })
      }
      const aweme = result.data?.aweme_detail ?? fromPlayer(id)
      const formats = variantsOf(aweme).map((variant) => ({
        id: variant.height > 0 ? String(variant.height) : "file",
        sizeBytes: variant.sizeBytes,
        streams: [{ url: variant.url, kind: "both", sizeBytes: variant.sizeBytes }],
      }))
      return { title: aweme ? titleOf(aweme) : null, formats, manifests: [], files: [], error: aweme ? null : result.error }
    },

    /** Một trang danh sách bài của hồ sơ `secUid` từ `cursor` (API `aweme/post`, lật bằng `max_cursor`). */
    async posts(secUid, cursor) {
      const result = await call("/aweme/v1/web/aweme/post/", {
        sec_user_id: secUid,
        max_cursor: String(cursor ?? 0),
        count: "18",
        locate_query: "false",
        show_live_replay_strategy: "1",
        need_time_list: "1",
        time_list_query: "0",
        whale_cut_token: "",
        cut_version: "1",
        publish_video_strategy_type: "2",
      })
      if (!result.data) return { error: result.error ?? "blocked" }
      const list = Array.isArray(result.data.aweme_list) ? result.data.aweme_list : []
      const entries = list.flatMap((aweme) => {
        const id = idOf(aweme)
        if (!/^\d{8,25}$/.test(id)) return []
        const image = IMAGE_TYPES.has(Number(aweme.aweme_type)) || (Array.isArray(aweme.images) && aweme.images.length > 0)
        const variants = variantsOf(aweme).slice(0, 4)
        return [
          {
            id,
            url: `https://www.douyin.com/${image ? "note" : "video"}/${id}`,
            title: titleOf(aweme),
            timestamp: Number(aweme.create_time) || null,
            durationSeconds: Number(aweme.video?.duration) > 0 ? Math.round(Number(aweme.video.duration) / 1000) : null,
            thumbnail: bestUrl(aweme.video?.cover ?? aweme.video?.origin_cover),
            variants,
          },
        ]
      })
      const author = list[0]?.author
      return {
        entries,
        cursor: String(result.data.max_cursor ?? result.data.cursor ?? ""),
        hasMore: Boolean(result.data.has_more),
        author: author ? { name: String(author.nickname ?? "").slice(0, 120) || null } : null,
      }
    },
  })
})()
