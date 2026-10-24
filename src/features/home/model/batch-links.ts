// Danh sách link của hộp Thêm hàng loạt (S-35): tách link từ chữ dán / file .txt (mỗi dòng một link, bỏ
// trùng, bỏ dòng rác) và mở mẫu kiểu IDM `https://site/img[001-120].jpg`, `[a-z]` (chặn mẫu quá lớn).
// App kiểm lại lần nữa (src-tauri/src/batch.rs): ở đây chỉ để xem trước cho đúng.

import { fileNameFromUrl } from "./download-mapper"
import { isMediaPage } from "./media-pages"

/** Trần một lần thêm, bằng `MAX_BATCH_ITEMS` của app. */
const MAX_BATCH_LINKS = 10_000
const MAX_URL_LENGTH = 8192

/** Link http(s) dùng được (như `validate_url` của engine: không kèm tên/mật khẩu); trả dạng chuẩn. */
function normalizeLink(raw: string): string | null {
  let value = raw.trim().replace(/^[<"']+|[>"',;]+$/g, "")
  // "(xem https://a/b.zip)": bỏ ngoặc bọc ngoài, giữ ngoặc của chính link (`…/Foo_(bar)`).
  if (value.startsWith("(")) value = value.slice(1)
  if (value.endsWith(")") && !value.includes("(")) value = value.slice(0, -1)
  if (!value || value.length > MAX_URL_LENGTH) return null
  try {
    const url = new URL(value)
    if (url.protocol !== "http:" && url.protocol !== "https:") return null
    if (url.username || url.password || !url.hostname) return null
    return url.href
  } catch {
    return null
  }
}

export interface ParsedLinks {
  links: string[]
  /** Link đã có (trong lần dán này hoặc trong danh sách) nên bỏ. */
  duplicates: number
  /** Dòng có chữ nhưng không có link http(s). */
  junk: number
  /** Link vượt trần `MAX_BATCH_LINKS` bị bỏ. */
  over: number
}

/** Mỗi dòng một link (dòng có nhiều link cách nhau bằng khoảng trắng cũng nhận); giữ thứ tự. */
export function parseLinks(text: string, known: readonly string[] = []): ParsedLinks {
  const seen = new Set(known)
  const result: ParsedLinks = { links: [], duplicates: 0, junk: 0, over: 0 }
  for (const line of text.split(/\r?\n/)) {
    if (!line.trim()) continue
    const found = line.split(/\s+/).map(normalizeLink).filter((link) => link !== null)
    if (found.length === 0) result.junk += 1
    for (const link of found) {
      if (seen.has(link)) result.duplicates += 1
      else if (seen.size >= MAX_BATCH_LINKS) result.over += 1
      else {
        seen.add(link)
        result.links.push(link)
      }
    }
  }
  return result
}

interface PatternRange {
  start: number
  end: number
  /** Số chữ số khi đệm 0 (`[001-120]` ⇒ 3); 0 = không đệm. */
  width: number
  letters: boolean
}

// [001-120] hoặc [a-z] / [A-Z] (cùng kiểu chữ). Ngoặc khác (vd. IPv6 `[::1]`) giữ nguyên.
const RANGE = /\[(\d{1,9})-(\d{1,9})\]|\[([a-z])-([a-z])\]|\[([A-Z])-([A-Z])\]/g

function rangesOf(pattern: string): { parts: string[]; ranges: PatternRange[] } {
  const parts: string[] = []
  const ranges: PatternRange[] = []
  let last = 0
  for (const match of pattern.matchAll(RANGE)) {
    parts.push(pattern.slice(last, match.index))
    last = match.index + match[0].length
    if (match[1] !== undefined && match[2] !== undefined) {
      const padded = match[1].length > 1 && match[1].startsWith("0")
      ranges.push({
        start: Number(match[1]),
        end: Number(match[2]),
        width: padded ? match[1].length : 0,
        letters: false,
      })
    } else {
      const from = match[3] ?? match[5] ?? "a"
      const to = match[4] ?? match[6] ?? "a"
      ranges.push({ start: from.charCodeAt(0), end: to.charCodeAt(0), width: 0, letters: true })
    }
  }
  parts.push(pattern.slice(last))
  return { parts, ranges }
}

const sizeOf = (range: PatternRange) => Math.abs(range.end - range.start) + 1

/** Số link mẫu sẽ tạo (0 = chưa có `[..]`). Dừng đếm khi đã vượt trần, khỏi tràn số. */
export function patternCount(pattern: string): number {
  const { ranges } = rangesOf(pattern.trim())
  if (ranges.length === 0) return 0
  let count = 1
  for (const range of ranges) {
    count *= sizeOf(range)
    if (count > MAX_BATCH_LINKS) return count
  }
  return count
}

function valueAt(range: PatternRange, index: number): string {
  const step = range.end >= range.start ? 1 : -1
  const value = range.start + index * step
  if (range.letters) return String.fromCharCode(value)
  return range.width ? String(value).padStart(range.width, "0") : String(value)
}

export type PatternResult =
  | { ok: true; links: string[] }
  | { ok: false; error: string }

/** Mở mẫu thành danh sách link theo thứ tự (ngoặc đầu đổi chậm nhất). */
export function expandPattern(pattern: string): PatternResult {
  const { parts, ranges } = rangesOf(pattern.trim())
  const count = patternCount(pattern)
  if (count === 0) return { ok: false, error: "Thêm [001-120] hoặc [a-z] vào link." }
  if (count > MAX_BATCH_LINKS) {
    return { ok: false, error: `Mẫu quá lớn: tối đa ${MAX_BATCH_LINKS.toLocaleString("vi-VN")} link.` }
  }
  const links: string[] = []
  for (let index = 0; index < count; index += 1) {
    let rest = index
    const values = new Array<string>(ranges.length)
    for (let position = ranges.length - 1; position >= 0; position -= 1) {
      const range = ranges[position]
      values[position] = valueAt(range, rest % sizeOf(range))
      rest = Math.floor(rest / sizeOf(range))
    }
    const link = normalizeLink(parts.map((part, at) => part + (values[at] ?? "")).join(""))
    if (!link) return { ok: false, error: "Mẫu phải là link http hoặc https." }
    links.push(link)
  }
  return { ok: true, links }
}

type BatchKind = "file" | "media" | "page"

export interface BatchRow {
  url: string
  name: string
  /** media = trang video (tải bằng chất lượng chung); page = server trả trang web. */
  kind: BatchKind
  /** undefined = chưa dò; null = server không báo. */
  size?: number | null
  checked: boolean
}

// Đuôi tệp quen thuộc: link từ trình duyệt không có đuôi (và không phải trang video) để bỏ tick sẵn.
const FILE_EXTENSIONS = new Set([
  "zip", "rar", "7z", "tar", "gz", "xz", "bz2", "iso", "img", "exe", "msi", "dmg", "pkg", "deb", "rpm",
  "apk", "appimage", "pdf", "epub", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "csv", "json",
  "mp4", "mkv", "webm", "mov", "avi", "m4v", "flv", "mp3", "m4a", "flac", "wav", "ogg", "opus", "aac",
  "jpg", "jpeg", "png", "gif", "webp", "avif", "bmp", "svg", "tif", "tiff", "heic", "psd",
  "gguf", "safetensors", "onnx", "bin", "torrent",
])

/** Đuôi file của link (chữ thường), "" khi không có. */
function linkExtension(url: string): string {
  const name = fileNameFromUrl(url)
  const dot = name.lastIndexOf(".")
  return dot > 0 && dot < name.length - 1 ? name.slice(dot + 1).toLowerCase() : ""
}

/** Dòng bảng xem trước. `fromBrowser`: link gom trên trang ⇒ link không rõ là file thì bỏ tick sẵn. */
export function toRow(url: string, fromBrowser = false): BatchRow {
  const media = isMediaPage(url)
  const known = FILE_EXTENSIONS.has(linkExtension(url))
  // Trang video chưa có tiêu đề (UTD Phân tích lúc tới lượt): hiện link gọn, bỏ https://www.
  const short = url.replace(/^https?:\/\/(www\.)?/i, "")
  return {
    url,
    name: media ? short : fileNameFromUrl(url) || short,
    kind: media ? "media" : "file",
    checked: media || known || !fromBrowser,
  }
}

/** Nhóm của bộ lọc nhanh: "Video" cho trang video, đuôi file, hoặc "Khác". */
export function groupOf(row: BatchRow): string {
  if (row.kind === "media") return "Video"
  if (row.kind === "page") return "Trang web"
  return linkExtension(row.url) || "Khác"
}
