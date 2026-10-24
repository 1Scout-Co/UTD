import AxeBuilder from '@axe-core/playwright'
import { expect, test, type Page } from '@playwright/test'

import { expandPattern, parseLinks, patternCount } from '../src/features/home/model/batch-links'

// Chờ hiệu ứng mở hộp (mờ dần, phóng to) chạy xong: đo màu giữa chừng thì độ tương phản sai.
async function axe(page: Page) {
  await page.waitForFunction(() => document.getAnimations().every((animation) => animation.playState !== 'running'))
  return new AxeBuilder({ page })
    .include('[role="dialog"]')
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
}

const VIDEO = 'https://www.youtube.com/watch?v=09OGSRptb0M'

test('batch-links: tách link, bỏ trùng và dòng rác, giữ thứ tự', () => {
  const parsed = parseLinks(
    [
      'https://example.com/a.zip',
      '',
      'rác rưởi',
      '  https://EXAMPLE.com/a.zip  ',
      '(xem https://example.com/b.jpg)',
      'ftp://example.com/c.zip javascript:alert(1)',
      'https://en.wikipedia.org/wiki/Foo_(bar) https://user:pass@example.com/x.zip',
    ].join('\n'),
    ['https://example.com/known.zip'],
  )
  expect(parsed.links).toEqual([
    'https://example.com/a.zip',
    'https://example.com/b.jpg',
    'https://en.wikipedia.org/wiki/Foo_(bar)',
  ])
  expect(parsed.duplicates).toBe(1)
  expect(parsed.junk).toBe(2)
  expect(parseLinks('https://example.com/known.zip', ['https://example.com/known.zip']).duplicates).toBe(1)
})

test('batch-links: mẫu kiểu IDM [001-120], [a-z], chặn mẫu quá lớn', () => {
  const numbers = expandPattern('https://site.com/img[001-120].jpg')
  expect(numbers.ok && numbers.links.length).toBe(120)
  expect(numbers.ok && [numbers.links[0], numbers.links[119]]).toEqual([
    'https://site.com/img001.jpg',
    'https://site.com/img120.jpg',
  ])
  const mixed = expandPattern('https://site.com/[a-c]/p[1-2].png')
  expect(mixed.ok && mixed.links).toEqual([
    'https://site.com/a/p1.png',
    'https://site.com/a/p2.png',
    'https://site.com/b/p1.png',
    'https://site.com/b/p2.png',
    'https://site.com/c/p1.png',
    'https://site.com/c/p2.png',
  ])
  const down = expandPattern('https://site.com/[3-1].jpg')
  expect(down.ok && down.links.map((link) => link.slice(-5))).toEqual(['3.jpg', '2.jpg', '1.jpg'])
  expect(patternCount('https://site.com/[1-100000].jpg')).toBeGreaterThan(10_000)
  expect(expandPattern('https://site.com/[1-100000].jpg')).toEqual({
    ok: false,
    error: 'tooBig',
  })
  expect(patternCount('https://site.com/[0001-9999]/[a-z].jpg')).toBeGreaterThan(10_000)
  expect(expandPattern('https://site.com/a.jpg').ok).toBe(false)
  expect(expandPattern('site[1-3]').ok).toBe(false)
  expect(patternCount('http://[::1]/x[1-3].bin')).toBe(3)
})

test('S-05: hộp Thêm hàng loạt mở bằng nút và Ctrl+B, đếm link đã chọn', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Hàng loạt' }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('heading', { name: 'Thêm hàng loạt' })).toBeVisible()
  await expect(page.locator('[data-s="S-05"]')).toBeVisible()
  await expect(dialog.getByRole('button', { name: 'Bắt đầu ngay' })).toBeDisabled()

  await dialog
    .getByLabel('Danh sách link')
    .fill(['https://example.com/a.zip', 'không phải link', 'https://example.com/a.zip', 'https://example.com/b.jpg', VIDEO].join('\n'))
  await expect(page.locator('[data-s="S-07"]')).toBeVisible()
  await expect(dialog.getByText('Đã bỏ 1 trùng và 1 dòng không phải link.')).toBeVisible()
  await expect(dialog.getByText('Đã chọn 3/3')).toBeVisible()
  await expect(dialog.getByRole('cell', { name: 'a.zip', exact: true })).toBeVisible()
  await expect(dialog.getByRole('group', { name: 'Chất lượng video' })).toBeVisible()

  await dialog.getByRole('checkbox', { name: 'Chọn a.zip' }).click()
  await expect(dialog.getByText('Đã chọn 2/3')).toBeVisible()
  await dialog.getByRole('checkbox', { name: 'Chọn tất cả' }).click()
  await expect(dialog.getByText('Đã chọn 3/3')).toBeVisible()
  await dialog.getByRole('button', { name: /^Video/ }).click()
  await expect(dialog.getByText('Đã chọn 2/3')).toBeVisible()

  await page.keyboard.press('Escape')
  await expect(dialog).not.toBeVisible()
  await page.keyboard.press('Control+b')
  await expect(page.getByRole('heading', { name: 'Thêm hàng loạt' })).toBeVisible()
  await expect(page.getByLabel('Danh sách link')).toHaveValue('')
})

test('S-05: dán nhiều link vào ô URL của Thêm download thì chuyển sang hộp hàng loạt', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Thêm URL' }).first().click()
  const field = page.getByRole('dialog').getByLabel('URL cần tải')
  await field.evaluate((input, text) => {
    const data = new DataTransfer()
    data.setData('text', text)
    input.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }))
  }, 'https://example.com/a.zip\nhttps://example.com/b.zip\n')
  await expect(page.getByRole('heading', { name: 'Thêm hàng loạt' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Thêm download' })).toHaveCount(0)
  await expect(page.getByLabel('Danh sách link')).toHaveValue('https://example.com/a.zip\nhttps://example.com/b.zip')
})

test('S-07: mẫu [001-120] hiện trước số link, mẫu quá lớn bị chặn', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByRole('button', { name: 'Hàng loạt' })).toBeVisible()
  await page.keyboard.press('Control+b')
  const dialog = page.getByRole('dialog')
  await dialog.getByRole('button', { name: 'Mẫu' }).click()
  await dialog.getByLabel('Mẫu link').fill('https://example.com/anh[001-120].jpg')
  await expect(dialog.getByText('Mẫu tạo 120 link.')).toBeVisible()
  await expect(dialog.getByText('Đã chọn 120/120')).toBeVisible()
  await expect(dialog.getByRole('cell', { name: 'anh001.jpg', exact: true })).toBeVisible()
  // Bảng chỉ vẽ các dòng đang thấy.
  expect(await dialog.getByRole('row').count()).toBeLessThan(40)

  await dialog.getByLabel('Mẫu link').fill('https://example.com/anh[1-99999].jpg')
  await expect(dialog.getByRole('alert')).toHaveText('Mẫu quá lớn: tối đa 10.000 link.')
  await expect(dialog.getByRole('button', { name: 'Bắt đầu ngay' })).toBeDisabled()
})

test('S-07: cửa sổ nhỏ nhất (1080×680) — hộp không tràn, cuộn tới nút chính được', async ({ page }) => {
  await page.setViewportSize({ width: 1080, height: 680 })
  await page.goto('/')
  await page.getByRole('button', { name: 'Hàng loạt' }).click()
  const dialog = page.getByRole('dialog')
  const links = Array.from({ length: 30 }, (_, index) => `https://example.com/f${index}.zip`)
  await dialog.getByLabel('Danh sách link').fill([...links, VIDEO].join('\n'))
  await expect(dialog.getByText('Đã chọn 31/31')).toBeVisible()
  const box = await dialog.boundingBox()
  expect(box && box.y >= 0 && box.y + box.height <= 680).toBe(true)
  const start = dialog.getByRole('button', { name: 'Bắt đầu ngay' })
  await start.scrollIntoViewIfNeeded()
  await expect(start).toBeInViewport()
})

test('S-07: a11y — hộp Thêm hàng loạt không có lỗi truy cập', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Hàng loạt' }).click()
  await page.getByLabel('Danh sách link').fill(`https://example.com/a.zip\n${VIDEO}`)
  await expect(page.getByText('Đã chọn 2/2')).toBeVisible()
  expect((await axe(page)).violations).toEqual([])
})

test('S-27: cài đặt có mục Tải cùng lúc, mặc định 3', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Cài đặt', exact: true }).first().click()
  const group = page.getByRole('group', { name: 'Số download chạy cùng lúc' })
  await expect(group).toBeVisible()
  await expect(group.getByRole('button', { name: '3', exact: true })).toHaveAttribute('aria-pressed', 'true')
  await expect(group.getByRole('button', { name: '5', exact: true })).toBeDisabled()
})

// Giả app desktop: `invoke`/`listen` của @tauri-apps/api đi qua __TAURI_INTERNALS__ (như media-routing.spec.ts).
function mockDesktop({ early, failSecond }: { early: string[] | null; failSecond: boolean }) {
  type Callback = (message: unknown) => void
  type Args = Record<string, unknown>
  const callbacks = new Map<number, Callback>()
  const listeners: { event: string; id: number }[] = []
  const calls: { cmd: string; args: Args }[] = []
  let nextId = 1
  let pending = early
  let inflight = 0
  let peak = 0
  let limit = 3
  const task = (url: string, index: number) => ({
    id: `t${index}`,
    url,
    finalUrl: null,
    fileName: url.split('/').pop() ?? 'x',
    destDir: 'C:/Users/Test/Downloads/UTD',
    tempPath: '',
    kind: 'http',
    status: 'queued',
    totalBytes: null,
    downloadedBytes: 0,
    etag: null,
    lastModified: null,
    expectedSha256: null,
    hashState: 'unavailable',
    connections: 16,
    error: null,
    createdAt: 1_700_000_000_000,
    updatedAt: 1_700_000_000_000,
    completedAt: null,
  })
  const inspect = (url: string) =>
    new Promise((resolve) => {
      inflight += 1
      peak = Math.max(peak, inflight)
      setTimeout(() => {
        inflight -= 1
        const name = url.split('/').pop() ?? 'x'
        resolve({
          url,
          fileName: name.includes('.') ? name : `${name}.bin`,
          totalBytes: 2 * 1024 * 1024,
          contentType: name === 'about' ? 'text/html' : 'application/zip',
          page: name === 'about',
        })
      }, 120)
    })
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
    list_downloads: () => [task('https://example.com/old.iso', 0)],
    default_download_dir: () => 'C:/Users/Test/Downloads/UTD',
    disk_space: () => ({ freeBytes: 5e11, totalBytes: 1e12, label: 'C:' }),
    get_update_check: () => ({ ready: true, enabled: false, available: null, installing: false }),
    take_media_request: () => null,
    take_batch_request: () => {
      const urls = pending
      pending = null
      return urls ? { urls } : null
    },
    file_icons: () => ({}),
    inspect_link: (args) => inspect(String(args.url)),
    queue_settings: () => ({ limit, maxLimit: 10 }),
    set_queue_limit: (args) => {
      limit = Number(args.limit)
      return { limit, maxLimit: 10 }
    },
    add_batch_downloads: (args) => {
      const input = args.input as { urls: string[] }
      const failed = failSecond ? input.urls.slice(1, 2) : []
      const ok = input.urls.filter((url) => !failed.includes(url))
      return {
        tasks: ok.map((url, index) => task(url, index + 1)),
        failed: failed.map((url) => ({ url, message: 'Máy chủ từ chối.' })),
      }
    },
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
    calls: (cmd: string) => calls.filter((call) => call.cmd === cmd).map((call) => call.args),
    peak: () => peak,
    listening: (event: string) => listeners.filter((item) => item.event === event).length,
    emit: (event: string, payload: unknown) => {
      for (const item of listeners.filter((entry) => entry.event === event)) {
        callbacks.get(item.id)?.({ event, id: item.id, payload })
      }
    },
  }
}

interface Probe {
  calls: (cmd: string) => Record<string, unknown>[]
  peak: () => number
  listening: (event: string) => number
  emit: (event: string, payload: unknown) => void
}

const app = (page: Page) => ({
  calls: (cmd: string) => page.evaluate((name) => (window as unknown as { __utd: Probe }).__utd.calls(name), cmd),
  peak: () => page.evaluate(() => (window as unknown as { __utd: Probe }).__utd.peak()),
  listening: () =>
    page.evaluate(() => (window as unknown as { __utd: Probe }).__utd.listening('utd://batch-request')),
  send: (urls: string[]) =>
    page.evaluate(
      (links) => (window as unknown as { __utd: Probe }).__utd.emit('utd://batch-request', { urls: links }),
      urls,
    ),
})

test('S-07: Bắt đầu ngay gửi cả lô một lần; dò kích thước song song có giới hạn', async ({ page }) => {
  await page.addInitScript(mockDesktop, { early: null, failSecond: false })
  await page.goto('/')
  const desktop = app(page)
  await page.getByRole('button', { name: 'Hàng loạt' }).click()
  const dialog = page.getByRole('dialog')
  const files = Array.from({ length: 20 }, (_, index) => `https://example.com/f${index}.zip`)
  // Trang web đứng đầu: bảng chỉ vẽ các dòng đang thấy.
  await dialog.getByLabel('Danh sách link').fill(['https://example.com/about', ...files, VIDEO].join('\n'))
  await expect(dialog.getByText('Đã chọn 21/22 · 40 MB')).toBeVisible()
  await expect(dialog.getByRole('row').filter({ hasText: 'about.bin' }).getByText('Trang web')).toBeVisible()
  await expect(dialog.getByRole('checkbox', { name: 'Chọn about.bin' })).not.toBeChecked()

  const probed = (await desktop.calls('inspect_link')).map((args) => args.url)
  expect(probed).toHaveLength(21)
  expect(probed).not.toContain(VIDEO)
  expect(await desktop.peak()).toBeLessThanOrEqual(6)
  expect(await desktop.peak()).toBeGreaterThan(1)

  await dialog.getByRole('button', { name: '720p' }).click()
  await dialog.getByRole('button', { name: 'Bắt đầu ngay' }).click()
  await expect(dialog).not.toBeVisible()
  expect(await desktop.calls('add_batch_downloads')).toEqual([
    { input: { urls: [...files, VIDEO], quality: '720', later: false } },
  ])
  await expect(page.getByText('f19.zip')).toBeVisible()
})

test('S-07: lô từ trình duyệt mở hộp hàng loạt; Thêm vào hàng đợi xếp cuối hàng; link lỗi giữ lại', async ({
  page,
}) => {
  await page.addInitScript(mockDesktop, { early: null, failSecond: true })
  await page.goto('/')
  const desktop = app(page)
  await expect.poll(desktop.listening).toBe(1)
  await desktop.send(['https://example.com/a.zip', 'https://example.com/gallery/view', 'https://example.com/b.jpg'])
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByLabel('Danh sách link')).toHaveValue(
    'https://example.com/a.zip\nhttps://example.com/gallery/view\nhttps://example.com/b.jpg',
  )
  // Link không rõ là file bỏ tick sẵn; dò ra là file (không phải trang web) thì tick lại.
  await expect(dialog.getByText('Đã chọn 3/3')).toBeVisible()

  await dialog.getByRole('button', { name: 'Thêm vào hàng đợi' }).click()
  await expect(dialog.getByRole('alert')).toHaveText('Đã thêm 2. 1 link lỗi: Máy chủ từ chối.')
  await expect(dialog.getByLabel('Danh sách link')).toHaveValue('https://example.com/gallery/view')
  const sent = await desktop.calls('add_batch_downloads')
  expect(sent[0]).toMatchObject({ input: { later: true, quality: 'best' } })
})

test('S-07: lô rồi link video từ trình duyệt: hộp mới thay hộp cũ, không chồng lên nhau', async ({ page }) => {
  await page.addInitScript(mockDesktop, { early: null, failSecond: false })
  await page.goto('/')
  const desktop = app(page)
  await expect.poll(desktop.listening).toBe(1)
  await desktop.send(['https://example.com/a.zip', 'https://example.com/b.zip'])
  await expect(page.getByLabel('Danh sách link')).toBeVisible()
  await page.evaluate(() =>
    (window as unknown as { __utd: Probe }).__utd.emit('utd://media-request', {
      url: 'https://www.youtube.com/watch?v=jNQXAC9IVRw',
    }),
  )
  await expect(page.getByRole('dialog')).toHaveCount(1)
  await expect(page.getByLabel('Danh sách link')).toHaveCount(0)
})

test('S-07: lô tới lúc UTD vừa mở (chưa kịp nghe) vẫn mở hộp hàng loạt', async ({ page }) => {
  await page.addInitScript(mockDesktop, { early: ['https://example.com/early.zip'], failSecond: false })
  await page.goto('/')
  await expect(page.getByLabel('Danh sách link')).toHaveValue('https://example.com/early.zip')
  expect(await app(page).calls('take_batch_request')).toHaveLength(1)
})

test('S-27: Tải cùng lúc đổi được trong app', async ({ page }) => {
  await page.addInitScript(mockDesktop, { early: null, failSecond: false })
  await page.goto('/')
  await page.getByRole('button', { name: 'Cài đặt', exact: true }).first().click()
  const group = page.getByRole('group', { name: 'Số download chạy cùng lúc' })
  await group.getByRole('button', { name: '5', exact: true }).click()
  await expect(group.getByRole('button', { name: '5', exact: true })).toHaveAttribute('aria-pressed', 'true')
  expect(await app(page).calls('set_queue_limit')).toEqual([{ limit: 5 }])
})

// Extension: nạp background.js với API trình duyệt giả, gom link trên một trang mẫu.
const GALLERY = `<!doctype html><html><body>
  <a href="/files/setup.exe">Cài</a> <a href="/about">Giới thiệu</a> <a href="/files/setup.exe#top">Lặp</a>
  <a href="https://www.youtube.com/watch?v=09OGSRptb0M">Video</a> <a href="https://www.youtube.com/@kenh">Kênh</a>
  <a href="mailto:a@b.c">Thư</a> <a href="/docs/guide.pdf?v=2">PDF</a>
  <img src="/img/1.jpg"> <img src="data:image/png;base64,AA"> <video src="/clip"></video>
</body></html>`

function stubExtension() {
  const sent: unknown[] = []
  const clicks: ((info: unknown, tab: unknown) => void)[] = []
  const noop = { addListener: () => undefined }
  ;(window as unknown as Record<string, unknown>).chrome = {
    runtime: {
      onInstalled: noop,
      onStartup: noop,
      onMessage: noop,
      sendNativeMessage: async (_host: string, payload: unknown) => {
        sent.push(payload)
        return { ok: true, message: 'Đã mở UTD để chọn link' }
      },
    },
    contextMenus: {
      removeAll: async () => undefined,
      create: async () => undefined,
      onClicked: { addListener: (listener: (info: unknown, tab: unknown) => void) => clicks.push(listener) },
    },
    action: { setBadgeText: async () => undefined, setBadgeBackgroundColor: async () => undefined },
    storage: { local: { set: async () => undefined } },
    scripting: {
      executeScript: async ({ func, args }: { func: (...values: unknown[]) => unknown; args: unknown[] }) => [
        { result: func(...args) },
      ],
    },
  }
  ;(window as unknown as Record<string, unknown>).__ext = { sent, clicks }
}

test('extension: "Tải tất cả link" gom link, lọc theo đuôi, gửi một lô', async ({ page }) => {
  await page.route('http://gallery.test/**', (route) =>
    route.fulfill({ contentType: 'text/html', body: GALLERY }),
  )
  await page.addInitScript(stubExtension)
  await page.goto('http://gallery.test/page')
  await page.addScriptTag({ path: 'extensions/chromium/background.js' })
  const sent = await page.evaluate(async () => {
    const ext = (window as unknown as { __ext: { sent: unknown[]; clicks: ((info: unknown, tab: unknown) => void)[] } })
      .__ext
    for (const click of ext.clicks) {
      click({ menuItemId: 'utd-download-all', frameId: 0 }, { id: 1, url: location.href })
    }
    await new Promise((resolve) => setTimeout(resolve, 50))
    return ext.sent
  })
  expect(sent).toEqual([
    {
      version: 1,
      type: 'batch',
      pageUrl: 'http://gallery.test/page',
      urls: [
        'http://gallery.test/files/setup.exe',
        'https://www.youtube.com/watch?v=09OGSRptb0M',
        'http://gallery.test/docs/guide.pdf?v=2',
        'http://gallery.test/img/1.jpg',
        'http://gallery.test/clip',
      ],
    },
  ])
})
