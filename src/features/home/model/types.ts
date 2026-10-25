export type ScreenState = "data" | "empty" | "loading" | "error"

export type DownloadFilter = "all" | "active" | "completed" | "error"

export type DownloadKind = "game" | "model" | "iso" | "video" | "archive" | "document" | "app"

export type DownloadStatus =
  | "downloading"
  | "paused"
  | "queued"
  | "completed"
  | "failed"
  | "quarantined"
  | "verifying"

export type HashState = "verified" | "mismatch" | "pending" | "unavailable"

export type EngineTaskKind = "http" | "hls" | "dash" | "media"

export type EngineTaskStatus =
  | "queued"
  | "resolving"
  | "downloading"
  | "paused"
  | "verifying"
  | "completed"
  | "failed"
  | "quarantined"

export type EngineErrorClass = "user" | "transient" | "permanent" | "security"

export interface EngineTaskError {
  class: EngineErrorClass
  code: string
  message: string
}

/** UTD đang chờ gì để tự chạy lại (src-tauri/src/recover.rs). */
export type EngineRecoveryWait = "retry" | "network" | "disk" | "link"

export interface EngineRecovery {
  wait: EngineRecoveryWait
  /** Giờ tự chạy lại (ms); không có = chờ sự kiện (có mạng, đủ chỗ, link mới). */
  retryAt?: number
  attempts: number
  progress: number
  /** Ổ đầy: cần trống chừng này byte. */
  needBytes?: number
}

export interface EngineTask {
  id: string
  url: string
  finalUrl: string | null
  fileName: string
  destDir: string
  tempPath: string
  kind: EngineTaskKind
  status: EngineTaskStatus
  totalBytes: number | null
  downloadedBytes: number
  etag: string | null
  lastModified: string | null
  expectedSha256: string | null
  hashState: HashState
  connections: number
  error: EngineTaskError | null
  createdAt: number
  updatedAt: number
  completedAt: number | null
  recovery?: EngineRecovery
}

export type EngineSegmentStatus = "pending" | "active" | "done" | "failed"

export interface EngineSegment {
  index: number
  start: number
  end: number
  downloaded: number
  status: EngineSegmentStatus
}

export interface EngineProgress {
  taskId: string
  status: EngineTaskStatus
  downloadedBytes: number
  totalBytes: number | null
  bytesPerSecond: number
  connections: number
  segmentsDone: number
  segmentsTotal: number
  hashState: HashState
}

export interface AddDownloadPayload {
  url: string
  destDir?: string
  fileName?: string
  connections?: number
  expectedSha256?: string
}

export interface StorageInfo {
  freeBytes: number
  totalBytes: number
  label: string
}

export interface StorageSummary {
  label: string
  free: string
  usedPercent: number
}

export type MediaAudioFormat = "m4a" | "mp3"

export interface EngineMediaFormat {
  id: string
  label: string
  detail: string
  sizeBytes: number | null
  bitrateKbps: number | null
  selector: string
  audioFormat: MediaAudioFormat | null
  badge: string | null
  recommended: boolean
}

export interface MediaProbe {
  url: string
  webpageUrl: string
  title: string
  uploader: string | null
  durationSeconds: number | null
  thumbnail: string | null
  formats: EngineMediaFormat[]
  ffmpegAvailable: boolean
}

export interface AddMediaPayload {
  url: string
  title: string
  selector: string
  audioFormat?: MediaAudioFormat
  pageUrl?: string
  /** Tải cả kênh: thư mục con (tên kênh) trong thư mục tải. */
  subfolder?: string
  /** Tải cả kênh: để app nhớ id video đã tải. */
  channel?: { key: string; videoId: string }
  /** Thư mục tải; không gửi ⇒ thư mục mặc định trong Cài đặt. */
  destDir?: string
  /** true ⇒ xếp cuối hàng đợi chung (tải cả kênh); mặc định chen đầu hàng. */
  later?: boolean
}

/** Chất lượng chung cho link trang video trong lô (src-tauri/src/media/preset.rs). */
export type BatchQuality = "best" | "1080" | "720" | "audio-m4a" | "audio-mp3"

export interface AddBatchPayload {
  urls: string[]
  destDir?: string
  quality: BatchQuality
  /** true = "Thêm vào hàng đợi" (cuối hàng); false = "Bắt đầu ngay" (đầu hàng). */
  later: boolean
}

export interface BatchResult {
  tasks: EngineTask[]
  failed: { url: string; message: string }[]
}

/** Một link dò trước khi thêm (`inspect_link`). */
export interface RemoteFile {
  url: string
  fileName: string
  totalBytes: number | null
  contentType: string | null
  /** Server trả trang web, không phải file. */
  page: boolean
}

export interface QueueSettings {
  /** Số download chạy cùng lúc. */
  limit: number
  maxLimit: number
}

/** deno: runtime JavaScript yt-dlp dùng để giải thử thách của YouTube. */
export type MediaToolName = "yt-dlp" | "deno" | "ffmpeg"

/** utd = bản UTD tự tải vào thư mục của nó; còn lại là bản có sẵn trên máy. */
export type MediaToolSource = "utd" | "path" | "winget" | "scoop" | "choco"

export interface MediaToolInfo {
  path: string
  version: string
  source: MediaToolSource
}

export interface MediaToolsStatus {
  ytDlp: MediaToolInfo | null
  ffmpeg: MediaToolInfo | null
  deno: MediaToolInfo | null
  binDir: string
  installing: MediaToolName | null
}

/** App tự tải công cụ còn thiếu (lần đầu) và tự cập nhật yt-dlp/deno của UTD. */
export interface MediaToolsAuto {
  /** Công tắc trong Cài đặt (mặc định bật). */
  enabled: boolean
  /** Đang dò công cụ lúc mở app. */
  checking: boolean
  /** Còn phải tải, theo thứ tự. */
  pending: MediaToolName[]
  /** Đang tải thật (null khi đang chờ thử lại). */
  active: MediaToolName | null
  /** Đang cập nhật nền (đã có bản dùng được). */
  updating: MediaToolName | null
  /** Công cụ mà lượt tải vừa rồi không tải được. */
  failed: MediaToolName[]
  /** Câu lỗi của công cụ đầu tiên trong `failed`. */
  error: string | null
  /** Giờ thử lại (ms). */
  retryAt: number | null
  /** Số lượt tải đã xong. */
  rounds: number
  /** Người dùng bấm Huỷ: thôi tự tải tới khi cần lại. */
  paused: boolean
}

export type MediaToolStage =
  | "checksum"
  | "download"
  | "extract"
  | "check"
  | "install"

export interface MediaToolProgress {
  tool: MediaToolName
  stage: MediaToolStage
  downloadedBytes: number
  totalBytes: number | null
}

/** Bản UTD mới trên GitHub Releases (từ latest.json). */
export interface UpdateInfo {
  version: string
  currentVersion: string
  notes: string | null
  /** dd/mm/yyyy */
  date: string | null
}

export interface UpdateCheckState {
  /** false: bản này không bật được tự cập nhật (cấu hình thiếu/hỏng). */
  ready: boolean
  /** Tự kiểm tra khi mở app. */
  enabled: boolean
  available: UpdateInfo | null
  installing: boolean
}

/** download = đang tải gói; install = dừng download, sao lưu, chạy bộ cài. */
type UpdateStage = "download" | "install"

export interface UpdateProgress {
  stage: UpdateStage
  downloadedBytes: number
  totalBytes: number | null
}

export interface BrowserIntegrationStatus {
  installed: boolean
  hostPath: string
  manifestPath: string
  chromiumExtensionId: string
  firefoxExtensionId: string
  extensionDir: string
}

export interface DownloadItem {
  id: string
  name: string
  subtitle: string
  description: string
  kind: DownloadKind
  size: string
  sizeBytes: number
  progress: number
  speed?: string
  speedValue?: number
  eta?: string
  etaMinutes?: number
  status: DownloadStatus
  connections?: number
  totalConnections?: number
  hash?: string
  hashState?: HashState
  note?: string
  errorCode?: string
  /** UTD đang tự cứu: chờ thử lại / chờ mạng / chờ chỗ trống (không phải lỗi). */
  waiting?: Exclude<EngineRecoveryWait, "link">
  /** Download thường (không phải media) chưa xong: đổi được link (link ký hết hạn, cần đăng nhập lại). */
  relinkable?: boolean
  tone: string
  date: string
  time: string
  sortTime: number
}

export interface HomeScreenProps {
  state: ScreenState
  items: DownloadItem[]
  storage?: StorageSummary
  onAdd: () => void
  onMedia: () => void
  onRetry: () => void
  onPause: (id: string) => void
  onResume: (id: string) => void
  onRemove: (id: string) => void
  onDiagnose: (id: string) => void
  /** Mở màn Chi tiết (nhấp đúp, Enter, menu "Chi tiết"). */
  onDetails: (id: string) => void
  onSettings: () => void
  /** Mở hộp "Cập nhật link" (app desktop). */
  onRefreshLink?: (id: string) => void
}
