import AxeBuilder from '@axe-core/playwright'
import { expect, test, type Page } from '@playwright/test'

// Thanh tiêu đề riêng (src/shared/window/title-bar.tsx). Bản xem trước không có Tauri: chỉ tên app, không nút cửa sổ.
// Ca "desktop" giả Tauri qua __TAURI_INTERNALS__ và đổi user agent để chọn Windows hay macOS.
const WINDOWS_UA =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0'
const MAC_UA = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)'

interface FramePatch {
  maximized?: boolean
  fullscreen?: boolean
  focused?: boolean
}

interface FakeWindow {
  calls: (cmd: string) => Record<string, unknown>[]
  set: (patch: FramePatch) => void
  emit: (event: string, payload: unknown) => void
}

// Chạy trong trang (addInitScript): chỉ dùng biến trong hàm.
function fakeTauri() {
  type Handler = (message: unknown) => void
  const handlers = new Map<number, Handler>()
  const subscriptions: { event: string; handler: number }[] = []
  const log: { cmd: string; args: Record<string, unknown> }[] = []
  const frame = { maximized: false, fullscreen: false, focused: true }
  let lastHandler = 0
  const replies: Record<string, (args: Record<string, unknown>) => unknown> = {
    'plugin:event|listen': (args) => {
      subscriptions.push({ event: String(args.event), handler: Number(args.handler) })
      return args.handler
    },
    'plugin:window|is_maximized': () => frame.maximized,
    'plugin:window|is_fullscreen': () => frame.fullscreen,
    is_window_active: () => frame.focused,
    list_downloads: () => [],
    default_download_dir: () => 'C:/Users/Test/Downloads/UTD',
    disk_space: () => ({ freeBytes: 4e11, totalBytes: 1e12, label: 'C:' }),
    get_update_check: () => ({ ready: true, enabled: false, available: null, installing: false }),
    take_media_request: () => null,
  }
  Object.assign(window, {
    isTauri: true,
    __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => undefined },
    __TAURI_INTERNALS__: {
      metadata: {
        currentWindow: { label: 'main' },
        currentWebview: { windowLabel: 'main', label: 'main' },
      },
      transformCallback: (handler: Handler) => {
        lastHandler += 1
        handlers.set(lastHandler, handler)
        return lastHandler
      },
      unregisterCallback: (id: number) => handlers.delete(id),
      invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
        log.push({ cmd, args })
        return replies[cmd]?.(args) ?? null
      },
    },
    __fakeWindow: {
      calls: (cmd: string) => log.filter((entry) => entry.cmd === cmd).map((entry) => entry.args),
      set: (patch: Partial<typeof frame>) => Object.assign(frame, patch),
      emit: (event: string, payload: unknown) => {
        for (const item of subscriptions.filter((entry) => entry.event === event)) {
          handlers.get(item.handler)?.({ event, id: item.handler, payload })
        }
      },
    },
  })
}

const fake = (page: Page) => ({
  calls: (cmd: string) =>
    page.evaluate((name) => (window as unknown as { __fakeWindow: FakeWindow }).__fakeWindow.calls(name), cmd),
  set: (patch: FramePatch) =>
    page.evaluate((value) => (window as unknown as { __fakeWindow: FakeWindow }).__fakeWindow.set(value), patch),
  emit: (event: string, payload: unknown = null) =>
    page.evaluate(
      ([name, value]) => (window as unknown as { __fakeWindow: FakeWindow }).__fakeWindow.emit(name as string, value),
      [event, payload] as const,
    ),
})

test('S-01: thanh tiêu đề bản xem trước có tên app một lần, không nút cửa sổ', async ({ page }) => {
  await page.goto('/')
  const bar = page.locator('[data-titlebar]')
  await expect(bar).toHaveAttribute('data-titlebar', 'web')
  await expect(bar).toContainText('utd')
  await expect(page.getByText('desktop', { exact: true })).toHaveCount(1)
  await expect(page.getByRole('group', { name: 'Điều khiển cửa sổ' })).toHaveCount(0)
  const box = await bar.boundingBox()
  expect(box).toMatchObject({ x: 0, y: 0, height: 40 })
  // Thẻ nội dung bắt đầu ngay dưới thanh tiêu đề, không đè lên nó.
  const main = await page.locator('main').boundingBox()
  expect(main?.y).toBe(40)
})

test('S-01: hộp thoại mở thì thanh tiêu đề vẫn ở trên lớp phủ, hộp không lấn lên thanh', async ({ page }) => {
  await page.setViewportSize({ width: 1080, height: 680 })
  await page.goto('/')
  await page.getByRole('button', { name: 'Thêm URL' }).first().click()
  const dialog = page.getByRole('dialog')
  await expect(dialog).toBeVisible()
  const onTop = await page.evaluate(() => {
    const hit = document.elementFromPoint(window.innerWidth / 2, 20)
    return Boolean(hit?.closest('[data-titlebar]'))
  })
  expect(onTop).toBe(true)
  expect((await dialog.boundingBox())!.y).toBeGreaterThanOrEqual(40)
})

// Trên Windows dải 40px trên cùng do Windows nhận chuột (vùng kéo, cửa sổ con của 3 nút) bất kể lớp DOM:
// menu lật lên ở cửa sổ thấp nhất không được lọt vào đó, kẻo bấm mục menu lại kéo cửa sổ / bấm Thu nhỏ, Đóng.
test('S-01: menu chuột phải và menu ⋯ không lấn vào dải thanh tiêu đề khi cửa sổ thấp nhất', async ({ page }) => {
  await page.setViewportSize({ width: 1080, height: 680 })
  await page.goto('/')
  const rows = page.locator('[data-s="S-01"] tbody tr')
  await expect(rows).toHaveCount(10)
  const menu = page.getByRole('menu')
  let checked = 0
  for (let index = 0; index < 10; index += 1) {
    const row = rows.nth(index)
    const box = await row.boundingBox()
    if (!box || box.y + box.height > 680) break
    const point = { x: box.x + box.width - 40, y: box.y + box.height / 2 }
    // Hàng bị khung cuộn của bảng che (dưới mép khung) thì điểm bấm rơi vào nền card: dừng ở đó.
    const onRow = await row.evaluate(
      (element, { x, y }) => element.contains(document.elementFromPoint(x, y)),
      point,
    )
    if (!onRow) break
    await page.mouse.click(point.x, point.y, { button: 'right' })
    await expect(menu).toBeVisible()
    expect((await menu.boundingBox())!.y).toBeGreaterThanOrEqual(44)
    await page.keyboard.press('Escape')
    await expect(menu).toHaveCount(0)

    await row.getByRole('button', { name: /^Tuỳ chọn cho / }).click()
    await expect(menu).toBeVisible()
    expect((await menu.boundingBox())!.y).toBeGreaterThanOrEqual(44)
    await page.keyboard.press('Escape')
    await expect(menu).toHaveCount(0)
    checked += 1
  }
  // Đủ các hàng ở giữa màn (menu cao hơn chỗ trống bên dưới ⇒ lật lên): ca từng lấn vào thanh.
  expect(checked).toBeGreaterThanOrEqual(4)
})

test.describe('Windows', () => {
  test.use({ userAgent: WINDOWS_UA })

  test('S-01: thanh tiêu đề Windows có 3 nút, báo chỗ nút cho app, bấm gọi lệnh cửa sổ', async ({ page }) => {
    await page.addInitScript(fakeTauri)
    await page.goto('/')
    const app = fake(page)
    const controls = page.getByRole('group', { name: 'Điều khiển cửa sổ' })
    for (const name of ['Thu nhỏ', 'Phóng to', 'Đóng']) {
      await expect(controls.getByRole('button', { name, exact: true })).toBeVisible()
    }
    // Ô nút báo sang Rust bằng pixel thật: sát mép phải và mép trên, cao bằng thanh.
    await expect.poll(async () => (await app.calls('set_caption_buttons')).length).toBeGreaterThan(0)
    const [last] = (await app.calls('set_caption_buttons')).slice(-1)
    const buttons = last.buttons as { button: string; x: number; y: number; width: number; height: number }[]
    expect(buttons.map((item) => item.button)).toEqual(['minimize', 'maximize', 'close'])
    const width = page.viewportSize()!.width
    expect(buttons[2].x + buttons[2].width).toBe(width)
    expect(buttons.every((item) => item.y === 0 && item.height === 40)).toBe(true)

    await controls.getByRole('button', { name: 'Thu nhỏ' }).click()
    await controls.getByRole('button', { name: 'Phóng to' }).click()
    await expect.poll(async () => (await app.calls('plugin:window|minimize')).length).toBe(1)
    await expect.poll(async () => (await app.calls('plugin:window|toggle_maximize')).length).toBe(1)
  })

  test('S-01: nút Phóng to đổi thành Khôi phục, tô theo chuột từ app, mờ khi mất focus', async ({ page }) => {
    await page.addInitScript(fakeTauri)
    await page.goto('/')
    const app = fake(page)
    const bar = page.locator('[data-titlebar]')
    await expect(bar).toHaveAttribute('data-titlebar', 'windows')
    await expect.poll(async () => (await app.calls('plugin:event|listen')).length).toBeGreaterThan(3)

    await app.set({ maximized: true })
    await app.emit('tauri://resize', { width: 1280, height: 720 })
    await expect(bar.getByRole('button', { name: 'Khôi phục' })).toBeVisible()

    await app.emit('utd://caption', { hover: 'close', pressed: null })
    await expect(bar.getByRole('button', { name: 'Đóng' })).toHaveAttribute('data-hover', 'true')
    await app.emit('utd://caption', { hover: null, pressed: null })
    await expect(bar.getByRole('button', { name: 'Đóng' })).not.toHaveAttribute('data-hover')

    // Windows: cửa sổ active theo WM_NCACTIVATE (utd://active), không theo focus của webview.
    await app.emit('utd://active', false)
    await expect(bar).toHaveAttribute('data-focused', 'false')
    await app.emit('utd://active', true)
    await expect(bar).toHaveAttribute('data-focused', 'true')
    await app.emit('tauri://blur')
    await expect(bar).toHaveAttribute('data-focused', 'true')
  })

  // Chuột trên nút đi vào cửa sổ con của Rust ⇒ webview không hiện `title`: app tự hiện chú thích theo `utd://caption`.
  test('S-01: rê yên trên nút cửa sổ thì hiện chú thích dưới nút, nhấn hoặc rời là tắt', async ({ page }) => {
    await page.addInitScript(fakeTauri)
    await page.goto('/')
    const app = fake(page)
    const controls = page.getByRole('group', { name: 'Điều khiển cửa sổ' })
    await expect.poll(async () => (await app.calls('plugin:event|listen')).length).toBeGreaterThan(3)
    const tip = controls.locator('[data-caption-tip]')

    await app.emit('utd://caption', { hover: 'minimize', pressed: null })
    await expect(tip).toHaveText('Thu nhỏ')
    expect((await tip.boundingBox())!.y).toBeGreaterThanOrEqual(40)
    await app.emit('utd://caption', { hover: 'minimize', pressed: 'minimize' })
    await expect(tip).toHaveCount(0)

    await app.emit('utd://caption', { hover: 'close', pressed: null })
    await expect(tip).toHaveText('Đóng')
    const box = (await tip.boundingBox())!
    expect(box.x + box.width).toBeLessThanOrEqual(page.viewportSize()!.width)
    await app.emit('utd://caption', { hover: null, pressed: null })
    await expect(tip).toHaveCount(0)

    // Đường lui không có cửa sổ con: chuột của webview.
    await controls.getByRole('button', { name: 'Phóng to' }).hover()
    await expect(tip).toHaveText('Phóng to')
  })

  test('S-01: mở app khi cửa sổ không active thì thanh tiêu đề mờ ngay (không chờ sự kiện focus)', async ({ page }) => {
    await page.addInitScript(fakeTauri)
    // Cửa sổ mất focus trước khi giao diện kịp nghe sự kiện: không còn sự kiện nào báo nữa.
    await page.addInitScript(() =>
      (window as unknown as { __fakeWindow: FakeWindow }).__fakeWindow.set({ focused: false }),
    )
    await page.goto('/')
    const bar = page.locator('[data-titlebar]')
    await expect(bar).toHaveAttribute('data-focused', 'false')
    await fake(page).emit('utd://active', true)
    await expect(bar).toHaveAttribute('data-focused', 'true')
  })

  test('S-01: a11y — thanh tiêu đề Windows không có lỗi truy cập', async ({ page }) => {
    await page.addInitScript(fakeTauri)
    await page.goto('/')
    await expect(page.getByRole('group', { name: 'Điều khiển cửa sổ' })).toBeVisible()
    const result = await new AxeBuilder({ page })
      .include('[data-titlebar]')
      .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
      .analyze()
    expect(result.violations).toEqual([])
  })
})

test.describe('macOS', () => {
  test.use({ userAgent: MAC_UA })

  test('S-01: macOS không vẽ nút giả, chừa chỗ đèn giao thông trừ khi toàn màn hình', async ({ page }) => {
    await page.addInitScript(fakeTauri)
    await page.goto('/')
    const app = fake(page)
    const bar = page.locator('[data-titlebar]')
    await expect(bar).toHaveAttribute('data-titlebar', 'macos')
    // Kéo/nhấp đúp do thanh tự lo (useTitleBarMouse), không qua drag.js của Tauri (nhấp đúp luôn phóng to).
    await expect(bar).not.toHaveAttribute('data-tauri-drag-region')
    await expect(page.getByRole('group', { name: 'Điều khiển cửa sổ' })).toHaveCount(0)
    await expect(bar).toHaveCSS('padding-left', '84px')

    await expect.poll(async () => (await app.calls('plugin:event|listen')).length).toBeGreaterThan(2)
    await app.set({ fullscreen: true })
    await app.emit('tauri://resize', { width: 1440, height: 900 })
    await expect(bar).toHaveCSS('padding-left', '22px')
  })

  test('S-01: macOS kéo thanh, nhấp đúp theo cài đặt hệ thống, không làm mất focus của hộp thoại', async ({ page }) => {
    await page.addInitScript(fakeTauri)
    await page.goto('/')
    const app = fake(page)
    // App vẽ sau khi nạp file dịch (bất đồng bộ): chờ thanh có mặt rồi mới bấm.
    await expect(page.locator('[data-titlebar]')).toBeVisible()
    await page.mouse.click(300, 20)
    await expect.poll(async () => (await app.calls('plugin:window|start_dragging')).length).toBe(1)

    await page.getByRole('button', { name: 'Thêm URL' }).first().click()
    const dialog = page.getByRole('dialog')
    const url = dialog.getByLabel('URL cần tải')
    await url.fill('https://a')
    await page.mouse.dblclick(300, 20)
    await expect.poll(async () => (await app.calls('title_bar_double_click')).length).toBe(1)
    expect(await app.calls('plugin:window|internal_toggle_maximize')).toHaveLength(0)
    // Như thanh gốc: bấm lên thanh không lấy focus ⇒ vẫn gõ tiếp vào ô đang gõ trong hộp thoại.
    await expect(dialog).toBeVisible()
    await expect(url).toBeFocused()
    await page.keyboard.type('bc')
    await expect(url).toHaveValue('https://abc')
  })

  test('S-01: macOS toàn màn hình thì thanh không kéo, nhấp đúp không làm gì', async ({ page }) => {
    await page.addInitScript(fakeTauri)
    await page.addInitScript(() =>
      (window as unknown as { __fakeWindow: FakeWindow }).__fakeWindow.set({ fullscreen: true }),
    )
    await page.goto('/')
    const app = fake(page)
    await expect(page.locator('[data-titlebar]')).toHaveCSS('padding-left', '22px')
    await page.mouse.dblclick(300, 20)
    await page.mouse.click(300, 20)
    expect(await app.calls('plugin:window|start_dragging')).toHaveLength(0)
    expect(await app.calls('title_bar_double_click')).toHaveLength(0)
    expect(await app.calls('plugin:window|internal_toggle_maximize')).toHaveLength(0)
  })
})
