import AxeBuilder from '@axe-core/playwright'
import { expect, test, type Page } from '@playwright/test'

import { calls, emitEvent, mockTauri } from './tauri-mock'

// Cảnh báo trước khi tải (S-22): luật nằm ở Download Guard của engine (lệnh `check_links`); giao diện dò link
// (`inspect_link`) rồi hỏi một lần — một link ở hộp Thêm URL, cả lô ở hộp Thêm hàng loạt.

const SETUP = 'https://cdn.example.com/tools/setup.exe'
const EXE_WARNING = [{ kind: 'dangerous_type', detail: 'exe' }]

const remote = (url: string, fileName: string, contentType: string | null = null) => ({
  url,
  fileName,
  totalBytes: 2_000_000,
  contentType,
  page: false,
})

async function addLink(page: Page, url: string) {
  await page.goto('/')
  await page.getByRole('button', { name: 'Thêm URL' }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByLabel('URL cần tải').fill(url)
  await dialog.getByRole('button', { name: 'Bắt đầu' }).click()
  return dialog
}

test('S-22: link file chạy được — hỏi trước, Xem chi tiết, Vẫn tải mới tạo task', async ({ page }) => {
  await mockTauri(page, {
    inspect_link: remote(SETUP, 'setup.exe', 'application/x-msdownload'),
    check_links: [EXE_WARNING],
    add_download: { id: 'n1' },
  })
  const dialog = await addLink(page, SETUP)
  const warning = dialog.locator('[data-s="S-22"]')
  await expect(warning.getByRole('heading', { name: 'Cảnh báo trước khi tải' })).toBeVisible()
  await expect(warning.getByText('File chạy được')).toBeVisible()
  expect(await calls(page, 'add_download')).toHaveLength(0)
  // Kết quả dò (tên server đặt, kiểu nội dung) đi vào phép kiểm.
  expect((await calls(page, 'check_links'))[0].args).toEqual({
    links: [{ url: SETUP, finalUrl: SETUP, fileName: 'setup.exe', contentType: 'application/x-msdownload' }],
  })

  await warning.getByRole('button', { name: 'Xem chi tiết' }).click()
  await expect(warning.getByText(/khu cách ly, không tự mở/)).toBeVisible()
  await warning.getByRole('button', { name: 'Vẫn tải' }).click()
  await expect.poll(async () => (await calls(page, 'add_download')).length).toBe(1)
  expect((await calls(page, 'add_download'))[0].args).toMatchObject({ input: { url: SETUP } })
})

test('S-22: Huỷ thì không tải gì và đóng hộp', async ({ page }) => {
  await mockTauri(page, {
    inspect_link: remote('http://mirror.example.org/a.iso', 'a.iso'),
    check_links: [[{ kind: 'insecure_http', detail: 'mirror.example.org' }]],
  })
  const dialog = await addLink(page, 'http://mirror.example.org/a.iso')
  const warning = dialog.locator('[data-s="S-22"]')
  await expect(warning.getByText('Link không mã hoá')).toBeVisible()
  await warning.getByRole('button', { name: 'Huỷ' }).click()
  await expect(dialog).toBeHidden()
  expect(await calls(page, 'add_download')).toHaveLength(0)
})

test('S-22: tên đảo chiều hiện mã ký tự thay vì tên lừa mắt', async ({ page }) => {
  // `hoa-don` + U+202E + `fdp.exe` hiện thành "hoa-donexe.pdf" nếu để nguyên ký tự đảo chiều.
  const sneaky = 'https://cdn.example.com/hoa-don%E2%80%AEfdp.exe'
  await mockTauri(page, {
    inspect_link: remote(sneaky, 'download.bin'),
    check_links: [[{ kind: 'hidden_chars', detail: 'U+202E' }, ...EXE_WARNING]],
  })
  const dialog = await addLink(page, sneaky)
  await expect(dialog.locator('[data-s="S-22"]').getByText('hoa-don[U+202E]fdp.exe')).toBeVisible()
  expect((await calls(page, 'check_links'))[0].args).toMatchObject({
    links: [{ fileName: 'hoa-don\u202Efdp.exe' }],
  })
})

test('S-22: link sạch thì tải luôn, không hỏi', async ({ page }) => {
  await mockTauri(page, {
    inspect_link: remote('https://example.com/a.zip', 'a.zip', 'application/zip'),
    check_links: [[]],
    add_download: { id: 'n1' },
  })
  const dialog = await addLink(page, 'https://example.com/a.zip')
  await expect.poll(async () => (await calls(page, 'add_download')).length).toBe(1)
  await expect(dialog.locator('[data-s="S-22"]')).toHaveCount(0)
})

test('S-22: lô từ trình duyệt — hỏi một lần cho cả lô, bỏ link cảnh báo', async ({ page }) => {
  const links = ['https://example.com/a.zip', SETUP, 'http://mirror.example.org/b.iso']
  await mockTauri(page, {
    inspect_link: null,
    check_links: [[], EXE_WARNING, [{ kind: 'insecure_http', detail: 'mirror.example.org' }]],
    add_batch_downloads: { tasks: [], failed: [] },
  })
  await page.goto('/')
  await expect(page.locator('[data-s="S-01"], [data-s="S-02"]').first()).toBeVisible()
  await emitEvent(page, 'utd://batch-request', { urls: links })
  const dialog = page.getByRole('dialog')
  await dialog.getByRole('button', { name: 'Bắt đầu ngay' }).click()

  const warning = dialog.locator('[data-s="S-22"]')
  await expect(warning.getByText('Link có cảnh báo: 2/3')).toBeVisible()
  await expect(warning.getByRole('listitem').filter({ hasText: 'File chạy được' })).toContainText('1')
  await expect(warning.getByRole('listitem').filter({ hasText: 'Link không mã hoá' })).toContainText('1')
  expect(await calls(page, 'add_batch_downloads')).toHaveLength(0)
  await warning.getByRole('button', { name: 'Xem chi tiết' }).click()
  await expect(warning.getByText('setup.exe')).toBeVisible()

  await warning.getByRole('button', { name: 'Bỏ link cảnh báo' }).click()
  await expect.poll(async () => (await calls(page, 'add_batch_downloads')).length).toBe(1)
  expect((await calls(page, 'add_batch_downloads'))[0].args).toMatchObject({
    input: { urls: ['https://example.com/a.zip'] },
  })
})

test('S-22: lô toàn link cảnh báo — chỉ còn Vẫn tải cả lô hoặc Huỷ', async ({ page }) => {
  await mockTauri(page, {
    inspect_link: null,
    check_links: [EXE_WARNING],
    add_batch_downloads: { tasks: [], failed: [] },
  })
  await page.goto('/')
  await expect(page.locator('[data-s="S-01"], [data-s="S-02"]').first()).toBeVisible()
  await emitEvent(page, 'utd://batch-request', { urls: [SETUP] })
  const dialog = page.getByRole('dialog')
  await dialog.getByRole('button', { name: 'Bắt đầu ngay' }).click()
  const warning = dialog.locator('[data-s="S-22"]')
  await expect(warning.getByRole('button', { name: 'Bỏ link cảnh báo' })).toHaveCount(0)
  await warning.getByRole('button', { name: 'Huỷ' }).click()
  await expect(warning).toBeHidden()
  await expect(dialog.getByRole('button', { name: 'Bắt đầu ngay' })).toBeVisible()
  await dialog.getByRole('button', { name: 'Bắt đầu ngay' }).click()
  await dialog.locator('[data-s="S-22"]').getByRole('button', { name: 'Vẫn tải cả lô' }).click()
  await expect.poll(async () => (await calls(page, 'add_batch_downloads')).length).toBe(1)
  expect((await calls(page, 'add_batch_downloads'))[0].args).toMatchObject({ input: { urls: [SETUP] } })
})

test('S-22: a11y — cảnh báo trước khi tải', async ({ page }) => {
  await mockTauri(page, {
    inspect_link: remote(SETUP, 'setup.exe', 'application/x-msdownload'),
    check_links: [[...EXE_WARNING, { kind: 'insecure_http', detail: 'cdn.example.com' }]],
  })
  const dialog = await addLink(page, SETUP)
  await expect(dialog.locator('[data-s="S-22"]')).toBeVisible()
  await dialog.getByRole('button', { name: 'Xem chi tiết' }).click()
  await page.waitForFunction(() => document.getAnimations().every((animation) => animation.playState !== 'running'))
  const result = await new AxeBuilder({ page })
    .include('[role="dialog"]')
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  expect(result.violations).toEqual([])
})
