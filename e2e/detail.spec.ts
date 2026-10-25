import AxeBuilder from '@axe-core/playwright'
import { expect, test, type Page } from '@playwright/test'

import { PENDING, calls, emitEvent, failWith, mockTauri, respond } from './tauri-mock'

// Màn Chi tiết download (S-15..S-21): app giả trả task + bản đồ đoạn + lịch sử như `download_detail` thật
// (src-tauri/src/detail.rs); bản đồ đoạn mới tới qua `utd://segments`.

const MB = 1024 * 1024
const DIR = 'C:\\Users\\utd\\Downloads\\UTD'
const AT = Date.parse('2026-10-25T09:30:00+07:00')

type Status = 'queued' | 'downloading' | 'paused' | 'completed' | 'failed' | 'quarantined'

function task(status: Status, over: Record<string, unknown> = {}) {
  const total = 400 * MB
  return {
    id: 't1',
    url: 'https://cdn.example.com/isos/ubuntu.iso?token=abc',
    finalUrl: null,
    fileName: 'ubuntu.iso',
    destDir: DIR,
    tempPath: `${DIR}\\ubuntu.iso.t1.part`,
    kind: 'http',
    status,
    totalBytes: total,
    downloadedBytes: status === 'completed' ? total : 150 * MB,
    etag: null,
    lastModified: null,
    expectedSha256: null,
    hashState: status === 'completed' ? 'unavailable' : 'pending',
    connections: 8,
    error: null,
    createdAt: AT,
    updatedAt: AT + 60_000,
    completedAt: status === 'completed' ? AT + 125_000 : null,
    ...over,
  }
}

/** 4 đoạn 100 MB; `done` = số byte đã có của từng đoạn. */
function segments(done: number[], active = true) {
  return done.map((bytes, index) => ({
    index,
    start: index * 100 * MB,
    end: (index + 1) * 100 * MB - 1,
    downloaded: bytes,
    status: bytes >= 100 * MB ? 'done' : active && bytes > 0 ? 'active' : 'pending',
  }))
}

function detail(engineTask: ReturnType<typeof task>, over: Record<string, unknown> = {}) {
  return {
    task: engineTask,
    parts: [
      {
        id: engineTask.id,
        role: 'file',
        fileName: engineTask.fileName,
        status: engineTask.status,
        totalBytes: engineTask.totalBytes,
        downloadedBytes: engineTask.downloadedBytes,
        segments: segments([100 * MB, 50 * MB, 0, 0]),
      },
    ],
    events: [
      { at: AT, status: 'queued', downloadedBytes: 0 },
      { at: AT + 5_000, status: 'downloading', downloadedBytes: 0 },
    ],
    targetPath: `${DIR}\\ubuntu.iso`,
    fileExists: engineTask.status === 'completed' ? true : null,
    mediaTitle: null,
    ...over,
  }
}

async function open(page: Page, responses: Record<string, unknown>) {
  await mockTauri(page, responses)
  await page.goto('/')
  await page.getByText('ubuntu.iso', { exact: true }).first().dblclick()
  return page.getByRole('dialog')
}

async function axe(page: Page) {
  await page.waitForFunction(() => document.getAnimations().every((animation) => animation.playState !== 'running'))
  return new AxeBuilder({ page })
    .include('[role="dialog"]')
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
}

test('S-15: nhấp đúp mở chi tiết đang tải — số liệu sống, bản đồ đoạn đẩy từ app', async ({ page }) => {
  const running = task('downloading')
  const dialog = await open(page, { list_downloads: [running], download_detail: detail(running) })
  const screen = dialog.locator('[data-s="S-15"]')
  await expect(screen).toBeVisible()
  await expect(dialog.getByRole('heading', { name: 'ubuntu.iso' })).toBeVisible()
  await expect(screen.getByText('1/4 đoạn xong')).toBeVisible()
  await expect(screen.getByRole('img', { name: /Bản đồ đoạn/ })).toBeVisible()
  expect((await calls(page, 'watch_download')).at(-1)?.args).toEqual({ taskId: 't1' })

  await emitEvent(page, 'utd://progress', {
    taskId: 't1',
    status: 'downloading',
    downloadedBytes: 220 * MB,
    totalBytes: 400 * MB,
    bytesPerSecond: 12 * MB,
    connections: 6,
    segmentsDone: 2,
    segmentsTotal: 4,
    hashState: 'pending',
  })
  await expect(screen.getByText('12 MB/giây')).toBeVisible()
  await expect(screen.getByText('6/8')).toBeVisible()
  await expect(screen.getByText('220 MB')).toBeVisible()

  await emitEvent(page, 'utd://segments', {
    taskId: 't1',
    parts: [
      {
        id: 't1',
        role: 'file',
        fileName: 'ubuntu.iso',
        status: 'downloading',
        totalBytes: 400 * MB,
        downloadedBytes: 320 * MB,
        segments: segments([100 * MB, 100 * MB, 100 * MB, 20 * MB]),
      },
    ],
  })
  await expect(screen.getByText('3/4 đoạn xong')).toBeVisible()
  // Bản đồ đoạn của task khác không lọt vào.
  await emitEvent(page, 'utd://segments', { taskId: 'khac', parts: [] })
  await expect(screen.getByText('3/4 đoạn xong')).toBeVisible()

  await screen.getByText('Từng đoạn').click()
  await expect(screen.getByRole('cell', { name: '20 MB / 100 MB' })).toBeVisible()

  await screen.getByRole('button', { name: 'Tạm dừng' }).click()
  expect((await calls(page, 'pause_download')).at(-1)?.args).toEqual({ taskId: 't1' })
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  expect((await calls(page, 'watch_download')).at(-1)?.args).toEqual({ taskId: null })
})

test('S-15: mở bằng menu chuột phải "Chi tiết" và bằng phím Enter', async ({ page }) => {
  const running = task('downloading')
  await mockTauri(page, { list_downloads: [running], download_detail: detail(running) })
  await page.goto('/')
  const row = page.locator('tr[data-row-id="t1"]')
  await row.click({ button: 'right' })
  await page.getByRole('menuitem', { name: 'Chi tiết' }).click()
  await expect(page.locator('[data-s="S-15"]')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('dialog')).toBeHidden()

  await row.click()
  await page.keyboard.press('Enter')
  await expect(page.locator('[data-s="S-15"]')).toBeVisible()
})

test('S-15: task media gộp hình và tiếng thành một màn', async ({ page }) => {
  const media = task('downloading', {
    kind: 'media',
    fileName: 'ubuntu.iso',
    url: 'https://www.youtube.com/watch?v=x',
  })
  const part = (id: string, role: string, done: number[]) => ({
    id,
    role,
    fileName: `clip.${role}.mp4`,
    status: 'downloading',
    totalBytes: 400 * MB,
    downloadedBytes: done.reduce((sum, bytes) => sum + bytes, 0),
    segments: segments(done),
  })
  const dialog = await open(page, {
    list_downloads: [media],
    download_detail: detail(media, {
      parts: [part('v', 'video', [100 * MB, 100 * MB, 10 * MB, 0]), part('a', 'audio', [100 * MB, 100 * MB, 100 * MB, 100 * MB])],
      mediaTitle: 'Một clip',
    }),
  })
  const screen = dialog.locator('[data-s="S-15"]')
  await expect(screen.getByText('Hình', { exact: true })).toBeVisible()
  await expect(screen.getByText('Tiếng', { exact: true })).toBeVisible()
  await expect(screen.getByText('6/8 đoạn xong')).toBeVisible()
})

test('S-16: download không còn ⇒ báo và về danh sách', async ({ page }) => {
  const dialog = await open(page, { list_downloads: [task('paused')], download_detail: null })
  const screen = dialog.locator('[data-s="S-16"]')
  await expect(screen.getByText('Download này không còn')).toBeVisible()
  await screen.getByRole('button', { name: 'Về danh sách' }).click()
  await expect(dialog).toBeHidden()
})

test('S-17: đang đọc chi tiết', async ({ page }) => {
  const dialog = await open(page, { list_downloads: [task('paused')], download_detail: PENDING })
  await expect(dialog.locator('[data-s="S-17"]')).toContainText('Đang đọc chi tiết…')
})

test('S-18: không đọc được dữ liệu ⇒ thử lại được', async ({ page }) => {
  const paused = task('paused')
  const dialog = await open(page, {
    list_downloads: [paused],
    download_detail: failWith('database: disk I/O error'),
  })
  const screen = dialog.locator('[data-s="S-18"]')
  await expect(screen.getByText('Không đọc được dữ liệu')).toBeVisible()
  await expect(screen.getByText('database: disk I/O error')).toBeVisible()
  await respond(page, 'download_detail', detail(paused))
  await screen.getByRole('button', { name: 'Thử lại' }).click()
  await expect(dialog.locator('[data-s="S-19"]')).toBeVisible()
})

test('S-19: tạm dừng — dừng ở đâu, đoạn đã xong, lý do; tiếp tục', async ({ page }) => {
  const paused = task('paused')
  const dialog = await open(page, {
    list_downloads: [paused],
    download_detail: detail(paused, {
      parts: [{ ...detail(paused).parts[0], segments: segments([100 * MB, 50 * MB, 0, 0], false) }],
      events: [
        { at: AT, status: 'queued', downloadedBytes: 0 },
        { at: AT + 9_000, status: 'paused', code: 'app.interrupted', downloadedBytes: 150 * MB },
      ],
    }),
  })
  const screen = dialog.locator('[data-s="S-19"]')
  await expect(screen.getByText('UTD bị đóng giữa chừng')).toBeVisible()
  await expect(screen.getByText('1/4', { exact: true })).toBeVisible()
  await expect(screen.getByText('150 MB', { exact: true })).toBeVisible()
  await expect(screen.getByRole('button', { name: 'Huỷ tải' })).toBeVisible()
  await screen.getByRole('button', { name: 'Tiếp tục' }).click()
  expect((await calls(page, 'resume_download')).at(-1)?.args).toEqual({ taskId: 't1' })
})

test('S-20: hoàn tất — hash, đường dẫn, thời gian; mở file, chép đường dẫn', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  const done = task('completed', { hashState: 'verified', expectedSha256: 'a'.repeat(64) })
  const dialog = await open(page, { list_downloads: [done], download_detail: detail(done) })
  const screen = dialog.locator('[data-s="S-20"]')
  await expect(screen.getByText('SHA-256 khớp')).toBeVisible()
  await expect(screen.getByText(`${DIR}\\ubuntu.iso`)).toBeVisible()
  await expect(screen.getByText('2 phút', { exact: true })).toBeVisible()
  await screen.getByRole('button', { name: 'Sao chép đường dẫn' }).click()
  await expect(screen.getByRole('button', { name: 'Đã chép đường dẫn' })).toBeVisible()
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(`${DIR}\\ubuntu.iso`)
  await screen.getByRole('button', { name: 'Mở file' }).click()
  expect((await calls(page, 'open_download')).at(-1)?.args).toEqual({ taskId: 't1' })
})

test('S-20: không có SHA-256 thì nói rõ "chưa kiểm", file mất thì báo', async ({ page }) => {
  const done = task('completed')
  const dialog = await open(page, { list_downloads: [done], download_detail: detail(done, { fileExists: false }) })
  const screen = dialog.locator('[data-s="S-20"]')
  await expect(screen.getByText('Chưa kiểm hash')).toBeVisible()
  await expect(screen.getByText(/không khẳng định file an toàn/)).toBeVisible()
  await expect(screen.getByRole('status')).toContainText('File không còn ở đây')
  await expect(screen.getByRole('button', { name: 'Mở file' })).toBeDisabled()
})

test('S-21: lỗi — Doctor, bằng chứng, sao chép log, thử lại', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  const failed = task('failed', {
    error: { class: 'transient', code: 'network.closed', message: 'The download ended before all bytes arrived.' },
  })
  const dialog = await open(page, {
    list_downloads: [failed],
    download_detail: detail(failed, {
      events: [
        { at: AT, status: 'queued', downloadedBytes: 0 },
        { at: AT + 60_000, status: 'failed', code: 'http.503', downloadedBytes: 90 * MB },
        { at: AT + 120_000, status: 'failed', code: 'network.closed', downloadedBytes: 150 * MB },
      ],
    }),
    download_log: 'UTD download log\nerror: network.closed',
  })
  const screen = dialog.locator('[data-s="S-21"]')
  await expect(screen.getByRole('heading', { name: 'Mạng bị gián đoạn' })).toBeVisible()
  await expect(screen.getByText('network.closed', { exact: true })).toBeVisible()
  await expect(screen.getByText(/^2 \(.*http\.503\)$/)).toBeVisible()
  await expect(screen.getByText('cdn.example.com')).toBeVisible()
  await screen.getByRole('button', { name: 'Sao chép log' }).click()
  await expect(screen.getByRole('button', { name: 'Đã chép log' })).toBeVisible()
  expect(await page.evaluate(() => navigator.clipboard.readText())).toContain('network.closed')
  await screen.getByRole('button', { name: 'Thử lại' }).click()
  expect((await calls(page, 'resume_download')).at(-1)?.args).toEqual({ taskId: 't1' })
})

test('S-21: file bị cách ly không có nút Thử lại, việc chính là mở thư mục cách ly', async ({ page }) => {
  const held = task('quarantined', {
    fileName: 'ubuntu.iso',
    error: { class: 'security', code: 'guard.dangerous_type', message: 'quarantined' },
  })
  const dialog = await open(page, { list_downloads: [held], download_detail: detail(held) })
  const screen = dialog.locator('[data-s="S-21"]')
  await expect(screen.getByText('File bị Download Guard cách ly')).toBeVisible()
  await expect(screen.getByRole('button', { name: 'Thử lại' })).toHaveCount(0)
  await screen.getByRole('button', { name: 'Mở thư mục' }).click()
  expect((await calls(page, 'reveal_download')).at(-1)?.args).toEqual({ taskId: 't1' })
})

test('S-21: link bị từ chối ⇒ "Cập nhật link…" như Doctor, mở hộp cập nhật link', async ({ page }) => {
  const refused = task('failed', {
    error: { class: 'user', code: 'http.403', message: 'HTTP 403' },
    recovery: { wait: 'link', attempts: 0, progress: 150 * MB },
  })
  const dialog = await open(page, { list_downloads: [refused], download_detail: detail(refused) })
  const screen = dialog.locator('[data-s="S-21"]')
  await screen.getByRole('button', { name: 'Cập nhật link…' }).click()
  await expect(page.getByRole('dialog', { name: 'Cập nhật link' })).toBeVisible()
  // Hộp Chi tiết (tiêu đề là tên file) đã đóng; hộp Cập nhật link cũng mang data-s="S-21" nên tìm theo tên.
  await expect(page.getByRole('dialog', { name: 'ubuntu.iso' })).toBeHidden()
})

test('S-21: file trên máy chủ đã đổi ⇒ "Tải lại từ đầu", không có "Cập nhật link…"', async ({ page }) => {
  const changed = task('failed', {
    error: { class: 'permanent', code: 'file.changed', message: 'The file changed on the server.' },
  })
  const dialog = await open(page, { list_downloads: [changed], download_detail: detail(changed) })
  const screen = dialog.locator('[data-s="S-21"]')
  await expect(screen.getByRole('button', { name: 'Cập nhật link…' })).toHaveCount(0)
  await screen.getByRole('button', { name: 'Tải lại từ đầu' }).click()
  expect((await calls(page, 'restart_download')).at(-1)?.args).toEqual({ taskId: 't1' })
})

test('S-21: link là trang web ⇒ không có "Thử lại"; cách làm (cả Drive) và "Cập nhật link…"', async ({ page }) => {
  const notFile = task('failed', {
    url: 'https://drive.google.com/file/d/1hfaJWoyFptsG9niNHVfOB8m6snwbibpl/view',
    totalBytes: null,
    downloadedBytes: 0,
    error: { class: 'user', code: 'http.not_a_file', message: 'Link này là một trang web, không phải file.' },
  })
  const dialog = await open(page, { list_downloads: [notFile], download_detail: detail(notFile) })
  const screen = dialog.locator('[data-s="S-21"]')
  await expect(screen.getByRole('heading', { name: 'Link này là trang web, không phải file' })).toBeVisible()
  await expect(screen).toContainText('Google Drive: mở file ra trang riêng rồi thử lại.')
  await expect(screen.getByRole('button', { name: 'Thử lại' })).toHaveCount(0)
  await screen.getByRole('button', { name: 'Cập nhật link…' }).click()
  await expect(page.getByRole('dialog', { name: 'Cập nhật link' })).toBeVisible()
})

test('S-15: a11y — màn chi tiết đang tải', async ({ page }) => {
  const running = task('downloading')
  await open(page, { list_downloads: [running], download_detail: detail(running) })
  await expect(page.locator('[data-s="S-15"]')).toBeVisible()
  expect((await axe(page)).violations).toEqual([])
})

test('S-19: a11y — màn chi tiết tạm dừng', async ({ page }) => {
  const paused = task('paused')
  await open(page, { list_downloads: [paused], download_detail: detail(paused) })
  await expect(page.locator('[data-s="S-19"]')).toBeVisible()
  expect((await axe(page)).violations).toEqual([])
})

test('S-20: a11y — màn chi tiết hoàn tất', async ({ page }) => {
  const done = task('completed')
  await open(page, { list_downloads: [done], download_detail: detail(done) })
  await expect(page.locator('[data-s="S-20"]')).toBeVisible()
  expect((await axe(page)).violations).toEqual([])
})

test('S-21: a11y — màn chi tiết lỗi', async ({ page }) => {
  const failed = task('failed', {
    error: { class: 'transient', code: 'network.closed', message: 'closed' },
  })
  await open(page, { list_downloads: [failed], download_detail: detail(failed) })
  await expect(page.locator('[data-s="S-21"]')).toBeVisible()
  expect((await axe(page)).violations).toEqual([])
})
