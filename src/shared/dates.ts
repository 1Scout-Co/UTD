// Định dạng ngày dùng chung (giây Unix ⇒ chữ), theo giờ máy.

/** `2026-10-24`: đặt đầu tên file cho dễ sắp xếp, cũng là giá trị của ô `<input type="date">`. */
export function isoDay(seconds: number): string {
  const date = new Date(seconds * 1000)
  const pad = (value: number) => String(value).padStart(2, "0")
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
}

/** Giây Unix lúc 00:00 (giờ máy) của ngày `YYYY-MM-DD`; sai định dạng ⇒ null. */
export function dayStart(day: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day)
  if (!match) return null
  const date = new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]))
  return Number.isNaN(date.getTime()) ? null : Math.floor(date.getTime() / 1000)
}
