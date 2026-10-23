import AxeBuilder from '@axe-core/playwright'
import { expect, test, type Page } from '@playwright/test'

async function openSettings(page: Page) {
  await page.goto('/')
  await page.getByRole('button', { name: 'Cài đặt', exact: true }).first().click()
  const dialog = page.locator('[data-s="S-27"]')
  await expect(dialog).toBeVisible()
  return dialog
}

// Bản xem trước trên trình duyệt (mock data): chỉ xem được thư mục, nút đổi bị khoá.
test('S-27: cài đặt hiện thư mục tải, preview khoá nút đổi', async ({ page }) => {
  const dialog = await openSettings(page)
  await expect(dialog.getByRole('heading', { name: 'Tải xuống' })).toBeVisible()
  await expect(dialog.getByText('Downloads\\UTD', { exact: true })).toBeVisible()
  await expect(dialog.getByRole('button', { name: 'Đổi thư mục' })).toBeDisabled()
  await expect(dialog.getByRole('button', { name: 'Dùng mặc định' })).toBeDisabled()
  await expect(dialog.getByText('Đổi thư mục chỉ có trong app UTD.')).toBeVisible()
  await expect(dialog.getByRole('alert')).toHaveCount(0)
})

// Tên có chữ "a11y": flow-check chạy riêng các test trợ năng bằng --grep=a11y.
test('S-27: a11y — cài đặt thư mục tải không có lỗi truy cập', async ({ page }) => {
  await openSettings(page)
  const result = await new AxeBuilder({ page })
    .include('[data-s="S-27"]')
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  expect(result.violations).toEqual([])
})

// S-31 (thiếu thành phần): preview không dò được công cụ ⇒ hiện nút tải bị khoá, kèm nguồn,
// giấy phép, dung lượng và SHA-256 để người dùng biết trước khi bấm.
test('S-31: công cụ media — preview khoá nút tải, hiện nguồn và giấy phép', async ({ page }) => {
  const dialog = await openSettings(page)
  await expect(dialog.getByRole('heading', { name: 'Công cụ media' })).toBeVisible()
  const tools = dialog.locator('[data-s="S-31"]')
  await expect(tools).toBeVisible()
  await expect(tools.getByRole('button', { name: 'Tải yt-dlp' })).toBeDisabled()
  await expect(tools.getByRole('button', { name: 'Tải ffmpeg' })).toBeDisabled()
  await expect(tools.getByRole('button', { name: 'Cập nhật' })).toHaveCount(0)
  await expect(tools.getByText('yt-dlp/yt-dlp', { exact: true })).toBeVisible()
  await expect(tools.getByText('yt-dlp/FFmpeg-Builds', { exact: true })).toBeVisible()
  await expect(tools.getByText(/Unlicense/)).toBeVisible()
  await expect(tools.getByText(/GPLv3, tải riêng, không kèm bộ cài/)).toBeVisible()
  await expect(tools.getByText(/Kiểm SHA-256 trước khi dùng/)).toHaveCount(2)
  await expect(tools.getByText('Tải công cụ chỉ có trong app UTD.')).toBeVisible()
  await expect(tools.getByRole('alert')).toHaveCount(0)
})

test('S-31: a11y — mục công cụ media không có lỗi truy cập', async ({ page }) => {
  await openSettings(page)
  const result = await new AxeBuilder({ page })
    .include('[data-s="S-31"]')
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  expect(result.violations).toEqual([])
})

// Tự cập nhật (AC-20, P-03): preview không gọi được app ⇒ nút kiểm tra và công tắc bị khoá,
// không có nút "Cập nhật", vẫn nói rõ chỉ hỏi GitHub, không gửi dữ liệu.
test('S-27: cập nhật — preview khoá kiểm tra và công tắc tự kiểm tra', async ({ page }) => {
  const dialog = await openSettings(page)
  await expect(dialog.getByRole('heading', { name: 'Cập nhật', exact: true })).toBeVisible()
  const updates = dialog.getByTestId('settings-updates')
  await expect(updates.getByText('bản xem trước', { exact: true })).toBeVisible()
  await expect(updates.getByRole('button', { name: 'Kiểm tra' })).toBeDisabled()
  await expect(updates.getByRole('button', { name: 'Cập nhật' })).toHaveCount(0)
  const auto = updates.getByRole('switch', { name: 'Tự kiểm tra cập nhật' })
  await expect(auto).toBeVisible()
  await expect(auto).toBeChecked()
  await expect(auto).toBeDisabled()
  await expect(dialog.getByText('Chỉ hỏi GitHub có bản mới không, không gửi dữ liệu.')).toBeVisible()
  await expect(updates.getByText('Cập nhật chỉ có trong app UTD.')).toBeVisible()
  await expect(updates.getByRole('alert')).toHaveCount(0)
  await expect(dialog.getByText('UTD · Apache-2.0', { exact: true })).toBeVisible()
})
