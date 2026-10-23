const api = globalThis.browser ?? globalThis.chrome
const button = document.getElementById("send")
const status = document.getElementById("status")

async function currentTab() {
  const tabs = await api.tabs.query({ active: true, currentWindow: true })
  return tabs[0]
}

async function refresh() {
  const stored = await api.storage.local.get(["lastResult", "lastAt"])
  if (!stored.lastAt || Date.now() - stored.lastAt > 60_000) return
  if (stored.lastResult === "ok") {
    status.textContent = "Đã gửi sang UTD."
    status.className = "ok"
  } else if (stored.lastResult) {
    status.textContent = stored.lastResult
    status.className = "error"
  }
}

button.addEventListener("click", async () => {
  button.disabled = true
  status.className = ""
  status.textContent = "Đang gửi…"
  try {
    const tab = await currentTab()
    const url = tab?.url
    if (!url || !/^https?:\/\//i.test(url)) {
      throw new Error("Trang này không phải HTTP/HTTPS.")
    }
    const response = await api.runtime.sendMessage({ type: "utd.send-page", url })
    if (!response?.ok) throw new Error(response?.message || "Không gửi được.")
    status.textContent = "Đã gửi sang UTD."
    status.className = "ok"
  } catch (error) {
    status.textContent =
      error?.message ||
      "Không kết nối được UTD. Mở UTD và bật tích hợp trình duyệt."
    status.className = "error"
  } finally {
    button.disabled = false
  }
})

void refresh()
