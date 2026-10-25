// Màn Chi tiết download (S-15..S-21): dữ liệu journal (src-tauri/src/detail.rs) và phép tính thuần cho bản đồ
// đoạn, lý do dừng, các lần lỗi. Không biết giao diện.

import type {
  DownloadItem,
  EngineProgress,
  EngineSegment,
  EngineTask,
  EngineTaskStatus,
} from "./types"

/** Vai của một luồng: task media có hình + tiếng (task con ẩn); task HTTP là chính nó. */
export type DetailPartRole = "file" | "video" | "audio" | "stream"

export interface DetailPart {
  id: string
  role: DetailPartRole
  fileName: string
  status: EngineTaskStatus
  totalBytes: number | null
  downloadedBytes: number
  /** Trống: chưa chia đoạn hoặc tải một luồng. */
  segments: EngineSegment[]
}

export interface TaskEvent {
  at: number
  status: EngineTaskStatus
  /** Mã lỗi (lỗi/cách ly) hoặc `app.interrupted` (UTD tắt ngang). */
  code?: string
  message?: string
  downloadedBytes: number
}

export interface DownloadDetail {
  task: EngineTask
  parts: DetailPart[]
  events: TaskEvent[]
  targetPath: string
  /** Task đã xong: file còn ở chỗ cũ không; chưa xong: null. */
  fileExists: boolean | null
  mediaTitle: string | null
}

/** Sự kiện `utd://segments`: bản đồ đoạn mới của task đang mở. */
export interface PartsUpdate {
  taskId: string
  parts: DetailPart[]
}

export type DetailLoad = "loading" | "ready" | "missing" | "error"

export type DetailScreen =
  | "S-15"
  | "S-16"
  | "S-17"
  | "S-18"
  | "S-19"
  | "S-20"
  | "S-21"

/** Mỗi trạng thái một màn như SPEC; đang chờ / đang kiểm / đang ghép tính là đang tải (S-15). */
export function screenOf(
  load: DetailLoad,
  status: EngineTaskStatus | undefined
): DetailScreen {
  if (load === "loading") return "S-17"
  if (load === "missing") return "S-16"
  if (load === "error" || !status) return "S-18"
  if (status === "completed") return "S-20"
  if (status === "failed" || status === "quarantined") return "S-21"
  if (status === "paused") return "S-19"
  return "S-15"
}

export const INTERRUPTED_CODE = "app.interrupted"

export type PauseReason = "user" | "interrupted"

/** Lý do của lần dừng gần nhất (null: journal cũ chưa có lịch sử). */
export function pauseReason(events: TaskEvent[]): PauseReason | null {
  const last = events.findLast((event) => event.status === "paused")
  if (!last) return null
  return last.code === INTERRUPTED_CODE ? "interrupted" : "user"
}

/** Các lần lỗi, mới trước. */
export function failures(events: TaskEvent[]): TaskEvent[] {
  return events
    .filter(
      (event) =>
        (event.status === "failed" || event.status === "quarantined") &&
        event.code
    )
    .reverse()
}

const byteLength = (segment: EngineSegment) => segment.end - segment.start + 1

export type BlockState = "done" | "active" | "partial" | "pending" | "failed"

/** Một khối trên bản đồ: rộng theo số byte của đoạn, tô theo phần đã tải. */
export interface MapBlock {
  key: string
  width: number
  fill: number
  state: BlockState
}

function blockState(
  segment: EngineSegment,
  live: boolean
): BlockState {
  if (segment.downloaded >= byteLength(segment)) return "done"
  if (segment.status === "failed") return "failed"
  if (live && segment.status === "active") return "active"
  return segment.downloaded > 0 ? "partial" : "pending"
}

/** `live`: luồng đang chạy (đoạn `active` mới là đang tải; tạm dừng thì chỉ còn phần đã có). */
export function mapBlocks(part: DetailPart, live: boolean): MapBlock[] {
  const total = part.segments.reduce(
    (sum, segment) => sum + byteLength(segment),
    0
  )
  if (part.segments.length === 0 || total === 0) {
    const fill =
      part.status === "completed"
        ? 100
        : part.totalBytes
          ? (part.downloadedBytes / part.totalBytes) * 100
          : 0
    const state: BlockState =
      part.status === "completed"
        ? "done"
        : part.status === "failed"
          ? "failed"
          : live
            ? "active"
            : fill > 0
              ? "partial"
              : "pending"
    return [{ key: part.id, width: 100, fill: clamp(fill), state }]
  }
  return part.segments.map((segment) => ({
    key: `${part.id}:${segment.index}`,
    width: (byteLength(segment) / total) * 100,
    fill: clamp((segment.downloaded / byteLength(segment)) * 100),
    state: blockState(segment, live),
  }))
}

export function segmentCounts(parts: DetailPart[], live: boolean) {
  const segments = parts.flatMap((part) => part.segments)
  const done = segments.filter(
    (segment) => segment.downloaded >= byteLength(segment)
  ).length
  const active = live
    ? segments.filter(
        (segment) =>
          segment.status === "active" &&
          segment.downloaded < byteLength(segment)
      ).length
    : 0
  return { done, active, total: segments.length }
}

export { byteLength }

function clamp(value: number): number {
  return Math.max(0, Math.min(100, Number.isFinite(value) ? value : 0))
}

/** Số liệu sống: tiến độ engine báo (nhanh hơn journal) hoặc số trong journal. */
export interface LiveNumbers {
  downloaded: number
  total: number | null
  connections: number | null
  maxConnections: number | null
}

export function liveNumbers(
  item: DownloadItem | undefined,
  task: EngineTask | undefined,
  progress: EngineProgress | undefined
): LiveNumbers {
  if (task) {
    const total = task.totalBytes ?? progress?.totalBytes ?? null
    const downloaded =
      task.status === "completed"
        ? (total ?? task.downloadedBytes)
        : Math.max(task.downloadedBytes, progress?.downloadedBytes ?? 0)
    return {
      downloaded,
      total,
      connections: item?.connections ?? null,
      maxConnections: task.connections,
    }
  }
  // Bản xem trước trên trình duyệt: suy từ % của dữ liệu mẫu.
  const total = item?.sizeBytes ?? null
  return {
    downloaded: total ? Math.round((total * (item?.progress ?? 0)) / 100) : 0,
    total,
    connections: item?.connections ?? null,
    maxConnections: item?.totalConnections ?? null,
  }
}

/** Thời gian tải: từ lúc bắt đầu chạy lần đầu (hoặc lúc thêm) tới lúc xong. */
export function tookMs(detail: DownloadDetail): number | null {
  const end = detail.task.completedAt
  if (!end) return null
  const started = detail.events.find(
    (event) => event.status === "resolving" || event.status === "downloading"
  )
  return Math.max(0, end - (started?.at ?? detail.task.createdAt))
}

/** Máy chủ của link (đã chuyển hướng nếu có). */
export function hostOfTask(task: EngineTask): string {
  try {
    return new URL(task.finalUrl ?? task.url).hostname.replace(/^www\./, "")
  } catch {
    return ""
  }
}
