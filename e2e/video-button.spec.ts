import fs from 'node:fs'
import path from 'node:path'

import { expect, test, type Page } from '@playwright/test'

// Nút "Tải video này" trên trang (extensions/*/content.js; notes/auth-plan.md GĐ3): content script thật trên một trang
// thử có <video>, API trình duyệt giả (tin gửi nền được ghi lại, nền trả định dạng giả). Không gọi native host thật.

const MB = 1024 * 1024
const FORMATS = [
  { id: 'best', sizeBytes: 1.2 * 1024 * MB, selector: '137+140/bv*+ba/b', audioFormat: null },
  { id: '1080', sizeBytes: 820 * MB, selector: '137+140/bv*[height<=1080]+ba/b[height<=1080]', audioFormat: null },
  { id: '720', sizeBytes: 410 * MB, selector: '136+140/bv*[height<=720]+ba/b[height<=720]', audioFormat: null },
  { id: '144', sizeBytes: 9 * MB, selector: '160+140/bv*[height<=144]+ba/b[height<=144]', audioFormat: null },
  { id: 'audio-m4a', sizeBytes: 31 * MB, selector: '140/ba[ext=m4a]/ba', audioFormat: 'm4a' },
  { id: 'audio-mp3', sizeBytes: null, selector: '140/ba/b', audioFormat: 'mp3' },
]

const PAGE = `<!doctype html><html><body style="margin:0;font:14px sans-serif">
<p style="height:140px;margin:0">Trang thử</p>
<video id="main" width="640" height="360" style="display:block;margin-left:40px;background:#000"></video>
<video id="tiny" width="100" height="60" style="display:block;margin:20px 40px;background:#333"></video>
</body></html>`

type Reply = Record<string, unknown>

// API trình duyệt giả cho content script: `__ext.replies` = câu trả lời lần lượt cho `utd.video-formats`;
// `pageMedia` = câu trả lời lần lượt cho `utd.page-media` (nền đọc luồng trong trang, page-media.js).
function stubContentApi(input: {
  messages: Record<string, { message: string }>
  replies: Reply[]
  enabled: boolean
  pageMedia?: Reply[]
}) {
  const sent: Record<string, unknown>[] = []
  const store: Record<string, unknown> = { videoButton: input.enabled }
  const changed: ((changes: Record<string, { newValue: unknown }>, area: string) => void)[] = []
  const onMessage: ((message: unknown, sender: unknown, reply: (value: unknown) => void) => boolean)[] = []
  const replies = [...input.replies]
  const pageMedia = [...(input.pageMedia ?? [])]
  ;(window as unknown as Record<string, unknown>).chrome = {
    i18n: { getMessage: (key: string) => input.messages[key]?.message ?? '', getUILanguage: () => 'vi' },
    storage: {
      local: { get: async (key: string) => (key in store ? { [key]: store[key] } : {}) },
      onChanged: { addListener: (listener: (typeof changed)[number]) => changed.push(listener) },
    },
    runtime: {
      onMessage: { addListener: (listener: (typeof onMessage)[number]) => onMessage.push(listener) },
      sendMessage: async (message: Record<string, unknown>) => {
        sent.push(message)
        if (message.type === 'utd.video-formats') {
          await new Promise((resolve) => setTimeout(resolve, 300))
          return replies.shift() ?? { ok: false, message: 'hết câu trả lời' }
        }
        if (message.type === 'utd.page-media') return pageMedia.shift() ?? { ok: false }
        return { ok: true, taskId: 't1' }
      },
    },
  }
  const toggle = (value: boolean) => {
    store.videoButton = value
    for (const listener of changed) listener({ videoButton: { newValue: value } }, 'local')
  }
  ;(window as unknown as Record<string, unknown>).__ext = { sent, onMessage, toggle }
}

const MESSAGES = JSON.parse(
  fs.readFileSync(path.resolve('extensions/chromium/_locales/vi/messages.json'), 'utf8'),
) as Record<string, { message: string }>

const media = (signed = false) => ({
  ok: true,
  media: { title: 'Video thử', webpageUrl: 'http://video.utd.test/watch', signed, formats: FORMATS },
})

async function openPage(page: Page, replies: Reply[], enabled = true) {
  await page.route('http://video.utd.test/**', (route) => route.fulfill({ contentType: 'text/html', body: PAGE }))
  await page.addInitScript(stubContentApi, { messages: MESSAGES, replies, enabled })
  await page.goto('http://video.utd.test/watch')
  await page.addScriptTag({ path: 'extensions/chromium/content.js' })
  await page.waitForTimeout(50)
}

const sent = (page: Page) =>
  page.evaluate(() => (window as unknown as { __ext: { sent: Record<string, unknown>[] } }).__ext.sent)

async function hover(page: Page, selector: string) {
  const box = (await page.locator(selector).boundingBox())!
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
  await page.mouse.move(box.x + box.width / 2 + 5, box.y + box.height / 2 + 5)
  return box
}

const pill = (page: Page) => page.locator('utd-video-button .pill')
const panel = (page: Page) => page.locator('utd-video-button .panel')

test('rê chuột vào video ⇒ nút "Tải video này" ngay trên mép trên-phải; video nhỏ không có nút', async ({ page }) => {
  await openPage(page, [])
  await hover(page, '#tiny')
  await expect(pill(page)).toBeHidden()
  const video = await hover(page, '#main')
  await expect(pill(page)).toBeVisible()
  await expect(pill(page)).toHaveText('Tải video này')
  const button = (await pill(page).boundingBox())!
  expect(button.y + button.height).toBeLessThanOrEqual(video.y + 1)
  expect(Math.abs(button.x + button.width - (video.x + video.width))).toBeLessThanOrEqual(2)
  // Rời video: nút ẩn sau một lúc.
  await page.mouse.move(5, 5)
  await expect(pill(page)).toBeHidden({ timeout: 4000 })
})

test('ngay trên video là thanh cố định của trang (Douyin) ⇒ nút vào góc trong video, không che thanh', async ({ page }) => {
  const body = `<!doctype html><html><body style="margin:0">
<header style="position:fixed;top:0;left:0;right:0;height:60px;background:#222;z-index:10"></header>
<div style="padding-top:64px"><video id="main" width="640" height="360" style="display:block;margin-left:40px;background:#000"></video></div>
</body></html>`
  await page.route('http://fixed.utd.test/**', (route) => route.fulfill({ contentType: 'text/html', body }))
  await page.addInitScript(stubContentApi, { messages: MESSAGES, replies: [], enabled: true })
  await page.goto('http://fixed.utd.test/watch')
  await page.addScriptTag({ path: 'extensions/chromium/content.js' })
  const video = await hover(page, '#main')
  await expect(pill(page)).toBeVisible()
  const button = (await pill(page).boundingBox())!
  expect(button.y).toBeGreaterThanOrEqual(video.y)
  expect(button.x + button.width).toBeLessThanOrEqual(video.x + video.width)
})

test('bấm nút ⇒ bảng "đang đọc" rồi các định dạng kèm dung lượng; chọn 1080p ⇒ UTD tải luôn, không mở app', async ({ page }) => {
  await openPage(page, [media()])
  await hover(page, '#main')
  await pill(page).click()
  await expect(panel(page)).toContainText('Đang đọc định dạng…')
  const rows = panel(page).locator('.row')
  await expect(rows).toHaveText([
    'Tốt nhất≈ 1,2 GB',
    '1080p≈ 820 MB',
    '720p≈ 410 MB',
    '144p≈ 9 MB',
    'Chỉ âm thanh (M4A)≈ 31 MB',
    'MP3',
  ])
  await expect(panel(page).locator('.title')).toHaveText('Video thử')
  await rows.nth(1).click()
  await expect(panel(page)).toContainText('Đã thêm vào UTD')
  const messages = await sent(page)
  expect(messages[0]).toEqual({ type: 'utd.video-formats', url: 'http://video.utd.test/watch', src: null, signIn: false })
  expect(messages.at(-1)).toEqual({
    type: 'utd.video-download',
    url: 'http://video.utd.test/watch',
    title: 'Video thử',
    selector: FORMATS[1].selector,
    audioFormat: null,
    signIn: false,
  })
  await expect(panel(page)).toBeHidden({ timeout: 4000 })
})

test('nguồn đòi đăng nhập ⇒ đọc lại kèm đăng nhập trong cùng cú bấm, lượt tải cũng kèm', async ({ page }) => {
  await openPage(page, [{ ok: false, code: 'media.login', message: 'cần đăng nhập' }, media(true)])
  await hover(page, '#main')
  await pill(page).click()
  await panel(page).locator('.row').first().click()
  await expect(panel(page)).toContainText('Đã thêm vào UTD')
  const messages = await sent(page)
  expect(messages.map((message) => [message.type, message.signIn])).toEqual([
    ['utd.video-formats', false],
    ['utd.video-formats', true],
    ['utd.video-download', true],
  ])
})

test('không đọc được ⇒ báo gọn trong bảng; UTD chưa chạy ⇒ câu của nền', async ({ page }) => {
  await openPage(page, [{ ok: false, code: 'media.unsupported', message: 'x' }])
  await hover(page, '#main')
  await pill(page).click()
  await expect(panel(page).locator('.status.error')).toHaveText('UTD không đọc được video này.')
  await page.keyboard.press('Escape')
  await expect(panel(page)).toBeHidden()
})

// Facebook Tin: trang chặn mọi cú bấm ngay ở window (chạm là dừng / sang tin khác). content.js chạy từ document_start
// nên nghe trước trang: mô phỏng bằng cách nạp content.js rồi mới nạp bộ chặn của trang.
test('trang chặn mọi cú bấm (Facebook Tin) ⇒ nút và bảng vẫn bấm được, trang không thấy cú bấm vào UTD', async ({ page }) => {
  await page.route('http://video.utd.test/**', (route) => route.fulfill({ contentType: 'text/html', body: PAGE }))
  await page.addInitScript(stubContentApi, { messages: MESSAGES, replies: [media()], enabled: true })
  await page.addInitScript({ path: 'extensions/chromium/content.js' })
  await page.addInitScript(() => {
    const seen: string[] = []
    ;(window as unknown as { __pageSaw: string[] }).__pageSaw = seen
    for (const type of ['pointerdown', 'pointerup', 'mousedown', 'mouseup', 'click']) {
      addEventListener(
        type,
        (event) => {
          seen.push(type)
          event.stopImmediatePropagation()
          event.preventDefault()
        },
        true,
      )
    }
  })
  await page.goto('http://video.utd.test/watch')
  await hover(page, '#main')
  await pill(page).click()
  await expect(panel(page).locator('.title')).toHaveText('Video thử')
  await panel(page).locator('.row').nth(1).click()
  await expect(panel(page)).toContainText('Đã thêm vào UTD')
  expect((await sent(page)).at(-1)).toMatchObject({ type: 'utd.video-download', selector: FORMATS[1].selector })
  expect(await page.evaluate(() => (window as unknown as { __pageSaw: string[] }).__pageSaw)).toEqual([])
  // Bấm ra ngoài vẫn tới trang như thường.
  await page.mouse.click(5, 5)
  expect(await page.evaluate(() => (window as unknown as { __pageSaw: string[] }).__pageSaw)).toContain('click')
})

test('cú bấm giả từ trang (không phải người dùng) không làm gì', async ({ page }) => {
  await openPage(page, [media()])
  await hover(page, '#main')
  await expect(pill(page)).toBeVisible()
  await page.evaluate(() => {
    const host = document.querySelector('utd-video-button') as HTMLElement & { shadowRoot: ShadowRoot }
    ;(host.shadowRoot.querySelector('.pill') as HTMLButtonElement).click()
  })
  await page.waitForTimeout(400)
  await expect(panel(page)).toBeHidden()
  expect(await sent(page)).toEqual([])
})

test('tắt "Hiện nút Tải video" trong popup ⇒ không có nút; chuột phải › UTD vẫn mở bảng ở video', async ({ page }) => {
  await openPage(page, [media()], false)
  await hover(page, '#main')
  await page.waitForTimeout(300)
  await expect(pill(page)).toBeHidden()
  // Chuột phải lên video rồi chọn mục UTD: nền gửi `utd.open-formats` cho content script.
  await page.locator('#main').click({ button: 'right' })
  const opened = await page.evaluate(() => {
    const ext = (window as unknown as { __ext: { onMessage: ((message: unknown, sender: unknown, reply: (value: unknown) => void) => boolean)[] } }).__ext
    let answer: unknown = null
    for (const listener of ext.onMessage) listener({ type: 'utd.open-formats', url: 'http://video.utd.test/watch' }, {}, (value) => (answer = value))
    return answer
  })
  expect(opened).toEqual({ ok: true })
  await expect(panel(page).locator('.row')).toHaveCount(FORMATS.length)
  // Bật lại: rê chuột lại có nút.
  await page.keyboard.press('Escape')
  await page.evaluate(() => (window as unknown as { __ext: { toggle: (value: boolean) => void } }).__ext.toggle(true))
  await hover(page, '#main')
  await expect(pill(page)).toBeVisible()
})

// Tin Facebook (notes/fb-media.md): yt-dlp không đọc được ⇒ bảng lấy luồng đọc thẳng trong trang (nền chạy page-media.js).
const STORY = 'https://www.facebook.com/stories/111/UzpfSVNDOjIyMg==/'
const CDN = 'https://video.fbcdn.test'
const STORY_MEDIA = {
  ok: true,
  media: {
    title: 'Như Quỳnh',
    formats: [
      {
        id: '1080',
        sizeBytes: 2_580_000,
        streams: [
          { url: `${CDN}/v1080.mp4`, kind: 'video', codecs: 'avc1.640028', sizeBytes: 2_500_000 },
          { url: `${CDN}/a.mp4`, kind: 'audio', codecs: 'mp4a.40.5', sizeBytes: 80_000 },
        ],
      },
      { id: 'hd', sizeBytes: null, streams: [{ url: `${CDN}/hd.mp4`, kind: 'both', codecs: null, sizeBytes: null }] },
      { id: 'sd', sizeBytes: null, streams: [{ url: `${CDN}/sd.mp4`, kind: 'both', codecs: null, sizeBytes: null }] },
    ],
    manifests: [],
    files: [],
  },
}

async function openAt(page: Page, url: string, replies: Reply[], pageMedia: Reply[], body = PAGE, scripts = ['content.js']) {
  await page.route(`${new URL(url).origin}/**`, (route) =>
    route.fulfill({ contentType: 'text/html; charset=utf-8', body }),
  )
  await page.addInitScript(stubContentApi, { messages: MESSAGES, replies, enabled: true, pageMedia })
  await page.goto(url)
  for (const script of scripts) await page.addScriptTag({ path: `extensions/chromium/${script}` })
  await page.waitForTimeout(50)
}

test('Tin Facebook ⇒ bảng ra định dạng đọc trong trang (1080p ghép, HD, SD); chọn ⇒ UTD tải đúng luồng, không chạy yt-dlp', async ({ page }) => {
  await openAt(page, STORY, [], [STORY_MEDIA])
  const video = await hover(page, '#main')
  await pill(page).click()
  const rows = panel(page).locator('.row')
  await expect(rows).toHaveText(['1080p≈ 2,5 MB', 'HD', 'SD'])
  await expect(panel(page).locator('.title')).toHaveText('Như Quỳnh')
  await rows.first().click()
  await expect(panel(page)).toContainText('Đã thêm vào UTD')
  const messages = await sent(page)
  expect(messages.map((message) => message.type)).toEqual(['utd.page-media', 'utd.video-streams'])
  // Điểm hỏi là giữa video đang gắn nút: page-media.js chọn đúng video đó.
  expect(messages[0].x).toBeCloseTo(video.x + video.width / 2, 0)
  expect(messages[0].y).toBeCloseTo(video.y + video.height / 2, 0)
  expect(messages[1]).toEqual({ type: 'utd.video-streams', title: 'Như Quỳnh', streams: STORY_MEDIA.media.formats[0].streams })
})

test('Facebook chưa có luồng nào và yt-dlp không đọc được ⇒ báo gọn "Chưa thấy luồng video", không treo', async ({ page }) => {
  const empty = { ok: true, media: { title: null, formats: [], manifests: [], files: [] } }
  await openAt(page, STORY, [{ ok: false, code: 'media.unsupported', message: 'x' }], [empty])
  await hover(page, '#main')
  await pill(page).click()
  await expect(panel(page).locator('.status.error')).toHaveText('Chưa thấy luồng video. Cho video phát rồi bấm lại.')
  expect((await sent(page)).map((message) => message.type)).toEqual(['utd.page-media', 'utd.video-formats'])
})

test('trang khác yt-dlp không đọc được ⇒ bắt manifest HLS trang đã tải, UTD đọc định dạng từ đó (kiểu IDM)', async ({ page }) => {
  const sniffed = { ok: true, media: { title: null, formats: [], manifests: ['https://cdn.utd.test/hls/master.m3u8'], files: [] } }
  await openAt(page, 'http://video.utd.test/watch', [{ ok: false, code: 'media.unsupported', message: 'x' }, media()], [sniffed])
  await hover(page, '#main')
  await pill(page).click()
  await expect(panel(page).locator('.row')).toHaveCount(FORMATS.length)
  const messages = await sent(page)
  expect(messages.map((message) => [message.type, message.url])).toEqual([
    ['utd.video-formats', 'http://video.utd.test/watch'],
    ['utd.page-media', 'http://video.utd.test/watch'],
    ['utd.video-formats', 'https://cdn.utd.test/hls/master.m3u8'],
  ])
})

// TikTok (extensions/*/tiktok.js): feed và lưới hồ sơ có địa chỉ trang không phải link video ⇒ gửi yt-dlp link riêng
// `/@kênh/video/<mã>` của đúng video đang gắn nút. Trang giả theo DOM của TikTok web (khung `xgwrapper-<số>-<mã>`,
// mục feed `recommend-list-item-container`, ô lưới `user-post-item` bọc trong link bài đăng).
const TIKTOK_FEED = `<!doctype html><html><head><title>TikTok - Make Your Day</title></head><body style="margin:0">
<article data-e2e="recommend-list-item-container" style="padding:30px 40px">
  <div id="xgwrapper-0-7412345678901234567"><video id="main" width="360" height="640" style="display:block;background:#000"></video></div>
  <a href="/@davidnguyenjr" data-e2e="video-author-avatar">David Nguyen Jr.</a>
</article>
<article data-e2e="recommend-list-item-container" style="padding:30px 40px">
  <div id="xgwrapper-1-7499999999999999999"><video id="next" width="360" height="640" style="display:block;background:#000"></video></div>
  <a href="/@khac">Khác</a>
</article>
</body></html>`

const TIKTOK_GRID = `<!doctype html><html><head><title>Dav (@_littledav_) | TikTok</title></head><body style="margin:0">
<div data-e2e="user-post-item-list" style="display:flex;gap:10px;padding:40px">
  <div data-e2e="user-post-item"><a href="https://www.tiktok.com/@_littledav_/video/7400000000000000001"><video id="tile1" width="200" height="300" style="display:block;background:#000"></video></a></div>
  <div data-e2e="user-post-item"><a href="https://www.tiktok.com/@_littledav_/video/7400000000000000002"><video id="tile2" width="200" height="300" style="display:block;background:#000"></video></a></div>
  <div data-e2e="user-post-item"><a href="/@_littledav_/photo/7400000000000000003"><video id="tile3" width="200" height="300" style="display:block;background:#000"></video></a></div>
</div>
</body></html>`

test('TikTok feed (For You): nút gửi yt-dlp link riêng của video đang xem, không phải trang feed', async ({ page }) => {
  await openAt(page, 'https://www.tiktok.com/foryou', [media()], [], TIKTOK_FEED, ['tiktok.js', 'content.js'])
  await hover(page, '#main')
  await pill(page).click()
  await expect(panel(page).locator('.row')).toHaveCount(FORMATS.length)
  expect((await sent(page))[0]).toEqual({
    type: 'utd.video-formats',
    url: 'https://www.tiktok.com/@davidnguyenjr/video/7412345678901234567',
    src: null,
    signIn: false,
  })
  await page.keyboard.press('Escape')
  await page.locator('#next').scrollIntoViewIfNeeded()
  await hover(page, '#next')
  await pill(page).click()
  await expect(panel(page).locator('.row')).toHaveCount(0)
  expect((await sent(page)).at(-1)?.url).toBe('https://www.tiktok.com/@khac/video/7499999999999999999')
})

test('TikTok lưới video trên hồ sơ: đúng ô đang rê (cả bài ảnh), không gửi link kênh, rê qua không chạy yt-dlp', async ({ page }) => {
  await openAt(page, 'https://www.tiktok.com/@_littledav_', [media(), media()], [], TIKTOK_GRID, ['tiktok.js', 'content.js'])
  await hover(page, '#tile1')
  await hover(page, '#tile2')
  await page.waitForTimeout(200)
  expect(await sent(page), 'rê qua các ô không đọc sẵn').toEqual([])
  await pill(page).click()
  await expect(panel(page).locator('.row')).toHaveCount(FORMATS.length)
  expect((await sent(page))[0].url).toBe('https://www.tiktok.com/@_littledav_/video/7400000000000000002')
  await page.keyboard.press('Escape')
  await hover(page, '#tile3')
  await pill(page).click()
  await expect(panel(page).locator('.row')).toHaveCount(FORMATS.length)
  // Bài ảnh: yt-dlp không nhận /photo/, gửi dạng /video/ (yt-dlp ra nhạc của bài).
  expect((await sent(page)).at(-1)?.url).toBe('https://www.tiktok.com/@_littledav_/video/7400000000000000003')
})

// Douyin (extensions/*/douyin.js + douyin-page.js; notes/douyin.md): yt-dlp không đọc được Douyin ⇒ bảng hỏi trong
// trang trước, với link riêng `/video/<mã>` của đúng video (feed: `feed-active-video` mang `data-e2e-vid`; lưới hồ sơ:
// link của ô; khung `?modal_id=`).
const DOUYIN_FEED = `<!doctype html><html><head><title>抖音-记录美好生活</title></head><body style="margin:0">
<div data-e2e="feed-item" style="padding:30px 40px"><div data-e2e="feed-active-video" data-e2e-vid="7300000000000000001">
  <video id="main" width="360" height="640" style="display:block;background:#000"></video></div></div>
</body></html>`

const DOUYIN_GRID = `<!doctype html><html><head><title>主页 - 抖音</title></head><body style="margin:0">
<ul data-e2e="user-post-list" style="display:flex;gap:10px;padding:40px;list-style:none">
  <li><a href="//www.douyin.com/video/7300000000000000011"><video id="tile1" width="200" height="300" style="display:block;background:#000"></video></a></li>
  <li><div><video id="tile2" width="200" height="300" style="display:block;background:#000"></video></div><a href="/note/7300000000000000012">图文</a></li>
</ul>
</body></html>`

const DOUYIN_MEDIA = {
  ok: true,
  media: {
    title: 'Video Douyin thử',
    formats: [
      { id: '1080', sizeBytes: 9 * MB, streams: [{ url: 'https://v3-web.douyinvod.com/a/1080.mp4', kind: 'both', codecs: null, sizeBytes: 9 * MB }] },
      { id: '720', sizeBytes: 5 * MB, streams: [{ url: 'https://v3-web.douyinvod.com/a/720.mp4', kind: 'both', codecs: null, sizeBytes: 5 * MB }] },
    ],
    manifests: [],
    files: [],
  },
}

test('Douyin feed: đọc trong trang với link /video/<mã> của video đang phát; chọn ⇒ UTD tải MP4 không watermark', async ({ page }) => {
  await openAt(page, 'https://www.douyin.com/?recommend=1', [], [DOUYIN_MEDIA], DOUYIN_FEED, ['douyin.js', 'content.js'])
  await hover(page, '#main')
  await pill(page).click()
  const rows = panel(page).locator('.row')
  await expect(rows).toHaveText(['1080p≈ 9 MB', '720p≈ 5 MB'])
  await rows.first().click()
  await expect(panel(page)).toContainText('Đã thêm vào UTD')
  const messages = await sent(page)
  expect(messages.map((message) => [message.type, message.url])).toEqual([
    ['utd.page-media', 'https://www.douyin.com/video/7300000000000000001'],
    ['utd.video-streams', undefined],
  ])
  expect(messages[1].streams).toEqual(DOUYIN_MEDIA.media.formats[0].streams)
})

test('Douyin lưới hồ sơ và khung modal_id: đúng ô đang rê (link bọc hoặc cạnh video), không chạy yt-dlp khi rê qua', async ({ page }) => {
  await openAt(page, 'https://www.douyin.com/user/MS4wLjABAAAAtest', [], [DOUYIN_MEDIA, DOUYIN_MEDIA], DOUYIN_GRID, ['douyin.js', 'content.js'])
  await hover(page, '#tile1')
  await hover(page, '#tile2')
  await page.waitForTimeout(200)
  expect(await sent(page)).toEqual([])
  await pill(page).click()
  await expect(panel(page).locator('.row')).toHaveCount(2)
  expect((await sent(page))[0].url).toBe('https://www.douyin.com/video/7300000000000000012')
  await page.keyboard.press('Escape')
  await hover(page, '#tile1')
  await pill(page).click()
  await expect(panel(page).locator('.row')).toHaveCount(2)
  expect((await sent(page)).at(-1)?.url).toBe('https://www.douyin.com/video/7300000000000000011')
})

test('Douyin chưa đọc được (Douyin chặn, yt-dlp cũng không) ⇒ báo gọn, không treo', async ({ page }) => {
  const empty = { ok: true, media: { title: null, formats: [], manifests: [], files: [] } }
  const body = DOUYIN_FEED.replace('data-e2e="feed-active-video" data-e2e-vid="7300000000000000001"', 'data-e2e="feed-active-video"')
  await openAt(page, 'https://www.douyin.com/jingxuan?modal_id=7300000000000000099', [{ ok: false, code: 'media.unsupported', message: 'x' }], [empty], body, ['douyin.js', 'content.js'])
  await hover(page, '#main')
  await pill(page).click()
  await expect(panel(page).locator('.status.error')).toHaveText('Chưa thấy luồng video. Cho video phát rồi bấm lại.')
  expect((await sent(page)).map((message) => [message.type, message.url])).toEqual([
    ['utd.page-media', 'https://www.douyin.com/video/7300000000000000099'],
    ['utd.video-formats', 'https://www.douyin.com/video/7300000000000000099'],
  ])
})
