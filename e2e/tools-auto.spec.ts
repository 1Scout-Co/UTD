import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

import { calls, emitEvent, mockTauri, respond } from './tauri-mock'

// Lần đầu mở app (ngay sau cài) mà thiếu công cụ: app tự tải nền, không hỏi. Giao diện chỉ có một
// thẻ nhỏ ở thanh bên (S-01/S-02 vẫn dùng bình thường); lỗi mạng thì đếm lùi tới lần thử lại.

const BUSY = {
  enabled: true,
  checking: false,
  pending: ['yt-dlp', 'deno', 'ffmpeg'],
  active: 'yt-dlp',
  updating: null,
  failed: [],
  error: null,
  retryAt: null,
  rounds: 0,
  paused: false,
}

const STATUS = {
  ytDlp: null,
  ffmpeg: null,
  deno: null,
  binDir: 'C:\\Users\\utd\\AppData\\Local\\UTD-Data\\bin',
  installing: 'yt-dlp',
}

test('S-02: tự tải công cụ — thẻ tiến độ ở thanh bên, lỗi mạng đếm lùi, xong thì ẩn', async ({ page }) => {
  await mockTauri(page, { media_tools_auto: BUSY, media_tools_status: STATUS })
  await page.goto('/')
  const card = page.getByTestId('tools-auto')
  await expect(card).toBeVisible()
  await expect(card).toContainText('Chuẩn bị yt-dlp')
  // Màn chính vẫn dùng được trong lúc tải.
  await expect(page.getByRole('button', { name: 'Cài đặt', exact: true }).first()).toBeEnabled()

  await emitEvent(page, 'utd://tool-progress', {
    tool: 'yt-dlp',
    stage: 'download',
    downloadedBytes: 9_000_000,
    totalBytes: 18_000_000,
  })
  await expect(card).toContainText('Đang tải yt-dlp')
  await expect(card).toContainText('50%')
  await expect(card.getByRole('progressbar', { name: 'Tiến độ tải yt-dlp' })).toBeVisible()

  await emitEvent(page, 'utd://tools-auto', {
    ...BUSY,
    pending: ['deno', 'ffmpeg'],
    active: null,
    failed: ['deno', 'ffmpeg'],
    error: 'Không kết nối được nguồn tải. Kiểm tra mạng rồi thử lại.',
    retryAt: Date.now() + 30_000,
    rounds: 1,
  })
  await expect(card).toContainText('Chưa tải được deno')
  await expect(card).toContainText(/hoặc chờ (29|30) s/)
  await card.screenshot({ path: test.info().outputPath('the-loi.png') })
  await respond(page, 'retry_media_tools', { ...BUSY, pending: ['deno', 'ffmpeg'], active: 'deno' })
  await card.getByRole('button', { name: 'Thử lại' }).click()
  await expect(card).toContainText('Chuẩn bị deno')
  expect(await calls(page, 'retry_media_tools')).toHaveLength(1)

  await emitEvent(page, 'utd://tools-auto', { ...BUSY, pending: [], active: null })
  await expect(card).toHaveCount(0)
})

test('S-02: a11y — thẻ tự tải công cụ không có lỗi truy cập', async ({ page }) => {
  await mockTauri(page, { media_tools_auto: BUSY, media_tools_status: STATUS })
  await page.goto('/')
  await expect(page.getByTestId('tools-auto')).toBeVisible()
  const result = await new AxeBuilder({ page })
    .include('[data-testid="tools-auto"]')
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  expect(result.violations).toEqual([])
})

test('S-31: công cụ media — công tắc tự tải bật sẵn, tắt được, đang tự tải thì hiện tiến độ', async ({ page }) => {
  await mockTauri(page, {
    media_tools_auto: BUSY,
    media_tools_status: STATUS,
    set_media_tools_auto: { ...BUSY, enabled: false, active: null },
  })
  await page.goto('/')
  await page.getByRole('button', { name: 'Cài đặt', exact: true }).first().click()
  const tools = page.locator('[data-s="S-31"]')
  await expect(tools).toBeVisible()
  const auto = tools.getByRole('switch', { name: 'Tự tải và cập nhật' })
  await expect(auto).toBeChecked()
  await expect(auto).toBeEnabled()
  // yt-dlp đang được app tự tải: thanh tiến độ và nút Huỷ, các nút tải khác bị khoá.
  await expect(tools.getByRole('progressbar', { name: 'Tiến độ tải yt-dlp' })).toBeVisible()
  await expect(tools.getByRole('button', { name: 'Huỷ' })).toBeVisible()
  await expect(tools.getByRole('button', { name: 'Tải deno' })).toBeDisabled()
  await expect(tools.getByText('denoland/deno', { exact: true })).toBeVisible()

  await auto.click()
  await expect(auto).not.toBeChecked()
  expect(await calls(page, 'set_media_tools_auto')).toEqual([
    { cmd: 'set_media_tools_auto', args: { enabled: false } },
  ])
})
