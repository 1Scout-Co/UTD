import AxeBuilder from '@axe-core/playwright'
import { expect, test, type Page } from '@playwright/test'

import { calls, emitEvent, mockTauri } from './tauri-mock'

// Cứu download (src-tauri/src/recover.rs): lỗi tạm thời không thành "Lỗi" mà chờ tự thử lại, mất mạng chờ
// có mạng, ổ đầy chờ chỗ trống; link hết hạn thì "Cập nhật link…" (dán link mới hoặc tải lại từ trình
// duyệt), file trên máy chủ đổi thì "Tải lại từ đầu".

const now = Date.now()
const DIR = 'C:\\Users\\utd\\Downloads\\UTD'

function task(id: string, fileName: string, extra: Record<string, unknown>) {
  return {
    id,
    url: `https://cdn.example.com/${fileName}`,
    finalUrl: null,
    fileName,
    destDir: DIR,
    tempPath: `${DIR}\\${fileName}.${id}.part`,
    kind: 'http',
    status: 'paused',
    totalBytes: 4_000_000_000,
    downloadedBytes: 1_500_000_000,
    etag: null,
    lastModified: null,
    expectedSha256: null,
    hashState: 'pending',
    connections: 16,
    error: null,
    createdAt: now - 60_000,
    updatedAt: now,
    completedAt: null,
    ...extra,
  }
}

const error = (cls: string, code: string, message = 'x') => ({ class: cls, code, message })

const TASKS = [
  task('busy', 'ubuntu-26.04.iso', {
    error: error('transient', 'http.503'),
    recovery: { wait: 'retry', retryAt: now + 120_000, attempts: 2, progress: 1_500_000_000 },
  }),
  task('offline', 'dataset.tar', {
    error: error('transient', 'network.connect'),
    recovery: { wait: 'network', retryAt: now + 60_000, attempts: 0, progress: 1_500_000_000 },
  }),
  task('full', 'video-4k.mkv', {
    error: error('user', 'disk.full'),
    recovery: { wait: 'disk', attempts: 0, progress: 1_500_000_000, needBytes: 2_600_000_000 },
  }),
  task('signed', 'model.safetensors', {
    status: 'failed',
    error: error('user', 'http.403'),
    recovery: { wait: 'link', attempts: 0, progress: 1_500_000_000 },
  }),
  task('changed', 'release.zip', {
    status: 'failed',
    error: error('permanent', 'file.changed'),
  }),
]

test.use({ viewport: { width: 1920, height: 1080 } })

async function openMenu(page: Page, name: string) {
  await page.getByRole('button', { name: `Tuỳ chọn cho ${name}` }).click()
}

test('S-01: download chờ tự thử lại / chờ mạng / chờ chỗ trống không hiện là lỗi', async ({ page }) => {
  await mockTauri(page, { list_downloads: TASKS })
  await page.goto('/')
  const busy = page.locator('tr[data-row-id="busy"]')
  await expect(busy).toContainText(/Sẽ thử lại lúc \d{2}:\d{2} · Máy chủ đang bận/)
  await expect(busy).toContainText(/Thử lại \d{2}:\d{2}/)
  await expect(busy).toContainText('Đang chờ')
  await expect(page.locator('tr[data-row-id="offline"]')).toContainText('Mất mạng · tự tải tiếp khi có mạng')
  await expect(page.locator('tr[data-row-id="full"]')).toContainText(/Ổ đầy · cần trống thêm .+ · tự tải tiếp khi đủ chỗ/)
  await page.screenshot({ path: test.info().outputPath('cho-tu-thu-lai.png') })

  // Cửa sổ hẹp (cột "Còn lại" ẩn): giờ hẹn nằm dưới tốc độ, không ghép thành "còn Thử lại …".
  await page.setViewportSize({ width: 1280, height: 800 })
  await expect(busy).toContainText(/Thử lại \d{2}:\d{2}/)
  await expect(busy).not.toContainText('Còn Thử lại')
  await expect(busy).not.toContainText('còn Thử lại')
  await page.setViewportSize({ width: 1920, height: 1080 })

  // Đang chờ: Tạm dừng để thôi tự chạy lại, Thử lại để chạy ngay.
  await openMenu(page, 'ubuntu-26.04.iso')
  await expect(page.getByRole('menuitem', { name: 'Tạm dừng' })).toBeEnabled()
  await page.getByRole('menuitem', { name: 'Thử lại' }).click()
  expect(await calls(page, 'resume_download')).toEqual([{ cmd: 'resume_download', args: { taskId: 'busy' } }])

  // Chờ chỗ trống: là "tạm dừng" kèm lý do, có nút Tiếp tục.
  await openMenu(page, 'video-4k.mkv')
  await expect(page.getByRole('menuitem', { name: 'Tiếp tục' })).toBeEnabled()
})

test('S-21: cập nhật link — dán link mới, hoặc tải lại từ trình duyệt', async ({ page }) => {
  await mockTauri(page, { list_downloads: TASKS })
  await page.goto('/')
  await openMenu(page, 'model.safetensors')
  await page.getByRole('menuitem', { name: 'Cập nhật link…' }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('heading', { name: 'Cập nhật link' })).toBeVisible()
  await expect(dialog).toContainText('Hoặc bấm tải lại file này trong trình duyệt')
  const lastAwait = async () => (await calls(page, 'await_download_link')).at(-1)?.args
  await expect.poll(lastAwait).toEqual({ taskId: 'signed' })
  await expect(dialog.getByRole('button', { name: 'Cập nhật' })).toBeDisabled()

  // Trình duyệt gửi một link khác file: báo, phần đã tải giữ nguyên, hộp vẫn mở.
  await emitEvent(page, 'utd://link-updated', {
    taskId: 'signed',
    error: '[recover.changed] Link mới trỏ tới một file khác. Phần đã tải được giữ nguyên.',
  })
  await expect(dialog.getByRole('alert')).toContainText('Link mới trỏ tới một file khác')

  const fresh = 'https://cdn.example.com/model.safetensors?sig=moi'
  await dialog.getByLabel('Link mới').fill(fresh)
  await dialog.getByRole('button', { name: 'Cập nhật' }).click()
  await expect(dialog).not.toBeVisible()
  expect(await calls(page, 'update_download_link')).toEqual([
    { cmd: 'update_download_link', args: { taskId: 'signed', url: fresh } },
  ])
  await expect.poll(lastAwait).toEqual({ taskId: null })

  // Mở lại rồi tải lại từ trình duyệt: app nhận link, hộp tự đóng.
  await openMenu(page, 'model.safetensors')
  await page.getByRole('menuitem', { name: 'Cập nhật link…' }).click()
  await expect(dialog).toBeVisible()
  await expect.poll(lastAwait).toEqual({ taskId: 'signed' })
  await emitEvent(page, 'utd://link-updated', { taskId: 'signed', error: null })
  await expect(dialog).not.toBeVisible()
})

test('S-21: Doctor — link hết hạn mở Cập nhật link, file đổi thì Tải lại từ đầu', async ({ page }) => {
  await mockTauri(page, { list_downloads: TASKS })
  await page.goto('/')
  await openMenu(page, 'model.safetensors')
  await page.getByRole('menuitem', { name: 'Chẩn đoán' }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByRole('button', { name: 'Cập nhật link…' }).click()
  await expect(dialog.getByRole('heading', { name: 'Cập nhật link' })).toBeVisible()
  await dialog.getByRole('button', { name: 'Huỷ' }).click()
  await expect(dialog).not.toBeVisible()

  await openMenu(page, 'release.zip')
  await page.getByRole('menuitem', { name: 'Chẩn đoán' }).click()
  await expect(dialog.getByRole('heading', { name: 'File trên máy chủ đã đổi' })).toBeVisible()
  await expect(dialog).toContainText('File trên máy chủ đã đổi trong lúc đang tải dở')
  await dialog.getByRole('button', { name: 'Tải lại từ đầu' }).click()
  await expect(dialog).not.toBeVisible()
  expect(await calls(page, 'restart_download')).toEqual([
    { cmd: 'restart_download', args: { taskId: 'changed' } },
  ])
})

test('S-21: Doctor — link là trang web: không có Thử lại; cách làm (cả Drive) và Cập nhật link…', async ({ page }) => {
  const notFile = task('page', 'view', {
    status: 'failed',
    url: 'https://drive.google.com/file/d/1hfaJWoyFptsG9niNHVfOB8m6snwbibpl/view',
    totalBytes: null,
    downloadedBytes: 0,
    error: error('user', 'http.not_a_file', 'Link này là một trang web, không phải file.'),
  })
  await mockTauri(page, { list_downloads: [notFile] })
  await page.goto('/')
  await openMenu(page, 'view')
  await page.getByRole('menuitem', { name: 'Chẩn đoán' }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('heading', { name: 'Link này là trang web, không phải file' })).toBeVisible()
  await expect(dialog).toContainText('Hoặc chuột phải đúng link tải › Tải bằng UTD.')
  await expect(dialog).toContainText('Google Drive: mở file ra trang riêng rồi thử lại.')
  await expect(dialog.getByRole('button', { name: /^(Thử lại|Tiếp tục)$/ })).toHaveCount(0)
  await dialog.getByRole('button', { name: 'Cập nhật link…' }).click()
  await expect(dialog.getByRole('heading', { name: 'Cập nhật link' })).toBeVisible()
})

test('S-21: a11y — hộp Cập nhật link không có lỗi truy cập', async ({ page }) => {
  await mockTauri(page, { list_downloads: TASKS })
  await page.goto('/')
  await openMenu(page, 'model.safetensors')
  await page.getByRole('menuitem', { name: 'Cập nhật link…' }).click()
  await expect(page.getByRole('dialog')).toBeVisible()
  await expect(page.getByRole('menu')).toHaveCount(0)
  const result = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  expect(result.violations).toEqual([])
})
