// Cảnh báo trước khi tải (S-22). Luật nằm ở Download Guard của engine (crates/utd-engine/src/safety/preflight.rs,
// lệnh `check_links`); đây chỉ gom kết quả cho một link hay cả lô và chuẩn bị chữ hiện ra.

/** Nặng trước, đúng thứ tự engine trả. Nhãn: t(`warn.kind.${kind}`), giải thích: t(`warn.why.${kind}`). */
export const WARNING_KINDS = [
  "hidden_chars",
  "spoofed_name",
  "dangerous_type",
  "mime_mismatch",
  "insecure_http",
] as const

export type WarningKind = (typeof WARNING_KINDS)[number]

export interface LinkWarning {
  kind: WarningKind
  /** Đuôi (`exe`), cặp đuôi giả (`pdf.exe`), mã ký tự (`U+202E`), kiểu nội dung hoặc tên máy chủ. */
  detail: string
}

/** Những gì giao diện biết về một link khi hỏi `check_links`. */
export interface LinkFacts {
  url: string
  finalUrl?: string
  fileName?: string
  contentType?: string
}

export interface WarnedLink {
  url: string
  fileName: string
  warnings: LinkWarning[]
}

/** Ghép kết quả `check_links` (cùng thứ tự) với link; chỉ giữ link có cảnh báo. */
export function warnedLinks(
  links: LinkFacts[],
  found: LinkWarning[][]
): WarnedLink[] {
  return links.flatMap((link, index) => {
    const warnings = found[index] ?? []
    return warnings.length
      ? [{ url: link.url, fileName: link.fileName ?? "", warnings }]
      : []
  })
}

/** Số link mắc từng loại cảnh báo (một link mắc nhiều loại thì tính ở mỗi loại), theo thứ tự nặng trước. */
export function countByKind(
  links: WarnedLink[]
): { kind: WarningKind; count: number }[] {
  return WARNING_KINDS.map((kind) => ({
    kind,
    count: links.filter((link) =>
      link.warnings.some((warning) => warning.kind === kind)
    ).length,
  })).filter((entry) => entry.count > 0)
}

/**
 * Tên để hiện trong cảnh báo: ký tự ẩn/đảo chiều mà engine đã chỉ ra (`U+202E`) thay bằng mã của nó, để
 * tên hiện đúng thứ tự thật thay vì lừa mắt người xem.
 */
export function visibleName(name: string, warnings: LinkWarning[]): string {
  const hidden = new Set(
    warnings
      .filter((warning) => warning.kind === "hidden_chars")
      .flatMap((warning) => warning.detail.split(" "))
      .map((code) => Number.parseInt(code.replace(/^U\+/, ""), 16))
      .filter(Number.isFinite)
  )
  if (hidden.size === 0) return name
  return [...name]
    .map((char) => {
      const code = char.codePointAt(0) ?? 0
      return hidden.has(code)
        ? `[U+${code.toString(16).toUpperCase().padStart(4, "0")}]`
        : char
    })
    .join("")
}

/** Máy chủ của link (để nhóm, hiện ngắn). */
export function hostOfLink(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "")
  } catch {
    return url
  }
}
