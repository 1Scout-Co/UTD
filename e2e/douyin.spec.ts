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

// douyin-channel.js thật trong background.js thật, API trình duyệt giả: executeScript trả các trang danh sách giả.
function stubBackground(pages: unknown[]) {
  const calls: { name: string; args: unknown[] }[] = []
  const queue = [...pages]
  const noop = { addListener: () => undefined }
  ;(window as unknown as Record<string, unknown>).chrome = {
    i18n: { getMessage: () => '' },
    runtime: {
      id: 'utd',
      getURL: (file: string) => `chrome-extension://utd/${file}`,
      onInstalled: noop,
      onStartup: noop,
      onMessage: noop,
      sendNativeMessage: async (_host: string, message: unknown) => {
        calls.push({ name: 'native', args: [message] })
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
      executeScript: async (options: { world?: string; files?: string[]; args?: unknown[] }) => {
        calls.push({ name: 'execute', args: [{ world: options.world, files: options.files, args: options.args }] })
        if (options.files) return [{ result: undefined }]
        return [{ result: queue.shift() ?? { error: 'blocked' } }]
      },
    },
  }
  ;(window as unknown as Record<string, unknown>).__bg = { calls }
}

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
  await page.route('http://ext.utd.test/**', (route) => route.fulfill({ contentType: 'text/html', body: '<p>x</p>' }))
  await page.addInitScript(stubBackground, pages)
  await page.goto('http://ext.utd.test/')
  await page.addScriptTag({ path: 'extensions/chromium/background.js' })
  await page.addScriptTag({ path: 'extensions/chromium/douyin-channel.js' })
  const handled = await page.evaluate(() =>
    (window as unknown as { douyinChannel: (url: string, tab: unknown) => Promise<boolean> }).douyinChannel(
      'https://www.douyin.com/user/MS4wLjABAAAAtest?from_tab_name=main',
      { id: 3, url: 'https://www.douyin.com/user/MS4wLjABAAAAtest?from_tab_name=main' },
    ),
  )
  expect(handled).toBe(true)
  const calls = await page.evaluate(() => (window as unknown as { __bg: { calls: { name: string; args: unknown[] }[] } }).__bg.calls)
  const native = calls.filter((call) => call.name === 'native').map((call) => call.args[0] as Record<string, unknown>)
  const profile = 'https://www.douyin.com/user/MS4wLjABAAAAtest'
  expect(native.map((message) => [message.type, message.url, (message.entries as unknown[] | undefined)?.length, message.done])).toEqual([
    ['channel-entries', profile, 0, false],
    ['download', profile, undefined, undefined],
    ['channel-entries', profile, 1, false],
    ['channel-entries', profile, 1, true],
  ])
  expect((native[2].entries as Record<string, unknown>[])[0].variants).toEqual([
    { url: `${CDN}/7300000000000000001/720.mp4`, height: 720, sizeBytes: 5_000_000, h265: false },
  ])
  expect(native[2].author).toEqual({ name: 'Người đăng' })
  const executed = calls.filter((call) => call.name === 'execute').map((call) => call.args[0] as Record<string, unknown>)
  expect(executed.every((options) => options.world === 'MAIN'), 'gọi API trong thế giới của trang').toBe(true)
  expect(executed.filter((options) => options.args).map((options) => options.args)).toEqual([
    ['posts', ['MS4wLjABAAAAtest', '0']],
    ['posts', ['MS4wLjABAAAAtest', '100']],
  ])
  expect(calls.some((call) => call.name === 'cookies'), 'không đọc cookie').toBe(false)
})
