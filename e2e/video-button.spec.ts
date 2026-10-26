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

// API trình duyệt giả cho content script: `__ext.replies` = câu trả lời lần lượt cho `utd.video-formats`.
function stubContentApi(input: { messages: Record<string, { message: string }>; replies: Reply[]; enabled: boolean }) {
  const sent: Record<string, unknown>[] = []
  const store: Record<string, unknown> = { videoButton: input.enabled }
  const changed: ((changes: Record<string, { newValue: unknown }>, area: string) => void)[] = []
  const onMessage: ((message: unknown, sender: unknown, reply: (value: unknown) => void) => boolean)[] = []
  const replies = [...input.replies]
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
