// Nhận ra link trang video (YouTube, TikTok, Facebook…): tải bằng HTTP chỉ được trang HTML, nên hộp Thêm
// chuyển sang hộp Tải media; link kênh (TikTok `@user`, kênh/playlist YouTube) mở hộp Tải cả kênh. Cùng luật
// với src-tauri/src/media_pages.rs: sửa một bên thì sửa cả bên kia (bộ ca thử chung ở
// e2e/media-routing.spec.ts và tests trong media_pages.rs).

// Đuôi tệp tải thẳng: `…/x.mp4` là tệp, không phải trang video.
const FILE_EXTENSIONS = new Set([
  "mp4", "m4v", "webm", "mkv", "mov", "avi", "flv", "ts", "m3u8", "mpd", "mp3", "m4a", "aac", "opus",
  "ogg", "wav", "flac", "jpg", "jpeg", "png", "gif", "webp", "zip", "rar", "7z", "exe", "msi", "iso",
  "pdf",
])

// Trang của SoundCloud không phải người đăng (`soundcloud.com/discover/…`).
const SOUNDCLOUD_PAGES = new Set([
  "discover", "stream", "search", "upload", "you", "charts", "settings", "messages", "notifications",
  "pages", "people", "terms-of-use", "mobile", "jobs", "imprint", "pro", "tags", "stations", "feed",
  "signin", "login", "logout", "connect", "artists", "creators", "stats",
])

// Tên miền của trang bài SoundCloud (như yt-dlp); `help.`, `developers.`… là trang tài liệu.
const SOUNDCLOUD_HOSTS = new Set(["soundcloud.com", "www.soundcloud.com", "m.soundcloud.com"])

// Thẻ trong trang người đăng (`soundcloud.com/<user>/likes`), không phải bài.
const SOUNDCLOUD_TABS = new Set([
  "sets", "likes", "tracks", "albums", "reposts", "followers", "following", "popular-tracks", "comments",
  "spotlight", "toptracks",
])

// Thẻ của kênh YouTube liệt kê được video (`/@kênh/videos`); `featured` là trang chủ kênh.
const YOUTUBE_TABS = new Set(["videos", "shorts", "streams", "featured"])

interface Page {
  host: string
  path: string[]
  /** Có `?v=<id>` (YouTube, Facebook). */
  videoParam: boolean
  /** Có `?list=<id>` (playlist YouTube). */
  listParam: boolean
  /** Có `?id=<id>` (`facebook.com/profile.php?id=`). */
  idParam: boolean
}

const at = (page: Page, index: number) => page.path[index] ?? ""
const has = (page: Page, index: number) => page.path.length > index
const isDigits = (part: string) => /^\d+$/.test(part)

const shortLink = (page: Page) => has(page, 0)

const youtube = (page: Page) =>
  (at(page, 0) === "watch" && page.videoParam) ||
  (["shorts", "live", "embed", "v", "clip"].includes(at(page, 0)) && has(page, 1))

const tiktok = (page: Page) =>
  (at(page, 0).startsWith("@") && at(page, 1) === "video" && has(page, 2)) ||
  (at(page, 0) === "t" && has(page, 1))

const facebook = (page: Page) =>
  (["watch", "video.php"].includes(at(page, 0)) && page.videoParam) ||
  (at(page, 0) === "reel" && has(page, 1)) ||
  (at(page, 0) === "share" && ["v", "r"].includes(at(page, 1)) && has(page, 2)) ||
  page.path.some((part, index) => part === "videos" && index < page.path.length - 1)

const instagram = (page: Page) => {
  const kind = (index: number) => ["reel", "reels", "p", "tv"].includes(at(page, index))
  return (kind(0) && has(page, 1)) || (kind(1) && has(page, 2))
}

const twitter = (page: Page) => at(page, 1) === "status" && isDigits(at(page, 2))

const vimeoPlayer = (page: Page) => at(page, 0) === "video" && isDigits(at(page, 1))

const vimeo = (page: Page) =>
  isDigits(at(page, 0)) ||
  (["channels", "groups"].includes(at(page, 0)) && isDigits(page.path.at(-1) ?? ""))

const dailymotion = (page: Page) => at(page, 0) === "video" && has(page, 1)

const twitch = (page: Page) =>
  (at(page, 0) === "videos" && has(page, 1)) || (at(page, 1) === "clip" && has(page, 2))

// `/<user>/<track>` hoặc link riêng tư `/<user>/<track>/s-<mã>`.
const soundcloud = (page: Page) => {
  const privateLink = page.path.length === 3 && at(page, 2).startsWith("s-")
  return (
    SOUNDCLOUD_HOSTS.has(page.host) &&
    (page.path.length === 2 || privateLink) &&
    !SOUNDCLOUD_PAGES.has(at(page, 0)) &&
    !SOUNDCLOUD_TABS.has(at(page, 1))
  )
}

const bilibili = (page: Page) => at(page, 0) === "video" && has(page, 1)

const isHandle = (part: string) => part.length > 1 && part.startsWith("@")

// `@user` đứng một mình (không phải `/video/…`, `/live`).
const tiktokUser = (page: Page) => page.path.length === 1 && isHandle(at(page, 0))

// `@kênh`, `channel/<id>`, `c/<tên>`, `user/<tên>` (kèm thẻ Video/Shorts/Trực tiếp) hoặc `playlist?list=`.
function youtubeChannel(page: Page): boolean {
  if (at(page, 0) === "playlist") return page.listParam && page.path.length === 1
  const owner = isHandle(at(page, 0))
    ? 1
    : ["channel", "c", "user"].includes(at(page, 0)) && has(page, 1)
      ? 2
      : 0
  if (owner === 0) return false
  const rest = page.path.length - owner
  return rest === 0 || (rest === 1 && YOUTUBE_TABS.has(at(page, owner)))
}

// (tên miền, luật đường dẫn). Tên miền cụ thể (vm.tiktok.com) đứng trước tên miền chung (tiktok.com).
// Tên miền khớp cả tên miền con: `youtube.com` gồm `www.`, `m.`, `music.`.
const SITES: [string, (page: Page) => boolean][] = [
  ["youtu.be", shortLink],
  ["youtube.com", youtube],
  ["youtube-nocookie.com", youtube],
  ["vm.tiktok.com", shortLink],
  ["vt.tiktok.com", shortLink],
  ["tiktok.com", tiktok],
  ["fb.watch", shortLink],
  ["facebook.com", facebook],
  ["instagram.com", instagram],
  ["x.com", twitter],
  ["twitter.com", twitter],
  ["player.vimeo.com", vimeoPlayer],
  ["vimeo.com", vimeo],
  ["dai.ly", shortLink],
  ["dailymotion.com", dailymotion],
  ["clips.twitch.tv", shortLink],
  ["twitch.tv", twitch],
  ["on.soundcloud.com", shortLink],
  ["soundcloud.com", soundcloud],
  ["b23.tv", shortLink],
  ["bilibili.com", bilibili],
]

// Link rút gọn (`vm.tiktok.com`) chưa biết là video hay kênh: đi đường trang video, app thấy là kênh thì
// trả mã `channel_page:` kèm link kênh.
const CHANNEL_SITES: [string, (page: Page) => boolean][] = [
  ["tiktok.com", tiktokUser],
  ["youtube.com", youtubeChannel],
]

// `host` là `domain` hoặc tên miền con của nó (`notyoutube.com` không tính).
const onDomain = (host: string, domain: string) =>
  host === domain || host.endsWith(`.${domain}`)

function isFileName(part: string): boolean {
  const dot = part.lastIndexOf(".")
  return dot > 0 && FILE_EXTENSIONS.has(part.slice(dot + 1).toLowerCase())
}

function parseUrl(raw: string): URL | null {
  try {
    const url = new URL(raw.trim())
    const web = url.protocol === "http:" || url.protocol === "https:"
    return web && url.hostname && !url.username && !url.password ? url : null
  } catch {
    return null
  }
}

function matchesSite(raw: string, sites: [string, (page: Page) => boolean][]): boolean {
  const url = parseUrl(raw)
  if (!url) return false
  const host = url.hostname.replace(/\.$/, "").toLowerCase()
  const path = url.pathname.split("/").filter(Boolean)
  if (isFileName(path.at(-1) ?? "")) return false
  const page: Page = {
    host,
    path,
    videoParam: url.searchParams.getAll("v").some(Boolean),
    listParam: url.searchParams.getAll("list").some(Boolean),
    idParam: url.searchParams.getAll("id").some(Boolean),
  }
  const site = sites.find(([domain]) => onDomain(host, domain))
  return site ? site[1](page) : false
}

/** Link là trang xem video/nghe nhạc của một trang media lớn (không phải trang chủ, kênh hay tệp trực tiếp). */
export function isMediaPage(raw: string): boolean {
  return matchesSite(raw, SITES)
}

/** Link là trang kênh TikTok (`tiktok.com/@user`) hoặc kênh/playlist YouTube. */
export function isChannelPage(raw: string): boolean {
  return matchesSite(raw, CHANNEL_SITES)
}

// --- Trang cá nhân / fanpage / nhóm / Tin (Facebook, Instagram, Threads): chỉ tải cả trang được bằng "Tải hàng loạt
// bằng UTD" của tiện ích (notes/social-bulk.md). Cùng luật `is_social_profile` trong media_pages.rs.

const FACEBOOK_RESERVED = new Set([
  "watch", "reel", "reels", "photo", "photo.php", "photos", "videos", "video.php", "marketplace", "gaming", "events",
  "search", "notifications", "messages", "friends", "saved", "settings", "help", "login", "login.php",
  "permalink.php", "story.php", "share", "hashtag", "bookmarks", "ads", "business", "pages", "groups", "stories",
  "home.php", "memories", "privacy", "policies", "dialog",
])
const FACEBOOK_TABS = new Set(["photos", "photos_by", "photos_of", "photos_albums", "videos", "reels"])
const INSTAGRAM_RESERVED = new Set([
  "p", "reel", "reels", "tv", "stories", "explore", "accounts", "direct", "about", "legal", "developer", "web", "api",
  "challenge", "emails", "session",
])
const INSTAGRAM_TABS = new Set(["reels", "tagged"])
const THREADS_TABS = new Set(["replies", "media", "reposts"])

// Tên người dùng Instagram/Threads, tên trang Facebook: chữ, số, `.`, `_` (`-` chỉ có ở Facebook).
const isName = (part: string, dash: boolean) => part.length <= 100 && (dash ? /^[\w.-]+$/ : /^[\w.]+$/).test(part)
// Không có đoạn sau, hoặc đúng một thẻ của trang.
const onlyTab = (page: Page, tabs: Set<string>) => !has(page, 1) || (page.path.length === 2 && tabs.has(at(page, 1)))

function facebookProfile(page: Page): boolean {
  const first = at(page, 0)
  if (first === "groups") return isName(at(page, 1), true) && (!has(page, 2) || at(page, 2) === "media")
  if (first === "profile.php") return page.idParam
  if (first === "people") return isDigits(at(page, 2))
  if (first === "stories") return isDigits(at(page, 1))
  return isName(first, true) && !FACEBOOK_RESERVED.has(first.toLowerCase()) && onlyTab(page, FACEBOOK_TABS)
}

function instagramProfile(page: Page): boolean {
  if (at(page, 0) === "stories") return isName(at(page, 1), false) && at(page, 1) !== "highlights"
  return isName(at(page, 0), false) && !INSTAGRAM_RESERVED.has(at(page, 0)) && onlyTab(page, INSTAGRAM_TABS)
}

const threadsProfile = (page: Page) =>
  isHandle(at(page, 0)) && isName(at(page, 0).slice(1), false) && onlyTab(page, THREADS_TABS)

const SOCIAL_SITES: [string, (page: Page) => boolean][] = [
  ["facebook.com", facebookProfile],
  ["instagram.com", instagramProfile],
  ["threads.net", threadsProfile],
  ["threads.com", threadsProfile],
]

/** Link là trang cá nhân / fanpage / nhóm / Tin: hộp Thêm nhắc dùng Tải hàng loạt trên trình duyệt. */
export function isSocialProfile(raw: string): boolean {
  return !isMediaPage(raw) && matchesSite(raw, SOCIAL_SITES)
}
