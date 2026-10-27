import { expect, test, type Page } from '@playwright/test'

// Douyin (notes/douyin.md): extensions/*/douyin-page.js gọi API web của chính Douyin trong trang (được trang tự ký),
// douyin-channel.js lật danh sách hồ sơ rồi đẩy sang UTD. API giả qua page.route, dữ liệu theo hình dạng thật
// (jiji262/douyin-downloader, yt-dlp DouyinIE). Không mạng thật, không đăng nhập.

const CDN = 'https://v3-web.douyinvod.com'
const ID = '7300000000000000001'

const ladder = (name: string) => [
  // Bản DASH chỉ có hình: bỏ.
  { gear_name: 'adapt_1080_dash', format: 'dash', play_addr: { url_list: [`${CDN}/${name}/dash.mp4`], width: 1080, height: 1920 } },
  // Cùng 1080p: H.265 và H.264 ⇒ giữ H.264.
  { gear_name: 'normal_1080_0', is_h265: 1, play_addr: { url_list: [`${CDN}/${name}/1080-h265.mp4`], width: 1080, height: 1920, data_size: 8_000_000 } },
  {
    gear_name: 'normal_1080_1',
    is_h265: 0,
    play_addr: {
      // Bản watermark và link /aweme/v1/play/ xếp sau máy chủ CDN.
      url_list: [`https://www.douyin.com/aweme/v1/play/?video_id=v0${name}`, `${CDN}/${name}/1080-playwm.mp4`, `${CDN}/${name}/1080.mp4`],
      width: 1080,
      height: 1920,
      data_size: 9_000_000,
    },
  },
  { gear_name: 'normal_720_0', is_h265: 0, play_addr: { url_list: [`${CDN}/${name}/720.mp4`], width: 720, height: 1280, data_size: 5_000_000 } },
]

const aweme = (id: string, name: string, extra: Record<string, unknown> = {}) => ({
  aweme_id: id,
  aweme_type: 0,
  desc: `Video ${name}\n#douyin`,
  create_time: 1_760_000_000,
  author: { nickname: 'Người đăng Douyin', sec_uid: 'MS4wLjABAAAAtest' },
  video: {
    duration: 12_500,
    play_addr: { url_list: [`${CDN}/${name}/base.mp4`], width: 1080, height: 1920 },
    download_addr: { url_list: [`${CDN}/${name}/playwm.mp4`] },
    bit_rate: ladder(name),
    cover: { url_list: [`https://p3-pc.douyinpic.com/${name}.jpeg`] },
  },
  ...extra,
})

const PAGE = '<!doctype html><html><head><meta charset="utf-8"><title>抖音</title></head><body><p>douyin</p></body></html>'

type Found = { title: string | null; formats: { id: string; sizeBytes: number | null; streams: { url: string; kind: string }[] }[] }

async function serve(page: Page, api: (url: URL) => { status?: number; body: string }) {
  const asked: URL[] = []
  await page.route('https://www.douyin.com/**', (route) => {
    const url = new URL(route.request().url())
    if (!url.pathname.startsWith('/aweme/')) return route.fulfill({ contentType: 'text/html', body: PAGE })
    asked.push(url)
    const reply = api(url)
    return route.fulfill({ status: reply.status ?? 200, contentType: 'application/json', body: reply.body })
  })
  await page.goto('https://www.douyin.com/?recommend=1')
  await page.addScriptTag({ path: 'extensions/chromium/douyin-page.js' })
  return asked
}

const call = <T>(page: Page, method: string, ...args: unknown[]) =>
  page.evaluate(
    ([name, values]) =>
      (window as unknown as Record<symbol, Record<string, (...a: unknown[]) => Promise<unknown>>>)[Symbol.for('utd.douyin')][name as string](
        ...(values as unknown[]),
      ),
    [method, args] as const,
  ) as Promise<T>

test('video Douyin: API chi tiết trong trang ⇒ MP4 có tiếng không watermark, H.264 trước, bỏ DASH; không tự điền chữ ký', async ({ page }) => {
  const asked = await serve(page, () => ({ body: JSON.stringify({ status_code: 0, aweme_detail: aweme(ID, 'a') }) }))
  const found = await call<Found>(page, 'media', `https://www.douyin.com/video/${ID}`)
  expect(found.title).toBe('Video a #douyin')
  expect(found.formats).toEqual([
    { id: '1080', sizeBytes: 9_000_000, streams: [{ url: `${CDN}/a/1080.mp4`, kind: 'both', sizeBytes: 9_000_000 }] },
    { id: '720', sizeBytes: 5_000_000, streams: [{ url: `${CDN}/a/720.mp4`, kind: 'both', sizeBytes: 5_000_000 }] },
  ])
  expect(asked[0].pathname).toBe('/aweme/v1/web/aweme/detail/')
  expect(asked[0].searchParams.get('aweme_id')).toBe(ID)
  expect(asked[0].searchParams.get('aid')).toBe('6383')
  for (const owned of ['msToken', 'a_bogus', 'X-Bogus', 'uifid', 'verifyFp', 'x-secsdk-web-signature']) {
    expect(asked[0].searchParams.has(owned), `${owned} do trang tự thêm`).toBe(false)
  }
  // Khung `?modal_id=`: cùng mã.
  const modal = await call<Found>(page, 'media', `https://www.douyin.com/jingxuan?modal_id=${ID}`)
  expect(modal.formats.length).toBe(2)
})

test('Douyin chặn (403 ArgusSecurityPlugin) ⇒ dùng bài đang phát trong trình phát của trang; không có thì rỗng, không treo', async ({ page }) => {
  await serve(page, () => ({ status: 403, body: 'Blocked by ArgusSecurityPlugin Uifid Not Found' }))
  const empty = await call<Found & { error: string }>(page, 'media', `https://www.douyin.com/video/${ID}`)
  expect(empty.formats).toEqual([])
  expect(empty.error).toBe('blocked')
  // window.player kiểu camelCase (douyin-dl-user-js): playAddr [{ src }], bitRateList.
  await page.evaluate((id) => {
    const awemeInfo = {
      awemeId: id,
      desc: 'Đang phát',
      video: {
        width: 720,
        height: 1280,
        bitRateList: [{ gearName: 'normal_720_0', isH265: 0, width: 720, height: 1280, dataSize: 4_000_000, playAddr: [{ src: '//v26-web.douyinvod.com/p/720.mp4' }] }],
      },
    }
    ;(window as unknown as { player: unknown }).player = { _config: { _config: { pluginsConfig: { WatchLaterPlugin: { awemeInfo } } } } }
  }, ID)
  const playing = await call<Found>(page, 'media', `https://www.douyin.com/video/${ID}`)
  expect(playing.title).toBe('Đang phát')
  expect(playing.formats).toEqual([
    { id: '720', sizeBytes: 4_000_000, streams: [{ url: 'https://v26-web.douyinvod.com/p/720.mp4', kind: 'both', sizeBytes: 4_000_000 }] },
  ])
})

test('window.player (camelCase) cũng bỏ bản bytevc2 và nhận cờ HEVC (isBytevc1, isH265) để H.264 đứng trước cùng mức', async ({ page }) => {
  await serve(page, () => ({ status: 403, body: 'Blocked by ArgusSecurityPlugin Uifid Not Found' }))
  await page.evaluate((id) => {
    const rate = (gearName: string, flags: Record<string, number>, src: string, dataSize: number, height = 1280) => ({
      gearName,
      ...flags,
      width: (height * 9) / 16,
      height,
      dataSize,
      playAddr: [{ src }],
    })
    const awemeInfo = {
      awemeId: id,
      desc: 'Đang phát',
      video: {
        bitRateList: [
          // Cùng 720p: HEVC (cờ camelCase) lớn hơn H.264 nhưng phải xếp sau; bytevc2 ở 1080p bị bỏ.
          rate('normal_720_0', { isBytevc1: 1 }, '//v26-web.douyinvod.com/p/720-hevc.mp4', 9_000_000),
          rate('normal_720_1', { isBytevc1: 0 }, '//v26-web.douyinvod.com/p/720-h264.mp4', 5_000_000),
          rate('normal_540_0', { isH265: 1 }, '//v26-web.douyinvod.com/p/540-hevc.mp4', 2_000_000, 960),
          rate('bytevc2_1080', {}, '//v26-web.douyinvod.com/p/1080-vc2.mp4', 12_000_000, 1920),
        ],
      },
    }
    ;(window as unknown as { player: unknown }).player = { _config: { _config: { pluginsConfig: { WatchLaterPlugin: { awemeInfo } } } } }
  }, ID)
  const found = await call<Found>(page, 'media', `https://www.douyin.com/video/${ID}`)
  expect(found.formats.map((format) => [format.id, format.streams[0].url])).toEqual([
    ['720', 'https://v26-web.douyinvod.com/p/720-h264.mp4'],
    ['540', 'https://v26-web.douyinvod.com/p/540-hevc.mp4'],
  ])
})

test('hồ sơ Douyin: một trang aweme/post ⇒ mục có link, thời lượng, ảnh bìa, các bản MP4; bài ảnh không có bản nào', async ({ page }) => {
  const asked = await serve(page, () => ({
    body: JSON.stringify({
      status_code: 0,
      max_cursor: 1_759_000_000_000,
      has_more: 1,
      aweme_list: [aweme(ID, 'a'), { aweme_id: '7300000000000000002', aweme_type: 68, desc: 'Ảnh', images: [{ url_list: ['https://p3/x.webp'] }], video: {} }],
    }),
  }))
  const result = await call<{ entries: Record<string, unknown>[]; cursor: string; hasMore: boolean; author: { name: string } }>(
    page,
    'posts',
    'MS4wLjABAAAAtest',
    '0',
  )
  expect(asked[0].pathname).toBe('/aweme/v1/web/aweme/post/')
  expect(asked[0].searchParams.get('sec_user_id')).toBe('MS4wLjABAAAAtest')
  expect(asked[0].searchParams.get('max_cursor')).toBe('0')
  expect(result.cursor).toBe('1759000000000')
  expect(result.hasMore).toBe(true)
  expect(result.author).toEqual({ name: 'Người đăng Douyin' })
  expect(result.entries[0]).toEqual({
    id: ID,
    url: `https://www.douyin.com/video/${ID}`,
    title: 'Video a #douyin',
    timestamp: 1_760_000_000,
    durationSeconds: 13,
    thumbnail: 'https://p3-pc.douyinpic.com/a.jpeg',
    variants: [
      { url: `${CDN}/a/1080.mp4`, height: 1080, sizeBytes: 9_000_000, h265: false },
      { url: `${CDN}/a/720.mp4`, height: 720, sizeBytes: 5_000_000, h265: false },
    ],
  })
  expect(result.entries[1]).toMatchObject({ url: 'https://www.douyin.com/note/7300000000000000002', variants: [] })
})

// Tab giả: tài liệu đang mở (host + mã `documentId`) và dữ liệu `window[Symbol.for("utd.douyin")]` của tài liệu đó nếu
// là trang lạ tự định nghĩa (douyin-page.js bỏ qua khi đã có sẵn).
type Doc = { host: string; id: string; data?: unknown }
type StubInput = {
  /** Kết quả lần lượt của lời gọi trong trang Douyin (`posts`, `media`). */
  pages: unknown[]
  /** Trình duyệt có trả `documentId` (Chromium ≥ 106) hay không (Firefox có thể không). */
  documentIds: boolean
  /** Người dùng mở trang khác trong cùng tab: sau lần đọc host (`probe`) hoặc lời gọi `posts` thứ `count`. */
  navigate?: { after: 'probe' | 'posts'; count: number; to: Doc }
  /** App trả `ok: false` (hộp đã dừng) từ lần đẩy danh sách có mục thứ N. */
  stopAfterEntries?: number
}
type Call = { name: string; args: unknown[] }

// douyin-channel.js thật trong background.js thật, API trình duyệt giả: executeScript trả các trang danh sách giả.
// Không `world` = đọc host ở thế giới ISOLATED (`probe`); `world: "MAIN"` chạy trong tài liệu hiện tại của tab, hoặc
// ném lỗi nếu nhắm `documentIds` của tài liệu đã đi (như Chromium).
function stubBackground(input: StubInput) {
  const calls: Call[] = []
  const queue = [...input.pages]
  let doc: Doc = { host: 'www.douyin.com', id: 'doc-1' }
  let probes = 0
  let posts = 0
  let withEntries = 0
  const leave = (when: 'probe' | 'posts', count: number) => {
    if (input.navigate?.after === when && input.navigate.count === count) doc = { ...input.navigate.to }
  }
  const noop = { addListener: () => undefined }
  ;(window as unknown as Record<string, unknown>).chrome = {
    i18n: { getMessage: () => '' },
    runtime: {
      id: 'utd',
      getURL: (file: string) => `chrome-extension://utd/${file}`,
      onInstalled: noop,
      onStartup: noop,
      onMessage: noop,
      sendNativeMessage: async (_host: string, message: { type: string; entries?: unknown[] }) => {
        calls.push({ name: 'native', args: [message] })
        if (message.type === 'channel-entries' && (message.entries?.length ?? 0) > 0) {
          withEntries += 1
          if (input.stopAfterEntries !== undefined && withEntries >= input.stopAfterEntries) {
            return { ok: false, message: 'Hộp Tải cả kênh đã dừng.' }
          }
        }
        return { ok: true, taskId: null }
      },
    },
    contextMenus: { removeAll: async () => undefined, create: async () => undefined, onClicked: noop },
    action: { setBadgeText: async () => undefined },
    storage: { local: { get: async () => ({}), set: async () => undefined } },
    cookies: { getAll: async () => { calls.push({ name: 'cookies', args: [] }); return [] }, getAllCookieStores: async () => [] },
    scripting: {
      getRegisteredContentScripts: async () => [],
      registerContentScripts: async () => undefined,
      executeScript: async (options: {
        target: { tabId: number; frameIds?: number[]; documentIds?: string[] }
        world?: string
        files?: string[]
        args?: unknown[]
      }) => {
        if (!options.world) {
          probes += 1
          calls.push({ name: 'probe', args: [{ host: doc.host }] })
          const reply = [{ result: doc.host, ...(input.documentIds ? { documentId: doc.id } : {}) }]
          leave('probe', probes)
          return reply
        }
        const wanted = options.target.documentIds
        if (wanted && !wanted.includes(doc.id)) throw new Error(`No document with id ${wanted[0]}`)
        calls.push({ name: 'execute', args: [{ world: options.world, files: options.files, args: options.args, target: options.target, host: doc.host }] })
        if (options.files) return [{ result: undefined }]
        posts += 1
        const result = doc.data ?? queue.shift() ?? { error: 'blocked' }
        leave('posts', posts)
        return [{ result }]
      },
    },
  }
  ;(window as unknown as Record<string, unknown>).__bg = {
    calls,
    go: (to: Doc) => {
      doc = { ...to }
    },
  }
}

type Background = {
  douyinChannel: (url: string, tab: unknown) => Promise<boolean>
  pageMedia: (point: { x: number; y: number; url: string }, tab: unknown) => Promise<{ ok: boolean; media?: { formats: unknown[] } }>
  __bg: { calls: Call[]; go: (to: Doc) => void }
  __run?: Promise<boolean>
}

const PROFILE = 'https://www.douyin.com/user/MS4wLjABAAAAtest'
const TAB = { id: 3, url: PROFILE }

async function openBackground(page: Page, input: StubInput) {
  await page.route('http://ext.utd.test/**', (route) => route.fulfill({ contentType: 'text/html', body: '<p>x</p>' }))
  await page.addInitScript(stubBackground, input)
  await page.goto('http://ext.utd.test/')
  await page.addScriptTag({ path: 'extensions/chromium/background.js' })
  await page.addScriptTag({ path: 'extensions/chromium/douyin-channel.js' })
}

const runChannel = (page: Page, url = PROFILE) =>
  page.evaluate(([link, tab]) => (window as unknown as Background).douyinChannel(link, tab), [url, TAB] as const)

const backgroundCalls = (page: Page) => page.evaluate(() => (window as unknown as Background).__bg.calls)

type Part = { type: string; url: string; entries?: { id: string; variants: { url: string }[] }[]; done?: boolean; error?: string }
const nativeParts = (calls: Call[]) => calls.filter((call) => call.name === 'native').map((call) => call.args[0] as Part)
const channelParts = (calls: Call[]) => nativeParts(calls).filter((part) => part.type === 'channel-entries')
const executedIn = (calls: Call[]) =>
  calls.filter((call) => call.name === 'execute').map((call) => call.args[0] as { world: string; files?: string[]; args?: unknown[]; target: Record<string, unknown>; host: string })

test('tải cả hồ sơ Douyin: báo UTD trước, mở hộp kênh, lật từng trang trong tab rồi đẩy dần; không đọc cookie', async ({ page }) => {
  test.setTimeout(30_000)
  const entry = (id: string) => ({
    id,
    url: `https://www.douyin.com/video/${id}`,
    title: `Video ${id}`,
    timestamp: 1_760_000_000,
    durationSeconds: 12,
    thumbnail: null,
    variants: [{ url: `${CDN}/${id}/720.mp4`, height: 720, sizeBytes: 5_000_000, h265: false }, { url: 'javascript:alert(1)', height: 1 }],
  })
  const pages = [
    { entries: [entry('7300000000000000001'), { id: 'x', url: 'https://evil.test/video/1' }], cursor: '100', hasMore: true, author: { name: 'Người đăng' } },
    { entries: [entry('7300000000000000003')], cursor: '200', hasMore: false, author: { name: 'Người đăng' } },
  ]
  await openBackground(page, { pages, documentIds: true })
  const handled = await runChannel(page, 'https://www.douyin.com/user/MS4wLjABAAAAtest?from_tab_name=main')
  expect(handled).toBe(true)
  const calls = await backgroundCalls(page)
  const native = nativeParts(calls)
  expect(native.map((message) => [message.type, message.url, message.entries?.length, message.done])).toEqual([
    ['channel-entries', PROFILE, 0, false],
    ['download', PROFILE, undefined, undefined],
    ['channel-entries', PROFILE, 1, false],
    ['channel-entries', PROFILE, 1, true],
  ])
  expect(native[2].entries?.[0].variants).toEqual([
    { url: `${CDN}/7300000000000000001/720.mp4`, height: 720, sizeBytes: 5_000_000, h265: false },
  ])
  expect((native[2] as unknown as { author: unknown }).author).toEqual({ name: 'Người đăng' })
  const executed = executedIn(calls)
  expect(executed.every((options) => options.world === 'MAIN'), 'gọi API trong thế giới của trang').toBe(true)
  expect(executed.filter((options) => options.args).map((options) => options.args)).toEqual([
    ['posts', ['MS4wLjABAAAAtest', '0']],
    ['posts', ['MS4wLjABAAAAtest', '100']],
  ])
  // Trước mỗi lời gọi trong trang: đọc host ở thế giới riêng của tiện ích; lời gọi nhắm đúng tài liệu đã đọc.
  expect(calls.filter((call) => call.name === 'probe')).toHaveLength(2)
  expect(executed.map((options) => options.target)).toEqual(Array(4).fill({ tabId: 3, documentIds: ['doc-1'] }))
  expect(calls.some((call) => call.name === 'cookies'), 'không đọc cookie').toBe(false)
})

test('hồ sơ Douyin khi chưa đăng nhập: trang 2 rỗng dù trang 1 báo còn ⇒ giữ trang đầu, báo cần đăng nhập để xem tiếp', async ({ page }) => {
  test.setTimeout(30_000)
  const entry = {
    id: '7300000000000000001',
    url: 'https://www.douyin.com/video/7300000000000000001',
    title: 'Video',
    variants: [{ url: `${CDN}/1/720.mp4`, height: 720, sizeBytes: 5_000_000, h265: false }],
  }
  // Douyin thật (đo 2026-10-26, chưa đăng nhập): trang 1 có 19 bài + has_more=1, trang 2 rỗng + has_more=0.
  const pages = [
    { entries: [entry], cursor: '100', hasMore: true, author: { name: 'Người đăng' } },
    { entries: [], cursor: '', hasMore: false, author: null },
  ]
  await openBackground(page, { pages, documentIds: true })
  await runChannel(page)
  const parts = channelParts(await backgroundCalls(page))
  expect(parts.map((part) => [part.entries?.length, part.done, part.error ?? null])).toEqual([
    [0, false, null],
    [1, false, null],
    [0, true, 'login'],
  ])
})

// --- Sửa sau review 2026-10-27 ---

const hostile = {
  // Trang lạ tự định nghĩa `window[Symbol.for("utd.douyin")].posts`: mục có link Douyin hợp lệ nhưng bản MP4 ở máy chủ khác.
  entries: [
    {
      id: '7300000000000000009',
      url: 'https://www.douyin.com/video/7300000000000000009',
      title: 'Giả',
      variants: [{ url: 'https://attacker.test/x.mp4', height: 1080, sizeBytes: 1, h265: false }],
    },
  ],
  cursor: '',
  hasMore: false,
  author: { name: 'Giả' },
}

const firstPage = {
  entries: [
    {
      id: '7300000000000000001',
      url: 'https://www.douyin.com/video/7300000000000000001',
      title: 'Video thật',
      variants: [{ url: `${CDN}/1/720.mp4`, height: 720, sizeBytes: 5_000_000, h265: false }],
    },
  ],
  cursor: '100',
  hasMore: true,
  author: { name: 'Người đăng' },
}

for (const documentIds of [true, false]) {
  test(`tải cả hồ sơ: người dùng mở website khác trong cùng tab giữa hai trang ⇒ không gọi API trong trang lạ, không nhận mục của nó (documentId: ${documentIds})`, async ({ page }) => {
    test.setTimeout(30_000)
    const to = { host: 'evil.test', id: 'doc-2', data: hostile }
    await openBackground(page, { pages: [firstPage], documentIds, navigate: { after: 'posts', count: 1, to } })
    expect(await runChannel(page)).toBe(true)
    const calls = await backgroundCalls(page)
    const parts = channelParts(calls)
    expect(JSON.stringify(parts), 'không có gì từ trang lạ').not.toContain('attacker.test')
    expect(parts.map((part) => [part.entries?.length, part.done, part.error ?? null])).toEqual([
      [0, false, null],
      [1, false, null],
      [0, true, 'blocked'],
    ])
    const executed = executedIn(calls)
    expect(executed.every((options) => options.host === 'www.douyin.com'), 'chỉ chạy trong trang Douyin').toBe(true)
    expect(executed.filter((options) => options.args)).toHaveLength(1)
  })
}

test('tải cả hồ sơ: tab sang tài liệu Douyin khác (điều hướng) ⇒ dừng vòng, không nhận trang từ tài liệu mới', async ({ page }) => {
  test.setTimeout(30_000)
  const next = { ...hostile, entries: [{ ...hostile.entries[0], variants: [{ url: `${CDN}/9/720.mp4`, height: 720, sizeBytes: 1, h265: false }] }] }
  const to = { host: 'www.douyin.com', id: 'doc-2', data: next }
  await openBackground(page, { pages: [firstPage], documentIds: true, navigate: { after: 'posts', count: 1, to } })
  await runChannel(page)
  const calls = await backgroundCalls(page)
  const parts = channelParts(calls)
  expect(parts.map((part) => [part.entries?.length, part.done, part.error ?? null])).toEqual([
    [0, false, null],
    [1, false, null],
    [0, true, 'blocked'],
  ])
  expect(executedIn(calls).every((options) => (options.target.documentIds as string[])[0] === 'doc-1'), 'mọi lời gọi nhắm tài liệu đầu').toBe(true)
})

test('tải cả hồ sơ: tab rời đi ngay sau lúc kiểm host ⇒ lời gọi nhắm tài liệu cũ bị trình duyệt từ chối, không chạy sang trang lạ', async ({ page }) => {
  test.setTimeout(30_000)
  const to = { host: 'evil.test', id: 'doc-2', data: hostile }
  const pages = [firstPage, { ...firstPage, cursor: '200', hasMore: false }]
  await openBackground(page, { pages, documentIds: true, navigate: { after: 'probe', count: 2, to } })
  await runChannel(page)
  const calls = await backgroundCalls(page)
  expect(JSON.stringify(channelParts(calls))).not.toContain('attacker.test')
  expect(channelParts(calls).at(-1)).toMatchObject({ done: true, error: 'blocked' })
  expect(executedIn(calls).every((options) => options.host === 'www.douyin.com')).toBe(true)
})

const FOUND = {
  title: 'Video thật',
  formats: [{ id: '720', sizeBytes: 5, streams: [{ url: `${CDN}/a/720.mp4`, kind: 'both', sizeBytes: 5 }] }],
  manifests: [],
  files: [],
}

test('nút "Tải video này" trên Douyin: chỉ gọi API trong trang khi tab đang ở Douyin; tab đã sang website khác thì không chạy gì', async ({ page }) => {
  await openBackground(page, { pages: [FOUND, FOUND], documentIds: true })
  const ask = () =>
    page.evaluate(
      ([tab]) => (window as unknown as Background).pageMedia({ x: 10, y: 10, url: 'https://www.douyin.com/video/7300000000000000001' }, tab),
      [{ id: 3, url: 'https://www.douyin.com/jingxuan' }] as const,
    )
  const ok = await ask()
  expect(ok.ok).toBe(true)
  expect(ok.media?.formats).toHaveLength(1)
  const before = executedIn(await backgroundCalls(page)).length
  expect(before).toBeGreaterThan(0)
  // Người dùng mở website khác trong cùng tab; trang đó tự định nghĩa lời đáp. `tab.url` của yêu cầu vẫn là Douyin (cũ).
  await page.evaluate((data) => (window as unknown as Background).__bg.go({ host: 'evil.test', id: 'doc-2', data }), {
    ...FOUND,
    formats: [{ id: '1080', sizeBytes: 1, streams: [{ url: 'https://attacker.test/x.mp4', kind: 'both', sizeBytes: 1 }] }],
  })
  expect(await ask()).toEqual({ ok: false })
  const calls = await backgroundCalls(page)
  expect(executedIn(calls)).toHaveLength(before)
  expect(executedIn(calls).every((options) => options.host === 'www.douyin.com')).toBe(true)
})

test('hộp Tải cả kênh bị dừng hoặc đóng (app báo không nhận nữa) ⇒ tiện ích thôi lật trang, không gọi trang tiếp', async ({ page }) => {
  test.setTimeout(30_000)
  const more = { ...firstPage, cursor: '200' }
  await openBackground(page, { pages: [firstPage, more, more], documentIds: true, stopAfterEntries: 1 })
  expect(await runChannel(page)).toBe(true)
  const calls = await backgroundCalls(page)
  expect(channelParts(calls).map((part) => [part.entries?.length, part.done])).toEqual([
    [0, false],
    [1, false],
  ])
  expect(executedIn(calls).filter((options) => options.args).map((options) => options.args)).toEqual([['posts', ['MS4wLjABAAAAtest', '0']]])
})

test('bấm "Tải bằng UTD" lại trên hồ sơ đang lật trang ⇒ lượt cũ dừng, lượt mới báo UTD và mở lại hộp', async ({ page }) => {
  test.setTimeout(30_000)
  const last = { ...firstPage, hasMore: false, cursor: '300' }
  await openBackground(page, { pages: [firstPage, last], documentIds: true })
  // Lượt 1 đẩy xong trang đầu rồi nghỉ 2,5 s; trong lúc đó người dùng bấm lại.
  await page.evaluate(([link, tab]) => {
    const w = window as unknown as Background
    w.__run = w.douyinChannel(link, tab)
  }, [PROFILE, TAB] as const)
  await expect.poll(async () => channelParts(await backgroundCalls(page)).length).toBe(2)
  expect(await runChannel(page)).toBe(true)
  await page.evaluate(() => (window as unknown as Background).__run)
  const calls = await backgroundCalls(page)
  expect(nativeParts(calls).map((part) => [part.type, part.entries?.length, part.done])).toEqual([
    ['channel-entries', 0, false],
    ['download', undefined, undefined],
    ['channel-entries', 1, false],
    // Lượt mới: như lần đầu.
    ['channel-entries', 0, false],
    ['download', undefined, undefined],
    ['channel-entries', 1, true],
  ])
  expect(executedIn(calls).filter((options) => options.args).map((options) => options.args)).toEqual([
    ['posts', ['MS4wLjABAAAAtest', '0']],
    ['posts', ['MS4wLjABAAAAtest', '0']],
  ])
})
