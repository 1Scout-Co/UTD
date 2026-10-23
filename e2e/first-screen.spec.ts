import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

// @smoke = bài thử nhanh sau phát hành (chạy vào bản đã đưa lên, xem playwright.config.ts): CHỈ ĐỌC (mở màn, nhìn), không bấm/nhập/ghi gì vào dữ liệu.
test('S-01: màn đầu hiện danh sách download', { tag: '@smoke' }, async ({ page }) => {
  await page.goto('/')
  await expect(
    page.getByRole('heading', { level: 1, name: 'Tải xuống' }),
  ).toBeVisible()
  const screen = page.locator('[data-s="S-01"]')
  await expect(screen).toBeVisible()
  await expect(screen).toContainText('Qwen3-32B-Q4_K_M.gguf')
})

// Tên có chữ "a11y": máy kiểm (flow-check) chạy riêng các test trợ năng bằng playwright test --grep=a11y.
test('S-01: a11y — không có lỗi truy cập (axe)', async ({ page }) => {
  await page.goto('/')
  await expect(page.locator('[data-s="S-01"]')).toBeVisible()
  const result = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  expect(result.violations).toEqual([])
})

test('S-01: thanh lọc nằm ngoài bảng, lọc và tìm được', async ({ page }) => {
  await page.goto('/')
  const screen = page.locator('[data-s="S-01"]')
  const table = screen.getByRole('table')
  const filters = screen.getByRole('group', { name: 'Lọc download' })
  await expect(table.getByRole('group')).toHaveCount(0)

  const errorTab = filters.getByRole('button', { name: /Có lỗi/ })
  await errorTab.click()
  await expect(errorTab).toHaveAttribute('aria-pressed', 'true')
  await expect(table.locator('tbody tr')).toHaveCount(2)
  await expect(table).toContainText('Tài liệu kiến trúc UTD.pdf')
  await expect(table).not.toContainText('Qwen3-32B-Q4_K_M.gguf')

  await filters.getByRole('button', { name: /Tất cả/ }).click()
  await page.keyboard.press('Control+k')
  const search = screen.getByLabel('Tìm download')
  await expect(search).toBeFocused()
  await search.fill('ubuntu')
  await expect(table.locator('tbody tr')).toHaveCount(1)
  await search.fill('khong-co-file-nay')
  await screen.getByRole('button', { name: 'Xoá bộ lọc' }).click()
  await expect(search).toHaveValue('')
  await expect(table.locator('tbody tr')).toHaveCount(10)
})

test('S-01: cửa sổ 1080px — bảng không cuộn ngang, nút chính vẫn thấy', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1080, height: 720 })
  await page.goto('/')
  const screen = page.locator('[data-s="S-01"]')
  await expect(screen.getByRole('button', { name: 'Thêm URL' })).toBeInViewport()
  const overflow = await screen
    .getByRole('table')
    .evaluate((table) => table.parentElement!.scrollWidth - table.parentElement!.clientWidth)
  expect(overflow).toBeLessThanOrEqual(0)
})

test('S-01: chuột phải vào một hàng mở menu kiểu IDM và chọn hàng đó', async ({ page }) => {
  await page.goto('/')
  const row = page.locator('tbody tr', { hasText: 'Ubuntu 24.04.3 LTS Desktop' })
  await row.click({ button: 'right' })
  const menu = page.getByRole('menu')
  await expect(menu).toBeVisible()
  for (const name of [
    'Mở file',
    'Mở thư mục',
    'Đổi tên…',
    'Di chuyển…',
    'Sao chép link',
    'Chẩn đoán',
    'Xoá khỏi danh sách',
    'Xoá file (vào Thùng rác)',
  ]) {
    await expect(menu.getByRole('menuitem', { name })).toBeVisible()
  }
  // Bản xem trước không có app desktop: mục cần app bị mờ, không ẩn.
  await expect(menu.getByRole('menuitem', { name: 'Mở file' })).toHaveAttribute('aria-disabled', 'true')
  await expect(row).toHaveAttribute('data-selected', 'true')
})

test('S-01: a11y — menu chuột phải không có lỗi truy cập', async ({ page }) => {
  await page.goto('/')
  await page.locator('tbody tr', { hasText: 'Docker Desktop 4.42' }).click({ button: 'right' })
  await expect(page.getByRole('menu')).toBeVisible()
  const result = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  expect(result.violations).toEqual([])
})
