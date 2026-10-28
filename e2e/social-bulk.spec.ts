import AxeBuilder from '@axe-core/playwright'
import { expect, test, type Page } from '@playwright/test'

import { isSocialProfile } from '../src/features/home/model/media-pages'

// Tải hàng loạt Facebook / Instagram / Threads (notes/social-bulk.md).
// Phần 1 — tiện ích: background.js + social-bulk.js thật với API trình duyệt giả; "tab" là chính trang thử (trang
// Instagram / Threads / Facebook giả qua page.route, dữ liệu giả theo hình dạng thật trong notes). Không mạng thật,
// không đăng nhập. Phần 2 — app: hộp Tải cả kênh ở chế độ hàng loạt với Tauri giả.

const IG_CDN = 'https://scontent-hkg1-1.cdninstagram.com'
const FB_CDN = 'https://scontent.xx.fbcdn.net'

type Native = { version: number; type: string; url: string; social: Record<string, unknown> }
type Entry = { id: string; source: string; kind: string; url: string | null; page: string | null; album: string | null; timestamp: number | null }

/** API trình duyệt giả cho nền: chạy hàm `executeScript` ngay trong trang này (trang = tab), ghi tin native. */
function stubBrowser() {
  type Listener = (...args: unknown[]) => unknown
  const native: unknown[] = []
  const executed: unknown[] = []
  const updated: Listener[] = []
  const clicks: Listener[] = []
  const noop = { addListener: () => undefined }
  const host = window as unknown as Record<string, unknown>
  host.__utdSocialFast = true
  host.__reply = { ok: true, media: { go: true, sources: null, since: null } }
  host.__navigate = null
  host.chrome = {
    i18n: { getMessage: () => '' },
    runtime: {
      id: 'utd',
      getURL: (file: string) => `chrome-extension://utd/${file}`,
      onInstalled: noop,
      onStartup: noop,
      onMessage: noop,
      sendNativeMessage: async (_host: string, message: unknown) => {
        native.push(JSON.parse(JSON.stringify(message)))
        return host.__reply
      },
    },
    contextMenus: { removeAll: async () => undefined, create: async () => undefined, onClicked: { addListener: (listener: Listener) => clicks.push(listener) } },
    action: { setBadgeText: async () => undefined },
    storage: { local: { get: async () => ({}), set: async () => undefined } },
    cookies: { getAll: async () => [], getAllCookieStores: async () => [] },
    tabs: {
      get: async (id: number) => ({ id, url: location.href }),
      update: async (_id: number, change: { url: string }) => {
        // Không chuyển trang thật (nền đang sống trong trang này): đổi địa chỉ, cho "trang mới" nạp dữ liệu.
        const navigate = host.__navigate as ((url: string) => void) | null
        navigate?.(change.url)
        setTimeout(() => updated.forEach((listener) => listener(7, { status: 'complete' })), 10)
        return {}
      },
      onUpdated: {
        addListener: (listener: Listener) => updated.push(listener),
        removeListener: (listener: Listener) => updated.splice(updated.indexOf(listener), 1),
      },
    },
    scripting: {
      getRegisteredContentScripts: async () => [],
      registerContentScripts: async () => undefined,
      executeScript: async (options: { world?: string; files?: string[]; func?: (...args: unknown[]) => unknown; args?: unknown[] }) => {
        if (options.files) {
          for (const file of options.files) {
            executed.push({ world: options.world ?? null, file })
            await new Promise((resolve, reject) => {
              const script = document.createElement('script')
              script.src = `/__ext/${file}`
              script.onload = resolve
              script.onerror = reject
              document.head.append(script)
            })
          }
          return [{}]
        }
        executed.push({ world: options.world ?? null, func: options.func?.name })
        const result = await options.func?.(...(options.args ?? []))
        return [{ result: result === undefined ? null : JSON.parse(JSON.stringify(result)) }]
      },
    },
  }
  host.__bg = { native, executed, clicks }
}

async function loadExtension(page: Page, url: string) {
  await page.route('**/__ext/**', (route) =>
    route.fulfill({ path: `extensions/chromium/${new URL(route.request().url()).pathname.split('/').pop()}`, contentType: 'text/javascript' }),
  )
  await page.addInitScript(stubBrowser)
  await page.goto(url)
  await page.addScriptTag({ path: 'extensions/chromium/background.js' })
  await page.addScriptTag({ path: 'extensions/chromium/social-bulk.js' })
}

/** Bấm "Tải hàng loạt bằng UTD" trên trang rồi chờ đợt cuối gửi UTD. */
async function runBulk(page: Page) {
  await page.evaluate(() => {
    const bg = (window as unknown as { __bg: { clicks: ((info: unknown, tab: unknown) => void)[] } }).__bg
    bg.clicks[0]({ menuItemId: 'utd-social-bulk' }, { id: 7, url: location.href })
  })
  await expect.poll(async () => (await sent(page)).some((message) => message.social.done), { timeout: 20_000 }).toBe(true)
  return sent(page)
}

const sent = (page: Page) => page.evaluate(() => (window as unknown as { __bg: { native: Native[] } }).__bg.native)
const entriesOf = (messages: Native[]) => messages.flatMap((message) => (message.social.entries as Entry[]) ?? [])

// --- Instagram: truy vấn GraphQL của trang (`/graphql/query`, `/api/graphql`, doc_id như Social Bulk Downloader) ---

const igImage = (name: string) => ({
  candidates: [
    { url: `${IG_CDN}/v/t51/${name}_1080.jpg?stp=dst-jpg&oh=1`, width: 1080, height: 1350 },
    { url: `${IG_CDN}/v/t51/${name}_320.jpg?oh=1`, width: 320, height: 400 },
  ],
})
const igVideo = (name: string) => [
  { url: `${IG_CDN}/o1/v/t16/${name}_720.mp4?oh=1`, width: 720, height: 1280, type: 101 },
  { url: `${IG_CDN}/o1/v/t16/${name}_480.mp4?oh=1`, width: 480, height: 854, type: 102 },
]
const NASA = { username: 'nasa', pk: '528817151' }
const IG_ALL = ['9001', '9101', '8001', '8003', '7001001', '7001002', '7002']

// HTML trang hồ sơ như Instagram thật: `"username":"nasa"` cũng nằm trong biến của truy vấn, cạnh mã của bài viết / người
// xem (mồi dễ nhặt nhầm). `pair`: có đối tượng hồ sơ `"pk":…,"username":…`; `route`: chỉ có thông số đường dẫn + thẻ OG
// (HTML tải bằng fetch thật). Trang nào cũng có mã `fb_dtsg` / `lsd` nhúng sẵn.
const IG_DECOY = '"variables":{"username":"nasa"},"viewer":{"pk":"111","username":"toi"},"media":{"pk":"3999305299946080602"}'
const IG_HTML = {
  pair: `{${IG_DECOY},"data":{"xig_user_by_username":${JSON.stringify({ pk: '528817151', username: 'nasa', profile_pic_url: `${IG_CDN}/v/avatar.jpg`, full_name: 'NASA', follower_count: 97000000 })}}}`,
  route: `{${IG_DECOY},"page_logging":{"name":"profilePage","params":{"page_id":"profilePage_528817151","profile_id":"528817151","sub_path":"posts"}},"url":"\\/nasa\\/","params":{"username":"nasa"}}`,
}
const IG_TOKENS = '{"define":[["DTSGInitialData",[],{"token":"dtsg-abc"},258],["LSD",[],{"token":"lsd-xyz"},323]]}'

type IgRequest = { path: string; name: string; doc: string; variables: Record<string, unknown>; body: URLSearchParams; headers: Record<string, string> }

// `guard`: câu trả lời mở đầu bằng `for (;;);` (CSRFGuard của web Instagram). `postsStatus` / `storiesStatus`: phần đó lỗi.
// `highlights`: số tin nổi bật (mặc định 1, "Sao Hoả"), `highlightCap`: mỗi lượt Instagram chỉ trả bấy nhiêu cái.
// `postsMax`: hỏi nhiều bài hơn số này một lượt ⇒ lỗi GraphQL.
type IgOptions = {
  postsStatus?: number
  storiesStatus?: number
  profile?: 'pair' | 'route'
  guard?: boolean
  highlights?: number
  highlightCap?: number
  postsMax?: number
}
function igRoutes(page: Page, options: IgOptions = {}) {
  const requests: IgRequest[] = []
  page.route('https://www.instagram.com/**', (route) => {
    const request = route.request()
    const url = new URL(request.url())
    if (url.pathname !== '/graphql/query' && url.pathname !== '/api/graphql') {
      const og = `<meta property="og:title" content="NASA (&#064;nasa) &#x2022; Instagram photos and videos"><meta property="og:image" content="${IG_CDN}/v/avatar.jpg">`
      const data = options.profile ? `<script type="application/json">${IG_HTML[options.profile]}</script>` : ''
      const body = `<!doctype html><html><head><title>NASA (@nasa) • Instagram</title>${og}</head><body><p>NASA</p><script type="application/json">${IG_TOKENS}</script>${data}</body></html>`
      return route.fulfill({ contentType: 'text/html; charset=utf-8', body })
    }
    const body = new URLSearchParams(request.postData() ?? '')
    const name = request.headers()['x-fb-friendly-name'] ?? ''
    const variables = JSON.parse(body.get('variables') ?? '{}') as Record<string, unknown>
    requests.push({ path: url.pathname, name, doc: body.get('doc_id') ?? '', variables, body, headers: request.headers() })
    const json = (value: unknown, status = 200) =>
      route.fulfill({ status, contentType: 'application/json', body: `${options.guard ? 'for (;;);' : ''}${JSON.stringify(value)}` })
    if (name === 'ProfileRootContentQuery') {
      if (options.profile) return json({ message: '', status: 'fail' }, 429)
      // Chữ "spam", "wait a few minutes" trong tiểu sử không phải lỗi bị chặn.
      return json({
        data: {
          xig_user_by_username: {
            pk: '528817151', username: 'nasa', full_name: 'NASA', profile_pic_url: `${IG_CDN}/v/avatar.jpg`, follower_count: 97000000, is_private: false,
            biography: 'No spam! Launch in a few hours, please wait a few minutes. require_login feedback_required',
          },
        },
        status: 'ok',
      })
    }
    if (name === 'PolarisStoriesV3ReelPageStandaloneQuery') {
      if (options.storiesStatus) return json({ message: "We're sorry, but something went wrong. Please try again.", status: 'fail' }, options.storiesStatus)
      return json({
        data: { xdt_api__v1__feed__reels_media: { reels_media: [{ id: '528817151', items: [{ pk: '9001', taken_at: 1790000000, code: 'S1', image_versions2: igImage('story'), user: NASA }] }] } },
      })
    }
    if (name === 'PolarisProfileStoryHighlightsTrayContentQuery') {
      if (options.highlights) {
        const edges = Array.from({ length: options.highlights }, (_, index) => ({ node: { id: `highlight:${100 + index}`, title: `HL${index}`, cover_media: {} } }))
        return json({ data: { highlights: { edges } } })
      }
      return json({ data: { highlights: { edges: [{ node: { id: 'highlight:17', title: 'Sao Hoả', cover_media: {} } }, { node: { id: 'oops', title: 'x' } }] } } })
    }
    if (name === 'PolarisStoriesV3HighlightsPageQuery' && options.highlights) {
      // Trả các tin nổi bật được hỏi, tối đa `highlightCap` cái, mỗi cái một tin.
      const asked = (variables.reel_ids as string[]).slice(0, options.highlightCap ?? Infinity)
      const edges = asked.map((id) => ({ node: { id, items: [{ pk: `9${id.split(':')[1]}`, taken_at: 1700000000, image_versions2: igImage(id.replace(':', '')), user: NASA }] } }))
      return json({ data: { xdt_api__v1__feed__reels_media__connection: { edges } } })
    }
    if (name === 'PolarisStoriesV3HighlightsPageQuery') {
      // Kèm một tin nổi bật khác trong câu trả lời: không lẫn vào tin nổi bật đang hỏi.
      return json({
        data: {
          xdt_api__v1__feed__reels_media__connection: {
            edges: [
              { node: { id: 'highlight:17', title: 'Sao Hoả', items: [{ pk: '9101', taken_at: 1700000000, video_versions: igVideo('mars'), image_versions2: igImage('mars'), user: NASA }] } },
              { node: { id: 'highlight:18', title: 'Khác', items: [{ pk: '9201', taken_at: 1700000000, image_versions2: igImage('other'), user: NASA }] } },
            ],
          },
        },
      })
    }
    if (name === 'PolarisProfileReelsTabContentQuery_connection') {
      return json({
        data: {
          fetch__XDTUserDict: {
            clips_connection: {
              edges: [
                { node: { media: { pk: '8001', code: 'R1', taken_at: 1780000000, product_type: 'clips', media_type: 2, video_versions: igVideo('reel'), image_versions2: igImage('reel'), user: NASA, caption: { text: 'Phóng tàu' } } } },
                // Reel của người khác (hợp tác) không lẫn vào.
                { node: { media: { pk: '8002', code: 'R2', taken_at: 1780000001, product_type: 'clips', media_type: 2, video_versions: igVideo('other'), user: { username: 'spacex' } } } },
                // Chỉ có ảnh bìa: vẫn là video, tải từ trang reel.
                { node: { media: { pk: '8003', code: 'R3', taken_at: 1780000002, image_versions2: igImage('r3'), user: NASA } } },
              ],
              page_info: { has_next_page: false, end_cursor: null },
            },
          },
        },
      })
    }
    if (name === 'PolarisProfilePostsTabContentQuery_connection') {
      // 401 thật của Instagram khi chưa đăng nhập: cùng câu "wait a few minutes", kèm `require_login`.
      const login = options.postsStatus === 401 ? { require_login: true } : {}
      if (options.postsStatus) return json({ message: 'Please wait a few minutes before you try again.', status: 'fail', ...login }, options.postsStatus)
      if (options.postsMax && Number(variables.first) > options.postsMax) return json({ errors: [{ message: 'Invalid count' }], data: null })
      const connection = (edges: unknown[], next: string | null) => ({
        data: { xdt_api__v1__feed__user_timeline_graphql_connection: { edges, page_info: { has_next_page: Boolean(next), end_cursor: next } } },
      })
      if (!variables.after) {
        return json(connection([{
          node: {
            pk: '7001', code: 'P1', taken_at: 1770000000, user: NASA, caption: { text: 'Thiên hà\nxoắn ốc' },
            carousel_media: [
              { pk: '7001001', image_versions2: igImage('c1') },
              { pk: '7001002', video_versions: igVideo('c2'), image_versions2: igImage('c2') },
            ],
          },
        }], 'cursor-2'))
      }
      return json(connection([{ node: { pk: '7002', code: 'P2', taken_at: 1760000000, user: NASA, image_versions2: igImage('p2') } }], null))
    }
    return json({ errors: [{ message: 'unknown doc_id' }], data: null })
  })
  return requests
}

test('Instagram: Tin, tin nổi bật, reels, bài viết (carousel tách từng ảnh/video) qua GraphQL của trang, đúng doc_id/header/mã trang, gửi UTD từng đợt', async ({ page, context }) => {
  await context.addCookies([{ name: 'csrftoken', value: 'tok123', domain: '.instagram.com', path: '/', secure: true }])
  const requests = igRoutes(page)
  await loadExtension(page, 'https://www.instagram.com/nasa/')
  const messages = await runBulk(page)

  expect(messages.every((message) => message.version === 2 && message.type === 'social-list' && message.url === 'https://www.instagram.com/nasa/')).toBe(true)
  expect(messages.map((message) => message.social.part)).toEqual(messages.map((_, index) => index))
  expect(messages[0].social.head).toEqual({ id: '528817151', name: 'NASA', handle: 'nasa', avatar: `${IG_CDN}/v/avatar.jpg`, followers: 97000000, private: false })
  expect(messages.at(-1)?.social.done).toEqual({ complete: true, error: null })
  const entries = entriesOf(messages)
  expect(entries.map((entry) => [entry.source, entry.id, entry.kind])).toEqual([
    ['stories', '9001', 'photo'],
    ['highlights', '9101', 'video'],
    ['reels', '8001', 'video'],
    ['reels', '8003', 'video'],
    ['posts', '7001001', 'photo'],
    ['posts', '7001002', 'video'],
    ['posts', '7002', 'photo'],
  ])
  const byId = Object.fromEntries(entries.map((entry) => [entry.id, entry]))
  expect(byId['9001']).toMatchObject({ url: `${IG_CDN}/v/t51/story_1080.jpg?stp=dst-jpg&oh=1`, page: null, timestamp: 1790000000 })
  expect(byId['9101']).toMatchObject({ url: `${IG_CDN}/o1/v/t16/mars_720.mp4?oh=1`, album: 'Sao Hoả' })
  expect(byId['8001']).toMatchObject({ url: `${IG_CDN}/o1/v/t16/reel_720.mp4?oh=1`, page: 'https://www.instagram.com/reel/R1/', title: 'Phóng tàu' })
  expect(byId['8003']).toMatchObject({ url: null, page: 'https://www.instagram.com/reel/R3/' })
  expect(byId['7001002']).toMatchObject({ url: `${IG_CDN}/o1/v/t16/c2_720.mp4?oh=1`, page: 'https://www.instagram.com/p/P1/', title: 'Thiên hà xoắn ốc' })

  // Gọi từ trong trang bằng phiên của trang: như web Instagram (doc_id + variables, mã fb_dtsg/lsd của trang, CSRF từ
  // cookie trang), không cần cookie riêng.
  expect(requests.map((request) => [request.path, request.name, request.doc])).toEqual([
    ['/api/graphql', 'ProfileRootContentQuery', '27981003384861049'],
    ['/graphql/query', 'PolarisStoriesV3ReelPageStandaloneQuery', '28284433337819802'],
    ['/api/graphql', 'PolarisProfileStoryHighlightsTrayContentQuery', '26970053832668570'],
    ['/graphql/query', 'PolarisStoriesV3HighlightsPageQuery', '38282502831363383'],
    ['/graphql/query', 'PolarisProfileReelsTabContentQuery_connection', '28143376935350124'],
    ['/graphql/query', 'PolarisProfilePostsTabContentQuery_connection', '27672504985785333'],
    ['/graphql/query', 'PolarisProfilePostsTabContentQuery_connection', '27672504985785333'],
  ])
  expect(requests[0].variables).toEqual({ username: 'nasa' })
  expect(requests[1].variables).toMatchObject({ reel_ids_arr: ['528817151'] })
  expect(requests[2].variables).toEqual({ user_id: '528817151' })
  expect(requests[3].variables).toMatchObject({ initial_reel_id: 'highlight:17', reel_ids: ['highlight:17'] })
  expect(requests[4].variables).toMatchObject({ id: '528817151', after: null, first: 12, data: { target_user_id: '528817151', include_feed_video: true } })
  expect(requests[5].variables).toMatchObject({ username: 'nasa', after: null, first: 50, data: { count: 50 } })
  expect(requests[6].variables).toMatchObject({ username: 'nasa', after: 'cursor-2' })
  expect(requests[0].headers).toMatchObject({ 'x-ig-app-id': '936619743392459', 'x-csrftoken': 'tok123', 'x-fb-lsd': 'lsd-xyz' })
  expect([requests[0].body.get('fb_dtsg'), requests[0].body.get('lsd')]).toEqual(['dtsg-abc', 'lsd-xyz'])
  await expect(page.locator('#utd-social-pill')).toBeAttached()
})

test('Instagram: UTD bỏ chọn nhóm ⇒ tiện ích không gọi phần đó; bị chặn tạm (429) ⇒ dừng, báo mã, giữ phần đã có', async ({ page }) => {
  const requests = igRoutes(page, { postsStatus: 429 })
  await loadExtension(page, 'https://www.instagram.com/stories/nasa/')
  // Người dùng chỉ để lại Tin và Bài viết trong hộp của UTD.
  await page.evaluate(() => {
    ;(window as unknown as { __reply: unknown }).__reply = { ok: true, media: { go: true, sources: ['stories', 'posts'], since: null } }
  })
  const messages = await runBulk(page)
  expect(requests.map((request) => request.name)).toEqual([
    'ProfileRootContentQuery',
    'PolarisStoriesV3ReelPageStandaloneQuery',
    'PolarisProfilePostsTabContentQuery_connection',
  ])
  expect(entriesOf(messages).map((entry) => entry.id)).toEqual(['9001'])
  expect(messages.at(-1)?.social.done).toEqual({ complete: false, error: 'social.rate_limited' })
})

for (const profile of ['pair', 'route'] as const) {
  test(`Instagram: mã hồ sơ đọc từ HTML trang (${profile === 'pair' ? 'đối tượng hồ sơ' : 'thông số đường dẫn + thẻ OG; câu trả lời kèm for (;;);'}), không nhặt mã mồi ⇒ không hỏi GraphQL theo tên`, async ({ page }) => {
    const requests = igRoutes(page, { profile, guard: profile === 'route' })
    await loadExtension(page, 'https://www.instagram.com/nasa/')
    const messages = await runBulk(page)
    expect(messages[0].social.head).toEqual({
      id: '528817151', name: 'NASA', handle: 'nasa', avatar: `${IG_CDN}/v/avatar.jpg`, followers: profile === 'pair' ? 97000000 : null, private: false,
    })
    expect(messages.at(-1)?.social.done).toEqual({ complete: true, error: null })
    expect(requests.map((request) => request.name)).not.toContain('ProfileRootContentQuery')
    expect(entriesOf(messages).map((entry) => entry.id)).toEqual(IG_ALL)
  })
}

test('Instagram: tin nổi bật hỏi 5 cái một lượt, Instagram trả thiếu thì hỏi lại phần thiếu; bài viết 50/lượt bị từ chối ⇒ về 12', async ({ page }) => {
  const requests = igRoutes(page, { profile: 'pair', highlights: 7, highlightCap: 2, postsMax: 12 })
  await loadExtension(page, 'https://www.instagram.com/nasa/')
  const messages = await runBulk(page)
  const asked = requests.filter((request) => request.name === 'PolarisStoriesV3HighlightsPageQuery').map((request) => request.variables.reel_ids)
  expect(asked).toEqual([
    ['highlight:100', 'highlight:101', 'highlight:102', 'highlight:103', 'highlight:104'],
    ['highlight:102', 'highlight:103', 'highlight:104', 'highlight:105', 'highlight:106'],
    ['highlight:104', 'highlight:105', 'highlight:106'],
    ['highlight:106'],
  ])
  const highlights = entriesOf(messages).filter((entry) => entry.source === 'highlights')
  expect(highlights.map((entry) => [entry.id, entry.album])).toEqual(Array.from({ length: 7 }, (_, index) => [`9${100 + index}`, `HL${index}`]))
  const posts = requests.filter((request) => request.name === 'PolarisProfilePostsTabContentQuery_connection').map((request) => request.variables.first)
  expect(posts).toEqual([50, 12, 12])
  expect(entriesOf(messages).filter((entry) => entry.source === 'posts').map((entry) => entry.id)).toEqual(['7001001', '7001002', '7002'])
  expect(messages.at(-1)?.social.done).toEqual({ complete: true, error: null })
})

test('Instagram: bài viết đòi đăng nhập (401 + require_login) ⇒ báo cần đăng nhập, không phải bị chặn', async ({ page }) => {
  igRoutes(page, { profile: 'pair', postsStatus: 401 })
  await loadExtension(page, 'https://www.instagram.com/nasa/')
  const messages = await runBulk(page)
  expect(messages.at(-1)?.social.done).toEqual({ complete: false, error: 'social.login' })
})

test('Instagram: một phần lỗi (Tin trả 500) ⇒ bỏ phần đó, vẫn lấy reels + bài viết, báo danh sách có thể chưa đủ', async ({ page }) => {
  igRoutes(page, { profile: 'pair', storiesStatus: 500 })
  await loadExtension(page, 'https://www.instagram.com/nasa/')
  const messages = await runBulk(page)
  expect(entriesOf(messages).map((entry) => entry.id)).toEqual(IG_ALL.filter((id) => id !== '9001'))
  expect(messages.at(-1)?.social.done).toEqual({ complete: false, error: 'social.partial' })
})

test('Instagram: UTD báo Dừng ⇒ tiện ích thôi gọi ở đợt sau', async ({ page }) => {
  const requests = igRoutes(page)
  await loadExtension(page, 'https://www.instagram.com/nasa/')
  await page.evaluate(() => {
    ;(window as unknown as { __reply: unknown }).__reply = { ok: true, media: { go: false, sources: null, since: null } }
  })
  const messages = await runBulk(page)
  expect(requests).toHaveLength(1)
  expect(messages.at(-1)?.social.done).toEqual({ complete: false, error: null })
})

test('trang không hỗ trợ (bài viết lẻ, trang chủ) ⇒ báo trên trang, không gửi gì cho UTD', async ({ page }) => {
  igRoutes(page)
  await loadExtension(page, 'https://www.instagram.com/p/Cabc/')
  await page.evaluate(() => {
    const bg = (window as unknown as { __bg: { clicks: ((info: unknown, tab: unknown) => void)[] } }).__bg
    bg.clicks[0]({ menuItemId: 'utd-social-bulk' }, { id: 7, url: location.href })
  })
  await expect(page.locator('#utd-social-pill')).toBeAttached()
  expect(await sent(page)).toEqual([])
  const plans = await page.evaluate(() => {
    const plan = (window as unknown as { socialPlan: (url: string) => Record<string, unknown> | null }).socialPlan
    return [
      'https://www.facebook.com/trangthu',
      'https://www.facebook.com/profile.php?id=1000123',
      'https://www.facebook.com/groups/12345/',
      'https://www.facebook.com/stories/555/UzpfSVNDOjE=/?source=profile_highlight',
      'https://www.facebook.com/watch/?v=1',
      'https://www.threads.net/@zuck',
      'https://www.instagram.com/explore/',
    ].map((url) => {
      const found = plan(url)
      return found ? { platform: found.platform, kind: found.kind ?? null, id: found.id, base: found.base ?? null } : null
    })
  })
  expect(plans).toEqual([
    { platform: 'facebook', kind: 'profile', id: 'trangthu', base: 'https://www.facebook.com/trangthu' },
    { platform: 'facebook', kind: 'profile', id: '1000123', base: 'https://www.facebook.com/profile.php?id=1000123' },
    { platform: 'facebook', kind: 'group', id: 'groups.12345', base: 'https://www.facebook.com/groups/12345' },
    { platform: 'facebook', kind: 'story', id: '555', base: null },
    null,
    { platform: 'threads', kind: null, id: 'zuck', base: 'https://www.threads.net/@zuck' },
    null,
  ])
})

test('nhịp gọi: Facebook nghỉ 0,3–0,6 s, không trần; Instagram / Threads nghỉ 1–2 s, đủ 200 yêu cầu / 10 phút thì chờ', async ({ page }) => {
  igRoutes(page)
  await loadExtension(page, 'https://www.instagram.com/explore/')
  const waits = await page.evaluate(async () => {
    type Job = { calls: number[]; wait: (ms: number) => Promise<void>; pace: () => Promise<void> }
    const SocialJob = (0, eval)('SocialJob') as new (tab: unknown, plan: unknown) => Job
    const host = window as unknown as Record<string, unknown>
    host.__utdSocialFast = false
    // 200 yêu cầu trong phút vừa qua; ghi lại các lần nghỉ thay vì nghỉ thật.
    const paced = async (platform: string) => {
      const job = new SocialJob({ id: 7, url: location.href }, { platform })
      const now = Date.now()
      job.calls = Array.from({ length: 200 }, (_, index) => now - 60_000 + index)
      const asked: number[] = []
      job.wait = async (ms) => {
        asked.push(ms)
      }
      await job.pace()
      return asked
    }
    return { facebook: await paced('facebook'), instagram: await paced('instagram'), threads: await paced('threads') }
  })
  expect(waits.facebook).toHaveLength(1)
  expect(waits.facebook[0]).toBeGreaterThanOrEqual(300)
  expect(waits.facebook[0]).toBeLessThan(600)
  for (const asked of [waits.instagram, waits.threads]) {
    expect(asked).toHaveLength(2)
    expect(asked[0]).toBeGreaterThan(8 * 60_000)
    expect(asked[1]).toBeGreaterThanOrEqual(1000)
    expect(asked[1]).toBeLessThan(2000)
  }
})

// --- Link trang cá nhân / fanpage / nhóm dán vào UTD hoặc bấm "Tải bằng UTD": nhắc dùng Tải hàng loạt ---

// Cùng bộ ca với `social_profiles_point_to_the_bulk_download` trong src-tauri/src/media_pages.rs.
const SOCIAL_PROFILES = [
  'https://www.instagram.com/mytay95/',
  'https://instagram.com/nasa',
  'https://www.instagram.com/nasa/reels/',
  'https://www.instagram.com/stories/nasa/',
  'https://www.facebook.com/trangthu',
  'https://web.facebook.com/trang.thu-2/photos_by',
  'https://www.facebook.com/profile.php?id=1000123',
  'https://www.facebook.com/people/Tr%E1%BA%A7n-A/1000123/',
  'https://www.facebook.com/groups/12345/',
  'https://www.facebook.com/groups/hoi.anh/media',
  'https://www.facebook.com/stories/555/UzpfSVNDOjE=/?source=profile_highlight',
  'https://www.threads.net/@zuck',
  'https://www.threads.com/@zuck/media',
]
const NOT_SOCIAL_PROFILES = [
  'https://www.instagram.com/',
  'https://www.instagram.com/p/Cabc123/',
  'https://www.instagram.com/explore/',
  'https://www.instagram.com/stories/highlights/1789/',
  'https://www.facebook.com/',
  'https://www.facebook.com/watch/',
  'https://www.facebook.com/somepage/videos/1234567890/',
  'https://www.facebook.com/profile.php',
  'https://www.facebook.com/marketplace',
  'https://www.facebook.com/groups/12345/posts/678',
  'https://www.threads.com/',
  'https://www.threads.com/@zuck/post/C1',
  'https://notinstagram.com/nasa',
  'https://example.com/nasa',
]

test('media-pages: nhận ra trang cá nhân / fanpage / nhóm / Tin của Facebook, Instagram, Threads', () => {
  for (const url of SOCIAL_PROFILES) expect(isSocialProfile(url), url).toBe(true)
  for (const url of NOT_SOCIAL_PROFILES) expect(isSocialProfile(url), url).toBe(false)
})

test('S-05: dán link trang cá nhân vào Thêm URL ⇒ thẻ nhắc Tải hàng loạt trên trình duyệt, không tải trang HTML', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Thêm URL' }).first().click()
  const add = page.getByRole('dialog')
  await add.getByLabel('URL cần tải').fill('https://www.instagram.com/mytay95/')
  await expect(add.getByText('Đây là trang cá nhân / fanpage / nhóm')).toBeVisible()
  await expect(add.getByText('Mở trang trên trình duyệt › chuột phải › UTD › Tải hàng loạt bằng UTD')).toBeVisible()
  await expect(add.getByLabel('Tên file')).toHaveCount(0)
  await expect(add.locator('[data-primary]')).toBeDisabled()
  await add.getByLabel('URL cần tải').press('Enter')
  await expect(add.getByRole('heading', { name: 'Thêm download' })).toBeVisible()
  // Link thường vẫn như cũ.
  await add.getByLabel('URL cần tải').fill('https://example.com/a.zip')
  await expect(add.getByText('Đây là trang cá nhân / fanpage / nhóm')).toHaveCount(0)
  await expect(add.locator('[data-primary]')).toBeEnabled()
})

test('tiện ích: "Tải bằng UTD" lên chính trang hồ sơ ⇒ nhắc Tải hàng loạt ngay trên trang, không gửi UTD', async ({ page }) => {
  await page.route('https://www.instagram.com/**', (route) => route.fulfill({ contentType: 'text/html; charset=utf-8', body: '<!doctype html><p>mytay95</p>' }))
  await loadExtension(page, 'https://www.instagram.com/mytay95/')
  await page.evaluate(() => {
    const bg = (window as unknown as { __bg: { clicks: ((info: unknown, tab: unknown) => void)[] } }).__bg
    bg.clicks[0]({ menuItemId: 'utd-download', pageUrl: location.href }, { id: 7, url: location.href })
  })
  await expect(page.locator('#utd-social-pill')).toBeAttached()
  const text = await page.evaluate(() => document.getElementById('utd-social-pill')?.shadowRoot?.querySelector('.text')?.textContent)
  expect(text).toBe('UTD · socialUseBulk')
  expect(await sent(page)).toEqual([])
})

// --- Threads: truy vấn GraphQL của trang (`/graphql/query` của threads.com, doc_id như Social Bulk Downloader) ---

const threadPost = (pk: string, username: string, extra: Record<string, unknown>) => ({
  thread_items: [{ post: { pk, id: `${pk}_1`, code: `C${pk}`, taken_at: 1785000000, user: { username }, caption: { text: `Bài ${pk}` }, ...extra } }],
})

test('Threads: hồ sơ qua hovercard, bài viết từng trang 10 bài (ảnh, carousel ảnh + video); bài của người khác bị bỏ; không cuộn, không chuyển trang', async ({ page, context }) => {
  await context.addCookies([{ name: 'csrftoken', value: 'th-tok', domain: '.threads.com', path: '/', secure: true }])
  const requests: IgRequest[] = []
  await page.route('https://www.threads.com/**', (route) => {
    const request = route.request()
    const url = new URL(request.url())
    if (url.pathname !== '/graphql/query') {
      return route.fulfill({ contentType: 'text/html; charset=utf-8', body: `<!doctype html><html><body><p>zuck</p><script type="application/json">${IG_TOKENS}</script></body></html>` })
    }
    const body = new URLSearchParams(request.postData() ?? '')
    const name = request.headers()['x-fb-friendly-name'] ?? ''
    const variables = JSON.parse(body.get('variables') ?? '{}') as Record<string, unknown>
    requests.push({ path: url.pathname, name, doc: body.get('doc_id') ?? '', variables, body, headers: request.headers() })
    const json = (value: unknown) => route.fulfill({ contentType: 'application/json', body: JSON.stringify(value) })
    if (name === 'BarcelonaUsernameHovercardImplDirectQuery') {
      return json({ data: { user: { id: '314216', username: 'zuck', full_name: 'Mark Zuckerberg', follower_count: 5000000, hd_profile_pic_versions: [{ url: `${IG_CDN}/v/zuck_320.jpg` }, { url: `${IG_CDN}/v/zuck_1080.jpg` }] } } })
    }
    if (name === 'BarcelonaProfileThreadsTabRefetchableDirectQuery') {
      if (!variables.after) {
        return json({ data: { mediaData: {
          edges: [{ node: threadPost('501', 'zuck', { image_versions2: igImage('t501') }) }, { node: threadPost('502', 'someone', { image_versions2: igImage('t502') }) }],
          page_info: { has_next_page: true, end_cursor: 'th-2' },
        } } })
      }
      return json({ data: { mediaData: {
        edges: [{ node: threadPost('503', 'zuck', { carousel_media: [{ pk: '503001', image_versions2: igImage('t503a') }, { pk: '503002', video_versions: igVideo('t503b') }] }) }],
        page_info: { has_next_page: false, end_cursor: null },
      } } })
    }
    return json({ errors: [{ message: 'unknown doc_id' }], data: null })
  })
  await loadExtension(page, 'https://www.threads.com/@zuck')
  const messages = await runBulk(page)
  expect(messages[0].social.head).toEqual({ id: '314216', name: 'Mark Zuckerberg', handle: 'zuck', avatar: `${IG_CDN}/v/zuck_1080.jpg`, followers: 5000000, private: false })
  const entries = entriesOf(messages)
  expect(entries.map((entry) => [entry.source, entry.id, entry.kind])).toEqual([
    ['posts', '501', 'photo'],
    ['posts', '503001', 'photo'],
    ['posts', '503002', 'video'],
  ])
  expect(entries[0]).toMatchObject({ page: 'https://www.threads.com/@zuck/post/C501', url: `${IG_CDN}/v/t51/t501_1080.jpg?stp=dst-jpg&oh=1` })
  expect(messages.at(-1)?.social.done).toEqual({ complete: true, error: null })
  expect(requests.map((request) => [request.name, request.doc])).toEqual([
    ['BarcelonaUsernameHovercardImplDirectQuery', '25954331127572347'],
    ['BarcelonaProfileThreadsTabRefetchableDirectQuery', '33544334045182488'],
    ['BarcelonaProfileThreadsTabRefetchableDirectQuery', '33544334045182488'],
  ])
  expect(requests[0].variables).toMatchObject({ username: 'zuck', __relay_internal__pv__BarcelonaIsLoggedInrelayprovider: true })
  expect(requests[1].variables).toMatchObject({ userID: '314216', after: null, first: 10, __relay_internal__pv__IsTagIndicatorEnabledrelayprovider: true })
  expect(requests[2].variables).toMatchObject({ userID: '314216', after: 'th-2' })
  expect(requests[0].headers).toMatchObject({ 'x-ig-app-id': '238260118697367', 'x-csrftoken': 'th-tok' })
  // Không cuộn, không chuyển trang.
  expect(page.url()).toBe('https://www.threads.com/@zuck')
})

// --- Facebook: truy vấn GraphQL của trang (`/api/graphql/`, doc_id như Social Bulk Downloader) ---

const FB_TOKENS = '{"define":[["DTSGInitialData",[],{"token":"dtsg-fb"},258],["LSD",[],{"token":"lsd-fb"},323]]}'
const fbVideo = (id: string) => ({
  __typename: 'Video',
  id,
  videoDeliveryResponseFragment: { videoDeliveryResponseResult: { progressive_urls: [
    { progressive_url: `${FB_CDN}/o1/${id}-sd.mp4`, failure_reason: null, metadata: { quality: 'SD' } },
    { progressive_url: `${FB_CDN}/o1/${id}-hd.mp4`, failure_reason: null, metadata: { quality: 'HD' } },
  ] } },
})
const fbStoryPhoto = (id: string) => ({ __typename: 'Photo', id, image: { uri: `${FB_CDN}/s/${id}.jpg`, width: 1080, height: 1920 } })
const fbBucket = (id: string, media: unknown[]) => ({
  id,
  story_bucket_owner: { name: 'Trang Thử' },
  unified_stories_with_notes: { edges: media.map((item, index) => ({ node: { id: `card-${id}-${index}`, creation_time: 1720000000, attachments: [{ media: item }] } })) },
})
type FbRequest = { name: string; doc: string; variables: Record<string, unknown>; body: URLSearchParams; headers: Record<string, string> }

/** Facebook giả: trang hồ sơ / nhóm (HTML có `profile_header_renderer`) + `/api/graphql/` theo tên truy vấn. */
function fbRoutes(page: Page, header: unknown, answer: (name: string, variables: Record<string, unknown>) => string) {
  const requests: FbRequest[] = []
  const fetched: string[] = []
  page.route('https://www.facebook.com/**', (route) => {
    const request = route.request()
    const url = new URL(request.url())
    if (url.pathname !== '/api/graphql/') {
      // Mở trang bằng trình duyệt: chưa có dữ liệu hồ sơ (như đã chuyển trang trong Facebook); tiện ích tự tải HTML.
      const document = request.resourceType() === 'document'
      if (!document) fetched.push(url.pathname)
      const data = document ? '' : `<script type="application/json">${JSON.stringify({ require: [['x', { profile_header_renderer: header }]] })}</script>`
      return route.fulfill({ contentType: 'text/html; charset=utf-8', body: `<!doctype html><html><head><title>Trang Thử | Facebook</title></head><body><script type="application/json">${FB_TOKENS}</script>${data}</body></html>` })
    }
    const body = new URLSearchParams(request.postData() ?? '')
    const name = request.headers()['x-fb-friendly-name'] ?? ''
    const variables = JSON.parse(body.get('variables') ?? '{}') as Record<string, unknown>
    requests.push({ name, doc: body.get('doc_id') ?? '', variables, body, headers: request.headers() })
    return route.fulfill({ contentType: 'text/html; charset=utf-8', body: answer(name, variables) })
  })
  return { requests, fetched }
}

test('Facebook: nhận đúng hồ sơ / nhóm theo đường dẫn (nasa ≠ nasaphoto, nhóm 12345 ≠ 123456); tải trang hồ sơ bị 429 ⇒ bị chặn tạm', async ({ page }) => {
  let status = 200
  const header = (owner: unknown) => `<script type="application/json">${JSON.stringify({ require: [['x', { profile_header_renderer: owner }]] })}</script>`
  await page.route('https://www.facebook.com/**', (route) => {
    const request = route.request()
    if (request.resourceType() === 'document') return route.fulfill({ contentType: 'text/html; charset=utf-8', body: '<!doctype html><html><body></body></html>' })
    if (status !== 200) return route.fulfill({ status, body: '' })
    // Hồ sơ / nhóm khác có tên là tiền tố đứng trước trong dữ liệu trang.
    const body = new URL(request.url()).pathname.startsWith('/groups/')
      ? header({ group: { id: '123456', name: 'Nhóm khác', url: 'https://www.facebook.com/groups/123456/' } }) + header({ group: { id: '12345', name: 'Nhóm thử', url: 'https://www.facebook.com/groups/12345/' } })
      : header({ user: { id: '1', name: 'Khác', url: 'https://www.facebook.com/nasaphoto' } }) + header({ user: { id: '2', name: 'NASA', url: 'https://www.facebook.com/NASA' } })
    return route.fulfill({ contentType: 'text/html; charset=utf-8', body: `<!doctype html><html><body>${body}</body></html>` })
  })
  await loadExtension(page, 'https://www.facebook.com/nasa')
  type Profile = (base: string, expect: string, group: boolean) => Promise<{ id?: string; code?: string } | null>
  const read = (base: string, expect: string, group: boolean) =>
    page.evaluate(([b, e, g]) => (window as unknown as { socialFacebookProfile: Profile }).socialFacebookProfile(b, e, g), [base, expect, group] as const)
  expect((await read('https://www.facebook.com/nasa', 'nasa', false))?.id).toBe('2')
  expect((await read('https://www.facebook.com/groups/12345', '12345', true))?.id).toBe('12345')
  status = 429
  expect(await read('https://www.facebook.com/nasa', 'nasa', false)).toEqual({ code: 'social.rate_limited' })
})

test('nhãn trên trang: bấm Dừng ở lượt trước không làm lượt sau (bấm lại trong lúc nhãn cũ còn hiện) dừng ngay', async ({ page }) => {
  igRoutes(page)
  await loadExtension(page, 'https://www.instagram.com/explore/')
  type Pill = (text: string, stop: string, hint: string, final: boolean) => boolean
  const pill = (text: string, final: boolean) =>
    page.evaluate(([value, last]) => (window as unknown as { socialPill: Pill }).socialPill(value, 'Dừng', '', last), [text, final] as const)
  expect(await pill('Đang lấy', false)).toBe(false)
  await page.locator('#utd-social-pill button').click()
  expect(await pill('Đang lấy', false)).toBe(true)
  await pill('Xong', true)
  expect(await pill('Đang lấy', false)).toBe(false)
})

test('Facebook: Tin, tin nổi bật, ảnh, reels, video fanpage qua GraphQL của trang (đúng doc_id, mã trang, tài khoản); không cuộn, không chuyển trang', async ({ page, context }) => {
  await context.addCookies([{ name: 'c_user', value: '100099', domain: '.facebook.com', path: '/', secure: true }])
  const header = {
    user: {
      id: '100044', name: 'Trang Thử', url: 'https://www.facebook.com/trangthu', profilePicLarge: { uri: `${FB_CDN}/p/avatar.jpg` },
      delegate_page: { id: '200055' }, story_bucket: { nodes: [{ id: '555' }] },
      profile_tabs: { tabs: [{ tab_key: 'photos_by', id: 'cGhvdG9zX2J5' }, { tab_key: 'owner_reels', id: 'cmVlbHM=' }, { tab_key: 'about', id: 'YWJvdXQ=' }] },
    },
  }
  const highlightId = Buffer.from('profile_tile_view:100044:featured_highlights:intro_featured_highlights_content:hscroll_cards:profile_timeline:3:7').toString('base64')
  const { requests, fetched } = fbRoutes(page, header, (name, variables) => {
    const json = (value: unknown) => JSON.stringify(value)
    if (name === 'StoriesSuspenseContentPaneRootWithEntryPointQuery') {
      if (variables.bucketID === '555') {
        // Bucket Tin kế tiếp (của người khác) tải sẵn cạnh đó: không lẫn vào.
        return json({ data: { bucket: fbBucket('555', [fbStoryPhoto('4001')]), next: fbBucket('777', [fbStoryPhoto('4999')]) } })
      }
      return json({ data: { bucket: fbBucket(String(variables.bucketID), [fbVideo('4101')]) } })
    }
    if (name === 'ProfileCometFeaturedHighlightsViewPaginationQuery') {
      return json({ data: { node: { profile_tile_items: { edges: [{ node: { node: { id: '666', title: { text: 'Đà Lạt' } } } }], page_info: { has_next_page: false, end_cursor: null } } } } })
    }
    if (name === 'ProfileCometAppCollectionPhotosRendererPaginationQuery') {
      if (!variables.cursor) {
        const photo = { __typename: 'Photo', id: '1001', image: { uri: `${FB_CDN}/v/1001_206.jpg`, width: 206, height: 206 }, viewer_image: { uri: `${FB_CDN}/v/1001_2048.jpg`, width: 2048, height: 2048 }, url: 'https://www.facebook.com/photo/?fbid=1001&set=a.1' }
        return json({ data: { node: { pageItems: { edges: [{ node: { node: photo } }], page_info: { has_next_page: true, end_cursor: 'ph-2' } } } } })
      }
      // Không có `__typename`: vẫn đọc theo `viewer_image` như tiện ích mẫu.
      return json({ data: { node: { pageItems: { edges: [{ node: { node: { id: '1002', viewer_image: { uri: `${FB_CDN}/v/1002_1440.jpg`, width: 1440, height: 1080 } } } }], page_info: { has_next_page: false } } } } })
    }
    if (name === 'ProfileCometAppCollectionReelsRendererPaginationQuery') {
      // Nhiều dòng JSON (dòng sau có `label`), mở đầu bằng `for (;;);`.
      const first = { data: { node: { aggregated_fb_shorts: { edges: [{ profile_reel_node: { node: { creation_time: 1730000000, message: { text: 'Reel\n1' }, attachments: [{ media: fbVideo('3001') }] } } }], page_info: { has_next_page: false } } } } }
      return `for (;;);${json(first)}\n${json({ label: 'FBUnifiedVideoFeedbackBar_feedback', data: { feedback: { total_comment_count: 3 } } })}`
    }
    if (name === 'PagesCometChannelTabAllVideosCardImplPaginationQuery') {
      return json({ data: { node: { all_videos: { edges: [{ node: { id: '2001' } }, { node: { id: '2002' } }], page_info: { has_next_page: false } } } } })
    }
    if (name === 'FBUnifiedVideoRootWithEntrypointQuery') {
      const id = String(variables.initial_node_id)
      const video = id === '2001' ? fbVideo('2001') : { __typename: 'Video', id }
      return json({ data: { video: { creation_story: { creation_time: 1710000000, message: { text: `Clip ${id}` }, attachments: [{ media: video }] } } } })
    }
    return json({ errors: [{ message: 'GraphQL document with ID was not found', code: 1675012 }], data: null })
  })
  await loadExtension(page, 'https://www.facebook.com/trangthu')
  const messages = await runBulk(page)

  expect(messages[0].social.head).toMatchObject({ id: 'trangthu', name: 'Trang Thử', avatar: `${FB_CDN}/p/avatar.jpg` })
  const entries = entriesOf(messages)
  expect(entries.map((entry) => [entry.source, entry.id, entry.kind])).toEqual([
    ['stories', '4001', 'photo'],
    ['highlights', '4101', 'video'],
    ['photos', '1001', 'photo'],
    ['photos', '1002', 'photo'],
    ['reels', '3001', 'video'],
    ['videos', '2001', 'video'],
    ['videos', '2002', 'video'],
  ])
  const byId = Object.fromEntries(entries.map((entry) => [entry.id, entry]))
  expect(byId['4101']).toMatchObject({ album: 'Đà Lạt', url: `${FB_CDN}/o1/4101-hd.mp4` })
  expect(byId['1001'].url).toBe(`${FB_CDN}/v/1001_2048.jpg`)
  expect(byId['1002']).toMatchObject({ url: `${FB_CDN}/v/1002_1440.jpg`, page: 'https://www.facebook.com/photo/?fbid=1002' })
  expect(byId['3001']).toMatchObject({ url: `${FB_CDN}/o1/3001-hd.mp4`, timestamp: 1730000000, title: 'Reel 1' })
  expect(byId['2001']).toMatchObject({ url: `${FB_CDN}/o1/2001-hd.mp4`, timestamp: 1710000000, title: 'Clip 2001' })
  expect(byId['2002']).toMatchObject({ url: null, page: 'https://www.facebook.com/watch/?v=2002' })
  expect(messages.at(-1)?.social.done).toEqual({ complete: true, error: null })

  expect(fetched, 'dữ liệu hồ sơ không có trong tab ⇒ tải HTML trang hồ sơ một lần').toEqual(['/trangthu'])
  expect(requests.map((request) => [request.name, request.doc])).toEqual([
    ['StoriesSuspenseContentPaneRootWithEntryPointQuery', '25170215872629838'],
    ['ProfileCometFeaturedHighlightsViewPaginationQuery', '29865722229740235'],
    ['StoriesSuspenseContentPaneRootWithEntryPointQuery', '25170215872629838'],
    ['ProfileCometAppCollectionPhotosRendererPaginationQuery', '24168515422830962'],
    ['ProfileCometAppCollectionPhotosRendererPaginationQuery', '24168515422830962'],
    ['ProfileCometAppCollectionReelsRendererPaginationQuery', '25443625298595118'],
    ['PagesCometChannelTabAllVideosCardImplPaginationQuery', '26070921649175153'],
    ['FBUnifiedVideoRootWithEntrypointQuery', '25333743559590947'],
    ['FBUnifiedVideoRootWithEntrypointQuery', '25333743559590947'],
  ])
  expect(requests[0].variables).toEqual({ bucketID: '555', focusCommentID: null, scale: 1 })
  expect(requests[1].variables).toEqual({ scale: 1, id: highlightId, count: 9, cursor: '' })
  expect(requests[2].variables).toMatchObject({ bucketID: '666' })
  expect(requests[3].variables).toEqual({ scale: 1, id: 'cGhvdG9zX2J5', count: 8, cursor: '' })
  expect(requests[4].variables).toMatchObject({ cursor: 'ph-2' })
  expect(requests[5].variables).toMatchObject({ id: 'cmVlbHM=', count: 10, cursor: '' })
  expect(requests[6].variables).toMatchObject({ pageID: '200055', id: '200055', count: 6 })
  expect(requests[6].variables).not.toHaveProperty('cursor')
  expect(requests[7].variables).toMatchObject({ initial_node_id: '2001', count: 1 })
  // Như web Facebook: tài khoản (cookie `c_user`), mã fb_dtsg / lsd của trang, tên truy vấn.
  expect([requests[0].body.get('av'), requests[0].body.get('__user'), requests[0].body.get('fb_dtsg'), requests[0].body.get('lsd'), requests[0].body.get('__a'), requests[0].body.get('__comet_req')]).toEqual(['100099', '100099', 'dtsg-fb', 'lsd-fb', '1', '15'])
  expect(requests[0].headers).toMatchObject({ 'x-fb-friendly-name': 'StoriesSuspenseContentPaneRootWithEntryPointQuery', 'x-fb-lsd': 'lsd-fb' })
  expect(page.url()).toBe('https://www.facebook.com/trangthu')
})

test('Facebook nhóm: ảnh chỉ có mã ⇒ hỏi trang ảnh lấy bản lớn; bị chặn tạm (1675004) ⇒ dừng, báo mã, giữ phần đã có', async ({ page }) => {
  const header = { group: { id: '12345', name: 'Nhóm Ảnh', url: 'https://www.facebook.com/groups/12345/' } }
  const { requests } = fbRoutes(page, header, (name, variables) => {
    if (name === 'GroupsCometMediaPhotosTabGridQuery') {
      return JSON.stringify({ data: { node: { group_mediaset: { media: { edges: [{ node: { id: '5001' } }, { node: { id: '5002' } }], page_info: { has_next_page: false } } } } } })
    }
    if (name === 'CometPhotoRootContentQuery' && variables.nodeID === '5001') {
      const uri = JSON.stringify(`${FB_CDN}/v/5001_full.jpg?oh=2`).slice(1, -1).replace(/\//g, '\\/')
      return `{"data":{"currMedia":{"id":"5001","prefetch_uris_v2":[{"uri":"${uri}"}]}}}`
    }
    return JSON.stringify({ errors: [{ code: 1675004, summary: 'Rate limit exceeded', message: 'Rate limit exceeded' }], data: null })
  })
  await loadExtension(page, 'https://www.facebook.com/groups/12345/')
  const messages = await runBulk(page)
  expect(messages[0].social.head).toMatchObject({ id: 'groups.12345', name: 'Nhóm Ảnh' })
  expect(requests.map((request) => request.name)).toEqual(['GroupsCometMediaPhotosTabGridQuery', 'CometPhotoRootContentQuery', 'CometPhotoRootContentQuery'])
  expect(requests[0].variables).toEqual({ count: 8, scale: 1, id: '12345', cursor: '' })
  expect(entriesOf(messages).map((entry) => [entry.source, entry.id, entry.url])).toEqual([['photos', '5001', `${FB_CDN}/v/5001_full.jpg?oh=2`]])
  expect(messages.at(-1)?.social.done).toEqual({ complete: false, error: 'social.rate_limited' })
})

// --- App: hộp Tải cả kênh ở chế độ hàng loạt ---

function mockSocialDesktop(early: boolean) {
  type Args = Record<string, unknown>
  type Callback = (message: unknown) => void
  const callbacks = new Map<number, Callback>()
  const listeners: { event: string; id: number }[] = []
  const calls: { cmd: string; args: Args }[] = []
  let nextId = 1
  let sink: Callback | null = null
  const entry = (id: string, source: string, kind: string, extra: Record<string, unknown> = {}) => ({
    id,
    url: `https://www.instagram.com/p/${id}/`,
    title: '',
    timestamp: Date.parse('2026-10-20T12:00:00') / 1000,
    durationSeconds: null,
    viewCount: null,
    thumbnail: null,
    blocked: null,
    downloaded: false,
    source,
    kind,
    file: `https://scontent.cdninstagram.com/${id}.jpg`,
    page: null,
    album: null,
    ...extra,
  })
  const request = { listingId: 'sb-abcdef123456', url: 'https://www.instagram.com/nasa/', platform: 'instagram' }
  const commands: Record<string, (args: Args) => unknown> = {
    'plugin:event|listen': (args) => {
      listeners.push({ event: String(args.event), id: Number(args.handler) })
      return args.handler
    },
    'plugin:event|unlisten': (args) => {
      const index = listeners.findIndex((item) => item.id === Number(args.eventId))
      if (index >= 0) listeners.splice(index, 1)
      return null
    },
    list_downloads: () => [],
    default_download_dir: () => 'C:/Users/Test/Downloads/UTD',
    disk_space: () => ({ freeBytes: 5e11, totalBytes: 1e12, label: 'C:' }),
    get_update_check: () => ({ ready: true, enabled: false, available: null, installing: false }),
    take_media_request: () => null,
    take_batch_request: () => null,
    take_social_request: () => (early ? request : null),
    social_watch: (args) => {
      sink = (args.onEvent as { onmessage: Callback }).onmessage
      const send = sink
      setTimeout(() => send({ kind: 'head', head: { key: 'instagram.com:528817151', name: 'NASA', handle: '@nasa' } }), 20)
      setTimeout(() => send({ kind: 'profile', profile: { name: 'NASA', avatar: null, followers: 97000000, videoCount: null, private: false } }), 30)
      setTimeout(
        () =>
          send({
            kind: 'entries',
            entries: [
              entry('p1', 'posts', 'photo', { title: 'Thiên hà' }),
              entry('p2', 'posts', 'video'),
              entry('p3', 'posts', 'photo', { downloaded: true }),
              entry('r1', 'reels', 'video', { title: 'Phóng tàu' }),
              entry('s1', 'stories', 'photo'),
              entry('h1', 'highlights', 'video', { album: 'Sao Hoả' }),
            ],
          }),
        40,
      )
      return null
    },
    social_control: () => true,
    social_close: () => true,
    add_social_downloads: (args) => ({
      tasks: ((args.input as { ids: string[] }).ids ?? []).map((id) => ({
        id: `task-${id}`, url: `https://scontent.cdninstagram.com/${id}.jpg`, finalUrl: null, fileName: `${id}.jpg`, destDir: 'C:/Users/Test/Downloads/UTD/NASA',
        tempPath: '', kind: 'http', status: 'queued', totalBytes: null, downloadedBytes: 0, etag: null, lastModified: null, expectedSha256: null,
        hashState: 'unavailable', connections: 1, error: null, createdAt: 1, updatedAt: 1, completedAt: null,
      })),
      failed: 0,
    }),
  }
  const target = window as unknown as Record<string, unknown>
  target.isTauri = true
  target.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined }
  target.__TAURI_INTERNALS__ = {
    transformCallback: (callback: Callback) => {
      const id = nextId++
      callbacks.set(id, callback)
      return id
    },
    unregisterCallback: (id: number) => callbacks.delete(id),
    convertFileSrc: (path: string) => path,
    invoke: async (cmd: string, args: Args = {}) => {
      calls.push({ cmd, args })
      const run = commands[cmd]
      return run ? run(args) : Promise.reject(`chưa giả lệnh ${cmd}`)
    },
  }
  target.__utd = {
    calls: (cmd: string) => calls.filter((call) => call.cmd === cmd).map((call) => JSON.parse(JSON.stringify(call.args, (key, value) => (key === 'onEvent' ? undefined : value)))),
    emit: (event: string, payload: unknown) => {
      for (const item of listeners.filter((entry) => entry.event === event)) callbacks.get(item.id)?.({ event, id: item.id, payload })
    },
    listening: (event: string) => listeners.filter((item) => item.event === event).length,
    send: (message: unknown) => sink?.(message),
    request,
  }
}

type Probe = {
  calls: (cmd: string) => Record<string, unknown>[]
  emit: (event: string, payload: unknown) => void
  listening: (event: string) => number
  send: (message: unknown) => void
  request: unknown
}
const app = (page: Page) => ({
  calls: (cmd: string) => page.evaluate((name) => (window as unknown as { __utd: Probe }).__utd.calls(name), cmd),
  send: (message: unknown) => page.evaluate((body) => (window as unknown as { __utd: Probe }).__utd.send(body), message),
  open: async () => {
    await expect.poll(() => page.evaluate(() => (window as unknown as { __utd: Probe }).__utd.listening('utd://social-request'))).toBe(1)
    await page.evaluate(() => {
      const probe = (window as unknown as { __utd: Probe }).__utd
      probe.emit('utd://social-request', probe.request)
    })
  },
})

test('S-61 hàng loạt: danh sách từ trình duyệt mở hộp; chip loại kèm số, Ảnh/Video; bật chip ⇒ chỉ lọc theo đó, báo tiện ích; Tải N mục gửi đúng id rồi đóng', async ({ page }) => {
  await page.addInitScript(mockSocialDesktop, false)
  await page.goto('/')
  const desktop = app(page)
  await desktop.open()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('heading', { name: 'Tải hàng loạt' })).toBeVisible()
  await expect(dialog.locator('[data-s="S-61"]')).toBeVisible()
  await expect(dialog.getByText('NASA', { exact: true })).toBeVisible()
  const chips = dialog.getByRole('group', { name: 'Loại' })
  await expect(chips.getByRole('button')).toHaveText(['Bài viết3', 'Reels1', 'Tin1', 'Tin nổi bật1', 'Ảnh3', 'Video3'])
  // Mặc định: không chip nào bật = mọi mục chưa tải (p3 đã tải bị ẩn vì "Chỉ mục mới").
  await expect(chips.getByRole('button', { pressed: true })).toHaveCount(0)
  await expect(dialog.getByRole('button', { name: 'Tải 5 mục' })).toBeVisible()
  await expect(dialog.getByRole('checkbox', { name: 'Tin nổi bật · Sao Hoả' })).toBeChecked()

  // Bật "Reels" + "Tin nổi bật": danh sách và nút đổi ngay; tiện ích được báo chỉ lấy hai nhóm đó.
  await chips.getByRole('button', { name: /^Reels/ }).click()
  await chips.getByRole('button', { name: /^Tin nổi bật/ }).click()
  await expect(dialog.getByRole('button', { name: 'Tải 2 mục' })).toBeVisible()
  await expect
    .poll(async () => (await desktop.calls('social_control')).at(-1))
    .toEqual({ listingId: 'sb-abcdef123456', go: true, sources: ['reels', 'highlights'], since: null })
  // Thêm "Video": cả hai mục đều là video.
  await chips.getByRole('button', { name: /^Video/ }).click()
  await expect(dialog.getByRole('button', { name: 'Tải 2 mục' })).toBeVisible()

  // Dừng: tiện ích thôi lấy; danh sách giữ nguyên.
  await dialog.getByRole('button', { name: 'Dừng' }).click()
  await expect
    .poll(async () => (await desktop.calls('social_control')).at(-1))
    .toEqual({ listingId: 'sb-abcdef123456', go: false, sources: null, since: null })
  await desktop.send({ kind: 'done', total: 6, complete: false, error: null })
  await expect(dialog.getByRole('button', { name: 'Dừng' })).toHaveCount(0)

  await dialog.getByRole('button', { name: 'Tải 2 mục' }).click()
  await expect(page.getByRole('dialog')).toHaveCount(0)
  expect(await desktop.calls('add_social_downloads')).toEqual([
    { input: { listingId: 'sb-abcdef123456', ids: ['r1', 'h1'], subfolder: 'NASA', later: true } },
  ])
  expect(await desktop.calls('social_close')).toEqual([{ listingId: 'sb-abcdef123456' }])
})

test('S-61 hàng loạt: bật riêng "Ảnh" ⇒ chỉ ảnh của mọi nhóm; bật rồi tắt hết các nhóm ⇒ quay về tất cả', async ({ page }) => {
  await page.addInitScript(mockSocialDesktop, false)
  await page.goto('/')
  await app(page).open()
  const dialog = page.getByRole('dialog')
  const chips = dialog.getByRole('group', { name: 'Loại' })
  await expect(dialog.getByRole('button', { name: 'Tải 5 mục' })).toBeVisible()
  // Chỉ Ảnh: p1, s1 (p3 đã tải bị ẩn); các nhóm không cần đụng tới.
  await chips.getByRole('button', { name: /^Ảnh/ }).click()
  await expect(dialog.getByRole('button', { name: 'Tải 2 mục' })).toBeVisible()
  await expect(dialog.getByText('Không có mục khớp bộ lọc.')).toHaveCount(0)
  await chips.getByRole('button', { name: /^Ảnh/ }).click()
  // Số trên chip Ảnh / Video đếm trong các nhóm đang bật: chỉ Reels (toàn video) ⇒ "Ảnh 0", khớp danh sách.
  await chips.getByRole('button', { name: /^Reels/ }).click()
  await expect(chips.getByRole('button', { name: /^(Ảnh|Video)/ })).toHaveText(['Ảnh0', 'Video1'])
  await chips.getByRole('button', { name: /^Reels/ }).click()
  await expect(chips.getByRole('button', { name: /^(Ảnh|Video)/ })).toHaveText(['Ảnh3', 'Video3'])
  // Bật cả 4 nhóm rồi tắt cả 4: không chip nào bật = tất cả.
  const groups = [/^Bài viết/, /^Reels/, /^Tin\s*\d/, /^Tin nổi bật/]
  for (const name of groups) await chips.getByRole('button', { name }).click()
  await expect(chips.getByRole('button', { pressed: true })).toHaveCount(4)
  for (const name of groups) await chips.getByRole('button', { name }).click()
  await expect(chips.getByRole('button', { pressed: true })).toHaveCount(0)
  await expect(dialog.getByRole('button', { name: 'Tải 5 mục' })).toBeVisible()
  // Tiện ích được báo quay về lấy mọi nhóm, không giữ nhóm hẹp lúc trước.
  await expect
    .poll(async () => (await app(page).calls('social_control')).at(-1))
    .toEqual({ listingId: 'sb-abcdef123456', go: true, sources: null, since: null })
})

test('S-63 hàng loạt: bị chặn tạm ⇒ câu rõ, không có nút thử lại; a11y không lỗi', async ({ page }) => {
  await page.addInitScript(mockSocialDesktop, true)
  await page.goto('/')
  const dialog = page.getByRole('dialog')
  // Danh sách tới lúc UTD vừa mở (chưa kịp nghe) vẫn mở hộp.
  await expect(dialog.getByRole('heading', { name: 'Tải hàng loạt' })).toBeVisible()
  await expect(dialog.getByRole('checkbox')).toHaveCount(5)
  await app(page).send({
    kind: 'done',
    total: 6,
    complete: false,
    error: '[social.rate_limited] Trang tạm chặn vì gọi nhiều. Đợi vài phút rồi thử lại; danh sách đã có vẫn tải được.',
  })
  await expect(dialog.getByText('Trang tạm chặn vì gọi nhiều.', { exact: false })).toBeVisible()
  await expect(dialog.getByRole('button', { name: 'Thử lại' })).toHaveCount(0)
  await page.waitForFunction(() => document.getAnimations().every((animation) => animation.playState !== 'running'))
  const result = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze()
  expect(result.violations).toEqual([])
  await dialog.getByRole('button', { name: 'Huỷ' }).click()
  expect(await app(page).calls('social_close')).toEqual([{ listingId: 'sb-abcdef123456' }])
})
