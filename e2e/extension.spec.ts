import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { chromium, expect, test } from '@playwright/test'

// Quyền của tiện ích (Q11, notes/auth-plan.md GĐ3): xin một lần lúc cài — mọi trang + cookies, content script gắn nút
// "Tải video này". Lỗi cũ (2026-10-25) của cách xin theo trang: manifest khai `http://*/*` mà app xin `*://*.<trang>/*`,
// Chrome ném lỗi và không hỏi; giờ không còn xin theo trang nữa.

const VIDEO_PAGE = `<!doctype html><body style="margin:0"><p style="height:120px;margin:0">x</p>
<video width="640" height="360" style="display:block;margin-left:40px;background:#000"></video></body>`

test('tiện ích thật: quyền mọi trang + cookies có ngay lúc cài; trang có video có nút, bấm thì bảng báo khi chưa nối UTD', async () => {
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'utd-ext-'))
  const extension = path.resolve('extensions/chromium')
  const context = await chromium.launchPersistentContext(profile, {
    channel: 'chromium',
    headless: true,
    args: [`--disable-extensions-except=${extension}`, `--load-extension=${extension}`],
  })
  try {
    const worker = context.serviceWorkers()[0] ?? (await context.waitForEvent('serviceworker'))
    const granted = await worker.evaluate(() => chrome.permissions.getAll())
    expect(granted.permissions).toContain('cookies')
    expect(granted.origins).toContain('*://*/*')

    await context.route('http://video.utd.test/**', (route) => route.fulfill({ contentType: 'text/html', body: VIDEO_PAGE }))
    const page = await context.newPage()
    await page.goto('http://video.utd.test/watch')
    const host = page.locator('utd-video-button')
    await expect(host).toHaveCount(0)
    const box = (await page.locator('video').boundingBox())!
    // Content script chạy lúc trang rảnh: rê chuột tới khi nút hiện.
    await expect(async () => {
      await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
      await page.mouse.move(box.x + box.width / 2 + 3, box.y + box.height / 2 + 3)
      await expect(host.locator('.pill')).toBeVisible({ timeout: 500 })
    }).toPass({ timeout: 10_000 })
    await host.locator('.pill').click()
    // Không có native host trong bài thử: bảng báo lỗi nối, không treo ở "đang đọc".
    await expect(host.locator('.status.error')).toBeVisible({ timeout: 10_000 })
  } finally {
    await context.close()
    fs.rmSync(profile, { recursive: true, force: true })
  }
})

// Popup thật (popup.html + popup.js) với API trình duyệt giả. Không có `permissions`: popup không còn xin quyền.
function stubPopupApi(input: { messages: Record<string, { message: string }>; pending: number }) {
  const calls: Record<string, unknown>[] = []
  const store: Record<string, unknown> = {}
  const api = {
    i18n: {
      getMessage: (key: string, values?: string[]) =>
        (input.messages[key]?.message ?? '').replace('$1', values?.[0] ?? ''),
      getUILanguage: () => 'vi',
    },
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
    tabs: { query: async () => [{ id: 7, url: 'https://drive.google.com/file/d/abc/view' }] },
    runtime: {
      sendMessage: async (message: Record<string, unknown>) => {
        calls.push(message)
        if (message.type !== 'utd.auth-pending') return { ok: true }
        return {
          ok: true,
          auth: {
            site: 'google.com',
            origin: '*://*.google.com/*',
            sensitive: false,
            enabled: true,
            pending: input.pending,
            urls: ['https://drive.google.com/file/d/abc/view'],
          },
        }
      },
    },
  }
  const target = window as unknown as Record<string, unknown>
  target.chrome = api
  target.__popup = { calls, store }
}

async function openPopup(page: import('@playwright/test').Page, pending: number) {
  const root = path.resolve('extensions/chromium')
  const messages = JSON.parse(fs.readFileSync(path.join(root, '_locales/vi/messages.json'), 'utf8')) as Record<
    string,
    { message: string }
  >
  await page.route('http://ext.utd.test/**', (route) => {
    const file = path.join(root, new URL(route.request().url()).pathname)
    return route.fulfill({ path: file, contentType: file.endsWith('.js') ? 'text/javascript' : 'text/html' })
  })
  await page.addInitScript(stubPopupApi, { messages, pending })
  await page.goto('http://ext.utd.test/popup.html')
  await expect(page.locator('#auth')).toBeVisible()
}

const popupState = (page: import('@playwright/test').Page) =>
  page.evaluate(() => (window as unknown as { __popup: { calls: Record<string, unknown>[]; store: Record<string, unknown> } }).__popup)

test('S-70: "Đăng nhập trên trang này" bật sẵn, tắt riêng trang này; "Hiện nút Tải video" lưu lại', async ({ page }) => {
  await openPopup(page, 0)
  const signIn = page.locator('#auth-switch')
  await expect(signIn).toBeChecked()
  await signIn.click()
  await expect.poll(async () => (await popupState(page)).store['signin-off:google.com']).toBe(true)
  await signIn.click()
  await expect.poll(async () => 'signin-off:google.com' in (await popupState(page)).store).toBe(false)

  const video = page.locator('#video-switch')
  await expect(video).toBeChecked()
  await expect(page.locator('label', { has: video })).toContainText('Hiện nút "Tải video này" trên trang')
  await video.click()
  await expect.poll(async () => (await popupState(page)).store.videoButton).toBe(false)
})

test('S-70: việc chờ đăng nhập ⇒ "Tiếp tục (1)" gửi cookie cho UTD, không hỏi quyền nữa', async ({ page }) => {
  await openPopup(page, 1)
  await page.locator('#auth-use').click()
  await expect(page.locator('#auth-status')).toHaveClass('ok')
  const calls = (await popupState(page)).calls.map((call) => call.type)
  expect(calls).toEqual(['utd.auth-pending', 'utd.auth-renew'])
})

// background.js thật với API giả tối thiểu: câu có mã của UTD (`[mã] câu`) đổi sang chữ của tiện ích.
function stubBackgroundApi(messages: Record<string, { message: string }>) {
  const noop = { addListener: () => undefined }
  ;(window as unknown as Record<string, unknown>).chrome = {
    i18n: { getMessage: (key: string) => messages[key]?.message ?? '' },
    runtime: { onInstalled: noop, onStartup: noop, onMessage: noop },
    contextMenus: { removeAll: async () => undefined, create: async () => undefined, onClicked: noop },
    storage: { local: { set: async () => undefined } },
  }
}

test('tiện ích: trang thư mục Drive ⇒ câu rõ của tiện ích; mã lạ thì bỏ mã, giữ câu', async ({ page }) => {
  const messages = JSON.parse(
    fs.readFileSync(path.resolve('extensions/chromium/_locales/vi/messages.json'), 'utf8'),
  ) as Record<string, { message: string }>
  await page.route('http://ext.utd.test/**', (route) => route.fulfill({ contentType: 'text/html', body: '<p>x</p>' }))
  await page.addInitScript(stubBackgroundApi, messages)
  await page.goto('http://ext.utd.test/')
  await page.addScriptTag({ path: 'extensions/chromium/background.js' })
  const shown = await page.evaluate(() => {
    const fromHost = (globalThis as unknown as { eval: (code: string) => (text: string) => string }).eval('fromHost')
    return [
      fromHost('[link.drive_folder] Đây là thư mục Google Drive, không phải file. Mở file ở trang riêng rồi thử lại.'),
      fromHost('[dir.data_dir] Không dùng thư mục dữ liệu của UTD để tải.'),
      fromHost('UTD không phản hồi.'),
    ]
  })
  expect(shown).toEqual([
    messages.driveFolder.message,
    'Không dùng thư mục dữ liệu của UTD để tải.',
    messages.noResponse.message,
  ])
})
