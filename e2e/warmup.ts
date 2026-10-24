import { chromium, type FullConfig } from '@playwright/test'

// Vite dev lần đầu phải quét và gom thư viện (~35 s trên máy dev, CI có thể lâu hơn) rồi mới trả trang, quá hạn
// 30 s của một bài thử ⇒ bài đầu của mỗi worker hết giờ ở page.goto. Mở trang một lần trước cả bộ, chờ lâu, cho
// mọi worker dùng bản đã gom sẵn.
export default async function warmup(config: FullConfig) {
  const baseURL = config.projects[0]?.use.baseURL
  if (!baseURL) return
  const browser = await chromium.launch()
  try {
    const page = await browser.newPage()
    await page.goto(baseURL, { timeout: 180_000 })
    await page.locator('#root > *').first().waitFor({ timeout: 180_000 })
  } finally {
    await browser.close()
  }
}
