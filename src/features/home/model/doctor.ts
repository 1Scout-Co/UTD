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

/** Nút chính của Doctor: Tiếp tục, Cập nhật link (link hết hạn), Tải lại từ đầu (file đổi / `.part` mất). */
export type DoctorAction = "resume" | "refreshLink" | "restart"

export interface DoctorReport {
  title: string
  summary: string
  steps: string[]
  retryable: boolean
  category: DoctorCategory
  action: DoctorAction | null
  /** S-72: cho phép trang trong popup tiện ích là tải tiếp được; hộp có thêm nút "Mở trang". */
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
  | "page"
  | "drm"
  | "tool"
  | "mux"
  | "lostParts"
  | "disk"
  | "exists"
  | "unsupported"
  | "unknown"
  // Cứu download (chuỗi ở recover.doctor.<id>.*): file đổi trên máy chủ, `.part` mất, ổ rút, ổ quá nhỏ.
  | "changed"
  | "partMissing"
  | "unplugged"
  | "tooSmall"

const RECOVER_REPORTS: ReadonlySet<ReportId> = new Set([
  "changed",
  "partMissing",
  "unplugged",
  "tooSmall",
])

const REPORTS: Record<ReportId, { retryable: boolean; category: DoctorCategory }> = {
  guard: { retryable: false, category: "security" },
  hash: { retryable: true, category: "security" },
  limited: { retryable: true, category: "network" },
  network: { retryable: true, category: "network" },
  access: { retryable: false, category: "access" },
  signIn: { retryable: false, category: "access" },
  gone: { retryable: false, category: "access" },
  // Link là trang web: thử lại lần nào cũng thế; cần link tải thật (Cập nhật link…) hoặc tải từ trình duyệt.
  page: { retryable: false, category: "access" },
  drm: { retryable: false, category: "security" },
  tool: { retryable: true, category: "tool" },
  // Đường tải thẳng của media: ghép bằng ffmpeg hỏng, hoặc mất file luồng đã tải.
  mux: { retryable: false, category: "tool" },
  lostParts: { retryable: true, category: "disk" },
  disk: { retryable: true, category: "disk" },
  exists: { retryable: true, category: "access" },
  unsupported: { retryable: false, category: "tool" },
  unknown: { retryable: true, category: "unknown" },
  changed: { retryable: false, category: "access" },
  partMissing: { retryable: false, category: "disk" },
  unplugged: { retryable: true, category: "disk" },
  tooSmall: { retryable: false, category: "disk" },
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
  "http.not_a_file": "page",
  "tool.ytdlp_missing": "tool",
  "tool.ffmpeg_missing": "tool",
  "media.mux": "mux",
  "media.no_output": "lostParts",
  "file.exists": "exists",
  "media.unsupported": "unsupported",
  "file.changed": "changed",
  "file.part_missing": "partMissing",
  "disk.unplugged": "unplugged",
  "disk.too_small": "tooSmall",
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

/** Link hết hạn / cần đăng nhập lại (401, 403, 407, 410) của download thường: dán link mới, giữ phần đã tải. */
const LINK_REFUSED = new Set(["http.401", "http.403", "http.407", "http.410"])

function actionOf(item: DownloadItem, id: ReportId, code: string, retryable: boolean) {
  // Cần đăng nhập lại cũng chờ link như link hết hạn: hộp "Cập nhật link" nhận link tải lại từ trình duyệt
  // (kèm đăng nhập của cú bấm).
  // Link là trang web: dán link tải thật, hoặc tải lại đúng file từ trình duyệt, cho chính download này.
  if (item.relinkable && (LINK_REFUSED.has(code) || id === "signIn" || id === "page")) return "refreshLink"
  if (id === "changed" || id === "partMissing") return "restart"
  return retryable ? "resume" : null
}

export function diagnose(item: DownloadItem): DoctorReport {
  const code = item.errorCode ?? ""
  const id = reportOf(item, code)
  const group = RECOVER_REPORTS.has(id) ? "recover.doctor" : "doctor"
  const key = (part: "title" | "summary" | "steps") =>
    `${group}.${id}.${part}` as MessageKey
  const quarantined = id === "unknown" && item.status === "quarantined"
  const retryable = REPORTS[id].retryable && !quarantined
  return {
    title: quarantined ? t("doctor.quarantined.title") : t(key("title")),
    summary: errorText(code, item.note) || t(key("summary")),
    steps: t(key("steps")).split("\n"),
    retryable,
    category: REPORTS[id].category,
    action: quarantined ? null : actionOf(item, id, code, retryable),
    signIn: id === "signIn",
  }
}
