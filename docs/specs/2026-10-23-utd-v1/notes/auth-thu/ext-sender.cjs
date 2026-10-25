// THỬ: background.js chỉ nhận "utd.send-page" từ popup thật; từ tab mở popup.html hay content script thì bỏ.
// Bản chép tạm KHÔNG có `key` và `nativeMessaging` => không bao giờ chạm native host thật của máy.
const path = require("path")
const { chromium } = require(process.argv[2])
const S = process.argv[3]
;(async () => {
  const ext = path.join(S, "ext-real")
  const context = await chromium.launchPersistentContext(path.join(S, "run", "pw-sender-" + Date.now()), {
    channel: "chromium", headless: true,
    args: [`--disable-extensions-except=${ext}`, `--load-extension=${ext}`],
  })
  let [worker] = context.serviceWorkers()
  if (!worker) worker = await context.waitForEvent("serviceworker")
  const id = new URL(worker.url()).host
  const send = (page) => page.evaluate(async () => {
    try { return JSON.stringify(await chrome.runtime.sendMessage({ type: "utd.send-page", url: "https://example.com/" })) ?? "undefined" }
    catch (error) { return "error: " + error.message }
  })
  // 1. popup.html mở như một tab: có sender.tab => phải bị bỏ.
  const tab = await context.newPage()
  await tab.goto(`chrome-extension://${id}/popup.html`)
  console.log("tab mo popup.html:", await send(tab))
  // 2. Popup thật qua action.openPopup().
  const popupPromise = context.waitForEvent("page", { timeout: 5000 }).catch(() => null)
  const opened = await worker.evaluate(async () => { try { await chrome.action.openPopup(); return "ok" } catch (e) { return "error: " + e.message } })
  console.log("openPopup:", opened)
  await popupPromise
  // Popup không hiện thành page của Playwright: gắn CDP vào target của nó (khác target của tab ở bước 1).
  const cdp = await context.newCDPSession(tab)
  const { targetInfos } = await cdp.send("Target.getTargets")
  const tabTarget = (await cdp.send("Target.getTargetInfo")).targetInfo.targetId
  const target = targetInfos.find((info) => info.url.endsWith("/popup.html") && info.targetId !== tabTarget)
  if (!target) { console.log("khong thay target popup:", targetInfos.map((i) => i.type + " " + i.url).join(" | ")) }
  else {
    const { sessionId } = await cdp.send("Target.attachToTarget", { targetId: target.targetId, flatten: false })
    const reply = new Promise((resolve) => cdp.on("Target.receivedMessageFromTarget", (event) => {
      const message = JSON.parse(event.message)
      if (message.id === 7) resolve(message.result?.result?.value ?? JSON.stringify(message))
    }))
    const expression = `chrome.runtime.sendMessage({ type: "utd.send-page", url: "https://example.com/" }).then((r) => JSON.stringify(r) ?? "undefined", (e) => "error: " + e.message)`
    await cdp.send("Target.sendMessageToTarget", { sessionId, message: JSON.stringify({ id: 7, method: "Runtime.evaluate", params: { expression, awaitPromise: true, returnByValue: true } }) })
    console.log("popup that (" + target.type + "):", await reply)
  }
  await context.close()
})().catch((error) => { console.error(error); process.exit(1) })
