// THỬ (prototype): extension MV3 đọc cookie bằng chrome.cookies.getAll trong Chromium của Playwright, profile tạm.
const path = require("path")
const { chromium } = require(process.argv[2])
const S = process.argv[3]
;(async () => {
  const profile = path.join(S, "run", "pw-profile-" + Date.now())
  const exts = ["ext-a", "ext-b", "ext-c"].map((d) => path.join(S, d)).join(",")
  const context = await chromium.launchPersistentContext(profile, {
    channel: "chromium",
    headless: true,
    args: [`--disable-extensions-except=${exts}`, `--load-extension=${exts}`],
  })
  const page = await context.newPage()
  await page.goto("http://127.0.0.1:8766/dologin")
  await page.evaluate(() => { document.cookie = "js_strict=1; SameSite=Strict; path=/"; document.cookie = "js_none=1; path=/files" })
  let workers = context.serviceWorkers()
  while (workers.length < 3) { await context.waitForEvent("serviceworker"); workers = context.serviceWorkers() }
  for (const worker of workers) {
    const result = await worker.evaluate(async () => {
      const name = chrome.runtime.getManifest().name
      const out = { name }
      try {
        const cookies = await chrome.cookies.getAll({ url: "http://127.0.0.1:8766/file.mp4" })
        out.cookies = cookies.map((c) => ({ n: c.name, len: c.value.length, httpOnly: c.httpOnly, sameSite: c.sameSite, hostOnly: c.hostOnly, path: c.path, store: c.storeId, session: c.session }))
        out.stores = (await chrome.cookies.getAllCookieStores()).map((s) => ({ id: s.id, tabs: s.tabIds.length }))
      } catch (error) { out.cookiesError = String(error.message || error) }
      try {
        const granted = await chrome.permissions.request({ permissions: ["cookies"], origins: ["http://127.0.0.1/*"] })
        out.request = granted
      } catch (error) { out.requestError = String(error.message || error) }
      return out
    })
    console.log(JSON.stringify(result))
  }
  await context.close()
})().catch((error) => { console.error(error); process.exit(1) })
