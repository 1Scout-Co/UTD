const api = globalThis.browser ?? globalThis.chrome
const button = document.getElementById("send")
const status = document.getElementById("status")
const authCard = document.getElementById("auth")
const authSite = document.getElementById("auth-site")
const authUse = document.getElementById("auth-use")
const authSwitch = document.getElementById("auth-switch")
const authStatus = document.getElementById("auth-status")
const videoSwitch = document.getElementById("video-switch")

// Chữ theo ngôn ngữ của trình duyệt (_locales/<mã>/messages.json, mặc định en); tiếng Ả Rập viết phải→trái.
const t = (key, substitutions) => api.i18n.getMessage(key, substitutions) || key
document.documentElement.lang = api.i18n.getUILanguage()
document.documentElement.dir = api.i18n.getMessage("@@bidi_dir") || "ltr"
for (const element of document.querySelectorAll("[data-i18n]")) {
  element.textContent = t(element.dataset.i18n)
}

// Mã lỗi đăng nhập UTD trả về ⇒ câu của tiện ích.
const AUTH_ERRORS = {
  "auth.disabled": "authDisabled",
  "auth.denied": "authDenied",
  "auth.sensitive": "authDenied",
  "auth.site_mismatch": "authDenied",
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
    const response = await api.runtime.sendMessage({ type: "utd.send-page", url, tabId: tab.id })
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

// --- Nút "Tải video này" trên trang (GĐ3): bật/tắt, mặc định bật. ---
api.storage.local.get("videoButton").then(
  (stored) => {
    videoSwitch.checked = stored.videoButton !== false
  },
  () => undefined,
)
videoSwitch.addEventListener("change", () => {
  void api.storage.local.set({ videoButton: videoSwitch.checked })
})

// --- Tự bắt link tải (S-76, takeover.js): bật/tắt (mặc định bật), ngưỡng MB, đuôi không bắt. ---
const takeoverSwitch = document.getElementById("takeover-switch")
const takeoverMin = document.getElementById("takeover-min")
const takeoverSkip = document.getElementById("takeover-skip")
api.storage.local.get(["takeover", "takeoverMinMb", "takeoverSkip"]).then(
  (stored) => {
    takeoverSwitch.checked = stored.takeover !== false
    takeoverMin.value = String(stored.takeoverMinMb ?? 1)
    takeoverSkip.value = stored.takeoverSkip ?? ""
  },
  () => undefined,
)
takeoverSwitch.addEventListener("change", () => {
  void api.storage.local.set({ takeover: takeoverSwitch.checked })
})
takeoverMin.addEventListener("change", () => {
  const megabytes = Number(takeoverMin.value)
  if (Number.isFinite(megabytes) && megabytes >= 0) void api.storage.local.set({ takeoverMinMb: megabytes })
})
takeoverSkip.addEventListener("change", () => {
  void api.storage.local.set({ takeoverSkip: takeoverSkip.value.slice(0, 500) })
})

// --- Đăng nhập (S-70, S-71; notes/auth-plan.md §8, Q11) ---
// Quyền đã xin một lần lúc cài; popup chỉ hỏi UTD trang này có việc chờ không. Cookie chỉ đọc khi người dùng bấm.
// Công tắc "Đăng nhập trên trang này" (mặc định bật) tắt riêng một trang: `signin-off:<trang>`.
let pending = null
const offKey = (site) => `signin-off:${site}`

function showAuthError(code) {
  authStatus.textContent = t(AUTH_ERRORS[code] ?? "authDenied")
  authStatus.className = "error"
  authCard.dataset.s = "S-71"
}

async function loadAuth(tab) {
  if (!tab?.url || !/^https?:\/\//i.test(tab.url)) return
  const reply = await api.runtime.sendMessage({ type: "utd.auth-pending", url: tab.url }).catch(() => null)
  const info = reply?.ok ? reply.auth : null
  if (!info || !info.enabled) return
  pending = { ...info, tabId: tab.id }
  authSite.textContent = info.site
  authCard.hidden = false
  const key = offKey(info.site)
  authSwitch.checked = !(await api.storage.local.get(key).catch(() => ({})))[key]
  if (info.sensitive) {
    authStatus.textContent = t("authSensitive")
  }
  if (info.pending > 0) {
    authUse.hidden = false
    authUse.textContent = authSwitch.checked ? t("authResume", [String(info.pending)]) : t("authUse")
    // Một nút chính mỗi màn: nút gửi trang lùi xuống nút phụ.
    button.className = "secondary"
  }
}

// Bấm "Dùng đăng nhập" / "Tiếp tục" là đồng ý dùng đăng nhập ở trang này (cả trang nhạy cảm): bật lại nếu đã tắt.
authUse.addEventListener("click", async () => {
  authSwitch.checked = true
  authUse.disabled = true
  await api.storage.local.remove(offKey(pending.site))
  const reply = await api.runtime
    .sendMessage({ type: "utd.auth-renew", tabId: pending.tabId, site: pending.site, urls: pending.urls })
    .catch(() => null)
  authUse.disabled = false
  if (!reply?.ok) {
    showAuthError(reply?.message)
    return
  }
  authUse.hidden = true
  button.className = ""
  authStatus.textContent = t("authResumed")
  authStatus.className = "ok"
})

authSwitch.addEventListener("change", async () => {
  if (authSwitch.checked) await api.storage.local.remove(offKey(pending.site))
  else await api.storage.local.set({ [offKey(pending.site)]: true })
  authStatus.textContent = ""
  authStatus.className = ""
  if (!authUse.hidden) {
    authUse.textContent = authSwitch.checked ? t("authResume", [String(pending.pending)]) : t("authUse")
  }
})

void refresh()
void currentTab().then(loadAuth)
