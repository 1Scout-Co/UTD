const api = globalThis.browser ?? globalThis.chrome
const button = document.getElementById("send")
const status = document.getElementById("status")
const authCard = document.getElementById("auth")
const authSite = document.getElementById("auth-site")
const authUse = document.getElementById("auth-use")
const authSwitch = document.getElementById("auth-switch")
const authStatus = document.getElementById("auth-status")

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

// --- Đăng nhập (S-70, S-71; notes/auth-plan.md §8) ---
// Popup chỉ hỏi UTD trang này có việc chờ không; cookie chỉ được đọc sau khi người dùng bấm.
let pending = null

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
  authSwitch.checked = await api.permissions
    .contains({ permissions: ["cookies"], origins: [info.origin] })
    .catch(() => false)
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

/** Phải gọi ngay trong cú bấm (trước mọi `await` khác): trình duyệt chỉ hỏi quyền khi người dùng vừa bấm. */
function askPermission() {
  return api.permissions
    .request({ permissions: ["cookies"], origins: [pending.origin] })
    .catch(() => false)
}

authUse.addEventListener("click", async () => {
  const granted = await askPermission()
  if (!granted) {
    showAuthError("auth.denied")
    authSwitch.checked = false
    return
  }
  authSwitch.checked = true
  authUse.disabled = true
  await api.storage.local.set({ [`sensitive:${pending.site}`]: Boolean(pending.sensitive) })
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
  if (authSwitch.checked) {
    const granted = await askPermission()
    authSwitch.checked = granted
    if (!granted) {
      showAuthError("auth.denied")
      return
    }
    await api.storage.local.set({ [`sensitive:${pending.site}`]: Boolean(pending.sensitive) })
  } else {
    await api.permissions.remove({ origins: [pending.origin] }).catch(() => false)
  }
  authStatus.textContent = ""
  authStatus.className = ""
  if (!authUse.hidden) {
    authUse.textContent = authSwitch.checked ? t("authResume", [String(pending.pending)]) : t("authUse")
  }
})

void refresh()
void currentTab().then(loadAuth)
