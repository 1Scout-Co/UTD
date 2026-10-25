import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

test('S-05: thêm URL, tên file tự lấy, chặn SHA-256 sai', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Thêm URL' }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog).toBeVisible()

  await dialog
    .getByLabel('URL cần tải')
    .fill('https://example.com/releases/app-1.2.3.zip')
  await expect(dialog.getByText('app-1.2.3.zip').first()).toBeVisible()

  await dialog.getByLabel('SHA-256 mong đợi').fill('abc')
  await dialog.getByRole('button', { name: 'Bắt đầu' }).click()
  await expect(dialog.getByRole('alert')).toContainText('64 ký tự hex')

  await dialog.getByRole('button', { name: 'Huỷ' }).click()
  await expect(dialog).not.toBeVisible()
})

test('S-01: menu hành động của một dòng mở được', async ({ page }) => {
  await page.goto('/')
  await page
    .getByRole('button', { name: 'Tuỳ chọn cho Qwen3-32B-Q4_K_M.gguf' })
    .click()
  await expect(page.getByRole('menuitem', { name: 'Tạm dừng' })).toBeVisible()
})

test('S-05: a11y — dialog thêm download không có lỗi truy cập', async ({
  page,
}) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Thêm URL' }).click()
  await expect(page.getByRole('dialog')).toBeVisible()
  const result = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  expect(result.violations).toEqual([])
})

test('S-27: mở cài đặt, xem hướng dẫn tích hợp ở chế độ preview', async ({
  page,
}) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Cài đặt' }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog).toBeVisible()
  await expect(
    dialog.getByRole('heading', { name: 'Tích hợp trình duyệt' }),
  ).toBeVisible()
  await expect(dialog.getByText(/Bản xem trước trên trình duyệt/)).toBeVisible()
  await dialog.getByRole('button', { name: 'Đóng', exact: true }).click()
  await expect(dialog).not.toBeVisible()
})

test('S-27: a11y — cài đặt không có lỗi truy cập', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Cài đặt' }).click()
  await expect(page.getByRole('dialog')).toBeVisible()
  const result = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  expect(result.violations).toEqual([])
})

test('S-06: mở dialog media, nhắc cần app desktop', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Media' }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('heading', { name: 'Tải media' })).toBeVisible()
  // Bản xem trước không còn dòng nhắc sẵn: chỉ khi bấm Phân tích mới báo cần app desktop.
  await dialog.getByLabel('URL media').fill('https://www.youtube.com/watch?v=demo')
  await dialog.getByRole('button', { name: 'Phân tích' }).click()
  await expect(dialog.getByRole('alert')).toContainText('Mở app UTD')
  await dialog.getByRole('button', { name: 'Huỷ' }).click()
  await expect(dialog).not.toBeVisible()
})

test('S-06: a11y — dialog media không có lỗi truy cập', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Media' }).click()
  await expect(page.getByRole('dialog')).toBeVisible()
  const result = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  expect(result.violations).toEqual([])
})

test('S-21: chẩn đoán một download lỗi', async ({ page }) => {
  await page.goto('/')
  await page
    .getByRole('button', {
      name: 'Tuỳ chọn cho Tài liệu kiến trúc UTD.pdf',
    })
    .click()
  await page.getByRole('menuitem', { name: 'Chẩn đoán' }).click()
  const dialog = page.getByRole('dialog')
  await expect(
    dialog.getByRole('heading', { name: 'Chẩn đoán download' }),
  ).toBeVisible()
  await expect(dialog.getByText('Mạng bị gián đoạn')).toBeVisible()
  await dialog.getByRole('button', { name: 'Đóng', exact: true }).click()
  await expect(dialog).not.toBeVisible()
})

test('S-21: file bị quarantine hiện chẩn đoán an toàn', async ({ page }) => {
  await page.goto('/')
  await page
    .getByRole('button', { name: 'Tuỳ chọn cho setup_office_2026.exe' })
    .click()
  await page.getByRole('menuitem', { name: 'Chẩn đoán' }).click()
  const dialog = page.getByRole('dialog')
  await expect(
    dialog.getByText('File bị Download Guard cách ly'),
  ).toBeVisible()
  await expect(
    dialog.getByRole('button', { name: 'Tiếp tục' }),
  ).toHaveCount(0)
})

test('S-21: a11y — chẩn đoán không có lỗi truy cập', async ({ page }) => {
  await page.goto('/')
  await page
    .getByRole('button', {
      name: 'Tuỳ chọn cho Tài liệu kiến trúc UTD.pdf',
    })
    .click()
  await page.getByRole('menuitem', { name: 'Chẩn đoán' }).click()
  await expect(page.getByRole('dialog')).toBeVisible()
  const result = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  expect(result.violations).toEqual([])
})
