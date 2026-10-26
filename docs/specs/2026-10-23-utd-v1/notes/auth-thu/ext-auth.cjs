// THỬ (GĐ1): background.js thật của tiện ích UTD đọc cookie đúng luật. Bản chép tạm: bỏ `key` (khác ID nên
// native host thật từ chối), bỏ `nativeMessaging`, thay `sendNativeMessage` bằng hàm ghi lại tin nhắn.
// Bản "granted" khai sẵn quyền cookies + *://*.utd.test/* (như sau khi người dùng bấm Cho phép).
const fs = require("fs")
const path = require("path")
const { chromium } = require(process.argv[2])
const S = process.argv[3]
const SOURCE = process.argv[4]

function copyExtension(name, granted) {
  const target = path.join(S, "run", name)
  fs.rmSync(target, { recursive: true, force: true })
  fs.cpSync(SOURCE, target, { recursive: true })
  const manifestPath = path.join(target, "manifest.json")
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"))
  delete manifest.key
  manifest.permissions = manifest.permissions.filter((item) => item !== "nativeMessaging")
  if (granted) {
    manifest.permissions.push("cookies")
    manifest.host_permissions = ["*://*.utd.test/*"]
  }
  fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2))
  return target
}

async function run(granted) {
  const ext = copyExtension(granted ? "ext-auth-granted" : "ext-auth-plain", granted)
  const context = await chromium.launchPersistentContext(path.join(S, "run", `pw-auth-${granted}-${Date.now()}`), {
    channel: "chromium",
    headless: true,
    args: [`--disable-extensions-except=${ext}`, `--load-extension=${ext}`, "--host-resolver-rules=MAP *.utd.test 127.0.0.1"],
  })
  const page = await context.newPage()
  await page.goto("http://drive.utd.test:8767/domlogin")
  await page.evaluate(() => {
    document.cookie = "strict=1; SameSite=Strict; path=/"
  })
  let [worker] = context.serviceWorkers()
  if (!worker) worker = await context.waitForEvent("serviceworker")
  const result = await worker.evaluate(async () => {
    const sent = []
    chrome.runtime.sendNativeMessage = async (_host, payload) => {
      sent.push(payload)
      return { ok: true, taskId: "t", message: null }
    }
    const [tab] = await chrome.tabs.query({ url: "http://drive.utd.test/*" })
    const summary = (payload) => ({
      version: payload.version,
      site: payload.auth?.site ?? null,
      cookies: (payload.auth?.cookies ?? []).map((cookie) => `${cookie.name}@${cookie.domain}`).sort().join(" "),
      hasValue: Boolean(payload.auth?.cookies?.every((cookie) => typeof cookie.value === "string" && cookie.value.length > 0)),
      ua: Boolean(payload.auth?.userAgent),
    })
    // 1. Link cùng trang (tab ở drive.utd.test).
    await download({ linkUrl: "http://usercontent.utd.test:8767/file.mp4" }, tab)
    // 2. Link utd.test nhưng trang đang mở ở trang khác: bỏ cookie SameSite=Strict.
    await download({ linkUrl: "http://drive.utd.test:8767/file.mp4" }, { ...tab, url: "http://other.example/page" })
    // 3. Trang nhạy cảm đã đánh dấu: không kèm cookie.
    await chrome.storage.local.set({ "sensitive:utd.test": true })
    await download({ linkUrl: "http://drive.utd.test:8767/file.mp4" }, tab)
    await chrome.storage.local.remove("sensitive:utd.test")
    // 4. Link trang chưa cho phép: không đọc cookie.
    await download({ linkUrl: "http://127.0.0.1:8767/file.mp4" }, tab)
    return sent.map(summary)
  })
  await context.close()
  return result
}

;(async () => {
  console.log("CHUA CHO PHEP:", JSON.stringify(await run(false)))
  console.log("DA CHO PHEP:")
  for (const line of await run(true)) console.log("  ", JSON.stringify(line))
})().catch((error) => {
  console.error(error)
  process.exit(1)
})
