import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import AxeBuilder from '@axe-core/playwright'
import { chromium, expect, test, type Page } from '@playwright/test'

import { calls as appCalls, emitEvent, mockTauri } from './tauri-mock'

// Tự bắt link tải (AC-15, S-76; notes/auth-plan.md §4.4): extensions/*/takeover.js. Trình duyệt bắt đầu tải ⇒ tạm
// dừng ⇒ hỏi UTD. UTD nhận ⇒ huỷ + xoá bản của trình duyệt; còn lại (UTD chưa chạy, từ chối, quá giờ) ⇒ tải tiếp.
// Không download nào bị mất.

type Call = { name: string; args: unknown[] }
type Reply = { ok: boolean; taskId?: string; message?: string; auth?: Record<string, unknown> } | 'throw' | 'hang'

// background.js + takeover.js thật với API trình duyệt giả: ghi lại mọi lời gọi theo thứ tự.
function stubExtensionApi(input: { replies: Record<string, Reply>; store: Record<string, unknown> }) {
  const calls: Call[] = []
  const listeners: Record<string, ((...args: unknown[]) => unknown)[]> = {}
  const event = (name: string) => ({
    addListener: (listener: (...args: unknown[]) => unknown) => {
      ;(listeners[name] ??= []).push(listener)
    },
  })
  const spy =
    (name: string, result: (...args: unknown[]) => unknown = () => undefined) =>
    async (...args: unknown[]) => {
      calls.push({ name, args })
      return result(...args)
    }
  const store = input.store
  ;(window as unknown as Record<string, unknown>).chrome = {
    i18n: { getMessage: () => '' },
    runtime: {
      id: 'utd',
      getURL: (file: string) => `chrome-extension://utd/${file}`,
      onInstalled: event('onInstalled'),
      onStartup: event('onStartup'),
      onMessage: event('onMessage'),
      sendNativeMessage: async (_host: string, message: { type: string }) => {
        calls.push({ name: 'native', args: [message] })
        const reply = input.replies[message.type] ?? 'throw'
        if (reply === 'throw') throw new Error('Specified native messaging host not found.')
        if (reply === 'hang') return new Promise(() => undefined)
        return reply
      },
    },
    contextMenus: { removeAll: async () => undefined, create: async () => undefined, onClicked: event('onClicked') },
    action: { setBadgeText: async () => undefined },
    storage: {
      local: {
        get: async (keys: string | string[]) => {
          const list = Array.isArray(keys) ? keys : [keys]
          return Object.fromEntries(list.filter((key) => key in store).map((key) => [key, store[key]]))
        },
        set: async (values: Record<string, unknown>) => Object.assign(store, values),
      },
    },
    cookies: {
      getAll: spy('cookies.getAll', () => [
        { name: 'sid', value: 'UTDCANARYtakeover', domain: '.example.com', hostOnly: false, path: '/', secure: true, session: true },
      ]),
      getAllCookieStores: async () => [{ id: '0', tabIds: [] }],
    },
    downloads: {
      onCreated: event('downloads.onCreated'),
      pause: spy('pause', (id) => {
        if (id === 404) throw new Error('Download must be in progress')
      }),
      resume: spy('resume'),
      cancel: spy('cancel'),
      erase: spy('erase'),
    },
    scripting: {
      getRegisteredContentScripts: async () => [],
      registerContentScripts: spy('registerContentScripts'),
    },
  }
  const target = window as unknown as Record<string, unknown>
  target.__ext = { calls, listeners, store }
}

const SIGNED_IN = { ok: true, auth: { site: 'example.com', enabled: true, sensitive: false } }

async function loadExtension(page: Page, replies: Record<string, Reply>, store: Record<string, unknown> = {}) {
  await page.route('http://ext.utd.test/**', (route) => route.fulfill({ contentType: 'text/html', body: '<p>x</p>' }))
  await page.addInitScript(stubExtensionApi, { replies, store })
  await page.goto('http://ext.utd.test/')
  await page.addScriptTag({ path: 'extensions/chromium/background.js' })
  await page.addScriptTag({ path: 'extensions/chromium/takeover.js' })
}

/** Trình duyệt báo một download mới (`downloads.onCreated`). */
const startDownload = (page: Page, item: Record<string, unknown>) =>
  page.evaluate((item) => {
    const ext = (window as unknown as { __ext: { listeners: Record<string, ((item: unknown) => void)[]> } }).__ext
    for (const listener of ext.listeners['downloads.onCreated']) listener({ state: 'in_progress', ...item })
  }, item)

const calls = (page: Page) => page.evaluate(() => (window as unknown as { __ext: { calls: Call[] } }).__ext.calls)
const names = async (page: Page) => (await calls(page)).map((call) => call.name)

const ZIP = {
  id: 1,
  url: 'https://example.com/get?id=7',
  finalUrl: 'https://dl.example.com/files/bao-cao.zip',
  referrer: 'https://example.com/downloads',
  filename: '',
  totalBytes: 50 * 1024 * 1024,
  incognito: false,
}

test('S-76: UTD nhận ⇒ tạm dừng, hỏi UTD kèm cookie của đúng link, rồi huỷ + xoá bản của trình duyệt', async ({ page }) => {
  await loadExtension(page, { 'auth-pending': SIGNED_IN, takeover: { ok: true, taskId: 't1' } })
  await startDownload(page, ZIP)
  await expect
    .poll(() => names(page))
    .toEqual(['registerContentScripts', 'pause', 'native', 'cookies.getAll', 'cookies.getAll', 'native', 'cancel', 'erase'])
  const all = await calls(page)
  expect(all[1].args).toEqual([1])
  const offer = all.filter((call) => call.name === 'native').map((call) => call.args[0] as Record<string, unknown>)[1]
  expect(offer).toMatchObject({ version: 2, type: 'takeover', url: ZIP.finalUrl, pageUrl: ZIP.referrer, suggestedName: null })
  expect((offer.auth as { site: string; cookies: { value: string }[] }).cookies[0].value).toBe('UTDCANARYtakeover')
  // Cookie chỉ đọc cho đúng link trình duyệt vừa tải, và trang chứa nó khi cùng trang gốc (như chuột phải).
  const read = all.filter((call) => call.name === 'cookies.getAll').map((call) => (call.args[0] as { url: string }).url)
  expect(read).toEqual([ZIP.finalUrl, ZIP.referrer])
  expect(all.find((call) => call.name === 'erase')?.args).toEqual([{ id: 1 }])
})

test('S-76: UTD chưa chạy, từ chối hoặc lỗi ⇒ trình duyệt tải tiếp, không huỷ gì', async ({ page }) => {
  for (const replies of [
    {},
    { 'auth-pending': { ok: false, message: 'UTD chưa chạy. Mở UTD rồi thử lại.' }, takeover: { ok: false, message: 'UTD chưa chạy. Mở UTD rồi thử lại.' } },
    { 'auth-pending': SIGNED_IN, takeover: { ok: false, message: 'UTD để trình duyệt tải file này.' } },
  ] satisfies Record<string, Reply>[]) {
    await loadExtension(page, replies)
    await startDownload(page, ZIP)
    await expect.poll(() => names(page)).toContain('resume')
    const done = await names(page)
    expect(done).not.toContain('cancel')
    expect(done).not.toContain('erase')
    expect(done.filter((name) => name === 'resume')).toHaveLength(1)
  }
})

test('S-76: UTD không trả lời ⇒ sau 20 s trình duyệt tải tiếp', async ({ page }) => {
  await page.clock.install()
  await loadExtension(page, { 'auth-pending': SIGNED_IN, takeover: 'hang' })
  await startDownload(page, ZIP)
  await expect.poll(() => names(page)).toContain('native')
  await expect.poll(async () => (await calls(page)).filter((call) => call.name === 'native').length).toBe(2)
  expect(await names(page)).not.toContain('resume')
  await page.clock.runFor(21_000)
  await expect.poll(() => names(page)).toContain('resume')
  expect(await names(page)).not.toContain('cancel')
})

test('S-76: bỏ qua blob:, data:, file nhỏ, đuôi bị loại, tiện ích khác tạo, công tắc tắt — không tạm dừng, không đọc gì', async ({ page }) => {
  await loadExtension(page, { 'auth-pending': SIGNED_IN, takeover: { ok: true } }, { takeoverSkip: 'iso, .torrent' })
  const skipped = [
    { ...ZIP, id: 2, url: 'blob:https://example.com/5b1c', finalUrl: 'blob:https://example.com/5b1c' },
    { ...ZIP, id: 3, url: 'data:text/plain,hi', finalUrl: 'data:text/plain,hi' },
    { ...ZIP, id: 4, totalBytes: 300 * 1024 },
    { ...ZIP, id: 5, finalUrl: 'https://dl.example.com/ubuntu.iso' },
    { ...ZIP, id: 6, filename: 'C:\\Users\\u\\Downloads\\film.torrent' },
    { ...ZIP, id: 7, byExtensionId: 'khac' },
    { ...ZIP, id: 8, state: 'complete' },
  ]
  for (const item of skipped) await startDownload(page, item)
  // Đã xong / hỏng trước khi kịp tạm dừng: để nguyên.
  await startDownload(page, { ...ZIP, id: 404 })
  await expect.poll(() => names(page)).toContain('pause')
  await page.evaluate(() => Object.assign((window as unknown as { __ext: { store: Record<string, unknown> } }).__ext.store, { takeover: false }))
  await startDownload(page, { ...ZIP, id: 9 })
  await page.waitForTimeout(300)
  const all = await calls(page)
  expect(all.filter((call) => call.name === 'pause').map((call) => call.args[0])).toEqual([404])
  expect(all.map((call) => call.name)).not.toContain('native')
  expect(all.map((call) => call.name)).not.toContain('cookies.getAll')
  expect(all.map((call) => call.name)).not.toContain('resume')
})

test('S-76: giữ Alt khi bấm ⇒ lần tải đó để trình duyệt; lần sau lại bắt', async ({ page }) => {
  await loadExtension(page, { 'auth-pending': SIGNED_IN, takeover: { ok: true } })
  const skip = (sender: Record<string, unknown>) =>
    page.evaluate((sender) => {
      const ext = (window as unknown as { __ext: { listeners: Record<string, ((...args: unknown[]) => unknown)[]> } }).__ext
      for (const listener of ext.listeners.onMessage) listener({ type: 'utd.takeover-skip' }, sender, () => undefined)
    }, sender)
  // Trang web (không phải content script của tiện ích) không bỏ qua được.
  await skip({ id: 'khac', tab: { id: 3 } })
  await skip({ id: 'utd', tab: { id: 3 } })
  await startDownload(page, { ...ZIP, id: 10 })
  await startDownload(page, { ...ZIP, id: 11 })
  await expect.poll(() => names(page)).toContain('erase')
  const paused = (await calls(page)).filter((call) => call.name === 'pause').map((call) => call.args[0])
  expect(paused).toEqual([11])
})

// Luật cookie (§4.3) giữ nguyên với takeover: trang chưa dùng đăng nhập (công tắc chung tắt, trang nhạy cảm, tắt
// riêng trang) thì không đọc cookie nào; trả lời của UTD không bao giờ dẫn tới việc đọc cookie.
test('S-76: luật cookie — chỉ đọc khi trang dùng đăng nhập, và chỉ trong lúc xử lý download vừa bắt đầu', async ({ page }) => {
  for (const pending of [
    { ok: true, auth: { site: 'example.com', enabled: false, sensitive: false } },
    { ok: true, auth: { site: 'example.com', enabled: true, sensitive: true } },
  ]) {
    await loadExtension(page, { 'auth-pending': pending, takeover: { ok: true, taskId: 't' } })
    await startDownload(page, ZIP)
    await expect.poll(() => names(page)).toContain('erase')
    const all = await calls(page)
    expect(all.map((call) => call.name)).not.toContain('cookies.getAll')
    expect((all.filter((call) => call.name === 'native')[1].args[0] as { auth?: unknown }).auth).toBeUndefined()
  }
  await loadExtension(page, { 'auth-pending': SIGNED_IN, takeover: { ok: true, taskId: 't' } }, { 'signin-off:example.com': true })
  await startDownload(page, ZIP)
  await expect.poll(() => names(page)).toContain('erase')
  expect(await names(page)).not.toContain('cookies.getAll')

  // Trả lời của UTD (kể cả có trường lạ) không kích hoạt thêm lần đọc nào: chỉ hai lần đọc của chính download đó,
  // đều trước khi hỏi UTD nhận hay không.
  const sneaky = { ok: true, taskId: 't', readCookies: 'https://mail.google.com/' } as unknown as Reply
  await loadExtension(page, { 'auth-pending': SIGNED_IN, takeover: sneaky })
  await startDownload(page, { ...ZIP, referrer: 'https://other.test/' })
  await expect.poll(() => names(page)).toContain('erase')
  await page.waitForTimeout(300)
  const all = await calls(page)
  const reads = all.filter((call) => call.name === 'cookies.getAll').map((call) => (call.args[0] as { url: string }).url)
  expect(reads, 'trang chứa link khác trang gốc: chỉ link').toEqual([ZIP.finalUrl])
  const offered = all.findIndex((call) => call.name === 'native' && (call.args[0] as { type: string }).type === 'takeover')
  expect(all.map((call) => call.name).lastIndexOf('cookies.getAll')).toBeLessThan(offered)
})

// Popup thật: công tắc "Tự bắt link tải" mặc định bật; ngưỡng và đuôi lưu vào storage cho takeover.js.
test('S-76: popup — công tắc mặc định bật, tắt được; ngưỡng MB và đuôi không bắt được lưu', async ({ page }) => {
  const root = path.resolve('extensions/chromium')
  const messages = JSON.parse(fs.readFileSync(path.join(root, '_locales/vi/messages.json'), 'utf8')) as Record<
    string,
    { message: string }
  >
  await page.route('http://ext.utd.test/**', (route) => {
    const file = path.join(root, new URL(route.request().url()).pathname)
    return route.fulfill({ path: file, contentType: file.endsWith('.js') ? 'text/javascript' : 'text/html' })
  })
  await page.addInitScript((messages: Record<string, { message: string }>) => {
    const store: Record<string, unknown> = {}
    ;(window as unknown as Record<string, unknown>).__store = store
    ;(window as unknown as Record<string, unknown>).chrome = {
      i18n: { getMessage: (key: string) => messages[key]?.message ?? '', getUILanguage: () => 'vi' },
      storage: {
        local: {
          get: async (keys: string | string[]) => {
            const list = Array.isArray(keys) ? keys : [keys]
            return Object.fromEntries(list.filter((key) => key in store).map((key) => [key, store[key]]))
          },
          set: async (values: Record<string, unknown>) => Object.assign(store, values),
          remove: async (key: string) => delete store[key],
        },
      },
      tabs: { query: async () => [{ id: 7, url: 'https://example.com/' }] },
      runtime: { sendMessage: async () => ({ ok: false }) },
    }
  }, messages)
  await page.goto('http://ext.utd.test/popup.html')
  const card = page.locator('[data-s="S-76"]')
  await expect(card).toContainText(messages.takeoverSwitch.message)
  await expect(card).toContainText(messages.takeoverHint.message)
  const toggle = page.locator('#takeover-switch')
  await expect(toggle).toBeChecked()
  await toggle.click()
  const store = () => page.evaluate(() => (window as unknown as { __store: Record<string, unknown> }).__store)
  await expect.poll(async () => (await store()).takeover).toBe(false)

  await card.locator('summary').click()
  await expect(page.locator('#takeover-min')).toHaveValue('1')
  await page.locator('#takeover-min').fill('5')
  await page.locator('#takeover-min').blur()
  await page.locator('#takeover-skip').fill('iso, torrent')
  await page.locator('#takeover-skip').blur()
  await expect.poll(async () => (await store()).takeoverMinMb).toBe(5)
  expect((await store()).takeoverSkip).toBe('iso, torrent')
})

// Cửa sổ UTD: nhận download của trình duyệt ⇒ chỉ báo nhỏ vài giây (src-tauri/src/takeover.rs, `utd://taken-over`).
test('S-77: UTD nhận download của trình duyệt ⇒ thông báo nhỏ "Nhận từ trình duyệt · tên file", tự ẩn, không mở hộp', async ({ page }) => {
  await mockTauri(page, {})
  await page.goto('/')
  await expect
    .poll(async () => (await appCalls(page, 'plugin:event|listen')).some((call) => (call.args as { event: string }).event === 'utd://taken-over'))
    .toBe(true)
  await emitEvent(page, 'utd://taken-over', { taskId: 't1', fileName: 'bao-cao quý 3.zip' })
  const notice = page.locator('[data-s="S-77"]')
  await expect(notice).toBeVisible()
  await expect(notice).toContainText('Nhận từ trình duyệt')
  await expect(notice).toContainText('bao-cao quý 3.zip')
  await expect(notice).toHaveAttribute('role', 'status')
  await expect(page.getByRole('dialog')).toHaveCount(0)
  const result = await new AxeBuilder({ page }).include('[data-s="S-77"]').withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze()
  expect(result.violations).toEqual([])
  await expect(notice).toBeHidden({ timeout: 6000 })
})

// Tiện ích thật trong Chromium, không có native host (như UTD chưa cài/chưa chạy): download thật bị tạm dừng rồi
// trình duyệt tải tiếp tới xong, đủ byte.
test('tiện ích thật: không nối được UTD ⇒ download của Chrome vẫn tải xong, đủ byte', async () => {
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'utd-takeover-'))
  const extension = path.resolve('extensions/chromium')
  const body = Buffer.alloc(3 * 1024 * 1024, 7)
  const context = await chromium.launchPersistentContext(profile, {
    channel: 'chromium',
    headless: true,
    acceptDownloads: true,
    args: [`--disable-extensions-except=${extension}`, `--load-extension=${extension}`],
  })
  try {
    const worker = context.serviceWorkers()[0] ?? (await context.waitForEvent('serviceworker'))
    const granted = await worker.evaluate(() => chrome.permissions.getAll())
    expect(granted.permissions).toEqual(expect.arrayContaining(['downloads', 'cookies']))
    // Ghi lại các lần tạm dừng / tải tiếp của takeover.js (cùng đối tượng chrome.downloads nó gọi).
    await worker.evaluate(() => {
      const log: string[] = []
      ;(self as unknown as { __log: string[] }).__log = log
      for (const name of ['pause', 'resume', 'cancel'] as const) {
        const original = chrome.downloads[name].bind(chrome.downloads)
        ;(chrome.downloads as unknown as Record<string, (id: number) => Promise<void>>)[name] = (id: number) => {
          log.push(name)
          return original(id)
        }
      }
    })
    await context.route('http://files.utd.test/**', (route) => {
      if (route.request().url().endsWith('/page')) {
        return route.fulfill({ contentType: 'text/html', body: '<a id="get" href="/bao-cao.bin">Tải</a>' })
      }
      return route.fulfill({
        status: 200,
        headers: { 'Content-Type': 'application/octet-stream', 'Content-Disposition': 'attachment; filename="bao-cao.bin"' },
        body,
      })
    })
    const page = await context.newPage()
    await page.goto('http://files.utd.test/page')
    const [download] = await Promise.all([page.waitForEvent('download'), page.click('#get')])
    const saved = await download.path()
    expect(fs.statSync(saved).size).toBe(body.length)
    await expect.poll(() => worker.evaluate(() => (self as unknown as { __log: string[] }).__log)).toEqual(['pause', 'resume'])
    const items = await worker.evaluate(() => chrome.downloads.search({}))
    expect(items.map((item) => item.state)).toEqual(['complete'])
  } finally {
    await context.close()
    fs.rmSync(profile, { recursive: true, force: true })
  }
})
