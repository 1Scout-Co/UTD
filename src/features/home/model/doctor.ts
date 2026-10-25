import { errorText, t, type MessageKey } from "@/shared/i18n/i18n"

import { needsSignIn } from "./sign-in"
import type { DownloadItem } from "./types"

export type DoctorCategory =
  | "network"
  | "access"
  | "security"
  | "tool"
  | "disk"
  | "unknown"

export interface DoctorReport {
  title: string
  summary: string
  steps: string[]
  retryable: boolean
  category: DoctorCategory
  /** S-72: cho phép trang trong popup tiện ích là tải tiếp được; hộp có nút "Mở trang". */
  signIn: boolean
}

// Mỗi chẩn đoán có 3 chuỗi trong file dịch: doctor.<id>.title, .summary (khi lỗi không kèm câu nào),
// .steps (các bước, mỗi dòng một bước).
type ReportId =
  | "guard"
  | "hash"
  | "limited"
  | "network"
  | "access"
  | "signIn"
  | "gone"
  | "drm"
  | "tool"
  | "mux"
  | "lostParts"
  | "disk"
  | "exists"
  | "unsupported"
  | "unknown"

const REPORTS: Record<ReportId, { retryable: boolean; category: DoctorCategory }> = {
  guard: { retryable: false, category: "security" },
  hash: { retryable: true, category: "security" },
  limited: { retryable: true, category: "network" },
  network: { retryable: true, category: "network" },
  access: { retryable: false, category: "access" },
  signIn: { retryable: false, category: "access" },
  gone: { retryable: false, category: "access" },
  drm: { retryable: false, category: "security" },
  tool: { retryable: true, category: "tool" },
  // Đường tải thẳng của media: ghép bằng ffmpeg hỏng, hoặc mất file luồng đã tải.
  mux: { retryable: false, category: "tool" },
  lostParts: { retryable: true, category: "disk" },
  disk: { retryable: true, category: "disk" },
  exists: { retryable: true, category: "access" },
  unsupported: { retryable: false, category: "tool" },
  unknown: { retryable: true, category: "unknown" },
}

const EXACT: Record<string, ReportId> = {
  "guard.dangerous_type": "guard",
  "guard.extension_mismatch": "guard",
  "http.429": "limited",
  "media.rate_limited": "limited",
  "http.408": "network",
  "media.network": "network",
  "media.forbidden": "access",
  "http.404": "gone",
  "http.410": "gone",
  "media.not_found": "gone",
  "media.unavailable": "gone",
  "tool.ytdlp_missing": "tool",
  "tool.ffmpeg_missing": "tool",
  "media.mux": "mux",
  "media.no_output": "lostParts",
  "file.exists": "exists",
  "media.unsupported": "unsupported",
}

function reportOf(item: DownloadItem, code: string): ReportId {
  if (EXACT[code] === "guard") return "guard"
  if (needsSignIn(code)) return "signIn"
  if (code.startsWith("verify.") || item.hashState === "mismatch") return "hash"
  if (EXACT[code]) return EXACT[code]
  if (code.startsWith("network.")) return "network"
  if (code.startsWith("media.drm")) return "drm"
  if (code.startsWith("disk.")) return "disk"
  return "unknown"
}

export function diagnose(item: DownloadItem): DoctorReport {
  const code = item.errorCode ?? ""
  const id = reportOf(item, code)
  const key = (part: "title" | "summary" | "steps") =>
    `doctor.${id}.${part}` as MessageKey
  const quarantined = id === "unknown" && item.status === "quarantined"
  return {
    title: quarantined ? t("doctor.quarantined.title") : t(key("title")),
    summary: errorText(code, item.note) || t(key("summary")),
    steps: t(key("steps")).split("\n"),
    retryable: REPORTS[id].retryable && !quarantined,
    category: REPORTS[id].category,
    signIn: id === "signIn",
  }
}
