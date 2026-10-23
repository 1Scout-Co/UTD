import AxeBuilder from '@axe-core/playwright'
import { expect, test, type Page } from '@playwright/test'

import { isMediaPage } from '../src/features/home/model/media-pages'

// Cùng bộ ca với tests trong src-tauri/src/media_pages.rs (bản Rust của luật): sửa một bên thì sửa cả bên kia.
const MEDIA_PAGES = [
  'https://www.youtube.com/watch?v=09OGSRptb0M',
  'https://youtube.com/watch?v=09OGSRptb0M&t=30s',
  'https://m.youtube.com/watch?v=09OGSRptb0M',
  'https://music.youtube.com/watch?v=09OGSRptb0M&list=RDAMVM09OGSRptb0M',
  'https://www.youtube.com/shorts/abcDEF12345',
  'https://www.youtube.com/live/abcDEF12345?feature=share',
  'https://www.youtube.com/embed/abcDEF12345',
  'https://www.youtube.com/clip/UgkxAbc123',
  'https://WWW.YOUTUBE.COM./watch?v=09OGSRptb0M',
  '  https://www.youtube.com/watch?v=09OGSRptb0M  ',
  'https://youtu.be/09OGSRptb0M',
  'https://youtu.be/09OGSRptb0M?si=abc',
  'https://www.tiktok.com/@user.name/video/7234567890123456789',
  'https://vm.tiktok.com/ZMabc123/',
  'https://vt.tiktok.com/ZSabc123/',
  'https://www.facebook.com/somepage/videos/1234567890/',
  'https://www.facebook.com/watch/?v=1234567890',
  'https://m.facebook.com/watch?v=1234567890',
  'https://www.facebook.com/reel/1234567890',
  'https://fb.watch/abcDEF/',
  'https://www.instagram.com/reel/Cabc123/',
  'https://www.instagram.com/reels/Cabc123/',
  'https://www.instagram.com/p/Cabc123/',
  'https://www.instagram.com/tv/Cabc123/',
  'https://x.com/someone/status/1234567890123456789',
  'https://twitter.com/someone/status/1234567890123456789?s=20',
  'https://mobile.twitter.com/someone/status/123',
  'https://vimeo.com/123456789',
  'https://player.vimeo.com/video/123456789',
  'https://www.dailymotion.com/video/x8abcd1',
  'https://dai.ly/x8abcd1',
  'https://www.twitch.tv/videos/1234567890',
  'https://clips.twitch.tv/FunnyClipSlug',
  'https://www.twitch.tv/streamer/clip/FunnyClipSlug',
  'https://soundcloud.com/artist-name/track-name',
  'https://m.soundcloud.com/artist-name/track-name',
  'https://www.bilibili.com/video/BV1xx411c7mD',
]

const OTHER_URLS = [
  'https://www.youtube.com/',
  'https://www.youtube.com/@SomeChannel',
  'https://www.youtube.com/@SomeChannel/videos',
  'https://www.youtube.com/channel/UC1234567890',
  'https://www.youtube.com/watch',
  'https://www.youtube.com/watch?v=',
  'https://www.youtube.com/playlist?list=PL123',
  'https://youtu.be/',
  'https://www.tiktok.com/',
  'https://www.tiktok.com/@user.name',
  'https://www.facebook.com/',
  'https://www.facebook.com/somepage',
  'https://www.facebook.com/watch/',
  'https://www.facebook.com/somepage/videos/',
  'https://www.facebook.com/somepage/videos/clip.mp4',
  'https://www.instagram.com/someone/',
  'https://www.instagram.com/reels/',
  'https://x.com/someone',
  'https://x.com/home',
  'https://vimeo.com/',
  'https://vimeo.com/someuser',
  'https://www.dailymotion.com/someuser',
  'https://www.twitch.tv/streamer',
  'https://soundcloud.com/artist-name',
  'https://soundcloud.com/artist-name/likes',
  'https://soundcloud.com/artist-name/sets/my-playlist',
  'https://soundcloud.com/discover/sets',
  'https://soundcloud.com/stats/overview',
  'https://help.soundcloud.com/hc/en-us',
  'https://developers.soundcloud.com/docs/api',
  'https://www.bilibili.com/',
  'https://example.com/videos/clip.mp4',
  'https://example.com/releases/app-1.2.3.zip',
  'https://cdn.example.com/watch?v=abc',
  'https://rr3---sn-8qj-i5oed.googlevideo.com/videoplayback?expire=1&itag=399',
  'https://notyoutube.com/watch?v=abc',
  'https://youtube.com.evil.example/watch?v=abc',
  'https://user:pass@www.youtube.com/watch?v=abc',
  'ftp://www.youtube.com/watch?v=abc',
  'www.youtube.com/watch?v=abc',
  'not a url',
  '',
]

const VIDEO = 'https://www.youtube.com/watch?v=09OGSRptb0M'

// Chờ hiệu ứng mở/đóng hộp thoại chạy xong: giữa chừng chữ còn mờ, axe đo tương phản sai.
async function axe(page: Page) {
  await page.waitForFunction(() =>
    document.getAnimations().every((animation) => animation.playState !== 'running'),
  )
  return new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
}

test('media-pages: nhận đúng link trang video, bỏ qua trang chủ/kênh/tệp', () => {
  for (const url of MEDIA_PAGES) expect(isMediaPage(url), url).toBe(true)
  for (const url of OTHER_URLS) expect(isMediaPage(url), url).toBe(false)
})

test('S-05: link video trong hộp Thêm chuyển sang Tải media', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Thêm URL' }).click()
  const add = page.getByRole('dialog')
  await add.getByLabel('URL cần tải').fill(VIDEO)
  await expect(add.getByText('Đây là link video')).toBeVisible()
  await expect(add.getByLabel('Tên file')).toHaveCount(0)
  await expect(add.getByRole('button', { name: 'Bắt đầu' })).toHaveCount(0)

  await add.getByRole('button', { name: 'Tải video' }).click()
  const media = page.getByRole('dialog')
  await expect(media.getByRole('heading', { name: 'Tải media' })).toBeVisible()
  await expect(media.getByLabel('URL media')).toHaveValue(VIDEO)
  // Bản xem trước không chạy được yt-dlp: tự phân tích ngay nên báo cần app desktop.
  await expect(media.getByRole('alert')).toContainText('Mở app UTD')
  await expect(page.getByRole('heading', { name: 'Thêm download' })).toHaveCount(0)

  await media.getByRole('button', { name: 'Huỷ' }).click()
  await expect(media).not.toBeVisible()
  await page.getByRole('button', { name: 'Thêm URL' }).click()
  await expect(page.getByLabel('URL cần tải')).toHaveValue('')
})

test('S-05: Enter trên link video cũng mở Tải media', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Thêm URL' }).click()
  const field = page.getByRole('dialog').getByLabel('URL cần tải')
  await field.fill('https://youtu.be/09OGSRptb0M')
  await field.press('Enter')
  await expect(page.getByLabel('URL media')).toHaveValue(
    'https://youtu.be/09OGSRptb0M',
  )
})

test('S-05: link tệp thường vẫn tải bằng HTTP', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Thêm URL' }).click()
  const add = page.getByRole('dialog')
  await add.getByLabel('URL cần tải').fill('https://www.youtube.com/')
  await expect(add.getByText('Đây là link video')).toHaveCount(0)
  await add
    .getByLabel('URL cần tải')
    .fill('https://example.com/video/clip.mp4')
  await expect(add.getByText('Đây là link video')).toHaveCount(0)
  await expect(add.getByLabel('Tên file')).toHaveValue('clip.mp4')
  await expect(add.getByRole('button', { name: 'Bắt đầu' })).toBeVisible()
})

test('S-05: a11y — báo link video và hộp Tải media mở sẵn không có lỗi truy cập', async ({
  page,
}) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Thêm URL' }).click()
  const add = page.getByRole('dialog')
  await add.getByLabel('URL cần tải').fill(VIDEO)
  await expect(add.getByText('Đây là link video')).toBeVisible()
  expect((await axe(page)).violations).toEqual([])

  await add.getByRole('button', { name: 'Tải video' }).click()
  await expect(page.getByLabel('URL media')).toHaveValue(VIDEO)
  await expect(page.getByRole('dialog').getByRole('alert')).toBeVisible()
  expect((await axe(page)).violations).toEqual([])
})

// Giả app desktop trong bản xem trước: `invoke`/`listen` của @tauri-apps/api đi qua __TAURI_INTERNALS__.
// `early`: link trình duyệt gửi tới trước khi giao diện kịp nghe (app giữ lại, trả qua take_media_request).
function mockDesktop({ early }: { early: string | null }) {
  type Callback = (message: unknown) => void
  type Args = Record<string, unknown>
  const callbacks = new Map<number, Callback>()
  const listeners: { event: string; id: number }[] = []
  const calls: { cmd: string; args: Args }[] = []
  let nextId = 1
  let pending = early
  const probe = (url: string) => ({
    url,
    webpageUrl: url,
    title: `Video: ${url}`,
    uploader: null,
    durationSeconds: 60,
    thumbnail: null,
    formats: [
      {
        id: 'best',
        label: 'Tốt nhất',
        detail: 'MP4 · hình + tiếng',
        sizeBytes: 1_000_000,
        bitrateKbps: null,
        selector: 'bv*+ba/b',
        audioFormat: null,
        badge: null,
        recommended: true,
      },
    ],
    ffmpegAvailable: true,
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
    list_downloads: () => [],
    default_download_dir: () => 'C:/Users/Test/Downloads/UTD',
    disk_space: () => ({ freeBytes: 5e11, totalBytes: 1e12, label: 'C:' }),
    get_update_check: () => ({ ready: true, enabled: false, available: null, installing: false }),
    take_media_request: () => {
      const url = pending
      pending = null
      return url ? { url } : null
    },
    media_probe: (args) =>
      new Promise((resolve) => setTimeout(() => resolve(probe(String(args.url))), 1500)),
    add_download: () =>
      Promise.reject('media_page: Đây là link video, dùng Tải video để chọn chất lượng.'),
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
    listening: (event: string) => listeners.filter((item) => item.event === event).length,
    emit: (event: string, payload: unknown) => {
      for (const item of listeners.filter((entry) => entry.event === event)) {
        callbacks.get(item.id)?.({ event, id: item.id, payload })
      }
    },
  }
}

interface DesktopProbe {
  calls: (cmd: string) => Record<string, unknown>[]
  listening: (event: string) => number
  emit: (event: string, payload: unknown) => void
}

const REQUEST_EVENT = 'utd://media-request'
const desktop = (page: Page) => ({
  calls: (cmd: string) =>
    page.evaluate((name) => (window as unknown as { __utd: DesktopProbe }).__utd.calls(name), cmd),
  listening: () =>
    page.evaluate(
      (name) => (window as unknown as { __utd: DesktopProbe }).__utd.listening(name),
      REQUEST_EVENT,
    ),
  send: (url: string) =>
    page.evaluate(
      ([name, link]) => (window as unknown as { __utd: DesktopProbe }).__utd.emit(name, { url: link }),
      [REQUEST_EVENT, url],
    ),
})

test('S-05: link video từ trình duyệt mở Tải media, mỗi link phân tích một lần', async ({ page }) => {
  await page.addInitScript(mockDesktop, { early: null })
  await page.goto('/')
  const app = desktop(page)
  await expect.poll(app.listening).toBe(1)

  const second = 'https://youtu.be/abcDEF12345'
  await app.send(VIDEO)
  const media = page.getByRole('dialog')
  await expect(media.getByLabel('URL media')).toHaveValue(VIDEO)
  await expect(media.getByRole('status')).toBeVisible()
  // Link thứ hai tới khi link đầu còn đang phân tích: hộp dựng lại với link mới, không hiện kết quả cũ.
  await app.send(second)
  await expect(media.getByLabel('URL media')).toHaveValue(second)
  await expect(media.getByText(`Video: ${second}`)).toBeVisible()
  await expect(media.getByText(`Video: ${VIDEO}`)).toHaveCount(0)
  expect(await app.calls('media_probe')).toEqual([{ url: VIDEO }, { url: second }])
  expect(await app.listening()).toBe(1)

  await media.getByRole('button', { name: 'Huỷ' }).click()
  await expect(media).not.toBeVisible()
  await expect
    .poll(() => page.evaluate(() => document.querySelectorAll('[inert]').length))
    .toBe(0)
})

test('S-05: link gửi tới lúc UTD vừa mở (chưa kịp nghe) vẫn mở Tải media', async ({ page }) => {
  await page.addInitScript(mockDesktop, { early: VIDEO })
  await page.goto('/')
  const app = desktop(page)
  await expect(page.getByLabel('URL media')).toHaveValue(VIDEO)
  await expect(page.getByRole('dialog').getByText(`Video: ${VIDEO}`)).toBeVisible()
  expect(await app.calls('media_probe')).toEqual([{ url: VIDEO }])
  expect(await app.calls('take_media_request')).toHaveLength(1)
})

test('S-05: add_download báo media_page thì mở Tải media', async ({ page }) => {
  await page.addInitScript(mockDesktop, { early: null })
  await page.goto('/')
  const app = desktop(page)
  // Luật TS không nhận ra link này, chỉ app (luật Rust) báo `media_page:`: lưới an toàn vẫn chuyển hộp.
  const url = 'https://example.com/watch/123'
  await page.getByRole('button', { name: 'Thêm URL' }).first().click()
  await page.getByRole('dialog').getByLabel('URL cần tải').fill(url)
  await page.getByRole('dialog').getByRole('button', { name: 'Bắt đầu' }).click()
  await expect(page.getByLabel('URL media')).toHaveValue(url)
  await expect(page.getByRole('heading', { name: 'Thêm download' })).toHaveCount(0)
  await expect(page.getByRole('dialog').getByRole('alert')).toHaveCount(0)
  expect(await app.calls('add_download')).toHaveLength(1)
  await expect(page.getByRole('dialog').getByText(`Video: ${url}`)).toBeVisible()
  expect(await app.calls('media_probe')).toEqual([{ url }])
})
