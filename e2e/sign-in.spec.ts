import AxeBuilder from '@axe-core/playwright'
import { expect, test, type Page } from '@playwright/test'

// Tải cần đăng nhập (S-14, S-72, S-73; docs/specs/2026-10-23-utd-v1/notes/auth-plan.md). Giao diện chỉ thấy
// mã lần cho phép và tên trang, không bao giờ thấy cookie.

const WATCH = 'https://www.youtube.com/watch?v=private01'

// Giả app desktop: `invoke`/`listen` của @tauri-apps/api đi qua __TAURI_INTERNALS__ (như batch.spec.ts).
function mockDesktop({ watch }: { watch: string | null }) {
  type Callback = (message: unknown) => void
  type Args = Record<string, unknown>
  const callbacks = new Map<number, Callback>()
  const listeners: { event: string; id: number }[] = []
  const calls: { cmd: string; args: Args }[] = []
  let nextId = 1
  let media: { url: string; authGrant: string } | null = watch ? { url: watch, authGrant: 'g1' } : null
  let status = { enabled: true, sites: ['youtube.com'] }
  const task = {
    id: 't1',
    url: 'https://drive.usercontent.google.com/download?id=abc',
    finalUrl: null,
    fileName: 'bao-cao.pdf',
    destDir: 'C:/Users/Test/Downloads/UTD',
    tempPath: '',
    kind: 'http',
    status: 'failed',
    totalBytes: null,
    downloadedBytes: 0,
    etag: null,
    lastModified: null,
    expectedSha256: null,
    hashState: 'unavailable',
    connections: 16,
    error: { class: 'user', code: 'auth.required', message: 'This page requires a sign-in.' },
    createdAt: 1_700_000_000_000,
    updatedAt: 1_700_000_000_000,
    completedAt: null,
    authSites: ['google.com'],
  }
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
    'plugin:app|version': () => '0.1.0',
    list_downloads: () => [task],
    default_download_dir: () => 'C:/Users/Test/Downloads/UTD',
    disk_space: () => ({ freeBytes: 5e11, totalBytes: 1e12, label: 'C:' }),
    get_update_check: () => ({ ready: true, enabled: false, available: null, installing: false }),
    take_media_request: () => {
      const pending = media
      media = null
      return pending
    },
    take_batch_request: () => null,
    file_icons: () => ({}),
    queue_settings: () => ({ limit: 3, maxLimit: 10 }),
    browser_integration_status: () => ({
      installed: true,
      hostPath: '',
      manifestPath: '',
      chromiumExtensionId: 'x',
      firefoxExtensionId: 'x',
      extensionDir: '',
    }),
    media_tools_status: () => null,
    media_probe: () => Promise.reject('[media.login] Nguồn này cần đăng nhập.'),
    open_web_page: () => null,
    auth_status: () => status,
    set_auth_enabled: (args) => {
      status = { enabled: Boolean(args.enabled), sites: args.enabled ? status.sites : [] }
      return status
    },
    forget_auth_cookies: () => {
      status = { ...status, sites: [] }
      return status
    },
  }
  const target = window as unknown as Record<string, unknown>
  target.isTauri = true
  target.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined }
  target.__TAURI_INTERNALS__ = {
    metadata: {
      currentWindow: { label: 'main' },
      currentWebview: { windowLabel: 'main', label: 'main' },
    },
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
      return run ? run(args) : null
    },
  }
  target.__utd = {
    calls: (cmd: string) => calls.filter((call) => call.cmd === cmd).map((call) => call.args),
    emit: (event: string, payload: unknown) => {
      for (const item of listeners.filter((entry) => entry.event === event)) {
        callbacks.get(item.id)?.({ event, id: item.id, payload })
      }
    },
  }
}

interface Probe {
  calls: (cmd: string) => Record<string, unknown>[]
  emit: (event: string, payload: unknown) => void
}

const app = (page: Page) => ({
  calls: (cmd: string) => page.evaluate((name) => (window as unknown as { __utd: Probe }).__utd.calls(name), cmd),
  emit: (event: string, payload: unknown) =>
    page.evaluate(
      ([name, value]) => (window as unknown as { __utd: Probe }).__utd.emit(name as string, value),
      [event, payload] as const,
    ),
})

test('S-14: Phân tích báo cần đăng nhập; popup cho phép thì tự phân tích lại với lần cho phép mới', async ({ page }) => {
  await page.addInitScript(mockDesktop, { watch: WATCH })
  await page.goto('/')
  const dialog = page.getByRole('dialog')
  const notice = dialog.locator('[data-s="S-14"]')
  await expect(notice).toContainText('Cần đăng nhập · www.youtube.com')
  await expect(notice).toContainText('Bấm biểu tượng UTD trên trình duyệt')
  // Lần Phân tích đầu dùng lần cho phép đi kèm cú bấm trên trình duyệt.
  expect((await app(page).calls('media_probe'))[0]).toEqual({ url: WATCH, authGrant: 'g1' })

  await notice.getByRole('button', { name: 'Mở trang' }).click()
  await expect.poll(() => app(page).calls('open_web_page')).toEqual([{ url: WATCH }])

  // Trang khác được cho phép: không phân tích lại.
  await app(page).emit('utd://auth-granted', { site: 'google.com', authGrant: 'other' })
  // Đúng trang: phân tích lại với mã mới.
  await app(page).emit('utd://auth-granted', { site: 'youtube.com', authGrant: 'g2' })
  await expect.poll(async () => (await app(page).calls('media_probe')).map((call) => call.authGrant)).toEqual([
    'g1',
    'g2',
  ])
  const results = await new AxeBuilder({ page }).include('[role="dialog"]').analyze()
  expect(results.violations).toEqual([])
})

test('S-72: task cần đăng nhập có nhãn riêng; Chẩn đoán có nút Mở trang', async ({ page }) => {
  await page.addInitScript(mockDesktop, { watch: null })
  await page.goto('/')
  const row = page.locator('tbody tr', { hasText: 'bao-cao.pdf' })
  await expect(row.locator('[data-s="S-72"]')).toHaveText('Cần đăng nhập')
  await row.click({ button: 'right' })
  await page.getByRole('menuitem', { name: 'Chẩn đoán' }).click()
  const doctor = page.getByRole('dialog')
  await expect(doctor.getByRole('heading', { name: 'Trang này cần đăng nhập' })).toBeVisible()
  await doctor.getByRole('button', { name: 'Mở trang' }).click()
  await expect
    .poll(() => app(page).calls('open_web_page'))
    .toEqual([{ url: 'https://drive.usercontent.google.com/download?id=abc' }])
})

test('S-73: Cài đặt có mục Đăng nhập của trình duyệt; Quên cookie và công tắc gọi app', async ({ page }) => {
  await page.addInitScript(mockDesktop, { watch: null })
  await page.goto('/')
  await page.getByRole('button', { name: 'Cài đặt', exact: true }).first().click()
  const section = page.locator('[data-s="S-73"]')
  await expect(section).toContainText('Trang đang giữ cookie: 1')
  await section.getByRole('button', { name: 'Quên cookie' }).click()
  await expect(section).toContainText('Chưa giữ cookie của trang nào.')
  expect(await app(page).calls('forget_auth_cookies')).toHaveLength(1)
  const toggle = page.getByRole('switch', { name: 'Dùng đăng nhập của trình duyệt' })
  await expect(toggle).toBeChecked()
  await toggle.click()
  await expect(toggle).not.toBeChecked()
  expect(await app(page).calls('set_auth_enabled')).toEqual([{ enabled: false }])
})

// Tiện ích: nạp background.js thật với API trình duyệt giả (như batch.spec.ts). Quyền xin một lần lúc cài (Q11):
// UTD giả trả trang gốc (`utd.test`) và trang nhạy cảm (`bank.test`); tắt riêng một trang bằng `signin-off:<trang>`.
// Không bao giờ gọi native host thật.
function stubSignedInExtension() {
  const sent: Record<string, unknown>[] = []
  const reads: string[] = []
  const clicks: ((info: unknown, tab: unknown) => void)[] = []
  const messages: ((message: unknown, sender: unknown, reply: (value: unknown) => void) => boolean)[] = []
  const noop = { addListener: () => undefined }
  const store: Record<string, unknown> = {}
  const jar = [
    { name: 'SID', value: 's1', domain: '.utd.test', hostOnly: false, path: '/', secure: true, session: true, sameSite: 'no_restriction' },
    { name: 'pref', value: 'p1', domain: 'drive.utd.test', hostOnly: true, path: '/', secure: false, session: false, expirationDate: 2_000_000_000.5, sameSite: 'unspecified' },
    { name: 'lock', value: 'l1', domain: 'drive.utd.test', hostOnly: true, path: '/', secure: false, session: true, sameSite: 'strict' },
    { name: 'bank', value: 'b1', domain: 'bank.test', hostOnly: true, path: '/', secure: true, session: true, sameSite: 'lax' },
  ]
  const siteOf = (url: string) => new URL(url).hostname.split('.').slice(-2).join('.')
  ;(window as unknown as Record<string, unknown>).chrome = {
    runtime: {
      id: 'ext-id',
      getURL: (path: string) => `chrome-extension://ext-id/${path}`,
      onInstalled: noop,
      onStartup: noop,
      onMessage: { addListener: (listener: (typeof messages)[number]) => messages.push(listener) },
      sendNativeMessage: async (_host: string, payload: Record<string, unknown>) => {
        sent.push(payload)
        const site = siteOf(String(payload.url))
        return { ok: true, auth: { site, sensitive: site === 'bank.test', enabled: true, pending: 0 } }
      },
    },
    contextMenus: {
      removeAll: async () => undefined,
      create: async () => undefined,
      onClicked: { addListener: (listener: (info: unknown, tab: unknown) => void) => clicks.push(listener) },
    },
    tabs: { sendMessage: async () => Promise.reject(new Error('no content script')) },
    action: { setBadgeText: async () => undefined, setBadgeBackgroundColor: async () => undefined },
    storage: {
      local: {
        set: async (values: Record<string, unknown>) => Object.assign(store, values),
        get: async (key: string) => (key in store ? { [key]: store[key] } : {}),
      },
    },
    cookies: {
      getAllCookieStores: async () => [{ id: '0', tabIds: [1] }],
      getAll: async ({ url }: { url: string }) => {
        reads.push(url)
        const host = new URL(url).hostname
        return jar.filter((cookie) =>
          cookie.hostOnly ? cookie.domain === host : host.endsWith(cookie.domain.replace(/^\./, '')),
        )
      },
    },
  }
  ;(window as unknown as Record<string, unknown>).__ext = { sent, reads, clicks, messages, store }
}

test('extension: chuột phải kèm cookie của trang; chéo trang bỏ Strict; trang tắt riêng và trang nhạy cảm không kèm', async ({ page }) => {
  await page.route('http://drive.utd.test/**', (route) => route.fulfill({ contentType: 'text/html', body: '<p>Drive</p>' }))
  await page.addInitScript(stubSignedInExtension)
  await page.goto('http://drive.utd.test/page')
  await page.addScriptTag({ path: 'extensions/chromium/background.js' })
  const summary = await page.evaluate(async () => {
    type Ext = {
      sent: Record<string, unknown>[]
      reads: string[]
      clicks: ((info: unknown, tab: unknown) => void)[]
      messages: ((message: unknown, sender: unknown, reply: (value: unknown) => void) => boolean)[]
      store: Record<string, unknown>
    }
    const ext = (window as unknown as { __ext: Ext }).__ext
    const wait = () => new Promise((resolve) => setTimeout(resolve, 50))
    const click = async (linkUrl: string, pageUrl: string) => {
      for (const listener of ext.clicks) listener({ menuItemId: 'utd-download', linkUrl }, { id: 1, url: pageUrl })
      await wait()
    }
    await click('http://usercontent.utd.test/file.pdf', 'http://drive.utd.test/page')
    await click('http://drive.utd.test/file.pdf', 'http://other.example/page')
    await click('http://other.example/file.pdf', 'http://drive.utd.test/page')
    ext.store['signin-off:utd.test'] = true
    await click('http://drive.utd.test/file.pdf', 'http://drive.utd.test/page')
    delete ext.store['signin-off:utd.test']
    await click('https://bank.test/statement.pdf', 'https://bank.test/account')
    const readsBefore = ext.reads.length
    // Popup hỏi việc chờ: không đọc cookie nào (T-E2). Tin từ trang/tab khác popup bị bỏ.
    const popup = { id: 'ext-id', url: 'chrome-extension://ext-id/popup.html' }
    for (const listener of ext.messages) listener({ type: 'utd.auth-pending', url: 'http://drive.utd.test/page' }, popup, () => undefined)
    for (const listener of ext.messages)
      listener({ type: 'utd.auth-pending', url: 'http://drive.utd.test/page' }, { ...popup, tab: { id: 1 } }, () => undefined)
    await wait()
    return {
      sent: ext.sent
        .filter((payload) => payload.type !== 'auth-pending')
        .map((payload) => {
          const auth = payload.auth as { site: string; cookies: { name: string }[]; userAgent: string } | undefined
          return {
            type: payload.type,
            version: payload.version,
            cookies: auth ? auth.cookies.map((cookie) => cookie.name).sort().join(',') : null,
            site: auth?.site ?? null,
            ua: Boolean(auth?.userAgent),
          }
        }),
      asked: ext.sent.filter((payload) => payload.type === 'auth-pending').length,
      bankRead: ext.reads.some((url) => url.includes('bank.test')),
      pendingReads: ext.reads.length - readsBefore,
    }
  })
  expect(summary.sent).toEqual([
    { type: 'download', version: 2, cookies: 'SID,lock,pref', site: 'utd.test', ua: true },
    { type: 'download', version: 2, cookies: 'SID,pref', site: 'utd.test', ua: true },
    { type: 'download', version: 1, cookies: null, site: null, ua: false },
    { type: 'download', version: 1, cookies: null, site: null, ua: false },
    { type: 'download', version: 1, cookies: null, site: null, ua: false },
  ])
  // Hỏi UTD trang gốc mỗi host một lần (nhớ 5 phút) + một lần của popup.
  expect(summary.asked).toBe(5)
  expect(summary.bankRead).toBe(false)
  expect(summary.pendingReads).toBe(0)
})

test('extension: nút trên trang — đọc định dạng không cookie, chỉ kèm cookie khi nguồn đòi đăng nhập; nhớ kết quả', async ({ page }) => {
  await page.route('http://drive.utd.test/**', (route) => route.fulfill({ contentType: 'text/html', body: '<p>Drive</p>' }))
  await page.addInitScript(stubSignedInExtension)
  await page.goto('http://drive.utd.test/page')
  await page.addScriptTag({ path: 'extensions/chromium/background.js' })
  const summary = await page.evaluate(async () => {
    type Listener = (message: unknown, sender: unknown, reply: (value: unknown) => void) => boolean
    const ext = (window as unknown as { __ext: { sent: Record<string, unknown>[]; messages: Listener[] } }).__ext
    const tab = { id: 1, url: 'http://drive.utd.test/page' }
    const content = { id: 'ext-id', tab, frameId: 0, url: tab.url }
    const ask = (message: unknown, sender: unknown = content) =>
      new Promise((resolve) => {
        let handled = false
        for (const listener of ext.messages) handled = listener(message, sender, resolve) || handled
        if (!handled) resolve('bỏ qua')
      })
    const video = 'http://drive.utd.test/watch?v=1'
    await ask({ type: 'utd.video-formats', url: video, signIn: false })
    await ask({ type: 'utd.video-formats', url: video, signIn: false })
    await ask({ type: 'utd.video-formats', url: video, signIn: true })
    await ask({ type: 'utd.video-download', url: video, title: 'Video', selector: 'bv*+ba/b', audioFormat: null, signIn: false })
    await ask({ type: 'utd.video-download', url: video, title: 'Video', selector: 'bv*+ba/b', audioFormat: null, signIn: true })
    // Khung con / trang khác không gửi được lệnh cho nền.
    const ignored = [
      await ask({ type: 'utd.video-formats', url: video, signIn: true }, { ...content, frameId: 3 }),
      await ask({ type: 'utd.video-formats', url: video, signIn: true }, { ...content, id: 'other-extension' }),
    ]
    return {
      native: ext.sent
        .filter((payload) => payload.type !== 'auth-pending')
        .map((payload) => [payload.type, Boolean(payload.auth), payload.selector ?? null]),
      ignored,
    }
  })
  expect(summary.native).toEqual([
    ['media-formats', false, null],
    ['media-formats', true, null],
    ['media-download', false, 'bv*+ba/b'],
    ['media-download', true, 'bv*+ba/b'],
  ])
  expect(summary.ignored).toEqual(['bỏ qua', 'bỏ qua'])
})
