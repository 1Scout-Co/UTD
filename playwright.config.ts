import { defineConfig, devices } from '@playwright/test'

// Cổng lấy từ biến PORT (mặc định 5173), cùng cách với vite.config.ts: chạy song song nhiều bản thì mỗi bản một PORT.
const port = Number(process.env.PORT) || 5173

// Bài thử nhanh sau phát hành (`playwright test --grep @smoke`) chạy vào bản đã đưa lên mạng: BASE_URL = địa chỉ bản đó,
// SMOKE_EXTRA_HTTP_HEADERS = JSON header thêm cho mọi yêu cầu (vd header chọn bản chưa mở cho khách). Không đặt BASE_URL
// ⇒ chạy vào bản trên máy như thường (tự bật pnpm dev).
const baseURL = process.env.BASE_URL || `http://localhost:${port}`
const extraHTTPHeaders = process.env.SMOKE_EXTRA_HTTP_HEADERS ? (JSON.parse(process.env.SMOKE_EXTRA_HTTP_HEADERS) as Record<string, string>) : undefined

export default defineConfig({
  testDir: 'e2e',
  reporter: 'list',
  globalSetup: './e2e/warmup.ts',
  use: { baseURL, extraHTTPHeaders },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: process.env.BASE_URL
    ? undefined
    : {
        command: 'pnpm dev --strictPort',
        url: `http://localhost:${port}`,
        reuseExistingServer: true,
      },
})
