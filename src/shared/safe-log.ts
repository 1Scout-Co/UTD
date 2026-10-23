// safeLog: cách DUY NHẤT để app ghi nhật ký. Chỉ các trường trong danh sách cho phép (LOG_ALLOW) được ghi; mọi thứ khác bị che
// (không ghi cả tên lẫn giá trị, chỉ đếm trong "che"). Mật khẩu, token, khoá, email, chữ người dùng nhập KHÔNG có trong danh sách
// ⇒ không bao giờ vào log. Máy kiểm `log-secrets` của flow-check quét log của bài thử: thấy bí mật lọt ra là đỏ.
//
// Thêm trường vào LOG_ALLOW chỉ khi giá trị chắc chắn không chứa bí mật hay dữ liệu cá nhân (mã trạng thái, số lượng, tên màn…);
// hỏi người dùng trước. Không gọi console.log thẳng với dữ liệu của người dùng.
//
// Dùng: safeLog('order_saved', { feature: 'orders', status: 201, durationMs: 42 })
//   → {"event":"order_saved","feature":"orders","status":201,"durationMs":42}
//   safeLog('login_failed', { email, password }) → {"event":"login_failed","che":2}
export const LOG_ALLOW = ['level', 'feature', 'route', 'method', 'status', 'code', 'durationMs', 'count', 'requestId', 'errorName'] as const

export type LogFields = Record<string, unknown>
type LogValue = string | number | boolean | null

const MASK = '[che]'
const MAX_TEXT = 200
const EVENT_NAME = /^[\w.:-]{1,60}$/
// Lớp thứ hai: trường ĐƯỢC PHÉP mà giá trị chữ trông như bí mật (JWT, "Bearer …", email, chuỗi dài ngẫu nhiên) vẫn bị che.
const LOOKS_SECRET = /eyJ[\w-]{10,}\.|bearer\s|[\w.+-]+@[\w-]+\.\w|[A-Za-z0-9_-]{40,}/i

// Chỉ chữ, số, true/false, null đi qua; object, mảng, hàm… không ghi được (undefined = không có giá trị, bỏ qua).
function clean(value: unknown): LogValue | undefined {
  if (value === null || typeof value === 'boolean') return value
  if (typeof value === 'number') return Number.isFinite(value) ? value : undefined
  if (typeof value === 'string') return LOOKS_SECRET.test(value) ? MASK : value.slice(0, MAX_TEXT)
  return undefined
}

export function redact(fields: LogFields, allow: readonly string[] = LOG_ALLOW): { kept: Record<string, LogValue>; hidden: number } {
  const kept: Record<string, LogValue> = {}
  let hidden = 0
  for (const [key, value] of Object.entries(fields)) {
    if (value === undefined) continue
    const ok = allow.includes(key) ? clean(value) : undefined
    if (ok === undefined) hidden++
    else kept[key] = ok
  }
  return { kept, hidden }
}

export function safeLog(event: string, fields: LogFields = {}, allow: readonly string[] = LOG_ALLOW): void {
  const { kept, hidden } = redact(fields, allow)
  const line = JSON.stringify({ event: EVENT_NAME.test(event) && !LOOKS_SECRET.test(event) ? event : MASK, ...kept, ...(hidden ? { che: hidden } : {}) })
  if (kept.level === 'error') console.error(line)
  else if (kept.level === 'warn') console.warn(line)
  else console.log(line)
}
