import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { chromium, expect, test } from '@playwright/test'

// Xin quyền đăng nhập theo trang (S-70, S-71; notes/auth-plan.md §8). Lỗi bản thử thật 2026-10-25: manifest khai
// `http://*/*`, `https://*/*` mà app xin `*://*.google.com/*` (src-tauri/src/auth/site.rs `permission_pattern`), nên
// Chrome ném "Only permissions specified in the manifest may be requested." và không hiện hộp hỏi; popup nuốt lỗi,
// chỉ báo "Trình duyệt chưa cho phép".

// Mẫu quyền app gửi cho popup (khớp bài `builds_permission_patterns` ở site.rs).
const ORIGINS = ['*://*.google.com/*', '*://127.0.0.1/*']

test('tiện ích thật: xin cookies + mẫu quyền của app thì Chrome hỏi người dùng, không ném lỗi', async () => {
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'utd-ext-'))
  const extension = path.resolve('extensions/chromium')
  const context = await chromium.launchPersistentContext(profile, {
    channel: 'chromium',
    headless: true,
    args: [`--disable-extensions-except=${extension}`, `--load-extension=${extension}`],
  })
  try {
    const worker = context.serviceWorkers()[0] ?? (await context.waitForEvent('serviceworker'))
    const page = await context.newPage()
    await page.goto(`chrome-extension://${new URL(worker.url()).host}/popup.html`)
    // Mỗi lần bấm xin một bộ quyền, ngay trong cú bấm (trình duyệt chỉ hỏi khi người dùng vừa bấm).
    await page.evaluate(() => {
      const state = window as unknown as { asks: string[][]; results: string[] }
      state.results = []
      const button = document.createElement('button')
      button.id = 'ask'
      button.addEventListener('click', () => {
        const index = state.results.push('pending') - 1
        const [permission, ...origins] = state.asks[index]
        chrome.permissions.request({ permissions: [permission], origins }).then(
          (granted) => (state.results[index] = String(granted)),
          (error: unknown) => (state.results[index] = `error: ${error instanceof Error ? error.message : String(error)}`),
        )
      })
      document.body.prepend(button)
    })
    const ask = async (permission: string, origins: string[]) => {
      await page.evaluate((next) => (window as unknown as { asks: string[][] }).asks.push(next), [permission, ...origins])
      await page.click('#ask')
      await page.waitForTimeout(800)
    }
    await page.evaluate(() => ((window as unknown as { asks: string[][] }).asks = []))

    // Đối chứng: quyền không khai trong manifest bị ném lỗi ngay, không cần ai trả lời.
    await ask('history', [])
    // Quyền app thật sự xin: Chrome mở hộp hỏi và chờ người dùng (không ném lỗi).
    await ask('cookies', ORIGINS)
    const results = await page.evaluate(() => (window as unknown as { results: string[] }).results)
    expect(results[0]).toMatch(/^error: Only permissions specified in the manifest may be requested/)
    expect(results[1]).toBe('pending')
  } finally {
    await context.close()
    fs.rmSync(profile, { recursive: true, force: true })
  }
})

// Popup thật (popup.html + popup.js) với API trình duyệt giả: tiện ích chưa cài vào trình duyệt nào.
function stubPopupApi(input: { messages: Record<string, { message: string }>; failure: string | null }) {
  const calls: unknown[] = []
  const api = {
    i18n: {
      getMessage: (key: string, values?: string[]) =>
        (input.messages[key]?.message ?? '').replace('$1', values?.[0] ?? ''),
      getUILanguage: () => 'vi',
    },
    storage: { local: { get: async () => ({}), set: async () => undefined } },
    tabs: { query: async () => [{ id: 7, url: 'https://drive.google.com/drive/my-drive' }] },
    runtime: {
      sendMessage: async (message: { type: string }) => {
        calls.push(message)
        if (message.type !== 'utd.auth-pending') return { ok: true }
        return {
          ok: true,
          auth: {
            site: 'google.com',
            origin: '*://*.google.com/*',
            sensitive: false,
            enabled: true,
            pending: 1,
            urls: ['https://drive.google.com/file/d/abc/view'],
          },
        }
      },
    },
    permissions: {
      contains: async () => false,
      request: async () => {
        if (input.failure) throw new Error(input.failure)
        return false
      },
      remove: async () => true,
    },
  }
  const target = window as unknown as Record<string, unknown>
  target.chrome = api
  target.__calls = calls
}

async function openPopup(page: import('@playwright/test').Page, failure: string | null) {
  const root = path.resolve('extensions/chromium')
  const messages = JSON.parse(fs.readFileSync(path.join(root, '_locales/vi/messages.json'), 'utf8')) as Record<
    string,
    { message: string }
  >
  await page.route('http://ext.utd.test/**', (route) => {
    const file = path.join(root, new URL(route.request().url()).pathname)
    return route.fulfill({ path: file, contentType: file.endsWith('.js') ? 'text/javascript' : 'text/html' })
  })
  await page.addInitScript(stubPopupApi, { messages, failure })
  await page.goto('http://ext.utd.test/popup.html')
  await expect(page.locator('#auth')).toBeVisible()
}

test('S-71: trình duyệt ném lỗi khi xin quyền ⇒ popup hiện đúng câu lỗi đó, không gửi gì cho UTD', async ({ page }) => {
  const failure = 'Only permissions specified in the manifest may be requested.'
  await openPopup(page, failure)
  await page.locator('#auth-use').click()
  await expect(page.locator('#auth-status')).toHaveText(failure)
  await expect(page.locator('#auth')).toHaveAttribute('data-s', 'S-71')
  await expect(page.locator('#auth-switch')).not.toBeChecked()
  const sent = await page.evaluate(() => (window as unknown as { __calls: { type: string }[] }).__calls.map((call) => call.type))
  expect(sent).toEqual(['utd.auth-pending'])
})

test('S-71: người dùng từ chối ⇒ "Trình duyệt chưa cho phép."', async ({ page }) => {
  await openPopup(page, null)
  await page.locator('#auth-switch').click()
  await expect(page.locator('#auth-status')).toHaveText('Trình duyệt chưa cho phép.')
  await expect(page.locator('#auth-switch')).not.toBeChecked()
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
