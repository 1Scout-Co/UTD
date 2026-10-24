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

export type MediaToolName = "yt-dlp" | "ffmpeg"

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
  binDir: string
  installing: MediaToolName | null
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
  tone: string
  date: string
  time: string
  sortTime: number
}

export interface MediaFormat {
  id: string
  label: string
  detail: string
  size: string
  bitrate: string
  badge?: string
}

export interface MediaShelfItem {
  id: string
  title: string
  meta: string
  duration: string
  quality: string
  art: string
  accent: string
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
  onSettings: () => void
}
