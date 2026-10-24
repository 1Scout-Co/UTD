const api = globalThis.browser ?? globalThis.chrome
const button = document.getElementById("send")
const status = document.getElementById("status")

// Chữ theo ngôn ngữ của trình duyệt (_locales/<mã>/messages.json, mặc định en); tiếng Ả Rập viết phải→trái.
const t = (key) => api.i18n.getMessage(key) || key
document.documentElement.lang = api.i18n.getUILanguage()
document.documentElement.dir = api.i18n.getMessage("@@bidi_dir") || "ltr"
for (const element of document.querySelectorAll("[data-i18n]")) {
  element.textContent = t(element.dataset.i18n)
}

async function currentTab() {
  const tabs = await api.tabs.query({ active: true, currentWindow: true })
  return tabs[0]
}

async function refresh() {
  const stored = await api.storage.local.get(["lastResult", "lastNote", "lastAt"])
  if (!stored.lastAt || Date.now() - stored.lastAt > 60_000) return
  if (stored.lastResult === "ok") {
    status.textContent = stored.lastNote || t("sent")
    status.className = "ok"
  } else if (stored.lastResult) {
    status.textContent = stored.lastResult
    status.className = "error"
  }
}

button.addEventListener("click", async () => {
  button.disabled = true
  status.className = ""
  status.textContent = t("sending")
  try {
    const tab = await currentTab()
    const url = tab?.url
    if (!url || !/^https?:\/\//i.test(url)) {
      throw new Error(t("notHttp"))
    }
    const response = await api.runtime.sendMessage({ type: "utd.send-page", url })
    if (!response?.ok) throw new Error(response?.message || t("sendFailed"))
    status.textContent = response.message || t("sent")
    status.className = "ok"
  } catch (error) {
    status.textContent = error?.message || t("notConnected")
    status.className = "error"
  } finally {
    button.disabled = false
  }
})

void refresh()
