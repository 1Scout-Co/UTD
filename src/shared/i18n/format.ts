// Định dạng số, dung lượng, tốc độ, thời gian, ngày theo ngôn ngữ đang chọn (Intl). Đơn vị cũng theo
// ngôn ngữ: "19,4 GB" (vi), "19.4 GB" (en), "19,4 Go" (fr), "19,4 ГБ" (ru).

let locale = "vi-VN"
const numberFormats = new Map<string, Intl.NumberFormat>()
const dateFormats = new Map<string, Intl.DateTimeFormat>()

/** i18n gọi khi đổi ngôn ngữ. */
export function setFormatLocale(next: string) {
  locale = next
}

function numberFormat(options: Intl.NumberFormatOptions): Intl.NumberFormat {
  const key = `${locale}|${JSON.stringify(options)}`
  let format = numberFormats.get(key)
  if (!format) {
    format = new Intl.NumberFormat(locale, options)
    numberFormats.set(key, format)
  }
  return format
}

function dateFormat(options: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  const key = `${locale}|${JSON.stringify(options)}`
  let format = dateFormats.get(key)
  if (!format) {
    format = new Intl.DateTimeFormat(locale, options)
    dateFormats.set(key, format)
  }
  return format
}

export function formatNumber(value: number, digits = 1): string {
  return numberFormat({ maximumFractionDigits: digits }).format(value)
}

const BYTE_UNITS = [
  "byte",
  "kilobyte",
  "megabyte",
  "gigabyte",
  "terabyte",
  "petabyte",
]

/** Dung lượng theo bậc 1024, nhãn KB/MB/GB như Explorer. */
export function formatBytes(bytes: number): string {
  const size = Number.isFinite(bytes) && bytes > 0 ? bytes : 0
  const exponent =
    size === 0
      ? 0
      : Math.min(
          BYTE_UNITS.length - 1,
          Math.floor(Math.log(size) / Math.log(1024))
        )
  return numberFormat({
    style: "unit",
    unit: BYTE_UNITS[exponent],
    maximumFractionDigits: 1,
  }).format(size / 1024 ** exponent)
}

/** Tốc độ tải tính bằng MB/s (bậc 1024). */
export function formatSpeed(megabytesPerSecond: number): string {
  return numberFormat({
    style: "unit",
    unit: "megabyte-per-second",
    maximumFractionDigits: 1,
  }).format(megabytesPerSecond)
}

/** Bitrate media: "128 kbps" (vi) · "128 kb/s" (fr). */
export function formatBitrate(kilobitsPerSecond: number): string {
  return numberFormat({
    style: "unit",
    unit: "kilobit-per-second",
    maximumFractionDigits: 0,
  }).format(kilobitsPerSecond)
}

/** Thời gian còn lại: "18 phút", "1 giờ 5 phút" (vi) · "1 hr 5 min" (en). */
export function formatMinutes(total: number): string {
  const minutes = numberFormat({ style: "unit", unit: "minute" })
  if (total < 60) return minutes.format(total)
  const hours = numberFormat({ style: "unit", unit: "hour" }).format(
    Math.floor(total / 60)
  )
  return `${hours} ${minutes.format(total % 60)}`
}

/** Khoảng thời gian (ms): "42 giây", "3 phút 5 giây", "1 giờ 5 phút" (vi) · "3 min 5 sec" (en). */
export function formatDuration(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000))
  const unit = (value: number, name: "second" | "minute" | "hour") =>
    numberFormat({ style: "unit", unit: name }).format(value)
  if (total < 60) return unit(total, "second")
  if (total < 3600) {
    const seconds = total % 60
    const minutes = unit(Math.floor(total / 60), "minute")
    return seconds ? `${minutes} ${unit(seconds, "second")}` : minutes
  }
  return formatMinutes(Math.round(total / 60))
}

/** `value` là % (0–100). */
export function formatPercent(value: number): string {
  return numberFormat({ style: "percent", maximumFractionDigits: 0 }).format(
    value / 100
  )
}

export function formatDate(time: number): string {
  return dateFormat({
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
  }).format(time)
}

export function formatTime(time: number): string {
  return dateFormat({
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(time)
}

/** Số gọn: "1,2 N" (vi), "1.2K" (en) — lượt xem, người theo dõi. */
export function formatCompact(value: number): string {
  return numberFormat({ notation: "compact", maximumFractionDigits: 1 }).format(
    value
  )
}

/** Ghép vài mục thành một câu liệt kê theo ngôn ngữ: "a, b và c" / "a, b, and c". */
export function formatList(items: string[]): string {
  return new Intl.ListFormat(locale, { style: "long", type: "conjunction" }).format(
    items
  )
}

/** So sánh tên theo cách xếp chữ của ngôn ngữ đang chọn. */
export function compareText(left: string, right: string): number {
  return left.localeCompare(right, locale)
}
