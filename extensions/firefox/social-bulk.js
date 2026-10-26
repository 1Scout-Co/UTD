// Tải hàng loạt ảnh/video từ Facebook, Instagram, Threads bằng UTD (notes/social-bulk.md). Chạy trong nền (Chromium
// nạp bằng importScripts, Firefox nạp sau background.js theo manifest). Tên ở đây đều có tiền tố social/SOCIAL để không
// đụng tên của background.js; dùng chung `api`, `t`, `sendNative` của nó.
//
// Người dùng bấm "Tải hàng loạt bằng UTD" trên trang cá nhân / fanpage / nhóm / Tin. Mọi yêu cầu tới mạng xã hội chạy
// NGAY TRONG TAB đó (scripting.executeScript) bằng phiên đăng nhập sẵn có của trình duyệt — tiện ích không đọc, không
// gửi cookie. Danh sách đi sang UTD từng đợt (`social-list`); mỗi đợt UTD trả lời đi tiếp hay dừng (nút Dừng, đóng hộp)
// và loại nào còn cần lấy. Giữa hai yêu cầu luôn nghỉ ngẫu nhiên 2–4 s và không quá 70 yêu cầu / 10 phút để tránh bị
// khoá tạm vì gọi nhiều.
//   - Instagram: API nội bộ của chính trang web (`/api/v1/…`, như gallery-dl), đọc thẳng, không cần cuộn.
//   - Facebook, Threads: tiện ích tự cuộn trang (Facebook: lần lượt thẻ Ảnh / Video / Reels / Tin nổi bật), đọc ảnh/video
//     trong dữ liệu JSON trang tự nạp (social-harvest.js). Không phụ thuộc mã truy vấn GraphQL hay đổi.

const SOCIAL_PART_ENTRIES = 150
const SOCIAL_PART_CHARS = 600_000
const SOCIAL_FLUSH_MS = 3000
const SOCIAL_MAX_ENTRIES = 20_000
// Nhịp gọi: nghỉ 2–4 s giữa hai yêu cầu, tối đa 70 yêu cầu trong 10 phút (instaloader: 75 / 11 phút).
const SOCIAL_GAP_MS = 2000
const SOCIAL_JITTER_MS = 2000
const SOCIAL_WINDOW_MS = 10 * 60_000
const SOCIAL_WINDOW_CALLS = 70
// Cuộn: hết mục mới sau chừng này lượt liên tiếp thì coi như hết danh sách.
const SOCIAL_IDLE_ROUNDS = 5
// Ảnh Facebook trong lưới nhỏ hơn chừng này (cạnh dài) ⇒ mở trang ảnh lấy bản gốc.
const SOCIAL_FULL_SIZE = 900
const SOCIAL_IG_APP_ID = "936619743392459"
const SOCIAL_IG_RESERVED = new Set(["p", "reel", "reels", "tv", "stories", "explore", "accounts", "direct", "about", "legal", "developer", "web", "api", "challenge", "emails", "session"])
const SOCIAL_FB_RESERVED = new Set([
  "watch", "reel", "reels", "photo", "photo.php", "photos", "videos", "video.php", "marketplace", "gaming", "events", "search",
  "notifications", "messages", "friends", "saved", "settings", "help", "login", "login.php", "permalink.php", "story.php", "share",
  "hashtag", "bookmarks", "ads", "business", "pages", "groups", "stories", "home.php", "memories", "privacy", "policies", "dialog",
])

const socialJobs = new Map() // tab ⇒ lượt đang chạy

/** Việc cần làm cho trang `url`, hoặc `null` khi không phải trang hồ sơ / fanpage / nhóm / Tin. */
function socialPlan(url) {
  let page
  try {
    page = new URL(url)
  } catch {
    return null
  }
  const host = page.hostname.toLowerCase()
  const parts = page.pathname.split("/").filter(Boolean)
  if (/(^|\.)instagram\.com$/.test(host)) {
    const user = parts[0] === "stories" ? parts[1] : parts[0]
    const name = /^[\w.]{1,30}$/.test(user ?? "") ? user : null
    if (!name || name === "highlights" || (parts[0] !== "stories" && SOCIAL_IG_RESERVED.has(name))) return null
    return { platform: "instagram", user: name, id: name }
  }
  if (/(^|\.)threads\.(net|com)$/.test(host)) {
    const user = parts[0]?.startsWith("@") ? parts[0].slice(1) : null
    if (!user || !/^[\w.]{1,30}$/.test(user)) return null
    return { platform: "threads", user, id: user, base: `${page.origin}/@${user}` }
  }
  if (!/(^|\.)facebook\.com$/.test(host)) return null
  if (parts[0] === "stories" && /^\d+$/.test(parts[1] ?? "")) {
    const highlight = /highlight/i.test(page.search)
    return { platform: "facebook", kind: "story", id: parts[1], bucket: parts[1], highlight }
  }
  if (parts[0] === "groups" && /^[\w.-]{1,100}$/.test(parts[1] ?? "")) {
    const base = `${page.origin}/groups/${parts[1]}`
    return { platform: "facebook", kind: "group", id: `groups.${parts[1]}`, base, photos: `${base}/media/photos`, videos: `${base}/media/videos` }
  }
  let base = null
  let id = null
  let tab = null
  const numeric = page.searchParams.get("id")
  if (parts[0] === "profile.php" && /^\d+$/.test(numeric ?? "")) {
    id = numeric
  } else if (parts[0] === "people" && /^\d+$/.test(parts[2] ?? "")) {
    id = parts[2]
  } else if (parts[0] && /^[\w.-]{1,100}$/.test(parts[0]) && !SOCIAL_FB_RESERVED.has(parts[0].toLowerCase())) {
    id = parts[0]
    base = `${page.origin}/${parts[0]}`
    tab = (name) => `${base}/${name}`
  }
  if (!id) return null
  if (!base) {
    base = `${page.origin}/profile.php?id=${id}`
    tab = (name) => `${base}&sk=${name === "reels" ? "reels_tab" : name}`
  }
  return { platform: "facebook", kind: "profile", id, base, photos: tab("photos_by"), videos: tab("videos"), reels: tab("reels") }
}

const socialSleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
// Bài thử e2e đặt `__utdSocialFast` để khỏi chờ nhịp gọi thật (vẫn nhường lượt cho trang).
const socialDelay = (ms) => (globalThis.__utdSocialFast ? 30 : ms)

/** Một lượt tải hàng loạt trên một tab. */
class SocialJob {
  constructor(tab, plan) {
    this.tabId = tab.id
    this.pageUrl = tab.url
    this.plan = plan
    this.listingId = `sb-${crypto.randomUUID().replace(/-/g, "").slice(0, 24)}`
    this.part = 0
    this.pending = []
    this.sent = new Set()
    this.count = 0
    this.head = null
    this.control = { go: true, sources: null, since: null }
    this.calls = []
    this.flushedAt = Date.now()
    this.world = "MAIN"
    this.stopped = false
    this.old = new Map() // nhóm ⇒ số mục cũ hơn mốc ngày liên tiếp
    // Mã lỗi khi dừng giữa chừng (`social.rate_limited`…), và câu lỗi khi không nối được UTD.
    this.failure = null
    this.hostError = null
  }

  /** Còn lấy nhóm `source` không (người dùng chưa Dừng, chưa bỏ chọn nhóm đó). */
  wants(source) {
    if (this.stopped || !this.control.go || this.count >= SOCIAL_MAX_ENTRIES) return false
    return !Array.isArray(this.control.sources) || this.control.sources.includes(source)
  }

  /** Đã đi qua mốc "từ ngày" của nhóm `source` (danh sách mới trước). */
  past(source) {
    return (this.old.get(source) ?? 0) >= 12
  }

  /** Chạy một hàm trong tab (thế giới của trang nếu được). */
  async run(func, args = [], world = this.world) {
    const target = { tabId: this.tabId, frameIds: [0] }
    const options = world ? { target, world, func, args } : { target, func, args }
    const [result] = await api.scripting.executeScript(options)
    return result?.result
  }

  /** Nạp social-harvest.js vào trang (sau mỗi lần chuyển trang). Trình duyệt không cho thế giới MAIN ⇒ content script. */
  async inject() {
    for (const world of this.world === "MAIN" ? ["MAIN", null] : [null]) {
      try {
        const target = { tabId: this.tabId, frameIds: [0] }
        await api.scripting.executeScript(world ? { target, world, files: ["social-harvest.js"] } : { target, files: ["social-harvest.js"] })
        this.world = world
        return true
      } catch {
        // Thử cách sau.
      }
    }
    return false
  }

  /** Nghỉ trước một yêu cầu tới mạng xã hội: 2–4 s, và không quá 70 yêu cầu / 10 phút. */
  async pace() {
    const now = Date.now()
    this.calls = this.calls.filter((at) => now - at < SOCIAL_WINDOW_MS)
    if (this.calls.length >= SOCIAL_WINDOW_CALLS) {
      const wait = SOCIAL_WINDOW_MS - (now - this.calls[0])
      await this.wait(socialDelay(wait))
    }
    if (this.calls.length > 0) await this.wait(socialDelay(SOCIAL_GAP_MS + Math.random() * SOCIAL_JITTER_MS))
    this.calls.push(Date.now())
  }

  /** Nghỉ `ms`, vẫn cập nhật nhãn trên trang và gửi đợt đến hạn (để biết người dùng có Dừng không). */
  async wait(ms) {
    const until = Date.now() + ms
    while (!this.stopped && Date.now() < until) {
      await socialSleep(Math.min(1500, until - Date.now()))
      await this.pill()
      if (Date.now() - this.flushedAt >= SOCIAL_FLUSH_MS * 3) await this.flush()
    }
  }

  async pill(final = null) {
    const text = final ?? t("socialCollecting", [String(this.count)])
    try {
      const stop = await this.run(socialPill, [text, t("socialStop"), t("socialHint"), Boolean(final)], null)
      if (stop === true && !final) this.stopped = true
    } catch {
      // Tab đang chuyển trang: lần sau vẽ lại.
    }
  }

  /** Mục từ trang ⇒ dòng gửi UTD (UTD kiểm lại kỹ). */
  add(items, source, album = null) {
    for (const item of items) {
      if (!item?.id || this.sent.has(item.id) || this.count >= SOCIAL_MAX_ENTRIES) continue
      if (this.control.since && item.timestamp) {
        const older = item.timestamp < this.control.since
        this.old.set(source, older ? (this.old.get(source) ?? 0) + 1 : 0)
        if (older) continue
      }
      this.sent.add(item.id)
      this.count += 1
      this.pending.push({
        id: String(item.id).slice(0, 128),
        source,
        kind: item.kind === "video" ? "video" : "photo",
        url: item.url ?? null,
        page: item.page ?? null,
        thumb: item.thumb ?? null,
        timestamp: item.timestamp ?? null,
        title: item.title ?? "",
        album: album ?? null,
        durationSeconds: item.durationSeconds ?? null,
      })
    }
  }

  /** Gửi phần đang chờ sang UTD (từng đợt ≤ 150 mục); `done` = đợt cuối. Trả lời của UTD cập nhật `control`. */
  async flush(done = null) {
    do {
      const entries = []
      let chars = 0
      while (this.pending.length > 0 && entries.length < SOCIAL_PART_ENTRIES) {
        const size = JSON.stringify(this.pending[0]).length
        if (entries.length > 0 && chars + size > SOCIAL_PART_CHARS) break
        chars += size
        entries.push(this.pending.shift())
      }
      const last = done && this.pending.length === 0
      const social = { listingId: this.listingId, part: this.part, entries }
      if (this.part === 0 && this.head) social.head = this.head
      if (last) social.done = done
      const reply = await sendNative({ version: 2, type: "social-list", url: this.pageUrl, social })
      this.part += 1
      this.flushedAt = Date.now()
      if (!reply || reply.ok !== true) {
        this.hostError = reply?.message ? fromHost(reply.message) : t("notConnected")
        this.stopped = true
        return
      }
      const control = reply.media ?? {}
      this.control = {
        go: control.go === true,
        sources: Array.isArray(control.sources) ? control.sources : null,
        since: Number.isFinite(control.since) ? control.since : null,
      }
    } while (done && this.pending.length > 0)
  }

  /** Thêm rồi gửi khi đủ đợt hoặc đến hạn. */
  async push(items, source, album) {
    this.add(items, source, album)
    if (this.pending.length >= SOCIAL_PART_ENTRIES || Date.now() - this.flushedAt >= SOCIAL_FLUSH_MS) await this.flush()
  }

  /** Mở `url` trong tab (nếu chưa ở đó) rồi nạp lại social-harvest.js. */
  async open(url) {
    const tab = await api.tabs.get(this.tabId)
    const same = (a, b) => a.replace(/[#?].*$/, "").replace(/\/$/, "") === b.replace(/[#?].*$/, "").replace(/\/$/, "")
    if (!same(tab.url ?? "", url) || new URL(url).search !== new URL(tab.url ?? url).search) {
      await this.pace()
      const tabId = this.tabId
      await new Promise((resolve) => {
        let timer = 0
        const listen = (id, change) => {
          if (id === tabId && change.status === "complete") done()
        }
        const done = () => {
          clearTimeout(timer)
          api.tabs.onUpdated.removeListener(listen)
          resolve()
        }
        timer = setTimeout(done, 30_000)
        api.tabs.onUpdated.addListener(listen)
        api.tabs.update(tabId, { url }).catch(done)
      })
      // Trang Comet còn vẽ sau "complete".
      await socialSleep(socialDelay(1500))
    }
    return this.inject()
  }

  /** Cuộn trang và đọc mục mới tới khi hết (hoặc người dùng dừng / bỏ chọn nhóm). */
  async scroll(source, keep, { rounds = Infinity, album = null } = {}) {
    let idle = 0
    let round = 0
    while (this.wants(source) && !this.past(source) && idle < SOCIAL_IDLE_ROUNDS && round < rounds) {
      round += 1
      const got = await this.run(socialTake).catch(() => null)
      if (!got) {
        this.failure = this.failure ?? "social.changed"
        return
      }
      const fresh = (got.items ?? []).filter((item) => !this.sent.has(item.id) && keep(item))
      idle = fresh.length > 0 ? 0 : idle + 1
      await this.push(await this.full(fresh), source, album)
      await this.pill()
      if (round < rounds) {
        // Mỗi lần cuộn trang thường tự gọi một yêu cầu nạp thêm: tính vào nhịp gọi.
        await this.run(socialScroll, [], null).catch(() => undefined)
        await this.pace()
      }
    }
  }

  /**
   * Facebook: ảnh trong lưới là ảnh thu nhỏ ⇒ mở trang ảnh lấy bản gốc (như gallery-dl). Không đọc được bản gốc thì giữ
   * ảnh lưới; bị chặn tạm / cần đăng nhập thì dừng, bỏ các ảnh chưa lấy được bản gốc. Trả các mục giữ lại.
   */
  async full(items) {
    if (this.plan.platform !== "facebook") return items
    const kept = []
    for (const item of items) {
      const side = Math.max(item.width ?? 0, item.height ?? 0)
      const small = item.kind === "photo" && side < SOCIAL_FULL_SIZE && !(side > 0 && (item.fullWidth ?? 0) > 0 && item.fullWidth <= side)
      if (small) {
        if (this.stopped || !this.control.go) break
        await this.pace()
        const found = await this.run(socialFacebookPhoto, [item.id], null).catch(() => null)
        if (found?.code) {
          this.failure = found.code
          this.stopped = true
          break
        }
        if (found?.url) Object.assign(item, { url: found.url, width: found.width, height: found.height })
      }
      kept.push(item)
    }
    return kept
  }

  /** Instagram: một lần gọi API của trang (đọc ảnh/video ngay trong trang). */
  async instagram(path, form = null) {
    await this.pace()
    const reply = await this.run(socialInstagramCall, [path, form, SOCIAL_IG_APP_ID]).catch(() => null)
    if (!reply) throw new Error("social.changed")
    const code = socialInstagramError(reply)
    if (code) {
      // Để chẩn đoán (trang service worker của tiện ích): mã HTTP và câu báo của Instagram, không có dữ liệu hồ sơ.
      console.warn("UTD social", code, path.split("?")[0], reply.status, reply.meta?.message ?? "")
      throw new Error(code)
    }
    return reply
  }
}

/**
 * Lỗi của một câu trả lời Instagram (mã cố định, UTD đổi sang câu của app). Chỉ đọc các trường báo lỗi ở cấp ngoài: chú
 * thích bài / tiểu sử trong câu trả lời có thể chứa chữ "spam", "wait a few minutes"…
 */
function socialInstagramError(reply) {
  const meta = reply.meta && typeof reply.meta === "object" ? reply.meta : {}
  const text = (value) => (typeof value === "string" ? value.toLowerCase() : "")
  const message = `${text(meta.message)} ${text(meta.error_type)}`
  const failed = meta.status === "fail"
  if (reply.status === 429 || meta.spam === true || (failed && /wait a few minutes|checkpoint_required|challenge_required|feedback_required|spam/.test(message))) return "social.rate_limited"
  if (reply.status === 401 || reply.status === 403 || /\/accounts\/login|\/challenge/.test(reply.url ?? "") || (failed && (meta.require_login === true || /login_required/.test(message)))) return "social.login"
  if (reply.status === 404) return "social.not_found"
  if (reply.status !== 200 || !reply.meta) return "social.changed"
  return null
}

async function socialInstagram(job) {
  const { user } = job.plan
  const info = await job.instagram(`/api/v1/users/web_profile_info/?username=${encodeURIComponent(user)}`)
  const profile = info.meta?.data?.user
  if (!profile?.id) throw new Error("social.not_found")
  const id = String(profile.id)
  job.head = {
    id,
    name: profile.full_name || user,
    handle: user,
    avatar: profile.profile_pic_url_hd || profile.profile_pic_url || null,
    followers: Number(profile.edge_followed_by?.count) || null,
    private: Boolean(profile.is_private && !profile.followed_by_viewer),
  }
  await job.flush()
  const own = (item) => !item.owner || item.owner.toLowerCase() === user.toLowerCase()
  // Tin hết hạn sau 24 giờ: lấy trước.
  if (job.wants("stories")) {
    const reply = await job.instagram(`/api/v1/feed/reels_media/?reel_ids=${id}`)
    await job.push(reply.items.map((item) => ({ ...item, page: null })), "stories")
  }
  if (job.wants("highlights")) {
    const tray = await job.instagram(`/api/v1/highlights/${id}/highlights_tray/`)
    const highlights = (Array.isArray(tray.meta?.tray) ? tray.meta.tray : []).filter((item) => /^highlight:\d+$/.test(item?.id ?? ""))
    for (let index = 0; index < highlights.length && job.wants("highlights"); index += 5) {
      const chunk = highlights.slice(index, index + 5)
      const query = chunk.map((item) => `reel_ids=${encodeURIComponent(item.id)}`).join("&")
      const reply = await job.instagram(`/api/v1/feed/reels_media/?${query}`)
      // Mỗi tin nổi bật một tên (album): đọc lại từng tin trong trả lời.
      for (const item of chunk) {
        const ids = new Set((reply.meta?.reels?.[item.id]?.itemIds ?? []).map(String))
        const title = typeof item.title === "string" ? item.title.slice(0, 100) : null
        await job.push(reply.items.filter((media) => ids.has(String(media.id))).map((media) => ({ ...media, page: null })), "highlights", title)
      }
    }
  }
  if (job.wants("reels")) {
    let cursor = ""
    do {
      const form = { target_user_id: id, page_size: "50", include_feed_video: "true" }
      if (cursor) form.max_id = cursor
      const reply = await job.instagram("/api/v1/clips/user/", form)
      await job.push(reply.items.filter(own), "reels")
      const paging = reply.meta?.paging_info ?? {}
      cursor = paging.more_available ? String(paging.max_id ?? "") : ""
    } while (cursor && job.wants("reels") && !job.past("reels"))
  }
  if (job.wants("posts")) {
    let cursor = ""
    do {
      const reply = await job.instagram(`/api/v1/feed/user/${id}/?count=30${cursor ? `&max_id=${encodeURIComponent(cursor)}` : ""}`)
      await job.push(reply.items.filter(own), "posts")
      cursor = reply.meta?.more_available ? String(reply.meta?.next_max_id ?? "") : ""
    } while (cursor && job.wants("posts") && !job.past("posts"))
  }
}

async function socialThreads(job) {
  const { user, base } = job.plan
  if (!(await job.open(base))) throw new Error("social.changed")
  const info = await job.run(socialPageInfo, [], null).catch(() => null)
  job.head = { id: user, name: info?.name || user, handle: user, avatar: info?.image ?? null }
  await job.flush()
  // Bài đăng lại / trích dẫn của người khác cũng hiện ở hồ sơ: chỉ giữ bài của chính người này.
  await job.scroll("posts", (item) => item.owner?.toLowerCase() === user.toLowerCase())
}

async function socialFacebook(job) {
  const { plan } = job
  const info = await job.run(socialPageInfo, [], null).catch(() => null)
  job.head = { id: plan.id, name: info?.name || plan.id, handle: null, avatar: info?.image ?? null }
  if (!(await job.inject())) throw new Error("social.changed")
  await job.flush()
  if (plan.kind === "story") {
    // Đang xem Tin / tin nổi bật: đọc bucket đang mở (dữ liệu có sẵn trong trang), không cuộn.
    const source = plan.highlight ? "highlights" : "stories"
    await job.scroll(source, (item) => item.story && (!item.bucket || item.bucket === plan.bucket), { rounds: 3 })
    return
  }
  // Link Tin / tin nổi bật nằm ở trang chính của hồ sơ: đọc trước khi chuyển thẻ.
  const stories = plan.kind === "profile" ? ((await job.run(socialStoryLinks).catch(() => null)) ?? []) : []
  const steps = [
    ["photos", plan.photos, (item) => item.kind === "photo" && !item.story],
    ["videos", plan.videos, (item) => item.kind === "video" && !item.story],
    ["reels", plan.reels, (item) => item.kind === "video" && !item.story],
  ]
  for (const [source, url, keep] of steps) {
    if (!url || !job.wants(source)) continue
    if (!(await job.open(url))) continue
    await job.scroll(source, keep)
  }
  for (const link of stories) {
    const source = link.highlight ? "highlights" : "stories"
    if (!job.wants(source)) continue
    if (!(await job.open(link.url))) continue
    await job.scroll(source, (item) => item.story && (!item.bucket || item.bucket === link.bucket), { rounds: 3, album: link.highlight ? link.title : null })
  }
  // Trả tab về trang người dùng đã mở.
  if (job.pageUrl !== (await api.tabs.get(job.tabId)).url) await api.tabs.update(job.tabId, { url: job.pageUrl }).catch(() => undefined)
}

/** Bấm "Tải bằng UTD" lên chính trang hồ sơ: trang không phải một file/video, nhắc mục Tải hàng loạt. */
async function socialHint(tab) {
  if (!tab?.id || socialJobs.has(tab.id)) return
  await new SocialJob(tab, { platform: "none" }).pill(t("socialUseBulk"))
}

/** Bấm "Tải hàng loạt bằng UTD". Bấm lần nữa khi đang chạy = Dừng. */
async function socialStart(tab) {
  if (!tab?.id) return
  const running = socialJobs.get(tab.id)
  if (running) {
    running.stopped = true
    return
  }
  const plan = socialPlan(tab.url ?? "")
  const job = new SocialJob(tab, plan ?? { platform: "none" })
  if (!plan) {
    await job.pill(t("socialUnsupported"))
    return
  }
  socialJobs.set(tab.id, job)
  let error = null
  try {
    await job.pill()
    if (plan.platform === "instagram") {
      if (!(await job.inject())) throw new Error("social.changed")
      await socialInstagram(job)
    } else if (plan.platform === "threads") {
      await socialThreads(job)
    } else {
      await socialFacebook(job)
    }
  } catch (reason) {
    error = /^social\.[a-z_]+$/.test(reason?.message ?? "") ? reason.message : "social.changed"
  } finally {
    socialJobs.delete(tab.id)
  }
  error = error ?? job.failure
  const complete = !error && !job.stopped && job.control.go
  // UTD chưa chạy / không nối được: báo trên trang, không gửi thêm.
  if (!job.hostError) await job.flush({ complete, error })
  if (job.hostError) await job.pill(job.hostError)
  else await job.pill(job.count === 0 && error ? t("socialFailed") : t("socialDone", [String(job.count)]))
}

// --- Các hàm chạy trong tab (scripting.executeScript): phải tự đủ, không dùng biến ngoài. ---

/** Mục mới mà social-harvest.js gom được từ lần trước. */
function socialTake() {
  const harvest = window[Symbol.for("utd.social-harvest")]
  return harvest ? harvest.take() : null
}

function socialStoryLinks() {
  const harvest = window[Symbol.for("utd.social-harvest")]
  return harvest ? harvest.storyLinks() : []
}

/** Cuộn xuống cuối trang để trang tự nạp thêm. */
function socialScroll() {
  const root = document.scrollingElement ?? document.documentElement
  window.scrollTo(0, root.scrollHeight)
  return root.scrollHeight
}

/** Tên + ảnh đại diện của trang đang mở (thẻ meta Open Graph, tiêu đề tab). */
function socialPageInfo() {
  const meta = (name) => document.querySelector(`meta[property="${name}"]`)?.getAttribute("content") ?? null
  const title = (meta("og:title") ?? document.title ?? "")
    .replace(/^\(\d+\+?\)\s*/, "")
    .replace(/\s*[|•·-]\s*(Facebook|Threads|Instagram)\s*$/i, "")
    .trim()
  const image = meta("og:image")
  return { name: title.slice(0, 200) || null, image: image && /^https:\/\//.test(image) ? image : null }
}

/**
 * Gọi API của trang Instagram bằng phiên đăng nhập của chính trang (cùng nguồn, không cần cookie riêng). Ảnh/video
 * trong câu trả lời đọc bằng social-harvest.js; phần còn lại (con trỏ trang sau, thông tin hồ sơ) trả về gọn.
 */
async function socialInstagramCall(path, form, appId) {
  const harvest = window[Symbol.for("utd.social-harvest")]
  const cookie = (name) => document.cookie.split("; ").find((item) => item.startsWith(`${name}=`))?.slice(name.length + 1) ?? ""
  let claim = "0"
  try {
    claim = sessionStorage.getItem("www-claim-v2") || "0"
  } catch {
    // Không đọc được: dùng "0" như trang lúc mới mở.
  }
  const headers = {
    "X-IG-App-ID": appId,
    "X-ASBD-ID": "129477",
    "X-IG-WWW-Claim": claim,
    "X-CSRFToken": cookie("csrftoken"),
    "X-Requested-With": "XMLHttpRequest",
  }
  const init = { credentials: "include", headers }
  if (form) {
    init.method = "POST"
    init.body = new URLSearchParams(form)
  }
  const request = typeof browser === "object" && typeof content === "object" && content?.fetch ? content.fetch.bind(content) : fetch
  try {
    const response = await request(new URL(path, location.origin).href, init)
    const text = await response.text()
    let data = null
    try {
      data = JSON.parse(text)
    } catch {
      // Không phải JSON (trang đăng nhập, lỗi).
    }
    // Ảnh/video: social-harvest.js đọc trực tiếp; `meta` bỏ phần nặng.
    harvest?.take()
    if (data && harvest) harvest.read(data)
    const items = harvest && data ? harvest.take().items : []
    let meta = null
    if (data) {
      const strip = new Set(["items", "carousel_media", "image_versions2", "video_versions", "media"])
      meta = JSON.parse(text, (key, value) => (strip.has(key) ? undefined : value))
      // Tin nổi bật: id từng tin của mỗi bucket (để gắn tên).
      for (const [reelId, reel] of Object.entries(data.reels ?? {})) {
        if (meta.reels?.[reelId]) meta.reels[reelId].itemIds = (reel.items ?? []).map((item) => String(item.pk ?? item.id))
      }
    }
    return { status: response.status, url: response.url, meta, items }
  } catch {
    return { status: 0, url: "", meta: null, items: [] }
  }
}

/** Facebook: bản gốc của một ảnh từ trang ảnh `/photo/?fbid=` (như gallery-dl), cùng nguồn với trang đang mở. */
async function socialFacebookPhoto(id) {
  const request = typeof browser === "object" && typeof content === "object" && content?.fetch ? content.fetch.bind(content) : fetch
  try {
    const response = await request(`${location.origin}/photo/?fbid=${encodeURIComponent(id)}`, { credentials: "include" })
    if (/\/login/.test(new URL(response.url).pathname)) return { code: "social.login" }
    const html = await response.text()
    if (html.includes("CometErrorRoot.react")) return { code: "social.rate_limited" }
    if (!response.ok) return null
    const pattern = /"image":\{"uri":"((?:[^"\\]|\\.){10,4000})","(width|height)":(\d+),"(width|height)":(\d+)\}/g
    let best = null
    for (let at = html.indexOf(`"${id}"`); at >= 0 && at < html.length; at = html.indexOf(`"${id}"`, at + 1)) {
      const window = html.slice(Math.max(0, at - 3000), at + 12000)
      for (const match of window.matchAll(pattern)) {
        const width = Number(match[2] === "width" ? match[3] : match[5])
        const height = Number(match[2] === "height" ? match[3] : match[5])
        if (!best || width * height > best.width * best.height) best = { raw: match[1], width, height }
      }
      if (best) break
    }
    if (!best) return null
    const url = JSON.parse(`"${best.raw}"`)
    return /^https:\/\/[^/]*fbcdn\.net\//.test(url) ? { url, width: best.width, height: best.height } : null
  } catch {
    return null
  }
}

/** Nhãn nhỏ góc dưới trang: tiến độ + nút Dừng. Trả `true` khi người dùng đã bấm Dừng. */
function socialPill(text, stopLabel, hint, final) {
  const ID = "utd-social-pill"
  let host = document.getElementById(ID)
  if (!host) {
    host = document.createElement("div")
    host.id = ID
    const shadow = host.attachShadow({ mode: "open" })
    shadow.innerHTML = `<style>
      :host { all: initial; }
      .box { position: fixed; z-index: 2147483647; inset-inline-end: 16px; bottom: 16px; display: flex; align-items: center; gap: 8px;
        padding: 6px 6px 6px 12px; border-radius: 8px; color: #0f1b2d; background: #fff; border: 1px solid rgb(0 0 0 / 0.12);
        box-shadow: 0 4px 16px rgb(0 0 0 / 0.18); font: 13px/1.35 Inter, system-ui, -apple-system, "Segoe UI", sans-serif; }
      .dot { width: 8px; height: 8px; border-radius: 50%; background: #0a6fd6; flex: none; }
      .text { white-space: nowrap; font-variant-numeric: tabular-nums; }
      button { border: 0; border-radius: 5px; padding: 4px 10px; color: #fff; background: #0a6fd6; font: inherit; font-weight: 600; cursor: pointer; }
      button:hover { background: #005bb8; }
      button:focus-visible { outline: 2px solid #8ec5ff; outline-offset: 1px; }
      [hidden] { display: none !important; }
    </style><div class="box" role="status"><span class="dot"></span><span class="text"></span><button type="button"></button></div>`
    shadow.querySelector("button").addEventListener("click", (event) => {
      if (event.isTrusted) host.dataset.stop = "1"
    })
    document.documentElement.append(host)
  }
  const shadow = host.shadowRoot
  shadow.querySelector(".text").textContent = `UTD · ${text}`
  shadow.querySelector(".box").title = final ? "" : hint
  const button = shadow.querySelector("button")
  button.textContent = stopLabel
  button.hidden = final || host.dataset.stop === "1"
  clearTimeout(Number(host.dataset.timer) || 0)
  if (final) host.dataset.timer = String(setTimeout(() => host.remove(), 8000))
  return host.dataset.stop === "1"
}
