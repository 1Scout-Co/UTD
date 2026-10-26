// Nút "Tải video này" trên trang, kiểu IDM (notes/auth-plan.md, GĐ3). Rê chuột vào video ⇒ nút nhỏ ở góc trên-phải;
// bấm ⇒ bảng chọn định dạng ngay trên trang (app phân tích bằng yt-dlp); bấm một dòng ⇒ UTD tải luôn định dạng đó.
// Không đọc cookie ở đây: nền (background.js) chỉ đọc khi nhận cú bấm thật (`isTrusted`) chuyển sang.
;(() => {
  if (window.top !== window || globalThis.__utdVideoButton) return
  globalThis.__utdVideoButton = true

  const api = globalThis.browser ?? globalThis.chrome
  const t = (key) => api.i18n.getMessage(key) || key
  const MIN_WIDTH = 160
  const MIN_HEIGHT = 90
  const PILL_HEIGHT = 22
  const GAP = 4
  const HIDE_AFTER = 1500
  const SHOW_ON_PLAY = 2500
  // Mã lỗi nghĩa là nguồn đòi đăng nhập (src-tauri/src/media/mod.rs `needs_sign_in`).
  const SIGN_IN = /^(media\.login|auth\.)/
  // Trang video quen: đọc sẵn định dạng (không cookie) khi nút hiện lần đầu.
  const KNOWN = /(^|\.)(youtube\.com|youtu\.be|tiktok\.com|facebook\.com|instagram\.com|vimeo\.com|dailymotion\.com|twitch\.tv|x\.com|twitter\.com|bilibili\.com)$/
  // YouTube: chỉ trang xem video (video xem thử ở trang chủ / danh sách không phải video của trang).
  const YOUTUBE = /(^|\.)youtube\.com$/
  const YOUTUBE_VIDEO = /^\/(watch|shorts\/|live\/|embed\/)/
  // Facebook: đọc luồng của video đang xem ngay trong trang (Tin, bảng tin: yt-dlp không đọc được; notes/fb-media.md).
  const FACEBOOK = /(^|\.)facebook\.com$/
  // Đọc trong trang mà nền không trả lời chừng này ms thì thôi, không để bảng treo.
  const PAGE_MEDIA_TIMEOUT = 8000

  const CSS = `
    :host { all: initial; }
    * { box-sizing: border-box; font: 12px/1.35 Inter, system-ui, -apple-system, "Segoe UI", sans-serif; }
    [hidden] { display: none !important; }
    .pill { position: fixed; display: flex; align-items: center; gap: 4px; height: ${PILL_HEIGHT}px; padding: 0 8px 0 6px;
      border: 0; border-radius: 5px; color: #fff; background: #0a6fd6; box-shadow: 0 1px 4px rgb(0 0 0 / 0.25);
      font-weight: 600; cursor: pointer; white-space: nowrap; pointer-events: auto; }
    .pill:hover { background: #005bb8; }
    .pill:focus-visible, .row:focus-visible, .close:focus-visible { outline: 2px solid #8ec5ff; outline-offset: 1px; }
    .pill svg { width: 13px; height: 13px; flex: none; }
    .panel { position: fixed; width: 232px; max-height: min(460px, 80vh); overflow: auto; padding: 0 0 3px; border-radius: 6px;
      color: #0f1b2d; background: #fff; border: 1px solid rgb(0 0 0 / 0.12); box-shadow: 0 4px 16px rgb(0 0 0 / 0.18);
      pointer-events: auto; overscroll-behavior: contain; scroll-behavior: smooth;
      scrollbar-width: thin; scrollbar-color: transparent transparent; }
    /* Thanh cuộn mảnh, chỉ hiện khi bảng dài hơn chỗ và chuột đang ở trên bảng. */
    .panel:hover, .panel:focus-within { scrollbar-color: #c3ccd8 transparent; }
    @media (prefers-reduced-motion: reduce) { .panel { scroll-behavior: auto; } }
    .head { position: sticky; top: 0; display: flex; align-items: center; gap: 4px; padding: 4px 4px 4px 10px;
      margin-bottom: 3px; border-bottom: 1px solid #e8ecf1; background: #fff; }
    .title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #3b4a5c;
      font-weight: 600; }
    .close { flex: none; width: 20px; height: 20px; border: 0; border-radius: 4px; color: #52647a; background: none;
      font-size: 14px; line-height: 1; cursor: pointer; }
    .close:hover, .row:hover { background: #e8f1fc; }
    .row { display: flex; width: 100%; align-items: center; justify-content: space-between; gap: 10px; padding: 4px 10px;
      border: 0; color: inherit; background: none; text-align: start; cursor: pointer; }
    .row.audio { margin-top: 3px; padding-top: 6px; border-top: 1px solid #e8ecf1; }
    .row:disabled { opacity: 0.55; cursor: default; }
    .size { color: #6b7a8c; font-size: 11px; font-variant-numeric: tabular-nums; }
    .status { padding: 8px 10px; color: #52647a; }
    .status.error { color: #b42318; }
    .status.ok { color: #0a7a4b; }
    .spin { display: inline-block; width: 12px; height: 12px; margin-inline-end: 8px; vertical-align: -1px;
      border: 2px solid #c9d8ea; border-top-color: #006bd6; border-radius: 50%; animation: spin 0.8s linear infinite; }
    @keyframes spin { to { transform: rotate(360deg); } }
    @media (prefers-reduced-motion: reduce) { .spin { animation: none; } }
  `
  const ICON =
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" ' +
    'stroke-linejoin="round" aria-hidden="true"><path d="M12 4v11"/><path d="m7 10 5 5 5-5"/><path d="M5 20h14"/></svg>'

  let enabled = true
  let host = null
  let shadow = null
  let pill = null
  let panel = null
  let target = null // video đang gắn nút
  let anchor = null // chỗ đặt bảng khi mở bằng chuột phải (không có video)
  let hideTimer = 0
  let frame = 0
  let lastContext = null
  const actions = new WeakMap() // nút trong bảng ⇒ việc khi bấm (chạy từ bộ nghe chung bên dưới)
  const formats = new Map() // url ⇒ Promise trả lời của nền (gộp yêu cầu trùng, nhớ trong trang)
  const prefetched = new Set() // link đã đọc sẵn (một lần, kể cả khi lỗi)

  function build() {
    if (host) return
    host = document.createElement("utd-video-button")
    host.style.cssText = "all: initial; position: fixed; inset: 0 auto auto 0; width: 0; height: 0; z-index: 2147483647; pointer-events: none;"
    shadow = host.attachShadow({ mode: "open" })
    try {
      const sheet = new CSSStyleSheet()
      sheet.replaceSync(CSS)
      shadow.adoptedStyleSheets = [sheet]
    } catch {
      const style = document.createElement("style")
      style.textContent = CSS
      shadow.append(style)
    }
    pill = document.createElement("button")
    pill.className = "pill"
    pill.dataset.s = "S-74"
    pill.type = "button"
    pill.hidden = true
    pill.innerHTML = ICON
    const label = document.createElement("span")
    label.textContent = t("videoButton")
    pill.append(label)
    actions.set(pill, () => {
      if (panel && !panel.hidden) close()
      else void open(analysisUrl(target), sourceOf(target))
    })
    panel = document.createElement("div")
    panel.className = "panel"
    panel.dataset.s = "S-75"
    panel.hidden = true
    panel.setAttribute("role", "dialog")
    panel.setAttribute("aria-label", t("videoButton"))
    shadow.append(pill, panel)
    host.addEventListener("pointerenter", () => clearTimeout(hideTimer))
    host.addEventListener("pointerleave", () => scheduleHide())
    document.documentElement.append(host)
  }

  /** Video lớn, đang hiện trong khung nhìn. */
  function usable(video) {
    if (!video.isConnected) return false
    const rect = video.getBoundingClientRect()
    return (
      rect.width >= MIN_WIDTH &&
      rect.height >= MIN_HEIGHT &&
      rect.bottom > 0 &&
      rect.right > 0 &&
      rect.top < innerHeight &&
      rect.left < innerWidth &&
      getComputedStyle(video).visibility !== "hidden"
    )
  }

  function videoAt(x, y) {
    for (const video of document.getElementsByTagName("video")) {
      const rect = video.getBoundingClientRect()
      if (x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom && usable(video)) return video
    }
    return null
  }

  function sourceOf(video) {
    const src = video?.currentSrc || video?.src || video?.querySelector("source")?.src || ""
    return /^https?:\/\//i.test(src) ? src : null
  }

  /**
   * Link cho yt-dlp: link riêng của `video` nếu module của trang tìm được (tiktok.js: feed, lưới hồ sơ — địa chỉ trang
   * không phải link video), không thì trang đang xem (yt-dlp đọc được trang video quen và nhiều trang nhúng video).
   */
  function analysisUrl(video) {
    for (const site of globalThis.__utdVideoSites ?? []) {
      if (site.hosts.test(location.hostname)) return (video && site.videoUrl(video)) || location.href
    }
    return location.href
  }

  function show(video, duration) {
    if (!enabled || document.fullscreenElement) return
    if (YOUTUBE.test(location.hostname) && !YOUTUBE_VIDEO.test(location.pathname)) return
    build()
    target = video
    anchor = null
    pill.hidden = false
    place()
    clearTimeout(hideTimer)
    if (duration) hideTimer = setTimeout(hide, duration)
    if (!frame) frame = requestAnimationFrame(follow)
    const url = analysisUrl(video)
    // Chỉ đọc sẵn video của chính trang: rê qua lưới video (hồ sơ TikTok…) không chạy yt-dlp cho từng ô.
    if (KNOWN.test(location.hostname) && !FACEBOOK.test(location.hostname) && url === location.href && !prefetched.has(url)) {
      prefetched.add(url)
      void formatsFor(url, sourceOf(video), false)
    }
  }

  function scheduleHide() {
    clearTimeout(hideTimer)
    hideTimer = setTimeout(hide, HIDE_AFTER)
  }

  function hide() {
    if (!pill || (panel && !panel.hidden)) return
    pill.hidden = true
    target = null
  }

  /** Bám theo video khi trang cuộn / đổi cỡ, chỉ chạy khi nút hoặc bảng đang hiện. */
  function follow() {
    frame = 0
    if (!pill || (pill.hidden && panel.hidden)) return
    if (target && !usable(target) && panel.hidden) {
      hide()
      return
    }
    place()
    frame = requestAnimationFrame(follow)
  }

  /** Nút nằm ngay trên mép trên-phải video (không đè điều khiển của trình phát); hết chỗ thì vào trong góc. */
  function place() {
    const rect = target ? target.getBoundingClientRect() : anchor
    if (!rect) return
    const width = pill.hidden ? 0 : pill.offsetWidth
    let top = rect.top - PILL_HEIGHT - GAP
    if (top < GAP) top = Math.max(GAP, rect.top + GAP)
    const left = Math.min(Math.max(GAP, rect.right - width - (top > rect.top ? GAP : 0)), innerWidth - width - GAP)
    pill.style.top = `${top}px`
    pill.style.left = `${left}px`
    if (!panel.hidden) {
      const below = top + PILL_HEIGHT + 4
      const height = panel.offsetHeight
      const panelTop = below + height > innerHeight - GAP ? Math.max(GAP, top - height - 4) : below
      const right = pill.hidden ? rect.right : left + width
      panel.style.top = `${panelTop}px`
      panel.style.left = `${Math.min(Math.max(GAP, right - panel.offsetWidth), innerWidth - panel.offsetWidth - GAP)}px`
    }
  }

  /** Hỏi nền định dạng của `url`; `signIn` = cú bấm thật, nền được đọc cookie nếu nguồn đòi đăng nhập. */
  function formatsFor(url, src, signIn) {
    const key = `${signIn ? "1" : "0"} ${url}`
    let request = formats.get(key)
    if (!request) {
      request = api.runtime
        .sendMessage({ type: "utd.video-formats", url, src, signIn })
        .catch(() => ({ ok: false, message: t("notConnected") }))
        .then((reply) => {
          if (!reply?.ok) formats.delete(key)
          return reply ?? { ok: false }
        })
      formats.set(key, request)
    }
    return request
  }

  function status(text, kind, spinning) {
    const line = document.createElement("div")
    line.className = `status ${kind ?? ""}`
    if (spinning) {
      const spin = document.createElement("span")
      spin.className = "spin"
      line.append(spin)
    }
    line.append(document.createTextNode(text))
    return line
  }

  function header(text) {
    const head = document.createElement("div")
    head.className = "head"
    const title = document.createElement("span")
    title.className = "title"
    title.textContent = text || t("videoButton")
    title.title = title.textContent
    const button = document.createElement("button")
    button.className = "close"
    button.type = "button"
    button.textContent = "×"
    button.setAttribute("aria-label", t("videoClose"))
    actions.set(button, close)
    head.append(title, button)
    return head
  }

  function render(...children) {
    panel.replaceChildren(...children)
    place()
  }

  /**
   * Luồng của video đang xem đọc thẳng trong trang (nền chạy page-media.js): `{ title, formats, manifests, files }`,
   * không đọc được thì `null`. Điểm gửi đi là giữa video đang gắn nút (hoặc chỗ chuột phải) để chọn đúng video.
   */
  function pageMedia(point) {
    const rect = target?.getBoundingClientRect()
    const at = rect ? { x: (rect.left + rect.right) / 2, y: (rect.top + rect.bottom) / 2 } : point
    const asked = api.runtime
      .sendMessage({ type: "utd.page-media", x: at?.x ?? null, y: at?.y ?? null })
      .then((reply) => (reply?.ok ? reply.media : null))
      .catch(() => null)
    return Promise.race([asked, new Promise((resolve) => setTimeout(() => resolve(null), PAGE_MEDIA_TIMEOUT))])
  }

  function showFormats(media, url) {
    const rows = (media.formats ?? []).map((format) => row(format, media, url))
    render(header(media.title), ...rows)
    rows[0]?.focus()
  }

  async function open(url, src, at, point) {
    build()
    if (at) {
      target = null
      anchor = at
    }
    panel.hidden = false
    render(header(null), status(t("videoReading"), null, true))
    if (!frame) frame = requestAnimationFrame(follow)
    // Chỉ đọc trong trang khi bảng là của chính trang đang mở (không phải link chuột phải trỏ đi nơi khác).
    const here = url === location.href
    const facebook = FACEBOOK.test(location.hostname)
    let found = here && facebook ? await pageMedia(point) : null
    if (panel.hidden) return
    if (found?.formats.length) return showFormats(found, url)
    let reply = await formatsFor(url, src, false)
    if (!reply.ok && SIGN_IN.test(reply.code ?? "")) {
      render(header(null), status(t("videoReading"), null, true))
      reply = await formatsFor(url, src, true)
    }
    if (panel.hidden) return
    // yt-dlp không đọc được trang (UTD có chạy): bắt luồng media trang đã tải, kiểu IDM.
    if (!reply.ok && reply.code && here) {
      found ??= await pageMedia(point)
      if (panel.hidden) return
      if (found?.formats.length) return showFormats(found, url)
      const manifest = found?.manifests[0]
      const file = found?.files[0]
      if (manifest) {
        const viaManifest = await formatsFor(manifest, null, false)
        if (viaManifest.ok) reply = viaManifest
      } else if (file) {
        reply = { ok: true, media: { title: found.title, webpageUrl: url, formats: [{ id: "file", direct: file }] } }
      }
      if (panel.hidden) return
    }
    if (!reply.ok) {
      const signIn = SIGN_IN.test(reply.code ?? "")
      // Mã lỗi của UTD (yt-dlp không đọc được…) ⇒ câu chung — Facebook: chưa thấy luồng nào, cho video phát rồi bấm
      // lại; không mã (UTD chưa chạy…) ⇒ câu của nền.
      const failed = facebook && here ? t("videoNoStream") : t("videoFailed")
      const text = signIn && !facebook ? t("videoSignIn") : reply.code ? failed : reply.message || failed
      render(header(null), status(text, "error"))
      return
    }
    showFormats(reply.media, url)
  }

  function label(format) {
    if (format.id === "best") return t("videoBest")
    if (format.id === "audio-m4a") return t("videoAudio")
    if (format.id === "audio-mp3") return "MP3"
    if (format.id === "file" || format.id === "stream") return t("videoFile")
    if (format.id === "hd" || format.id === "sd") return format.id.toUpperCase()
    return /^\d+$/.test(format.id) ? `${format.id}p` : format.id
  }

  function size(bytes) {
    if (!Number.isFinite(bytes) || bytes <= 0) return ""
    const units = ["KB", "MB", "GB", "TB"]
    let value = bytes / 1024
    let unit = 0
    while (value >= 1024 && unit < units.length - 1) {
      value /= 1024
      unit += 1
    }
    const number = new Intl.NumberFormat(api.i18n.getUILanguage(), {
      maximumFractionDigits: value < 10 ? 1 : 0,
    }).format(value)
    return `≈ ${number} ${units[unit]}`
  }

  function row(format, media, url) {
    const button = document.createElement("button")
    // Dòng âm thanh đầu tiên có vạch ngăn với các dòng video (như IDM).
    button.className = format.id === "audio-m4a" ? "row audio" : "row"
    button.type = "button"
    const name = document.createElement("span")
    name.textContent = label(format)
    const weight = document.createElement("span")
    weight.className = "size"
    weight.textContent = size(format.sizeBytes)
    button.append(name, weight)
    actions.set(button, () => void pick(format, media, url))
    return button
  }

  async function pick(format, media, url) {
    for (const button of panel.querySelectorAll(".row")) button.disabled = true
    panel.append(status(t("videoAdding"), null, true))
    place()
    // Dòng lấy từ trang (`streams`): UTD tải các luồng đó; dòng của yt-dlp: tải theo selector.
    const message = format.streams
      ? { type: "utd.video-streams", title: media.title, streams: format.streams }
      : format.direct
      ? { type: "utd.video-file", url: format.direct }
      : {
          type: "utd.video-download",
          url: media.webpageUrl || url,
          title: media.title,
          selector: format.selector,
          audioFormat: format.audioFormat ?? null,
          signIn: Boolean(media.signed),
        }
    const reply = await api.runtime.sendMessage(message).catch(() => null)
    if (panel.hidden) return
    if (reply?.ok) {
      render(header(media.title), status(t("videoAdded"), "ok"))
      setTimeout(close, 1200)
    } else {
      render(header(media.title), status(reply?.message || t("notConnected"), "error"))
    }
  }

  function close() {
    if (!panel) return
    panel.hidden = true
    panel.replaceChildren()
    anchor = null
    scheduleHide()
  }

  // Dò chuột theo khung của video: trình phát hay phủ lớp lên video nên sự kiện trên chính video không đủ.
  let moved = 0
  addEventListener(
    "pointermove",
    (event) => {
      if (!enabled || moved) return
      moved = requestAnimationFrame(() => {
        moved = 0
        if (host && event.composedPath().includes(host)) return
        const video = videoAt(event.clientX, event.clientY)
        if (video) show(video)
        else if (pill && !pill.hidden) scheduleHide()
      })
    },
    { capture: true, passive: true },
  )
  // Video vừa phát: hiện nút một lúc như IDM.
  addEventListener(
    "play",
    (event) => {
      if (event.target instanceof HTMLVideoElement && usable(event.target)) show(event.target, SHOW_ON_PLAY)
    },
    true,
  )
  addEventListener("fullscreenchange", () => {
    if (document.fullscreenElement && pill) {
      close()
      pill.hidden = true
    }
  })
  // Esc ở đâu trên trang cũng đóng bảng đang mở.
  addEventListener(
    "keydown",
    (event) => {
      if (event.key === "Escape" && panel && !panel.hidden) close()
    },
    true,
  )
  // Trang như Facebook Tin chặn mọi cú bấm ngay ở window (chạm là dừng / sang tin khác) nên cú bấm không tới được nút.
  // Vì vậy nghe ở window, pha bắt, từ document_start (manifest) để chạy trước trang: bấm vào UTD thì chạy việc của
  // nút và không cho trang thấy cú bấm; bấm ra ngoài bảng thì đóng bảng.
  for (const type of ["pointerdown", "pointerup", "mousedown", "mouseup", "touchstart", "touchend", "click", "dblclick"]) {
    addEventListener(
      type,
      (event) => {
        const path = event.composedPath()
        if (!host || !path.includes(host)) {
          if (type === "pointerdown" && panel && !panel.hidden) close()
          return
        }
        event.stopImmediatePropagation()
        if (type !== "click") return
        event.preventDefault()
        if (!event.isTrusted) return
        const button = path.find((node) => actions.has(node))
        if (button && !button.disabled) actions.get(button)()
      },
      true,
    )
  }
  // Chuột phải: nhớ chỗ bấm để bảng mở đúng đó khi chọn mục UTD trong menu.
  addEventListener(
    "contextmenu",
    (event) => {
      const element = event.target instanceof Element ? event.target : null
      const video = element?.closest("video") ?? videoAt(event.clientX, event.clientY)
      lastContext = { video, x: event.clientX, y: event.clientY }
    },
    true,
  )
  // SPA đổi trang (YouTube…): bảng của trang cũ thì đóng.
  let lastUrl = location.href
  setInterval(() => {
    if (location.href === lastUrl) return
    lastUrl = location.href
    close()
  }, 1000)

  api.storage.local.get("videoButton").then(
    (stored) => {
      enabled = stored.videoButton !== false
    },
    () => undefined,
  )
  api.storage.onChanged?.addListener((changes, area) => {
    if (area !== "local" || !changes.videoButton) return
    enabled = changes.videoButton.newValue !== false
    if (!enabled) {
      close()
      if (pill) pill.hidden = true
    }
  })

  // Mục UTD trong menu chuột phải: mở bảng ở chỗ vừa bấm (video nếu có), cho link đó.
  api.runtime.onMessage.addListener((message, _sender, reply) => {
    if (message?.type !== "utd.open-formats" || typeof message.url !== "string") return false
    const video = lastContext?.video && usable(lastContext.video) ? lastContext.video : null
    const point = lastContext ?? { x: innerWidth / 2, y: 80 }
    build()
    if (video) {
      target = video
      pill.hidden = false
      // Chuột phải lên video: nền gửi trang đang mở; module của trang (tiktok.js) có thể biết link riêng của video.
      void open(message.url === location.href ? analysisUrl(video) : message.url, sourceOf(video))
    } else {
      pill.hidden = true
      void open(message.url, null, { top: point.y + PILL_HEIGHT, right: point.x + 136, bottom: point.y, left: point.x }, point)
    }
    reply({ ok: true })
    return false
  })
})()
