// Tải hàng loạt ảnh/video từ Facebook, Instagram, Threads bằng UTD (notes/social-bulk.md). Chạy trong nền (Chromium
// nạp bằng importScripts, Firefox nạp sau background.js theo manifest). Tên ở đây đều có tiền tố social/SOCIAL để không
// đụng tên của background.js; dùng chung `api`, `t`, `sendNative` của nó.
//
// Người dùng bấm "Tải hàng loạt bằng UTD" trên trang cá nhân / fanpage / nhóm / Tin. Mọi yêu cầu tới mạng xã hội chạy
// NGAY TRONG TAB đó (scripting.executeScript) bằng phiên đăng nhập sẵn có của trình duyệt — tiện ích không đọc, không
// gửi cookie. Danh sách đi sang UTD từng đợt (`social-list`); mỗi đợt UTD trả lời đi tiếp hay dừng (nút Dừng, đóng hộp)
// và loại nào còn cần lấy. Cả ba mạng: truy vấn GraphQL của chính trang web (doc_id + biến như Social Bulk Downloader
// 1.8.8), đọc thẳng, không cuộn, không chuyển trang. Giữa hai yêu cầu luôn nghỉ ngẫu nhiên (Facebook 0,3–0,6 s;
// Instagram / Threads 1–2 s và không quá 200 yêu cầu / 10 phút) để tránh bị khoá tạm vì gọi nhiều.

const SOCIAL_PART_ENTRIES = 150
const SOCIAL_PART_CHARS = 600_000
const SOCIAL_FLUSH_MS = 3000
const SOCIAL_MAX_ENTRIES = 20_000
// Nhịp gọi Instagram / Threads: nghỉ 1–2 s giữa hai yêu cầu (câu trả lời JSON nhỏ như trang tự gọi khi cuộn), tối đa
// 200 yêu cầu trong 10 phút (instaloader tự giới hạn 275 truy vấn GraphQL / 10 phút).
const SOCIAL_GAP_MS = 1000
// Facebook: nghỉ 0,3–0,6 s, không trần (Social Bulk Downloader mặc định không nghỉ, SCrawler nghỉ 1 s); bị chặn tạm
// (1675004 / 429) thì dừng ngay, giữ phần đã lấy.
const SOCIAL_FB_GAP_MS = 300
// Instagram: bài viết hỏi bấy nhiêu một lượt (bị từ chối thì về 12 như web), tin nổi bật bấy nhiêu cái một lượt.
const SOCIAL_IG_POSTS = 50
const SOCIAL_IG_HIGHLIGHTS = 5
const SOCIAL_WINDOW_MS = 10 * 60_000
const SOCIAL_WINDOW_CALLS = 200
const SOCIAL_IG_APP_ID = "936619743392459"
// Instagram: truy vấn GraphQL web của trang (`doc_id` + tên truy vấn), như tiện ích Social Bulk Downloader 1.8.8 dùng
// (notes/social-bulk.md). `/api/v1/…` gọi từ tiện ích bị trả trang HTML / 429. `v2` = `/api/graphql`, còn lại
// `/graphql/query`. Instagram đổi mã ⇒ phần đó báo social.changed, cập nhật mã ở đây.
const SOCIAL_IG_QUERIES = {
  user: { doc: "27981003384861049", name: "ProfileRootContentQuery", v2: true },
  posts: { doc: "27672504985785333", name: "PolarisProfilePostsTabContentQuery_connection" },
  reels: { doc: "28143376935350124", name: "PolarisProfileReelsTabContentQuery_connection" },
  stories: { doc: "28284433337819802", name: "PolarisStoriesV3ReelPageStandaloneQuery" },
  highlights: { doc: "26970053832668570", name: "PolarisProfileStoryHighlightsTrayContentQuery", v2: true },
  highlight: { doc: "38282502831363383", name: "PolarisStoriesV3HighlightsPageQuery" },
}
// Threads: cùng kiểu GraphQL (`/graphql/query` của threads.com), mã và cờ `__relay_internal__pv__*` như Social Bulk
// Downloader 1.8.8 dùng (notes/social-bulk.md). Danh sách bài 10 bài một lượt như web.
const SOCIAL_TH_APP_ID = "238260118697367"
const SOCIAL_TH_POSTS = 10
const SOCIAL_TH_QUERIES = {
  user: { doc: "25954331127572347", name: "BarcelonaUsernameHovercardImplDirectQuery" },
  posts: { doc: "33544334045182488", name: "BarcelonaProfileThreadsTabRefetchableDirectQuery" },
}
const socialRelay = (flags) => Object.fromEntries(Object.entries(flags).map(([key, value]) => [`__relay_internal__pv__${key}relayprovider`, value]))
const SOCIAL_TH_USER_FLAGS = socialRelay({
  BarcelonaIsInternalUser: false, BarcelonaIsLoggedIn: true, BarcelonaThreadsWebCachingImprovements: false, BarcelonaHasSpoilerStylingInfo: true,
  BarcelonaShouldShowFediverseM1Features: true, BarcelonaHasEventBadge: false,
})
const SOCIAL_TH_POSTS_FLAGS = socialRelay({
  BarcelonaIsLoggedIn: true, BarcelonaHasProfileSelfReplyContext: true, BarcelonaHasInlineReplyComposer: true, BarcelonaIsReplyApprovalEnabled: true,
  BarcelonaIsReplyApprovalsConsumptionEnabled: true, BarcelonaHasDearAlgoConsumption: true, BarcelonaHasEventBadge: false,
  BarcelonaIsSearchDiscoveryEnabled: false, BarcelonaHasPodcastConsumption: true, BarcelonaHasCommunities: true, BarcelonaHasSelfThreadCount: true,
  IsTagIndicatorEnabled: true, BarcelonaHasDeepDive: false, BarcelonaHasGhostPostConsumption: true, BarcelonaHasSpoilerStylingInfo: true,
  BarcelonaHasGhostPostEmojiActivation: false, BarcelonaOptionalCookiesEnabled: true, BarcelonaHasDearAlgoWebProduction: false,
  BarcelonaQuotedPostUFIEnabled: true, BarcelonaHasTopicTags: true, BarcelonaIsCrawler: false, BarcelonaHasDisplayNames: false,
  BarcelonaHasCommunityTopContributors: true, BarcelonaCanSeeSponsoredContent: true, BarcelonaShouldShowFediverseM075Features: true,
  BarcelonaImplicitTrendsGK: false, BarcelonaIsInternalUser: false,
})
// Facebook: `POST /api/graphql/` của trang, như Social Bulk Downloader 1.8.8 (notes/social-bulk.md). Danh sách video /
// ảnh nhóm chỉ có mã ⇒ hỏi từng mục (`video`, `photo`).
const SOCIAL_FB_QUERIES = {
  photos: { doc: "24168515422830962", name: "ProfileCometAppCollectionPhotosRendererPaginationQuery" },
  reels: { doc: "25443625298595118", name: "ProfileCometAppCollectionReelsRendererPaginationQuery" },
  videos: { doc: "26070921649175153", name: "PagesCometChannelTabAllVideosCardImplPaginationQuery" },
  highlights: { doc: "29865722229740235", name: "ProfileCometFeaturedHighlightsViewPaginationQuery" },
  story: { doc: "25170215872629838", name: "StoriesSuspenseContentPaneRootWithEntryPointQuery" },
  groupPhotos: { doc: "30127100086888552", name: "GroupsCometMediaPhotosTabGridQuery" },
  groupVideos: { doc: "9352040021563569", name: "GroupsCometMediaVideosTabGridQuery" },
  photo: { doc: "26823688820652915", name: "CometPhotoRootContentQuery" },
  video: { doc: "25333743559590947", name: "FBUnifiedVideoRootWithEntrypointQuery" },
}
const SOCIAL_FB_VIDEO_FLAGS = socialRelay({
  FBUnifiedVideoMediaFooter_comet_enable_reels_ads_gk: true, FBUnifiedVideoMediaFooter_community_notes_reels_fb_web_unified_gk: true,
  FBReels_enable_view_dubbed_audio_type_gk: false, FBUnifiedVideoMediaFooter_enable_meta_ai_pill_gk: true, FBUnifiedVideoMediaFooter_enable_ai_info_pill_gk: true,
})
const SOCIAL_FB_REELS_FLAGS = socialRelay({
  FBReels_deprecate_short_form_video_context_gk: true, FBReels_enable_view_dubbed_audio_type_gk: false, FBReels_enable_meta_ai_label_gk: true,
  FBReelsMediaFooter_comet_enable_reels_ads_gk: true, FBUnifiedVideoMediaFooter_comet_enable_reels_ads_gk: true,
  FBUnifiedVideoMediaFooter_community_notes_reels_fb_web_unified_gk: true, FBUnifiedVideoMediaFooter_enable_meta_ai_pill_gk: true,
  FBUnifiedVideoMediaFooter_enable_ai_info_pill_gk: true,
})
const SOCIAL_FB_PHOTO_FLAGS = socialRelay({
  GHLShouldChangeSponsoredDataFieldName: true, TestPilotShouldIncludeDemoAdUseCase: false, CometUFIShareActionMigration: true,
  CometUFICommentAutoTranslationType: "AUTO_TRANSLATE", CometUFICommentAvatarStickerAnimatedImage: false, CometUFICommentActionLinksRewriteEnabled: false,
  IsWorkUser: false, CometUFIReactionsEnableShortName: false, CometUFISingleLineUFI: true, CometImmersivePhotoCanUserDisable3DMotion: false,
})
const SOCIAL_IG_RESERVED =new Set(["p", "reel", "reels", "tv", "stories", "explore", "accounts", "direct", "about", "legal", "developer", "web", "api", "challenge", "emails", "session"])
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
    return { platform: "facebook", kind: "group", id: `groups.${parts[1]}`, group: parts[1], base: `${page.origin}/groups/${parts[1]}` }
  }
  let id = null
  const numeric = page.searchParams.get("id")
  if (parts[0] === "profile.php" && /^\d+$/.test(numeric ?? "")) {
    id = numeric
  } else if (parts[0] === "people" && /^\d+$/.test(parts[2] ?? "")) {
    id = parts[2]
  } else if (parts[0] && /^[\w.-]{1,100}$/.test(parts[0]) && !SOCIAL_FB_RESERVED.has(parts[0].toLowerCase())) {
    id = parts[0]
  }
  if (!id) return null
  const base = /^\d+$/.test(id) ? `${page.origin}/profile.php?id=${id}` : `${page.origin}/${id}`
  return { platform: "facebook", kind: "profile", id, base }
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
      } catch (error) {
        // Thử cách sau.
        console.debug("UTD social: inject", world ?? "content", error?.message)
      }
    }
    return false
  }

  /** Nghỉ trước một yêu cầu tới mạng xã hội: Facebook 0,3–0,6 s; Instagram / Threads 1–2 s và không quá 200 yêu cầu / 10 phút. */
  async pace() {
    const facebook = this.plan.platform === "facebook"
    const now = Date.now()
    this.calls = this.calls.filter((at) => now - at < SOCIAL_WINDOW_MS)
    if (!facebook && this.calls.length >= SOCIAL_WINDOW_CALLS) {
      const wait = SOCIAL_WINDOW_MS - (now - this.calls[0])
      await this.wait(socialDelay(wait))
    }
    const gap = facebook ? SOCIAL_FB_GAP_MS : SOCIAL_GAP_MS
    if (this.calls.length > 0) await this.wait(socialDelay(gap + Math.random() * gap))
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
    } catch (error) {
      // Tab đang chuyển trang: lần sau vẽ lại.
      console.debug("UTD social: pill", error?.message)
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

  /**
   * Facebook: một truy vấn GraphQL của trang (`SOCIAL_FB_QUERIES[query]`). Trả `{data, items, prefetch}`: dòng JSON đầu của
   * câu trả lời, ảnh/video social-harvest.js đọc được trong mọi dòng, link ảnh `prefetch_uris_v2` (trang ảnh).
   */
  async facebook(query, variables) {
    const { doc, name } = SOCIAL_FB_QUERIES[query]
    await this.pace()
    const reply = await this.run(socialFacebookCall, [doc, name, JSON.stringify(variables)]).catch(() => null)
    if (!reply) throw new Error("social.changed")
    const code = socialFacebookError(reply)
    if (code) {
      console.warn("UTD social", code, name, reply.status, reply.message ?? "")
      throw new Error(code)
    }
    return reply
  }

  /** Instagram / Threads: một truy vấn GraphQL của trang (`SOCIAL_IG_QUERIES` / `SOCIAL_TH_QUERIES`), đọc ảnh/video ngay trong trang. */
  async graphql(query, variables) {
    const threads = this.plan.platform === "threads"
    const { doc, name, v2 } = (threads ? SOCIAL_TH_QUERIES : SOCIAL_IG_QUERIES)[query]
    await this.pace()
    const appId = threads ? SOCIAL_TH_APP_ID : SOCIAL_IG_APP_ID
    const reply = await this.run(socialInstagramCall, [v2 ? "/api/graphql" : "/graphql/query", doc, name, JSON.stringify(variables), appId]).catch(() => null)
    if (!reply) throw new Error("social.changed")
    const code = socialInstagramError(reply)
    if (code) {
      // Để chẩn đoán (trang service worker của tiện ích): mã HTTP và câu báo của Instagram, không có dữ liệu hồ sơ.
      const message = reply.meta?.message ?? reply.meta?.errors?.[0]?.message ?? ""
      console.warn("UTD social", code, name, reply.status, message, reply.type ?? "", reply.head ?? "")
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
  // Lỗi GraphQL: `{"errors":[{"message":…}],"data":null}`.
  const errors = Array.isArray(meta.errors) ? meta.errors.map((item) => `${text(item?.message)} ${text(item?.summary)}`).join(" ") : ""
  const message = `${text(meta.message)} ${text(meta.error_type)} ${errors}`
  const failed = meta.status === "fail" || errors !== ""
  // Chưa đăng nhập: Instagram trả 401 "Please wait a few minutes…" kèm `require_login: true`.
  if (failed && meta.require_login === true) return "social.login"
  if (reply.status === 429 || meta.spam === true || (failed && /wait a few minutes|rate limit|try again later|checkpoint_required|challenge_required|feedback_required|spam/.test(message))) return "social.rate_limited"
  if (reply.status === 401 || reply.status === 403 || /\/accounts\/login|\/challenge/.test(reply.url ?? "") || (failed && /login_required|not logged in/.test(message))) return "social.login"
  if (reply.status === 404) return "social.not_found"
  if (reply.status !== 200 || !reply.meta?.data) return "social.changed"
  return null
}

/** Trang sau của một danh sách GraphQL (`page_info`); hết trang / không có ⇒ null. */
const socialNext = (connection) => (connection?.page_info?.has_next_page && connection.page_info.end_cursor ? String(connection.page_info.end_cursor) : null)

async function socialInstagram(job) {
  const { user } = job.plan
  // Mã hồ sơ từ HTML trang (`profile_id`, như Social Bulk Downloader); không có thì hỏi GraphQL theo tên.
  await job.pace()
  const found = await job.run(socialInstagramUser, [user]).catch(() => null)
  if (found?.id) {
    job.head = { id: found.id, name: found.name || user, handle: user, avatar: found.avatar ?? null, followers: found.followers ?? null, private: false }
  } else {
    const info = await job.graphql("user", { username: user })
    const profile = info.meta?.data?.xig_user_by_username
    if (!profile?.pk) throw new Error("social.not_found")
    job.head = {
      id: String(profile.pk),
      name: profile.full_name || user,
      handle: user,
      avatar: profile.profile_pic_url || null,
      followers: Number(profile.follower_count) || null,
      private: profile.is_private === true,
    }
  }
  const { id } = job.head
  await job.flush()
  const own = (item) => !item.owner || item.owner.toLowerCase() === user.toLowerCase()
  // Một phần lỗi (Instagram trả 500…) thì bỏ phần đó, lấy tiếp phần sau.
  const part = (source, read) => socialPart(job, source, read)
  const next = socialNext
  const story = (item) => ({ ...item, page: null })
  // Tin hết hạn sau 24 giờ: lấy trước.
  await part("stories", async () => {
    const reply = await job.graphql("stories", { reel_ids_arr: [id], __relay_internal__pv__PolarisCommunityNoteStoriesLabelEnabledrelayprovider: true })
    if (!reply.meta.data.xdt_api__v1__feed__reels_media) throw new Error("social.changed")
    await job.push(reply.items.map(story), "stories")
  })
  await part("highlights", async () => {
    const tray = await job.graphql("highlights", { user_id: id })
    const edges = tray.meta.data.highlights?.edges
    if (!Array.isArray(edges)) throw new Error("social.changed")
    const waiting = edges.map((edge) => edge?.node).filter((node) => /^highlight:\d+$/.test(node?.id ?? ""))
    // Hỏi tối đa SOCIAL_IG_HIGHLIGHTS tin nổi bật một lượt; tin nổi bật nào câu trả lời không có thì hỏi lại ở lượt sau
    // (Instagram trả ít hơn thì cũng không chậm hơn hỏi từng cái).
    while (waiting.length > 0 && job.wants("highlights")) {
      const batch = waiting.slice(0, SOCIAL_IG_HIGHLIGHTS)
      const reply = await job.graphql("highlight", {
        first: batch.length,
        initial_reel_id: batch[0].id,
        reel_ids: batch.map((highlight) => highlight.id),
        __relay_internal__pv__PolarisCommunityNoteStoriesLabelEnabledrelayprovider: true,
      })
      const got = batch.filter((highlight) => Array.isArray(reply.reels?.[highlight.id]))
      // Câu trả lời không có cả tin nổi bật đầu tiên đã hỏi: thôi, khỏi hỏi mãi.
      if (!got.includes(batch[0])) throw new Error("social.changed")
      for (const highlight of got) {
        waiting.splice(waiting.indexOf(highlight), 1)
        // Mỗi tin nổi bật một tên (album): chỉ giữ các tin của đúng tin nổi bật này.
        const ids = new Set(reply.reels[highlight.id].map(String))
        const title = typeof highlight.title === "string" ? highlight.title.slice(0, 100) : null
        await job.push(reply.items.filter((media) => ids.has(String(media.id))).map(story), "highlights", title)
      }
    }
  })
  await part("reels", async () => {
    let cursor = null
    do {
      const reply = await job.graphql("reels", {
        after: cursor,
        // Thử thật: Instagram trả tối đa 12 reel một lượt (web dùng 4).
        data: { include_feed_video: true, page_size: 12, target_user_id: id },
        first: 12,
        id,
        __relay_internal__pv__PolarisShortDramaEnabledrelayprovider: false,
      })
      const connection = reply.meta.data.fetch__XDTUserDict?.clips_connection
      if (!connection) throw new Error("social.changed")
      // Danh sách reels có thể chỉ có ảnh bìa: mục vẫn là video, UTD tải từ trang reel.
      const reels = reply.items.filter(own).map((item) => (item.kind === "video" ? item : { ...item, kind: "video", url: null, page: item.page?.replace("/p/", "/reel/") ?? null }))
      await job.push(reels, "reels")
      cursor = next(connection)
    } while (cursor && job.wants("reels") && !job.past("reels"))
  })
  await part("posts", async () => {
    let cursor = null
    // Web hỏi 12 bài một lượt; hỏi nhiều hơn cho ít lượt, Instagram từ chối thì quay về 12.
    let size = SOCIAL_IG_POSTS
    for (;;) {
      const variables = {
        username: user,
        after: cursor,
        before: null,
        data: { count: size, include_reel_media_seen_timestamp: true, include_relationship_info: true, latest_besties_reel_media: true, latest_reel_media: true },
        first: size,
        include_multi_captions: true,
        last: null,
        __relay_internal__pv__PolarisMultiCaptionCarouselEnabledrelayprovider: true,
        __relay_internal__pv__PolarisShortDramaEnabledrelayprovider: false,
        __relay_internal__pv__PolarisReelsRecoDebugOverlayEnabledrelayprovider: false,
      }
      let reply
      try {
        reply = await job.graphql("posts", variables)
      } catch (reason) {
        if (size === 12 || reason?.message !== "social.changed") throw reason
        size = 12
        continue
      }
      const connection = reply.meta.data.xdt_api__v1__feed__user_timeline_graphql_connection
      if (!connection) throw new Error("social.changed")
      await job.push(reply.items.filter(own), "posts")
      cursor = next(connection)
      if (!cursor || !job.wants("posts") || job.past("posts")) break
    }
  })
}

async function socialThreads(job) {
  const { user } = job.plan
  // Mã hồ sơ từ truy vấn thẻ tên (hovercard), như Social Bulk Downloader.
  const info = await job.graphql("user", { username: user, ...SOCIAL_TH_USER_FLAGS })
  const profile = info.meta?.data?.user
  if (!profile?.id) throw new Error("social.not_found")
  const pictures = Array.isArray(profile.hd_profile_pic_versions) ? profile.hd_profile_pic_versions : []
  const avatar = [pictures.at(-1)?.url, profile.profile_pic_url].find((url) => typeof url === "string" && /^https:\/\//.test(url)) ?? null
  job.head = { id: String(profile.id), name: profile.full_name || user, handle: user, avatar, followers: Number(profile.follower_count) || null, private: false }
  await job.flush()
  // Bài trích dẫn của người khác nằm trong bài: chỉ giữ bài của chính người này.
  const own = (item) => item.owner?.toLowerCase() === user.toLowerCase()
  let cursor = null
  do {
    const reply = await job.graphql("posts", {
      userID: job.head.id,
      after: cursor,
      before: null,
      first: SOCIAL_TH_POSTS,
      last: null,
      allow_page_info_for_lox_user: false,
      ...SOCIAL_TH_POSTS_FLAGS,
    })
    const connection = reply.meta.data.mediaData
    if (!connection) throw new Error("social.changed")
    await job.push(reply.items.filter(own), "posts")
    await job.pill()
    cursor = socialNext(connection)
  } while (cursor && job.wants("posts") && !job.past("posts"))
}

/**
 * Lỗi của một câu trả lời GraphQL Facebook (mã cố định). Chỉ đọc trường báo lỗi ở cấp ngoài (`errors`, `error`): chú
 * thích ảnh / bài có thể chứa chữ "rate limit"…
 */
function socialFacebookError(reply) {
  const message = String(reply.message ?? "").toLowerCase()
  const codes = Array.isArray(reply.codes) ? reply.codes : []
  // Không có địa chỉ: lỗi mạng (`response.url` luôn là địa chỉ đầy đủ).
  const path = reply.url ? new URL(reply.url).pathname : ""
  // 1357001 / 1357004: chưa đăng nhập / phiên hết hạn.
  if (/^\/(login|checkpoint)/.test(path) || codes.includes(1357001) || codes.includes(1357004) || /not logged in|log in to continue/.test(message)) return "social.login"
  // 1675004: "Rate limit exceeded" (trả kèm HTTP 200).
  if (reply.status === 429 || codes.includes(1675004) || /rate limit|temporarily blocked|too many requests/.test(message)) return "social.rate_limited"
  if (reply.status === 404) return "social.not_found"
  if (reply.status !== 200 || !reply.data?.data) return "social.changed"
  return null
}

/** Một phần (nhóm `source`) lỗi thì bỏ phần đó, lấy tiếp phần sau; bị chặn / cần đăng nhập thì dừng hẳn. */
async function socialPart(job, source, read) {
  if (!job.wants(source)) return
  try {
    await read()
  } catch (reason) {
    if (reason?.message === "social.rate_limited" || reason?.message === "social.login") throw reason
    job.failure = job.failure ?? "social.partial"
  }
}

/** Facebook: mục của `id` social-harvest.js đọc được trong câu trả lời. */
const socialFacebookItem = (reply, id) => reply.items.find((item) => item.id === String(id)) ?? null

/**
 * Facebook: video trong danh sách chỉ có mã ⇒ hỏi từng video lấy link file (như Social Bulk Downloader). Không có link
 * thì vẫn giữ mục (UTD tải bằng yt-dlp từ trang video). Mỗi mục gửi ngay (bị chặn giữa chừng vẫn giữ phần đã có).
 */
async function socialFacebookVideos(job, ids, known, source) {
  for (const id of ids) {
    if (!job.wants(source)) break
    let item = known.find((media) => media.id === id && media.url) ?? null
    if (!item) {
      const reply = await job.facebook("video", {
        count: 1, initial_node_id: id, isAggregationProfileViewerOrShouldShowReelsForPage: false, page_id: "", scale: 1, should_use_stream: false,
        shouldIncludeInitialNodeFetch: true, shouldShowReelsForPage: false, stream_initial_count: 1, useDefaultActor: false, ...SOCIAL_FB_VIDEO_FLAGS,
      }).catch((reason) => {
        if (reason?.message === "social.rate_limited" || reason?.message === "social.login") throw reason
        return null
      })
      const story = reply?.data?.data?.video?.creation_story
      item = (reply && socialFacebookItem(reply, id)) ?? { id, kind: "video", url: null, page: `https://www.facebook.com/watch/?v=${id}` }
      item.timestamp = item.timestamp ?? (Number(story?.creation_time) || null)
      item.title = item.title || (typeof story?.message?.text === "string" ? story.message.text.replace(/\s+/g, " ").trim().slice(0, 300) : "")
    }
    await job.push([item], source)
  }
}

/** Facebook: ảnh nhóm chỉ có mã (hoặc ảnh nhỏ) ⇒ hỏi trang ảnh lấy bản lớn `prefetch_uris_v2` (như Social Bulk Downloader). */
async function socialFacebookPhotos(job, ids, known, source) {
  for (const id of ids) {
    if (!job.wants(source)) break
    const listed = known.find((media) => media.id === id && media.url) ?? null
    if (listed && Math.max(listed.width ?? 0, listed.height ?? 0) >= 960) {
      await job.push([listed], source)
      continue
    }
    const reply = await job.facebook("photo", {
      feedbackSource: 65, feedLocation: "COMET_MEDIA_VIEWER", focusCommentID: null, isMediaset: false, nodeID: id,
      privacySelectorRenderLocation: "COMET_MEDIA_VIEWER", renderLocation: "comet_media_viewer", scale: 1, useDefaultActor: false, ...SOCIAL_FB_PHOTO_FLAGS,
    }).catch((reason) => {
      if (reason?.message === "social.rate_limited" || reason?.message === "social.login") throw reason
      return null
    })
    const item = (reply && socialFacebookItem(reply, id)) ?? listed ?? (reply?.prefetch ? { id, kind: "photo", page: `https://www.facebook.com/photo/?fbid=${id}` } : null)
    if (!item) continue
    if (reply?.prefetch) item.url = reply.prefetch
    await job.push([item], source)
  }
}

/** Facebook: Tin của một bucket (Tin 24 giờ của người đó / một tin nổi bật). Trả bucket. */
async function socialFacebookBucket(job, bucketID, source, album) {
  const reply = await job.facebook("story", { bucketID, focusCommentID: null, scale: 1 })
  const bucket = reply.data.data.bucket
  if (!bucket) throw new Error("social.changed")
  // Câu trả lời có thể kèm bucket khác (Tin kế tiếp): chỉ giữ thẻ của bucket đã hỏi.
  const main = String(bucket.id ?? bucketID)
  await job.push(reply.items.filter((item) => item.story && (!item.bucket || item.bucket === main)), source, album)
  return bucket
}

async function socialFacebook(job) {
  const { plan } = job
  if (plan.kind === "story") {
    // Đang xem Tin / tin nổi bật: chỉ bucket đó.
    const source = plan.highlight ? "highlights" : "stories"
    job.head = { id: plan.id, name: plan.id, handle: null, avatar: null }
    const bucket = await socialFacebookBucket(job, plan.bucket, source, null)
    const owner = bucket.story_bucket_owner ?? {}
    const avatar = owner.profile_picture?.uri ?? owner.profilePicture?.uri
    job.head = { id: plan.id, name: owner.name || plan.id, handle: null, avatar: typeof avatar === "string" && /^https:\/\//.test(avatar) ? avatar : null }
    return
  }
  // Mã hồ sơ / nhóm và mã các thẻ (Ảnh, Reels) trong dữ liệu của trang hồ sơ.
  const group = plan.kind === "group"
  await job.pace()
  const info = await job.run(socialFacebookProfile, [plan.base, group ? plan.group : plan.id, group]).catch(() => null)
  if (info?.code) throw new Error(info.code)
  if (!info?.id) throw new Error("social.changed")
  job.head = { id: plan.id, name: info.name || plan.id, handle: null, avatar: info.avatar ?? null }
  await job.flush()
  const edges = (connection) => (Array.isArray(connection?.edges) ? connection.edges : [])
  // Lần lượt từng trang của một danh sách; `read` đẩy mục rồi trả đối tượng danh sách (`edges` + `page_info`).
  const pages = async (source, query, variables, read) => {
    let cursor = null
    do {
      const reply = await job.facebook(query, variables(cursor))
      const connection = await read(reply)
      if (!connection) throw new Error("social.changed")
      await job.pill()
      cursor = socialNext(connection)
    } while (cursor && job.wants(source) && !job.past(source))
  }
  const page = { count: 8, scale: 1, id: info.id }
  if (group) {
    await socialPart(job, "photos", () =>
      pages("photos", "groupPhotos", (cursor) => ({ ...page, cursor: cursor ?? "" }), async (reply) => {
        const media = reply.data.data.node?.group_mediaset?.media
        const ids = edges(media).map((edge) => String(edge?.node?.id ?? "")).filter((id) => /^\d+$/.test(id))
        await socialFacebookPhotos(job, ids, reply.items, "photos")
        return media
      }),
    )
    await socialPart(job, "videos", () =>
      pages("videos", "groupVideos", (cursor) => ({ ...page, cursor: cursor ?? "" }), async (reply) => {
        const media = reply.data.data.node?.group_mediaset?.media
        const ids = edges(media).map((edge) => String(edge?.node?.id ?? "")).filter((id) => /^\d+$/.test(id))
        await socialFacebookVideos(job, ids, reply.items, "videos")
        return media
      }),
    )
    return
  }
  // Tin hết hạn sau 24 giờ: lấy trước.
  if (info.story) await socialPart(job, "stories", () => socialFacebookBucket(job, info.story, "stories", null))
  await socialPart(job, "highlights", () =>
    pages(
      "highlights",
      "highlights",
      (cursor) => ({
        scale: 1,
        id: btoa(`profile_tile_view:${info.id}:featured_highlights:intro_featured_highlights_content:hscroll_cards:profile_timeline:3:7`),
        count: 9,
        cursor: cursor ?? "",
      }),
      async (reply) => {
        const tiles = reply.data.data.node?.profile_tile_items
        for (const edge of edges(tiles)) {
          const tile = edge?.node?.node
          if (!tile?.id || !job.wants("highlights")) continue
          const title = [tile.title?.text, tile.title, tile.name, edge.node.title?.text].find((value) => typeof value === "string" && value.trim())
          await socialFacebookBucket(job, String(tile.id), "highlights", title ? title.trim().slice(0, 100) : null)
        }
        return tiles
      },
    ),
  )
  if (info.photos) {
    await socialPart(job, "photos", () =>
      pages("photos", "photos", (cursor) => ({ scale: 1, id: info.photos, count: 8, cursor: cursor ?? "" }), async (reply) => {
        const items = reply.data.data.node?.pageItems
        const photos = edges(items).map((edge) => {
          const node = edge?.node?.node
          if (!node?.id) return null
          const known = socialFacebookItem(reply, node.id)
          if (known?.url) return known
          const image = node.viewer_image ?? node.image
          if (typeof image?.uri !== "string") return null
          const link = typeof node.url === "string" && /^https:\/\/(www|web|m)\.facebook\.com\//.test(node.url) ? node.url : `https://www.facebook.com/photo/?fbid=${node.id}`
          return { id: String(node.id), kind: "photo", url: image.uri, width: Number(image.width) || 0, height: Number(image.height) || 0, page: link }
        })
        await job.push(photos.filter(Boolean), "photos")
        return items
      }),
    )
  }
  if (info.reels) {
    await socialPart(job, "reels", () =>
      pages("reels", "reels", (cursor) => ({ count: 10, cursor: cursor ?? "", renderLocation: null, scale: 1, useDefaultActor: false, id: info.reels, ...SOCIAL_FB_REELS_FLAGS }), async (reply) => {
        const shorts = reply.data.data.node?.aggregated_fb_shorts
        const reels = edges(shorts).map((edge) => {
          const story = edge?.profile_reel_node?.node
          const media = story?.attachments?.[0]?.media
          if (!media?.id) return null
          const id = String(media.id)
          const item = socialFacebookItem(reply, id) ?? { id, kind: "video", url: null, page: `https://www.facebook.com/reel/${id}` }
          item.timestamp = item.timestamp ?? (Number(story.creation_time) || null)
          item.title = item.title || (typeof story.message?.text === "string" ? story.message.text.replace(/\s+/g, " ").trim().slice(0, 300) : "")
          return item
        })
        await job.push(reels.filter(Boolean), "reels")
        return shorts
      }),
    )
  }
  // Thẻ Video của fanpage (trang gắn với hồ sơ).
  if (info.page) {
    await socialPart(job, "videos", () =>
      pages(
        "videos",
        "videos",
        (cursor) => ({ alwaysIncludeAudioRooms: true, count: 6, ...(cursor ? { cursor } : {}), pageID: info.page, scale: 4, showReactions: true, useDefaultActor: false, id: info.page }),
        async (reply) => {
          const videos = reply.data.data.node?.all_videos
          const ids = edges(videos).map((edge) => String(edge?.node?.id ?? "")).filter((id) => /^\d+$/.test(id))
          await socialFacebookVideos(job, ids, reply.items, "videos")
          return videos
        },
      ),
    )
  }
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
    if (!(await job.inject())) throw new Error("social.changed")
    if (plan.platform === "instagram") {
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

/**
 * Facebook: mã hồ sơ / nhóm, tên, ảnh đại diện, mã thẻ Ảnh / Reels, fanpage gắn kèm, bucket Tin — trong đối tượng
 * `"profile_header_renderer"` của dữ liệu trang hồ sơ (như Social Bulk Downloader). Đọc dữ liệu đang có trong tab trước;
 * không thấy (hoặc là của hồ sơ khác vì đã chuyển trang trong Facebook) thì tải lại HTML trang `base`. `expect` = tên /
 * mã trong địa chỉ để kiểm đúng hồ sơ. Không thấy ⇒ null; bị chuyển sang trang đăng nhập ⇒ `{code}`.
 */
async function socialFacebookProfile(base, expect, group) {
  // Đối tượng JSON bắt đầu ở `from` (đếm ngoặc, bỏ qua chuỗi).
  const object = (text, from) => {
    let depth = 0
    let quote = false
    for (let at = from; at < text.length && at - from < 2_000_000; at += 1) {
      const char = text[at]
      if (quote) {
        if (char === "\\") at += 1
        else if (char === '"') quote = false
      } else if (char === '"') quote = true
      else if (char === "{") depth += 1
      else if (char === "}" && --depth === 0) return text.slice(from, at + 1)
    }
    return null
  }
  // Mã thẻ: đối tượng có `tab_key` (photos_by, owner_reels) và `id`.
  const tab = (root, key) => {
    const stack = [root]
    for (let budget = 50000; stack.length > 0 && budget > 0; budget -= 1) {
      const node = stack.pop()
      if (!node || typeof node !== "object") continue
      if (node.tab_key === key && typeof node.id === "string") return node.id
      for (const child of Object.values(node)) if (child && typeof child === "object") stack.push(child)
    }
    return null
  }
  const wanted = String(expect).toLowerCase()
  // Địa chỉ hồ sơ `/<tên>`, `profile.php?id=<mã>`, `/groups/<mã>`: so trọn từng đoạn (tên là tiền tố của tên khác không khớp).
  const named = (link) => {
    let url = null
    try {
      url = new URL(String(link ?? ""))
    } catch {
      return false
    }
    const parts = url.pathname.toLowerCase().split("/").filter(Boolean)
    if (group) return parts[0] === "groups" && parts[1] === wanted
    return parts[0] === wanted || url.searchParams.get("id") === wanted
  }
  const read = (text) => {
    for (let at = text.indexOf('"profile_header_renderer"'); at >= 0; at = text.indexOf('"profile_header_renderer"', at + 1)) {
      const raw = object(text, text.indexOf("{", at))
      let header = null
      try {
        header = raw ? JSON.parse(raw) : null
      } catch {
        continue
      }
      const owner = group ? header?.group : header?.user
      if (!owner?.id) continue
      if (String(owner.id) !== String(expect) && !named(owner.url)) continue
      const avatar = owner.profilePicLarge?.uri ?? owner.profilePicture?.uri ?? owner.profile_picture?.uri
      return {
        id: String(owner.id),
        name: typeof owner.name === "string" ? owner.name.slice(0, 200) : null,
        avatar: typeof avatar === "string" && /^https:\/\//.test(avatar) ? avatar : null,
        photos: group ? null : tab(owner, "photos_by"),
        reels: group ? null : tab(owner, "owner_reels"),
        page: group ? null : (owner.delegate_page?.id ? String(owner.delegate_page.id) : null),
        story: group ? null : (owner.story_bucket?.nodes?.[0]?.id ? String(owner.story_bucket.nodes[0].id) : null),
      }
    }
    return null
  }
  const here = read(Array.from(document.scripts, (script) => script.textContent ?? "").join("\n"))
  if (here) return here
  const request = typeof browser === "object" && typeof content === "object" && content?.fetch ? content.fetch.bind(content) : fetch
  try {
    const response = await request(base, { credentials: "include" })
    if (/^\/(login|checkpoint)/.test(new URL(response.url).pathname)) return { code: "social.login" }
    if (response.status === 404) return { code: "social.not_found" }
    if (response.status === 429) return { code: "social.rate_limited" }
    return response.ok ? read(await response.text()) : null
  } catch {
    return null
  }
}

/**
 * Facebook: gửi một truy vấn GraphQL bằng phiên của chính trang (cùng nguồn, như web Facebook): `doc_id` + `variables`,
 * mã `fb_dtsg` / `lsd` / tài khoản của trang. Câu trả lời có thể nhiều dòng JSON (dòng sau có `label`): social-harvest.js
 * đọc ảnh/video ở mọi dòng; trả dòng đầu, mã lỗi Facebook, link ảnh `prefetch_uris_v2`.
 */
async function socialFacebookCall(doc, name, variables) {
  const harvest = window[Symbol.for("utd.social-harvest")]
  const TOKENS = Symbol.for("utd.social-fb-tokens")
  if (!window[TOKENS]) {
    const module = (key, field) => {
      try {
        return typeof window.require === "function" ? String(window.require(key)?.[field] ?? "") : ""
      } catch {
        return ""
      }
    }
    const scripts = Array.from(document.scripts, (script) => script.textContent ?? "").join("\n")
    const embedded = (key) => scripts.match(new RegExp(`"${key}",\\[\\],\\{"token":"([^"]{1,300})"`))?.[1] ?? ""
    const cookie = document.cookie.split("; ").find((item) => item.startsWith("c_user="))?.slice(7) ?? ""
    const user = /^\d+$/.test(cookie) ? cookie : module("CurrentUserInitialData", "USER_ID") || (scripts.match(/"USER_ID":"(\d{1,25})"/)?.[1] ?? "")
    window[TOKENS] = { dtsg: module("DTSGInitialData", "token") || embedded("DTSGInitialData"), lsd: module("LSD", "token") || embedded("LSD"), user }
  }
  const { dtsg, lsd, user } = window[TOKENS]
  const body = new URLSearchParams({
    av: user || "0",
    __user: user || "0",
    __a: "1",
    __comet_req: "15",
    fb_api_caller_class: "RelayModern",
    fb_api_req_friendly_name: name,
    server_timestamps: "true",
    doc_id: doc,
    variables,
  })
  if (dtsg) body.set("fb_dtsg", dtsg)
  if (lsd) body.set("lsd", lsd)
  const headers = { "X-FB-Friendly-Name": name }
  if (lsd) headers["X-FB-LSD"] = lsd
  const request = typeof browser === "object" && typeof content === "object" && content?.fetch ? content.fetch.bind(content) : fetch
  try {
    const response = await request(new URL("/api/graphql/", location.origin).href, { method: "POST", credentials: "include", headers, body })
    const text = (await response.text()).trim().replace(/^for ?\(;;\);/, "")
    const lines = []
    for (const line of text.split("\n")) {
      try {
        lines.push(JSON.parse(line))
      } catch {
        // Dòng không phải JSON: bỏ qua.
        console.debug("UTD social: dòng không phải JSON", line.slice(0, 80))
      }
    }
    harvest?.take()
    if (harvest) for (const line of lines) harvest.read(line)
    const items = harvest ? harvest.take().items : []
    const codes = []
    const messages = []
    for (const line of lines) {
      if (Number.isFinite(line?.error)) {
        codes.push(line.error)
        messages.push(String(line.errorSummary ?? ""), String(line.errorDescription ?? ""))
      }
      for (const error of Array.isArray(line?.errors) ? line.errors : []) {
        if (Number.isFinite(error?.code)) codes.push(error.code)
        messages.push(String(error?.message ?? ""), String(error?.summary ?? ""))
      }
    }
    const raw = text.match(/"prefetch_uris_v2":\[\{"uri":"((?:[^"\\]|\\.){10,4000})"/)?.[1]
    let prefetch = null
    try {
      prefetch = raw ? JSON.parse(`"${raw}"`) : null
    } catch {
      // Link hỏng: không dùng.
      console.debug("UTD social: link prefetch_uris_v2 hỏng")
    }
    return { status: response.status, url: response.url, data: lines[0] ?? null, codes, message: messages.join(" ").trim().slice(0, 300), items, prefetch }
  } catch {
    return { status: 0, url: "", data: null, codes: [], message: "", items: [], prefetch: null }
  }
}

/**
 * Instagram: mã số, tên, ảnh đại diện của hồ sơ `user` từ HTML mới tải của chính trang hồ sơ (một lần tải trang thường,
 * không qua API bị giới hạn). Không dùng dữ liệu đang có trong trang: chuyển trang ngay trong Instagram thì đó có thể là
 * của hồ sơ trước. Không thấy ⇒ null.
 */
async function socialInstagramUser(user) {
  const name = user.replace(/\./g, "\\.")
  const string = (raw) => {
    try {
      return raw === undefined ? null : JSON.parse(`"${raw}"`)
    } catch {
      return null
    }
  }
  const read = (text) => {
    if (!new RegExp(`"username":"${name}"`, "i").test(text)) return null
    // Đối tượng hồ sơ `"pk":"<mã>","username":"<user>"…`, hoặc thông số đường dẫn của trang (`page_logging`). Chỗ có
    // `"username":"<user>"` khác (biến của truy vấn…) đứng cạnh mã của bài viết, người xem: không đoán theo khoảng cách.
    const pair = text.match(new RegExp(`"(?:pk|pk_id|id)":"(\\d{1,25})","username":"${name}"`, "i"))
    const id = pair?.[1] ?? text.match(/"profile_id":"(\d{1,25})"/)?.[1] ?? text.match(/"profilePage_(\d{1,25})"/)?.[1]
    if (!id) return null
    const found = { id, name: null, avatar: null, followers: null }
    // Tên, ảnh đại diện, số người theo dõi: trong cùng đối tượng, ngay sau `"pk":"<mã>","username":"<user>"`.
    const at = pair ? pair.index : -1
    if (at >= 0) {
      const object = text.slice(at, at + 8000)
      const avatar = string(object.match(/"profile_pic_url(?:_hd)?":"((?:[^"\\]|\\.){10,2000})"/)?.[1])
      found.name = string(object.match(/"full_name":"((?:[^"\\]|\\.){0,300})"/)?.[1])
      found.avatar = avatar && /^https:\/\//.test(avatar) ? avatar : null
      found.followers = Number(object.match(/"follower_count":(\d{1,12})/)?.[1]) || null
    }
    return found
  }
  const request = typeof browser === "object" && typeof content === "object" && content?.fetch ? content.fetch.bind(content) : fetch
  try {
    const response = await request(`${location.origin}/${encodeURIComponent(user)}/`, { credentials: "include" })
    // Chuyển sang trang đăng nhập / xác minh: không phải trang hồ sơ.
    if (!response.ok || !new URL(response.url).pathname.toLowerCase().startsWith(`/${user.toLowerCase()}`)) return null
    const text = await response.text()
    // JSON lồng trong chuỗi có dấu `\"`.
    const found = read(text) ?? read(text.replaceAll('\\"', '"'))
    if (!found || (found.name && found.avatar)) return found
    // HTML tải bằng fetch thường không kèm dữ liệu hồ sơ: tên "NASA (@nasa) • Instagram…" và ảnh từ thẻ Open Graph.
    const head = new DOMParser().parseFromString(text.slice(0, text.indexOf("</head>") + 7), "text/html")
    const meta = (property) => head.querySelector(`meta[property="${property}"]`)?.getAttribute("content") ?? ""
    const title = meta("og:title").match(/^(.{1,200}?)\s*\(@[\w.]+\)/)?.[1]
    const image = meta("og:image")
    found.name = found.name || title || null
    found.avatar = found.avatar || (/^https:\/\//.test(image) ? image : null)
    return found
  } catch {
    return null
  }
}

/**
 * Gửi một truy vấn GraphQL của trang Instagram bằng phiên đăng nhập của chính trang (cùng nguồn, không cần cookie riêng),
 * như web Instagram: `doc_id` + `variables`, mã `fb_dtsg` / `lsd` của trang, header app id / CSRF / tên truy vấn. Ảnh/video
 * trong câu trả lời đọc bằng social-harvest.js; phần còn lại (con trỏ trang sau, thông tin hồ sơ) trả về gọn.
 */
async function socialInstagramCall(path, doc, name, variables, appId) {
  const harvest = window[Symbol.for("utd.social-harvest")]
  const cookie = (key) => document.cookie.split("; ").find((item) => item.startsWith(`${key}=`))?.slice(key.length + 1) ?? ""
  // Mã của trang (đọc một lần): `require` của web Meta, không có thì từ dữ liệu nhúng.
  const TOKENS = Symbol.for("utd.social-ig-tokens")
  if (!window[TOKENS]) {
    const module = (key) => {
      try {
        return typeof window.require === "function" ? (window.require(key)?.token ?? "") : ""
      } catch {
        return ""
      }
    }
    const scripts = Array.from(document.scripts, (script) => script.textContent ?? "").join("\n")
    const embedded = (key) => scripts.match(new RegExp(`"${key}",\\[\\],\\{"token":"([^"]{1,300})"`))?.[1] ?? ""
    window[TOKENS] = { dtsg: module("DTSGInitialData") || embedded("DTSGInitialData"), lsd: module("LSD") || embedded("LSD") }
  }
  const { dtsg, lsd } = window[TOKENS]
  let claim = "0"
  try {
    claim = sessionStorage.getItem("www-claim-v2") || "0"
  } catch (error) {
    // Không đọc được: dùng "0" như trang lúc mới mở.
    console.debug("UTD social: www-claim-v2", error?.message)
  }
  const headers = {
    "X-IG-App-ID": appId,
    "X-IG-WWW-Claim": claim,
    "X-CSRFToken": cookie("csrftoken"),
    "X-FB-Friendly-Name": name,
    "X-Requested-With": "XMLHttpRequest",
  }
  if (lsd) headers["X-FB-LSD"] = lsd
  const body = new URLSearchParams({ doc_id: doc, variables, fb_api_req_friendly_name: name, fb_api_caller_class: "RelayModern", server_timestamps: "true" })
  if (dtsg) body.set("fb_dtsg", dtsg)
  if (lsd) body.set("lsd", lsd)
  const init = { method: "POST", credentials: "include", headers, body }
  const request = typeof browser === "object" && typeof content === "object" && content?.fetch ? content.fetch.bind(content) : fetch
  try {
    const response = await request(new URL(path, location.origin).href, init)
    // Như CSRFGuard của chính web Instagram: câu trả lời có thể mở đầu bằng `for (;;);`.
    const text = (await response.text()).trim().replace(/^for ?\(;;\);/, "")
    let data = null
    try {
      data = JSON.parse(text)
    } catch {
      // Không phải JSON (trang đăng nhập, lỗi): trả loại nội dung + vài chữ đầu để chẩn đoán.
      return { status: response.status, url: response.url, meta: null, items: [], type: response.headers.get("content-type") ?? "", head: text.slice(0, 80) }
    }
    // Ảnh/video: social-harvest.js đọc trực tiếp; `meta` bỏ phần nặng.
    harvest?.take()
    if (data && harvest) harvest.read(data)
    const items = harvest && data ? harvest.take().items : []
    const strip = new Set(["items", "carousel_media", "image_versions2", "video_versions", "media"])
    const meta = JSON.parse(text, (key, value) => (strip.has(key) ? undefined : value))
    // Tin / tin nổi bật: id các tin của từng bucket (`{"id":"highlight:1","items":[…]}`), để gắn tên tin nổi bật.
    const reels = {}
    const stack = [data]
    for (let budget = 20000; stack.length > 0 && budget > 0; budget -= 1) {
      const node = stack.pop()
      if (!node || typeof node !== "object") continue
      if (typeof node.id === "string" && Array.isArray(node.items)) reels[node.id] = node.items.map((item) => String(item?.pk ?? item?.id))
      for (const child of Array.isArray(node) ? node : Object.values(node)) if (child && typeof child === "object") stack.push(child)
    }
    return { status: response.status, url: response.url, meta, items, reels }
  } catch {
    return { status: 0, url: "", meta: null, items: [] }
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
    const style = document.createElement("style")
    style.textContent = `
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
    `
    const box = document.createElement("div")
    box.className = "box"
    box.setAttribute("role", "status")
    const dot = document.createElement("span")
    dot.className = "dot"
    const label = document.createElement("span")
    label.className = "text"
    const stop = document.createElement("button")
    stop.type = "button"
    box.append(dot, label, stop)
    shadow.append(style, box)
    stop.addEventListener("click", (event) => {
      if (event.isTrusted) host.dataset.stop = "1"
    })
    document.documentElement.append(host)
  }
  const shadow = host.shadowRoot
  shadow.querySelector(".text").textContent = `UTD · ${text}`
  shadow.querySelector(".box").title = final ? "" : hint
  const button = shadow.querySelector("button")
  button.textContent = stopLabel
  const stopped = host.dataset.stop === "1"
  button.hidden = final || stopped
  // Nhãn cuối: lượt đã xong; bấm "Tải hàng loạt" lại lúc nhãn còn hiện là lượt mới, chưa bị Dừng.
  if (final) delete host.dataset.stop
  clearTimeout(Number(host.dataset.timer) || 0)
  if (final) host.dataset.timer = String(setTimeout(() => host.remove(), 8000))
  return stopped
}
