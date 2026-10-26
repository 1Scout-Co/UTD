import { expect, test, type Page } from '@playwright/test'

// Tải Tin / video Facebook (notes/fb-media.md): extensions/*/fb-media.js (ghi nhớ link file video trong JSON của trang,
// thế giới MAIN) + page-media.js (tìm video đang xem lúc bấm nút) + background.js (kiểm kết quả, gửi UTD). Trang
// Facebook giả qua page.route, dữ liệu giả theo hình dạng thật (HushFacebookWeb fixture 2026-10, yt-dlp facebook.py).
// Không mạng thật, không đăng nhập.

const CDN = 'https://video.fbcdn.test'
const CARD = 'UzpfSVNDOjIyMg==' // base64 "S:_ISC:222"
const OTHER = 'UzpfSVNDOjMzMw==' // thẻ khác cùng bucket
const NEXT = 'UzpfSVNDOjQ0NA==' // thẻ tới sau bằng GraphQL (chuyển trang trong app)

const MPD =
  '<?xml version="1.0"?><MPD xmlns="urn:mpeg:dash:schema:mpd:2011" mediaPresentationDuration="PT10S"><Period>' +
  '<AdaptationSet mimeType="video/mp4">' +
  `<Representation id="1v" codecs="av01.0.08M.08" height="1080" width="1920" bandwidth="1500000"><BaseURL>${CDN}/v1080-av1.mp4</BaseURL></Representation>` +
  `<Representation id="2v" codecs="avc1.640028" height="1080" width="1920" bandwidth="2000000"><BaseURL>${CDN}/v1080.mp4?oh=1</BaseURL></Representation>` +
  `<Representation id="3v" codecs="avc1.4d401f" height="720" width="1280" bandwidth="900000"><BaseURL>${CDN}/v720.mp4</BaseURL></Representation>` +
  '</AdaptationSet><AdaptationSet mimeType="audio/mp4">' +
  `<Representation id="4a" codecs="mp4a.40.5" bandwidth="64000"><BaseURL>${CDN}/a.mp4</BaseURL></Representation>` +
  '</AdaptationSet></Period></MPD>'

const videoMedia = (id: string, name: string, manifest = true) => ({
  __typename: 'Video',
  id,
  playable_url: `${CDN}/${name}-sd.mp4`,
  videoDeliveryLegacyFields: null,
  videoDeliveryResponseFragment: {
    videoDeliveryResponseResult: {
      progressive_urls: [
        { progressive_url: `${CDN}/${name}-hd.mp4`, failure_reason: null, metadata: { quality: 'HD' } },
        { progressive_url: `${CDN}/${name}-sd.mp4`, failure_reason: null, metadata: { quality: 'SD' } },
      ],
      dash_manifests: manifest ? [{ manifest_xml: MPD }] : [],
      hls_playlist_urls: [{ hls_playlist_url: null, failure_reason: { type: 'NotEligibleForHls' } }],
    },
  },
})

const card = (id: string, media: unknown) => ({ node: { id, story_card_info: { story_card_id: id }, attachments: [{ media }] } })

// Khối data-sjs như Facebook nhúng trong HTML khi mở thẳng địa chỉ Tin.
const BUCKET = {
  require: [
    [
      'RelayPrefetchedStreamCache',
      'next',
      [],
      [
        'adp_StoriesQuery',
        {
          __bbox: {
            result: {
              data: {
                bucket: {
                  id: '111',
                  story_bucket_owner: { id: '5', name: 'Như Quỳnh' },
                  unified_stories_with_notes: {
                    edges: [card(CARD, videoMedia('9001', 'story')), card(OTHER, videoMedia('9002', 'other', false))],
                  },
                },
              },
              extensions: {
                // Luồng tải trước của nhiều video khác (không phải tin đang xem): không được chọn nhầm.
                all_video_dash_prefetch_representations: [
                  { video_id: '7777', representations: [{ mime_type: 'video/mp4', base_url: `${CDN}/prefetch.mp4`, height: 1920, bandwidth: 1 }] },
                ],
              },
            },
          },
        },
      ],
    ],
  ],
}

// Câu trả lời GraphQL khi chuyển sang Tin khác trong app (không tải lại trang).
const GRAPHQL = {
  data: {
    bucket: {
      id: '112',
      story_bucket_owner: { name: 'Bạn khác' },
      unified_stories_with_notes: { edges: [card(NEXT, { __typename: 'Video', id: '9003', playable_url: `${CDN}/next-sd.mp4` })] },
    },
  },
}

// `</` trong khối JSON phải viết `<\/` (như Facebook) để không đóng thẻ <script>.
const sjs = (value: unknown) => JSON.stringify(value).replace(/<\//g, '<\\/')

const STORY_PAGE = `<!doctype html><html><head><title>Facebook</title></head><body style="margin:0">
<div id="viewer" style="padding:40px"><video id="v" width="360" height="640" style="display:block;background:#000"></video></div>
<script type="application/json" data-sjs>${sjs(BUCKET)}</script>
<script>JSON.parse(document.querySelector('script[data-sjs]').textContent)</script>
</body></html>`

const PLAIN_PAGE = `<!doctype html><html><head><title>Video | Facebook</title></head><body style="margin:0">
<div style="padding:40px"><div id="player"><video id="v" width="640" height="360" style="display:block;background:#000"></video></div></div>
</body></html>`

async function servePages(page: Page) {
  await page.route('https://www.facebook.com/**', (route) => {
    const story = new URL(route.request().url()).pathname.startsWith('/stories/')
    return route.fulfill({ contentType: 'text/html; charset=utf-8', body: story ? STORY_PAGE : PLAIN_PAGE })
  })
  await page.route(`${CDN}/**`, (route) =>
    route.fulfill({ contentType: 'video/mp4', body: 'x', headers: { 'access-control-allow-origin': '*' } }),
  )
}

type Found = {
  title: string | null
  formats: { id: string; sizeBytes: number | null; streams: { url: string; kind: string; codecs?: string | null }[] }[]
  manifests: string[]
  files: string[]
}

/** Gọi page-media.js như nền làm (scripting.executeScript ở thế giới của trang), tại giữa thẻ <video>. */
async function findMedia(page: Page): Promise<Found> {
  await page.addScriptTag({ path: 'extensions/chromium/page-media.js' })
  const box = (await page.locator('#v').boundingBox())!
  return page.evaluate(
    ([x, y]) => (window as unknown as { utdFindPageMedia: (x: number, y: number) => Found }).utdFindPageMedia(x, y),
    [box.x + box.width / 2, box.y + box.height / 2],
  )
}

test('Tin Facebook mở thẳng: đúng video của thẻ trong địa chỉ — 1080p (H.264 + tiếng, UTD ghép), HD, SD có sẵn tiếng', async ({ page }) => {
  await servePages(page)
  await page.addInitScript({ path: 'extensions/chromium/fb-media.js' })
  await page.goto(`https://www.facebook.com/stories/111/${encodeURIComponent(CARD)}/?bucket_count=3`)
  await page.waitForTimeout(50)
  const found = await findMedia(page)
  expect(found.title).toBe('Như Quỳnh')
  expect(found.formats).toEqual([
    {
      id: '1080',
      sizeBytes: 2_500_000 + 80_000,
      streams: [
        { url: `${CDN}/v1080.mp4?oh=1`, kind: 'video', codecs: 'avc1.640028', sizeBytes: 2_500_000 },
        { url: `${CDN}/a.mp4`, kind: 'audio', codecs: 'mp4a.40.5', sizeBytes: 80_000 },
      ],
    },
    { id: 'hd', sizeBytes: null, streams: [{ url: `${CDN}/story-hd.mp4`, kind: 'both' }] },
    { id: 'sd', sizeBytes: null, streams: [{ url: `${CDN}/story-sd.mp4`, kind: 'both' }] },
  ])
  // JSON.parse của trang vẫn trả đúng kết quả (chỉ đọc theo).
  expect(await page.evaluate(() => JSON.parse('{"a":[1]}').a[0])).toBe(1)
})

test('Tin Facebook chuyển trong app (GraphQL) ⇒ video của thẻ mới; thẻ khác trong bucket không lẫn', async ({ page }) => {
  await servePages(page)
  await page.addInitScript({ path: 'extensions/chromium/fb-media.js' })
  await page.goto(`https://www.facebook.com/stories/111/${CARD}/`)
  await page.evaluate(
    ([text, next]) => {
      history.pushState({}, '', `/stories/112/${next}/`)
      JSON.parse(text)
    },
    [JSON.stringify(GRAPHQL), NEXT],
  )
  await page.waitForTimeout(50)
  const next = await findMedia(page)
  expect(next.title).toBe('Bạn khác')
  expect(next.formats).toEqual([{ id: 'sd', sizeBytes: null, streams: [{ url: `${CDN}/next-sd.mp4`, kind: 'both' }] }])
  await page.evaluate((other) => history.pushState({}, '', `/stories/111/${other}/`), OTHER)
  const other = await findMedia(page)
  expect(other.formats.map((format) => format.streams[0].url)).toEqual([`${CDN}/other-hd.mp4`, `${CDN}/other-sd.mp4`])
})

test('tiện ích nạp sau khi trang đã mở ⇒ đọc lại các khối JSON có sẵn trong trang', async ({ page }) => {
  await servePages(page)
  await page.goto(`https://www.facebook.com/stories/111/${CARD}/`)
  await page.addScriptTag({ path: 'extensions/chromium/fb-media.js' })
  const found = await findMedia(page)
  expect(found.formats.map((format) => format.id)).toEqual(['1080', 'hd', 'sd'])
})

test('không có dữ liệu ghi nhớ ⇒ link trong props React của trình phát (hdSrc/sdSrc)', async ({ page }) => {
  await servePages(page)
  await page.goto('https://www.facebook.com/watch/')
  await page.evaluate((cdn) => {
    const player = document.getElementById('player') as unknown as Record<string, unknown>
    const props = { children: { props: { implementations: [{ data: { hdSrc: `${cdn}/fiber-hd.mp4`, sdSrc: `${cdn}/fiber-sd.mp4` } }] } } }
    player['__reactFiber$utdtest'] = { memoizedProps: { className: 'x' }, return: { memoizedProps: props, return: null } }
  }, CDN)
  const found = await findMedia(page)
  expect(found.title).toBe('Video')
  expect(found.formats.map((format) => [format.id, format.streams[0].url])).toEqual([
    ['hd', `${CDN}/fiber-hd.mp4`],
    ['sd', `${CDN}/fiber-sd.mp4`],
  ])
})

const efg = (value: Record<string, unknown>) => btoa(JSON.stringify(value)).replace(/=+$/, '')

/** Danh sách yêu cầu mạng giả của trang (Resource Timing): Playwright không ghi mục cho yêu cầu chặn bằng page.route. */
const fakeRequests = (page: Page, list: (readonly [string, string])[]) =>
  page.evaluate((entries) => {
    const fake = entries.map(([name, initiatorType], index) => ({ name, initiatorType, startTime: index + 1 }))
    performance.getEntriesByType = ((type: string) => (type === 'resource' ? fake : [])) as typeof performance.getEntriesByType
  }, list)

test('kiểu IDM: luồng fbcdn trang đã tải từng đoạn ⇒ bỏ bytestart/byteend, video đang phát (nhiều đoạn nhất) + tiếng của nó', async ({ page }) => {
  await servePages(page)
  await page.goto('https://www.facebook.com/watch/')
  const video = `${CDN}/o1/v.mp4?efg=${efg({ vencode_tag: 'dash_h264-basic-gen2_720p', xpv_asset_id: 5 })}`
  const audio = `${CDN}/o1/a.mp4?efg=${efg({ vencode_tag: 'dash_ln_heaac_vbr3_audio', xpv_asset_id: 5 })}`
  const prefetch = `${CDN}/o1/p.mp4?efg=${efg({ vencode_tag: 'dash_h264-basic-gen2_1080p', xpv_asset_id: 6 })}`
  const ad = `${CDN}/o1/ad.mp4?efg=${efg({ vencode_tag: 'dash_h264.ad.720p', xpv_asset_id: 8 })}`
  const segments = [
    [video, 0, 999],
    [audio, 0, 499],
    [video, 1000, 1999],
    [prefetch, 0, 999],
    [audio, 500, 999],
    [video, 2000, 2999],
    [ad, 0, 99],
    [ad, 100, 199],
    [ad, 200, 299],
    [ad, 300, 399],
  ] as const
  await fakeRequests(
    page,
    segments.map(([url, start, end]) => [`${url}&bytestart=${start}&byteend=${end}`, 'xmlhttprequest'] as const),
  )
  const found = await findMedia(page)
  expect(found.formats).toEqual([
    { id: 'stream', sizeBytes: null, streams: [{ url: video, kind: 'video' }, { url: audio, kind: 'audio' }] },
  ])
})

test('trang khác: manifest HLS và file media trang đã tải được đưa ra làm đường dự phòng', async ({ page }) => {
  await page.route('https://media.utd.test/**', (route) => {
    const url = route.request().url()
    if (url.endsWith('/page')) return route.fulfill({ contentType: 'text/html', body: '<video id="v" width="320" height="180"></video>' })
    return route.fulfill({ contentType: 'application/octet-stream', body: 'x' })
  })
  await page.goto('https://media.utd.test/page')
  await fakeRequests(page, [
    ['https://media.utd.test/hls/master.m3u8?token=1', 'xmlhttprequest'],
    ['https://media.utd.test/clips/a.mp4', 'video'],
    ['https://media.utd.test/style/intro.mp4', 'link'],
    ['http://media.utd.test/plain.mp4', 'video'],
  ])
  const found = await findMedia(page)
  expect(found.formats).toEqual([])
  expect(found.manifests).toEqual(['https://media.utd.test/hls/master.m3u8?token=1'])
  expect(found.files).toEqual(['https://media.utd.test/clips/a.mp4'])
})

// background.js thật với API giả: lệnh của content script ⇒ chạy page-media.js trong trang (MAIN), kiểm kết quả;
// dòng đã chọn ⇒ tin `media-streams` cho UTD, không cookie.
function stubBackground({ found, native = { ok: true, taskId: 't9' } }: { found: unknown; native?: Record<string, unknown> }) {
  const calls: { name: string; args: unknown[] }[] = []
  const listeners: ((message: unknown, sender: unknown, reply: (value: unknown) => void) => boolean)[] = []
  const noop = { addListener: () => undefined }
  ;(window as unknown as Record<string, unknown>).chrome = {
    i18n: { getMessage: () => '' },
    runtime: {
      id: 'utd',
      getURL: (file: string) => `chrome-extension://utd/${file}`,
      onInstalled: noop,
      onStartup: noop,
      onMessage: { addListener: (listener: (typeof listeners)[number]) => listeners.push(listener) },
      sendNativeMessage: async (_host: string, message: unknown) => {
        calls.push({ name: 'native', args: [message] })
        return native
      },
    },
    contextMenus: { removeAll: async () => undefined, create: async () => undefined, onClicked: noop },
    action: { setBadgeText: async () => undefined },
    storage: { local: { get: async () => ({}), set: async () => undefined } },
    cookies: { getAll: async () => { calls.push({ name: 'cookies', args: [] }); return [] }, getAllCookieStores: async () => [] },
    scripting: {
      getRegisteredContentScripts: async () => [],
      registerContentScripts: async (scripts: unknown) => calls.push({ name: 'register', args: [scripts] }),
      executeScript: async (options: { world?: string; func: { name: string }; args: unknown[] }) => {
        calls.push({ name: 'execute', args: [{ world: options.world, func: options.func?.name, args: options.args }] })
        return [{ result: found }]
      },
    },
  }
  ;(window as unknown as Record<string, unknown>).__bg = { calls, listeners }
}

test('nền: chạy page-media.js trong trang, bỏ phần không hợp lệ; dòng đã chọn ⇒ media-streams không cookie', async ({ page }) => {
  const found = {
    title: 'Như Quỳnh',
    formats: [
      { id: '1080', sizeBytes: 2580000, streams: [{ url: `${CDN}/v.mp4`, kind: 'video', codecs: 'avc1.640028' }, { url: `${CDN}/a.mp4`, kind: 'audio' }] },
      { id: 'bad', streams: [{ url: 'blob:https://www.facebook.com/1', kind: 'both' }] },
      { id: 'x"><img>', streams: [{ url: `${CDN}/v.mp4`, kind: 'both' }] },
      { id: 'two-audio', streams: [{ url: `${CDN}/a.mp4`, kind: 'audio' }, { url: `${CDN}/a.mp4`, kind: 'audio' }] },
      { id: 'hd', streams: [{ url: `${CDN}/hd.mp4`, kind: 'both', codecs: 'avc1; rm -rf' }] },
    ],
    manifests: ['javascript:alert(1)', 'https://cdn.test/a.m3u8'],
    files: [],
  }
  await page.route('http://ext.utd.test/**', (route) => route.fulfill({ contentType: 'text/html', body: '<p>x</p>' }))
  await page.addInitScript(stubBackground, { found })
  await page.goto('http://ext.utd.test/')
  await page.addScriptTag({ path: 'extensions/chromium/page-media.js' })
  await page.addScriptTag({ path: 'extensions/chromium/background.js' })
  const ask = (message: Record<string, unknown>) =>
    page.evaluate(
      (message) =>
        new Promise((resolve) => {
          const bg = (window as unknown as { __bg: { listeners: ((m: unknown, s: unknown, r: (v: unknown) => void) => boolean)[] } }).__bg
          const sender = { id: 'utd', tab: { id: 7, url: 'https://www.facebook.com/stories/111/x/' }, frameId: 0, url: 'https://www.facebook.com/stories/111/x/' }
          const async = bg.listeners[0](message, sender, resolve)
          if (!async) resolve('sync')
        }),
      message,
    )
  const reply = (await ask({ type: 'utd.page-media', x: 100, y: 200 })) as { ok: boolean; media: Found }
  expect(reply.ok).toBe(true)
  expect(reply.media.formats.map((format) => format.id)).toEqual(['1080', 'hd'])
  expect(reply.media.formats[1].streams[0]).toEqual({ url: `${CDN}/hd.mp4`, kind: 'both', codecs: null, sizeBytes: null })
  expect(reply.media.manifests).toEqual(['https://cdn.test/a.m3u8'])
  expect(await ask({ type: 'utd.video-streams', title: 'Tin', streams: [{ url: 'blob:x', kind: 'both' }] })).toBe('sync')
  const added = await ask({ type: 'utd.video-streams', title: 'Như Quỳnh', streams: reply.media.formats[0].streams })
  expect(added).toMatchObject({ ok: true, taskId: 't9' })
  const calls = await page.evaluate(() => (window as unknown as { __bg: { calls: { name: string; args: unknown[] }[] } }).__bg.calls)
  expect(calls.find((call) => call.name === 'register')?.args[0]).toEqual([
    { id: 'utd-fb-media', js: ['fb-media.js'], matches: ['https://*.facebook.com/*'], runAt: 'document_start', world: 'MAIN' },
  ])
  expect(calls.find((call) => call.name === 'execute')?.args[0]).toEqual({ world: 'MAIN', func: 'utdFindPageMedia', args: [100, 200] })
  expect(calls.filter((call) => call.name === 'native').map((call) => call.args[0])).toEqual([
    {
      version: 2,
      type: 'media-streams',
      url: 'https://www.facebook.com/stories/111/x/',
      pageUrl: 'https://www.facebook.com/stories/111/x/',
      title: 'Như Quỳnh',
      streams: [
        { url: `${CDN}/v.mp4`, kind: 'video', codecs: 'avc1.640028', sizeBytes: null },
        { url: `${CDN}/a.mp4`, kind: 'audio', codecs: null, sizeBytes: null },
      ],
    },
  ])
  expect(calls.some((call) => call.name === 'cookies'), 'không đọc cookie').toBe(false)
})

test('nền: UTD trả câu lỗi không mã (vd channel_page: <link>) ⇒ mã chung, bảng không bao giờ thấy câu thô', async ({ page }) => {
  await page.route('http://ext.utd.test/**', (route) => route.fulfill({ contentType: 'text/html', body: '<p>x</p>' }))
  await page.addInitScript(stubBackground, { found: null, native: { ok: false, message: 'channel_page: https://www.tiktok.com/@_littledav_' } })
  await page.goto('http://ext.utd.test/')
  await page.addScriptTag({ path: 'extensions/chromium/background.js' })
  const reply = await page.evaluate(
    () =>
      new Promise((resolve) => {
        const bg = (window as unknown as { __bg: { listeners: ((m: unknown, s: unknown, r: (v: unknown) => void) => boolean)[] } }).__bg
        const url = 'https://www.tiktok.com/@_littledav_'
        bg.listeners[0]({ type: 'utd.video-formats', url, src: null, signIn: false }, { id: 'utd', tab: { id: 7, url }, frameId: 0, url }, resolve)
      }),
  )
  expect(reply).toMatchObject({ ok: false, code: 'media.failed' })
  expect(JSON.stringify(reply)).not.toContain('channel_page')
})
