import AxeBuilder from '@axe-core/playwright'
import { expect, test, type Page } from '@playwright/test'

// Đa ngôn ngữ (S-27 mục Ngôn ngữ). Bản xem trước lưu lựa chọn trong localStorage; app desktop lưu trong
// journal (src-tauri/src/language.rs). Mặc định playwright.config.ts chạy vi-VN.

async function openSettings(page: Page, name: string) {
  await page.getByRole('button', { name, exact: true }).first().click()
  const dialog = page.locator('[data-s="S-27"]')
  await expect(dialog).toBeVisible()
  return dialog
}

test.describe('hệ điều hành tiếng Anh', () => {
  test.use({ locale: 'en-US' })

  test('S-01: lần đầu mở theo ngôn ngữ hệ điều hành (English)', async ({ page }) => {
    await page.goto('/')
    await expect(page.getByRole('heading', { level: 1, name: 'Downloads' })).toBeVisible()
    await expect(page.locator('html')).toHaveAttribute('lang', 'en')
    await expect(page.getByRole('button', { name: 'Add URL' })).toBeVisible()
  })
})

test.describe('hệ điều hành chưa có bản dịch', () => {
  test.use({ locale: 'sw-KE' })

  test('S-01: ngôn ngữ không khớp thì dùng English', async ({ page }) => {
    await page.goto('/')
    await expect(page.getByRole('heading', { level: 1, name: 'Downloads' })).toBeVisible()
  })
})

test.describe('hệ điều hành tiếng Trung Hồng Kông', () => {
  test.use({ locale: 'zh-HK' })

  test('S-01: zh-HK dùng chữ Phồn thể (zh-TW)', async ({ page }) => {
    await page.goto('/')
    await expect(page.locator('html')).toHaveAttribute('lang', 'zh-TW')
  })
})

test('S-27: đổi sang English trong Cài đặt, áp ngay và nhớ sau khi mở lại', async ({ page }) => {
  await page.goto('/')
  const dialog = await openSettings(page, 'Cài đặt')
  await dialog.getByRole('button', { name: /Ngôn ngữ/ }).click()
  const menu = page.getByRole('menu')
  // Mỗi ngôn ngữ có cờ + tên bản địa.
  await expect(menu.getByRole('menuitemradio')).toHaveCount(16)
  await expect(menu.getByRole('menuitemradio', { name: 'Tiếng Việt' })).toHaveAttribute('aria-checked', 'true')
  await expect(menu.getByRole('menuitemradio', { name: '简体中文' }).locator('img')).toBeVisible()
  await menu.getByRole('menuitemradio', { name: 'English' }).click()

  // Hộp Cài đặt vẫn mở, chữ đổi ngay; màn chính phía sau cũng đổi.
  await expect(dialog.getByRole('heading', { name: 'Language' })).toBeVisible()
  await expect(dialog.getByRole('heading', { name: 'Media tools' })).toBeVisible()
  await expect(dialog.getByRole('button', { name: 'Get yt-dlp' })).toBeDisabled()
  await expect(page.locator('html')).toHaveAttribute('lang', 'en')
  await dialog.getByRole('button', { name: 'Close', exact: true }).click()
  await expect(page.getByRole('heading', { level: 1, name: 'Downloads' })).toBeVisible()
  const table = page.locator('[data-s="S-01"]').getByRole('table')
  await expect(table.getByRole('columnheader', { name: 'Size' })).toBeVisible()
  await expect(table).toContainText('19.4 GB')

  await page.reload()
  await expect(page.getByRole('heading', { level: 1, name: 'Downloads' })).toBeVisible()
})

test('S-27: menu chuột phải và Chẩn đoán dịch theo ngôn ngữ, lỗi Rust dịch theo mã', async ({ page }) => {
  await page.goto('/')
  await page.evaluate(() => window.localStorage.setItem('utd.language', 'en'))
  await page.reload()
  const row = page.locator('tbody tr', { hasText: 'Tài liệu kiến trúc UTD.pdf' })
  await row.click({ button: 'right' })
  await page.getByRole('menuitem', { name: 'Diagnose' }).click()
  const doctor = page.getByRole('dialog')
  await expect(doctor.getByRole('heading', { name: 'Network interrupted' })).toBeVisible()
  // Mã network.closed ⇒ câu dịch err.network.closed, không phải câu gốc của engine.
  await expect(doctor.getByText('The connection closed before the file finished.')).toBeVisible()
})

test.describe('hệ điều hành tiếng Ả Rập', () => {
  test.use({ locale: 'ar-SA' })

  test('S-01: tiếng Ả Rập viết phải→trái, bảng không cuộn ngang', async ({ page }) => {
    await page.setViewportSize({ width: 1080, height: 720 })
    await page.goto('/')
    await expect(page.locator('html')).toHaveAttribute('dir', 'rtl')
    const screen = page.locator('[data-s="S-01"]')
    await expect(screen).toBeVisible()
    // Thanh bên nằm bên phải, bảng bên trái.
    const sidebar = await page.locator('aside').boundingBox()
    const main = await page.locator('main').boundingBox()
    expect(sidebar!.x).toBeGreaterThan(main!.x)
    const overflow = await screen
      .getByRole('table')
      .evaluate((table) => table.parentElement!.scrollWidth - table.parentElement!.clientWidth)
    expect(overflow).toBeLessThanOrEqual(0)
  })

  test('S-27: a11y — Cài đặt tiếng Ả Rập không có lỗi truy cập', async ({ page }) => {
    await page.goto('/')
    await page.locator('aside').getByRole('button').last().click()
    await expect(page.locator('[data-s="S-27"]')).toBeVisible()
    const result = await new AxeBuilder({ page })
      .include('[data-s="S-27"]')
      .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
      .analyze()
    expect(result.violations).toEqual([])
  })
})
