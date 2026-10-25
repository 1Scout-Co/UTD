// THỬ: quyền host một tên (drive.utd.test) có đọc được cookie Domain=utd.test không, so với quyền *.utd.test.
const path = require("path")
const { chromium } = require(process.argv[2])
const S = process.argv[3]
;(async () => {
  const profile = path.join(S, "run", "pw-profile-dom-" + Date.now())
  const exts = ["ext-d", "ext-e"].map((d) => path.join(S, d)).join(",")
  const context = await chromium.launchPersistentContext(profile, {
    channel: "chromium", headless: true,
    args: [`--disable-extensions-except=${exts}`, `--load-extension=${exts}`, "--host-resolver-rules=MAP *.utd.test 127.0.0.1"],
  })
  const page = await context.newPage()
  await page.goto("http://drive.utd.test:8767/domlogin")
  console.log("page cookies:", (await context.cookies()).map((c) => `${c.name}@${c.domain}`).join(" "))
  let workers = context.serviceWorkers()
  while (workers.length < 2) { await context.waitForEvent("serviceworker"); workers = context.serviceWorkers() }
  for (const worker of workers) {
    console.log(JSON.stringify(await worker.evaluate(async () => {
      const name = chrome.runtime.getManifest().name
      const read = async (url) => (await chrome.cookies.getAll({ url })).map((c) => `${c.name}@${c.domain}`).join(" ") || "(rỗng)"
      return { name, drive: await read("http://drive.utd.test:8767/file.mp4"), usercontent: await read("http://usercontent.utd.test:8767/x") }
    })))
  }
  await context.close()
})().catch((error) => { console.error(error); process.exit(1) })
