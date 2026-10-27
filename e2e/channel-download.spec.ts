import AxeBuilder from '@axe-core/playwright'
import { expect, test, type Page } from '@playwright/test'

import {
  channelPayloads,
  CHANNEL_QUALITIES,
  DEFAULT_FILTER,
  DEFAULT_SELECTION,
  isSelected,
  pickedEntries,
  SELECT_NONE,
  selectAll,
  toggleEntry,
  visibleEntries,
  type ChannelEntry,
} from '../src/features/home/model/channel'
import { isChannelPage } from '../src/features/home/model/media-pages'

// Cùng bộ ca với tests trong src-tauri/src/media_pages.rs (bản Rust của luật): sửa một bên thì sửa cả bên kia.
const CHANNEL_PAGES = [
  'https://www.tiktok.com/@tiktok',
  'https://www.tiktok.com/@user.name?lang=en&is_from_webapp=1',
  'https://m.tiktok.com/@user.name/',
  'https://www.youtube.com/@SomeChannel',
  'https://www.youtube.com/@SomeChannel/videos',
  'https://www.youtube.com/@SomeChannel/shorts',
  'https://www.youtube.com/@SomeChannel/streams',
  'https://m.youtube.com/@SomeChannel/featured',
  'https://www.youtube.com/channel/UC1234567890',
  'https://www.youtube.com/channel/UC1234567890/videos',
  'https://www.youtube.com/c/SomeName',
  'https://www.youtube.com/user/SomeName/videos',
  'https://www.youtube.com/playlist?list=PL123',
  'https://music.youtube.com/playlist?list=OLAK5uy_abc',
]

const NOT_CHANNEL_PAGES = [
  'https://www.tiktok.com/',
  'https://www.tiktok.com/@',
  'https://www.tiktok.com/@user.name/video/7234567890123456789',
  'https://www.tiktok.com/@user.name/photo/7234567890123456789',
  'https://www.tiktok.com/@user.name/live',
  'https://www.tiktok.com/explore',
  'https://vm.tiktok.com/ZMabc123/',
  'https://www.youtube.com/',
  'https://www.youtube.com/watch?v=09OGSRptb0M&list=PL123',
  'https://www.youtube.com/playlist',
  'https://www.youtube.com/playlist?list=',
  'https://www.youtube.com/@SomeChannel/community',
  'https://www.youtube.com/@SomeChannel/playlists',
  'https://www.youtube.com/@SomeChannel/videos/extra',
  'https://www.youtube.com/@clip.mp4',
  'https://www.youtube.com/channel',
  'https://www.youtube.com/feed/subscriptions',
  'https://notyoutube.com/@SomeChannel',
  'https://tiktok.com.evil.example/@user',
  'https://user:pass@www.tiktok.com/@user',
  'ftp://www.tiktok.com/@user',
  'not a url',
  '',
]

const CHANNEL = 'https://www.tiktok.com/@kenh.mau'
const KEY = 'tiktok.com:MS4wLjABAAAAkenh'

type Scenario = 'full' | 'slow' | 'private'

// Giả app desktop: `invoke` đi qua __TAURI_INTERNALS__; `channel_list` đẩy sự kiện vào `Channel` (onEvent)
// như app thật. Task media thêm vào thì đứng ở "downloading" tới khi bài thử báo xong (`finish`).
function mockDesktop(scenario: Scenario) {
  type Args = Record<string, unknown>
  type Callback = (message: unknown) => void
  const callbacks = new Map<number, Callback>()
  const listeners: { event: string; id: number }[] = []
  const calls: { cmd: string; args: Args }[] = []
  // Lượt đang chạy: hết lượt (`done`) thì bỏ, như app (huỷ lượt đã xong không gửi gì thêm).
  const channels = new Map<string, Callback>()
  let nextId = 1
  const day = (date: string) => Date.parse(`${date}T12:00:00`) / 1000
  const entry = (id: number, date: string, extra: Record<string, unknown> = {}) => ({
    id: `77000000000000000${id}`,
    url: `https://www.tiktok.com/@kenh.mau/video/77000000000000000${id}`,
    title: `Video số ${id}`,
    timestamp: day(date),
    durationSeconds: 30 + id,
    viewCount: 1000 * id,
    thumbnail: null,
    blocked: null,
    downloaded: false,
    ...extra,
  })
  const first = [entry(5, '2026-10-20'), entry(4, '2026-10-12', { downloaded: true }), entry(3, '2026-10-05')]
  const rest = [entry(2, '2026-09-28', { blocked: 'private' }), entry(1, '2026-09-01')]
  const later = (ms: number, run: () => void) => setTimeout(run, ms)
  // `hold()`: lệnh thêm video chờ tới `release()` (kênh lớn gửi lâu, như app thật).
  let holding = false
  const gate: (() => void)[] = []
  const task = (args: Args) => {
    const input = args.input as Record<string, unknown>
    return {
      id: `task-${calls.filter((call) => call.cmd === 'add_media_download').length}`,
      url: input.url,
      finalUrl: null,
      fileName: `${String(input.title)}.mp4`,
      destDir: 'C:/Users/Test/Downloads/UTD',
      tempPath: '',
      kind: 'media',
      status: 'downloading',
      totalBytes: null,
      downloadedBytes: 0,
      etag: null,
      lastModified: null,
      expectedSha256: null,
      hashState: 'unavailable',
      connections: 1,
      error: null,
      createdAt: 1,
      updatedAt: 1,
      completedAt: null,
    }
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
    list_downloads: () => [],
    default_download_dir: () => 'C:/Users/Test/Downloads/UTD',
    disk_space: () => ({ freeBytes: 5e11, totalBytes: 1e12, label: 'C:' }),
    get_update_check: () => ({ ready: true, enabled: false, available: null, installing: false }),
    take_media_request: () => null,
    take_batch_request: () => null,
    add_download: (args) =>
      Promise.reject(`channel_page: ${String((args.input as Args).url)}`),
    // Hàm này chạy trong trang (addInitScript): không dùng được hằng của file test.
    media_probe: () => Promise.reject('channel_page: https://www.tiktok.com/@kenh.mau'),
    add_media_download: async (args) => {
      const added = task(args)
      if (holding) await new Promise<void>((resolve) => gate.push(resolve))
      return added
    },
    channel_list_cancel: (args) => {
      const send = channels.get(String(args.listingId))
      send?.({ kind: 'done', total: 3, complete: false, error: null })
      return Boolean(send)
    },
    channel_list: (args) => {
      const id = String(args.listingId)
      const sink = args.onEvent as { onmessage: Callback }
      const send = (message: unknown) => {
        if (!channels.has(id)) return
        if ((message as { kind: string }).kind === 'done') channels.delete(id)
        sink.onmessage(message)
      }
      channels.set(id, send)
      if (scenario === 'private') {
        later(50, () =>
          send({ kind: 'profile', profile: { name: 'Kênh Mẫu', avatar: null, followers: 12, videoCount: null, private: true } }),
        )
        later(80, () =>
          send({ kind: 'done', total: 0, complete: false, error: 'Tài khoản riêng tư hoặc mọi video đều riêng tư.' }),
        )
        return null
      }
      later(50, () => send({ kind: 'head', head: { key: 'tiktok.com:MS4wLjABAAAAkenh', name: 'Kênh Mẫu', handle: '@kenh.mau' } }))
      later(80, () =>
        send({ kind: 'profile', profile: { name: 'Kênh Mẫu', avatar: null, followers: 96400, videoCount: null, private: false } }),
      )
      later(120, () => send({ kind: 'entries', entries: first }))
      if (scenario === 'full') {
        later(200, () => send({ kind: 'entries', entries: rest }))
        later(240, () => send({ kind: 'done', total: 5, complete: true, error: null }))
      }
      return null
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
    calls: (cmd: string) =>
      calls
        .filter((call) => call.cmd === cmd)
        .map((call) => JSON.parse(JSON.stringify(call.args, (key, value) => (key === 'onEvent' ? undefined : value)))),
    emit: (event: string, payload: unknown) => {
      for (const item of listeners.filter((entry) => entry.event === event)) {
        callbacks.get(item.id)?.({ event, id: item.id, payload })
      }
    },
    listening: (event: string) => listeners.filter((item) => item.event === event).length,
    hold: () => {
      holding = true
    },
    release: () => {
      holding = false
      for (const resolve of gate.splice(0)) resolve()
    },
  }
}

interface DesktopProbe {
  calls: (cmd: string) => Record<string, unknown>[]
  emit: (event: string, payload: unknown) => void
  listening: (event: string) => number
  hold: () => void
  release: () => void
}

const desktop = (page: Page) => ({
  calls: (cmd: string) =>
    page.evaluate((name) => (window as unknown as { __utd: DesktopProbe }).__utd.calls(name), cmd),
  emit: (event: string, payload: unknown) =>
    page.evaluate(
      ([name, body]) => (window as unknown as { __utd: DesktopProbe }).__utd.emit(name as string, body),
      [event, payload],
    ),
  listening: (event: string) =>
    page.evaluate((name) => (window as unknown as { __utd: DesktopProbe }).__utd.listening(name), event),
  hold: () => page.evaluate(() => (window as unknown as { __utd: DesktopProbe }).__utd.hold()),
  release: () => page.evaluate(() => (window as unknown as { __utd: DesktopProbe }).__utd.release()),
})

// Chờ hiệu ứng mở/đóng hộp thoại chạy xong: giữa chừng chữ còn mờ, axe đo tương phản sai.
async function axe(page: Page) {
  await page.waitForFunction(() =>
    document.getAnimations().every((animation) => animation.playState !== 'running'),
  )
  return new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze()
}

async function openFromAdd(page: Page) {
  await page.getByRole('button', { name: 'Thêm URL' }).first().click()
  const add = page.getByRole('dialog')
  await add.getByLabel('URL cần tải').fill(CHANNEL)
  await expect(add.getByText('Đây là link kênh')).toBeVisible()
  await add.getByRole('button', { name: 'Tải kênh' }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('heading', { name: 'Tải cả kênh' })).toBeVisible()
  return dialog
}

test('channel-pages: nhận đúng link kênh TikTok, kênh/playlist YouTube', () => {
  for (const url of CHANNEL_PAGES) expect(isChannelPage(url), url).toBe(true)
  for (const url of NOT_CHANNEL_PAGES) expect(isChannelPage(url), url).toBe(false)
})

test('channel-model: lọc mới nhất N / khoảng ngày / chỉ video mới, chọn và tên file', () => {
  const at = (date: string) => Date.parse(`${date}T12:00:00`) / 1000
  const video = (id: string, date: string | null, extra: Partial<ChannelEntry> = {}): ChannelEntry => ({
    id,
    url: `https://www.tiktok.com/@a/video/${id}`,
    title: `Tiêu đề ${id}`,
    timestamp: date === null ? null : at(date),
    durationSeconds: 10,
    viewCount: 1,
    thumbnail: null,
    blocked: null,
    downloaded: false,
    ...extra,
  })
  // Video ghim cũ đứng đầu như TikTok trả; một video thiếu ngày.
  const entries = [
    video('pinned', '2026-01-01'),
    video('a', '2026-10-20'),
    video('b', '2026-10-12', { downloaded: true }),
    video('c', null),
    video('d', '2026-10-05', { blocked: 'private' }),
  ]
  const ids = (list: ChannelEntry[]) => list.map((entry) => entry.id)
  expect(ids(visibleEntries(entries, DEFAULT_FILTER))).toEqual(['pinned', 'a', 'c', 'd'])
  const all = { ...DEFAULT_FILTER, onlyNew: false }
  expect(ids(visibleEntries(entries, { ...all, range: 'latest', latest: 2 }))).toEqual(['a', 'b'])
  expect(ids(visibleEntries(entries, { ...all, range: 'latest', latest: 0 }))).toEqual(['a'])
  const dates = { ...all, range: 'dates' as const, from: '2026-10-05', to: '2026-10-12' }
  expect(ids(visibleEntries(entries, dates))).toEqual(['b', 'd'])
  expect(ids(visibleEntries(entries, { ...dates, from: '', to: '' }))).toEqual(ids(entries))

  const shown = visibleEntries(entries, all)
  expect(ids(pickedEntries(shown, DEFAULT_SELECTION))).toEqual(['pinned', 'a', 'c'])
  expect(ids(pickedEntries(shown, selectAll(shown)))).toEqual(['pinned', 'a', 'b', 'c'])
  expect(pickedEntries(shown, SELECT_NONE)).toEqual([])
  const toggled = toggleEntry(toggleEntry(DEFAULT_SELECTION, 'a'), 'b')
  expect(ids(pickedEntries(shown, toggled))).toEqual(['pinned', 'b', 'c'])
  expect(isSelected(entries[4], selectAll(shown)), 'video bị khoá không bao giờ được chọn').toBe(false)

  const [first, undated] = channelPayloads([entries[1], entries[3]], CHANNEL_QUALITIES[1], 'tiktok.com:X', null)
  expect(first.title).toBe('2026-10-20 Tiêu đề a')
  expect(first.selector).toContain('vcodec^=h264')
  expect(first.subfolder).toBeUndefined()
  expect(first.channel).toEqual({ key: 'tiktok.com:X', videoId: 'a' })
  expect(undated.title).toBe('Tiêu đề c')
  expect(channelPayloads([entries[1]], CHANNEL_QUALITIES[0], null, 'Kênh')[0].channel).toBeUndefined()
})

test('S-61: lọc, chọn, một chất lượng chung, cả kênh vào hàng đợi chung', async ({ page }) => {
  await page.addInitScript(mockDesktop, 'full')
  await page.goto('/')
  const app = desktop(page)
  const dialog = await openFromAdd(page)
  await expect(dialog.locator('[data-s="S-61"]')).toBeVisible()
  await expect(dialog.getByText('Kênh Mẫu', { exact: true })).toBeVisible()
  await expect(dialog.getByText('@kenh.mau · 5 video')).toBeVisible()
  // Thời lượng có đồng hồ đứng trước (không đọc nhầm là giờ đăng).
  await expect(dialog.locator('li svg.lucide-clock').first()).toBeVisible()
  // StrictMode (bản dev) chạy effect hai lần: lượt đầu bị huỷ ngay, lượt cuối là lượt thật.
  expect((await app.calls('channel_list')).at(-1)).toEqual({ url: CHANNEL, listingId: expect.any(String) })

  // Mặc định: mọi video chưa tải, trừ video riêng tư; video đã tải bị ẩn ("Chỉ video mới").
  const start = dialog.getByRole('button', { name: /^Tải \d+ video$/ })
  await expect(start).toHaveText('Tải 3 video')
  await expect(dialog.getByRole('checkbox')).toHaveCount(4)
  await dialog.getByText('Chỉ video mới').click()
  await expect(dialog.getByText('Đã tải', { exact: true })).toBeVisible()
  await dialog.getByText('Chỉ video mới').click()

  await dialog.getByRole('button', { name: 'Mới nhất' }).click()
  await dialog.getByLabel('Số video mới nhất').fill('2')
  await expect(start).toHaveText('Tải 2 video')
  await dialog.getByRole('button', { name: 'Theo ngày' }).click()
  await dialog.getByLabel('Từ ngày').fill('2026-09-01')
  await dialog.getByLabel('Đến ngày').fill('2026-10-05')
  await expect(start).toHaveText('Tải 2 video')
  await dialog.getByRole('button', { name: 'Tất cả', exact: true }).click()

  await dialog.getByRole('button', { name: 'Bỏ chọn' }).click()
  await expect(start).toBeDisabled()
  await dialog.getByRole('checkbox', { name: 'Video số 3' }).click()
  await expect(start).toHaveText('Tải 1 video')
  await dialog.getByRole('button', { name: 'Chọn tất cả' }).click()
  await expect(start).toHaveText('Tải 3 video')
  // Bấm vào cả dòng (ảnh, tiêu đề) cũng đổi ô chọn.
  await dialog.getByText('Video số 5', { exact: true }).click()
  await expect(start).toHaveText('Tải 2 video')
  await dialog.getByText('Video số 5', { exact: true }).click()
  await expect(start).toHaveText('Tải 3 video')

  await dialog.getByRole('button', { name: 'MP3' }).click()
  await start.click()
  await expect(dialog).not.toBeVisible()

  // Cả 3 video vào hàng đợi chung của app ngay (cuối hàng, đúng thứ tự); app tự giới hạn số chạy cùng lúc.
  await expect.poll(async () => (await app.calls('add_media_download')).length).toBe(3)
  const added = await app.calls('add_media_download')
  expect(added[0]).toEqual({
    input: {
      url: 'https://www.tiktok.com/@kenh.mau/video/770000000000000005',
      title: '2026-10-20 Video số 5',
      selector: 'ba/b',
      audioFormat: 'mp3',
      pageUrl: 'https://www.tiktok.com/@kenh.mau/video/770000000000000005',
      subfolder: 'Kênh Mẫu',
      channel: { key: KEY, videoId: '770000000000000005' },
      later: true,
    },
  })
  expect(added.map((call) => (call.input as { channel: { videoId: string } }).channel.videoId)).toEqual([
    '770000000000000005',
    '770000000000000003',
    '770000000000000001',
  ])
  await expect(page.getByRole('status').filter({ hasText: 'đang thêm' })).toHaveCount(0)
})

test('S-61: tải kênh thứ hai lúc kênh đầu còn đang gửi thì nối vào sau, không bỏ video nào', async ({ page }) => {
  await page.addInitScript(mockDesktop, 'full')
  await page.goto('/')
  const app = desktop(page)
  await app.hold()
  const first = await openFromAdd(page)
  await first.getByRole('button', { name: 'Tải 3 video' }).click()
  await expect(first).not.toBeVisible()
  const pill = page.getByRole('status').filter({ hasText: 'đang thêm' })
  await expect(pill).toContainText('đang thêm 3 video')

  await expect.poll(() => app.listening('utd://media-request')).toBe(1)
  await app.emit('utd://media-request', { url: CHANNEL })
  const second = page.getByRole('dialog')
  await second.getByRole('button', { name: 'Tải 3 video' }).click()
  await expect(pill).toContainText('đang thêm 6 video')

  // Gửi lần lượt: mới có một lệnh (đang chờ app trả lời).
  expect(await app.calls('add_media_download')).toHaveLength(1)
  await app.release()
  await expect.poll(async () => (await app.calls('add_media_download')).length).toBe(6)
  await expect(pill).toHaveCount(0)
})

test('S-60: kênh dài liệt kê dần, Dừng giữ phần đã nhận', async ({ page }) => {
  await page.addInitScript(mockDesktop, 'slow')
  await page.goto('/')
  const app = desktop(page)
  const dialog = await openFromAdd(page)
  await expect(dialog.getByText('@kenh.mau · 3 video')).toBeVisible()
  await dialog.getByRole('button', { name: 'Dừng' }).click()
  await expect(dialog.getByRole('button', { name: 'Dừng' })).toHaveCount(0)
  const listing = (await app.calls('channel_list')).at(-1)?.listingId
  const cancelled = (await app.calls('channel_list_cancel')).map((call) => call.listingId)
  expect(cancelled).toContain(listing)
  await expect(dialog.getByRole('button', { name: 'Tải 2 video' })).toBeEnabled()
  expect((await axe(page)).violations).toEqual([])

  await dialog.getByRole('button', { name: 'Huỷ' }).click()
  await expect(dialog).not.toBeVisible()
})

test('S-63: tài khoản riêng tư báo rõ, chỉ cách đăng nhập qua trình duyệt, không vượt bảo vệ', async ({ page }) => {
  await page.addInitScript(mockDesktop, 'private')
  await page.goto('/')
  const dialog = await openFromAdd(page)
  await expect(dialog.locator('[data-s="S-63"]')).toBeVisible()
  // 2026-10-25: UTD dùng đăng nhập của trình duyệt khi người dùng cho phép trong popup tiện ích.
  await expect(dialog.getByText('Bấm biểu tượng UTD trên trình duyệt › Dùng đăng nhập.').first()).toBeVisible()
  await expect(dialog.getByRole('button', { name: 'Mở trang' })).toBeVisible()
  await expect(dialog.getByRole('alert').filter({ hasText: 'Tài khoản riêng tư' })).toBeVisible()
  await expect(dialog.getByRole('button', { name: 'Thử lại' })).toBeVisible()
  await expect(dialog.getByRole('button', { name: 'Tải 0 video' })).toBeDisabled()
  expect((await axe(page)).violations).toEqual([])
})

test('S-60: link rút gọn trỏ tới kênh và link từ trình duyệt đều mở Tải cả kênh', async ({ page }) => {
  await page.addInitScript(mockDesktop, 'full')
  await page.goto('/')
  const app = desktop(page)
  // Link rút gọn: luật TS không biết là kênh, app (media_probe) trả `channel_page:` kèm link kênh.
  await page.getByRole('button', { name: 'Thêm URL' }).first().click()
  await page.getByRole('dialog').getByLabel('URL cần tải').fill('https://vm.tiktok.com/ZMabc123/')
  await page.getByRole('dialog').getByRole('button', { name: 'Tải video' }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('heading', { name: 'Tải cả kênh' })).toBeVisible()
  await expect(dialog.getByText('@kenh.mau · 5 video')).toBeVisible()
  expect(await app.calls('media_probe')).toEqual([{ url: 'https://vm.tiktok.com/ZMabc123/' }])
  const listing = (await app.calls('channel_list')).at(-1)?.listingId
  await dialog.getByRole('button', { name: 'Huỷ' }).click()
  await expect(dialog).not.toBeVisible()
  await expect
    .poll(async () => (await app.calls('channel_list_cancel')).map((call) => call.listingId))
    .toContain(listing)

  await expect.poll(() => app.listening('utd://media-request')).toBe(1)
  await app.emit('utd://media-request', { url: 'https://www.youtube.com/@SomeChannel' })
  await expect(page.getByRole('heading', { name: 'Tải cả kênh' })).toBeVisible()
  const lists = await app.calls('channel_list')
  expect(lists.at(-1)?.url).toBe('https://www.youtube.com/@SomeChannel')
})

test('S-63: bản xem trước không chạy được yt-dlp thì báo cần app', async ({ page }) => {
  await page.goto('/')
  const dialog = await openFromAdd(page)
  await expect(dialog.locator('[data-s="S-63"]')).toBeVisible()
  await expect(dialog.getByRole('alert')).toContainText('Mở app UTD')
})
