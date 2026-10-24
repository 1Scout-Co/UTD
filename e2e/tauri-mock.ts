import type { Page } from '@playwright/test'

// Giả app desktop ngay trong trình duyệt: `invoke` trả dữ liệu cố định (lệnh lạ trả null), `listen`
// ghi lại người nghe để bài thử bắn sự kiện như app thật (`emitEvent`). Mỗi lệnh đã gọi được lưu ở
// `window.__utdCalls` để kiểm. Chỉ dùng cho e2e; app thật đi qua Tauri.

type Responses = Record<string, unknown>

declare global {
  interface Window {
    __utdEmit?: (event: string, payload: unknown) => void
    __utdCalls?: Array<{ cmd: string; args: unknown }>
    __utdResponses?: Responses
  }
}

const DEFAULTS: Responses = {
  list_downloads: [],
  default_download_dir: 'C:\\Users\\utd\\Downloads\\UTD',
  disk_space: { freeBytes: 200e9, totalBytes: 500e9, label: 'Ổ C:' },
  take_media_request: null,
  take_batch_request: null,
  queue_settings: { limit: 3, maxLimit: 10 },
  get_update_check: { ready: true, enabled: true, available: null, installing: false },
  'plugin:app|version': '0.1.0',
  browser_integration_status: {
    installed: false,
    hostPath: '',
    manifestPath: '',
    chromiumExtensionId: 'x',
    firefoxExtensionId: 'x',
    extensionDir: '',
  },
}

export async function mockTauri(page: Page, responses: Responses) {
  await page.addInitScript((table: Responses) => {
    const callbacks = new Map<number, (message: unknown) => void>()
    const listeners = new Map<string, number[]>()
    let next = 1
    window.__utdCalls = []
    window.__utdResponses = table
    const host = window as unknown as Record<string, unknown>
    host.isTauri = true
    host.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined }
    host.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: 'main' },
        currentWebview: { windowLabel: 'main', label: 'main' },
      },
      transformCallback: (callback: (message: unknown) => void) => {
        const id = next++
        callbacks.set(id, callback)
        return id
      },
      unregisterCallback: (id: number) => callbacks.delete(id),
      invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
        window.__utdCalls?.push({ cmd, args })
        if (cmd === 'plugin:event|listen') {
          const event = String(args.event)
          listeners.set(event, [...(listeners.get(event) ?? []), Number(args.handler)])
          return args.handler
        }
        return window.__utdResponses?.[cmd] ?? null
      },
    }
    window.__utdEmit = (event, payload) => {
      for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id, payload })
    }
  }, { ...DEFAULTS, ...responses })
}

/** Bắn sự kiện như app (`utd://…`). */
export async function emitEvent(page: Page, event: string, payload: unknown) {
  await page.evaluate(([name, value]) => window.__utdEmit?.(name as string, value), [event, payload] as const)
}

/** Đổi kết quả của một lệnh giữa chừng bài thử. */
export async function respond(page: Page, cmd: string, value: unknown) {
  await page.evaluate(([name, result]) => {
    if (window.__utdResponses) window.__utdResponses[name as string] = result
  }, [cmd, value] as const)
}

export async function calls(page: Page, cmd: string) {
  return page.evaluate((name) => (window.__utdCalls ?? []).filter((call) => call.cmd === name), cmd)
}
