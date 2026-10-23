import type {
  DownloadItem,
  DownloadKind,
  DownloadStatus,
  EngineProgress,
  EngineTask,
  EngineTaskStatus,
} from "./types"

const KIND_LABEL: Record<DownloadKind, string> = {
  game: "Game",
  model: "Model AI",
  iso: "Disk image",
  video: "Video",
  archive: "Tệp nén",
  document: "Tài liệu",
  app: "Ứng dụng",
}

const KIND_TONE: Record<DownloadKind, string> = {
  game: "#ff9a76",
  model: "#7cb8ff",
  iso: "#8f9dff",
  video: "#ffb454",
  archive: "#7fd1a8",
  document: "#79b8ff",
  app: "#94a8c4",
}

const EXTENSION_KIND: Record<string, DownloadKind> = {
  iso: "iso",
  img: "iso",
  mp4: "video",
  mkv: "video",
  webm: "video",
  mov: "video",
  avi: "video",
  m4v: "video",
  mp3: "video",
  flac: "video",
  wav: "video",
  gguf: "model",
  safetensors: "model",
  onnx: "model",
  zip: "archive",
  rar: "archive",
  "7z": "archive",
  tar: "archive",
  gz: "archive",
  xz: "archive",
  pdf: "document",
  epub: "document",
  doc: "document",
  docx: "document",
  exe: "app",
  msi: "app",
  dmg: "app",
  pkg: "app",
  deb: "app",
  appimage: "app",
}

const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"]

const sizeFormatter = new Intl.NumberFormat("vi-VN", {
  maximumFractionDigits: 1,
})

const dateFormatter = new Intl.DateTimeFormat("vi-VN", {
  day: "2-digit",
  month: "2-digit",
  year: "numeric",
})

const timeFormatter = new Intl.DateTimeFormat("vi-VN", {
  hour: "2-digit",
  minute: "2-digit",
  hour12: false,
})

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B"
  const exponent = Math.min(
    UNITS.length - 1,
    Math.floor(Math.log(bytes) / Math.log(1024)),
  )
  const value = bytes / 1024 ** exponent
  return `${sizeFormatter.format(value)} ${UNITS[exponent]}`
}

export function formatNumber(value: number, digits = 1): string {
  return new Intl.NumberFormat("vi-VN", {
    maximumFractionDigits: digits,
  }).format(value)
}

export function fileNameFromUrl(value: string): string {
  try {
    const pathname = new URL(value).pathname
    const last = pathname.split("/").filter(Boolean).at(-1)
    return last ? decodeURIComponent(last) : ""
  } catch {
    return ""
  }
}

export function mapEngineTask(
  task: EngineTask,
  progress?: EngineProgress,
): DownloadItem {
  const kind = inferKind(task.fileName)
  const totalBytes = task.totalBytes ?? progress?.totalBytes ?? null
  const downloadedBytes =
    task.status === "completed"
      ? (totalBytes ?? task.downloadedBytes)
      : Math.max(task.downloadedBytes, progress?.downloadedBytes ?? 0)
  const active =
    task.status === "downloading" ||
    task.status === "resolving" ||
    task.status === "verifying"
  const bytesPerSecond = active ? (progress?.bytesPerSecond ?? 0) : 0
  const percent =
    task.status === "completed"
      ? 100
      : totalBytes && totalBytes > 0
        ? Math.min(100, (downloadedBytes / totalBytes) * 100)
        : 0
  const remaining =
    totalBytes !== null ? Math.max(0, totalBytes - downloadedBytes) : null
  const etaMinutes =
    bytesPerSecond > 0 && remaining !== null
      ? Math.max(1, Math.ceil(remaining / bytesPerSecond / 60))
      : undefined
  const speedMb = bytesPerSecond / (1024 * 1024)
  const created = new Date(task.createdAt)
  const host = hostOf(task.finalUrl ?? task.url)

  return {
    id: task.id,
    name: task.fileName,
    subtitle: `${host} · ${KIND_LABEL[kind]}`,
    description: describe(task, progress),
    kind,
    size: totalBytes !== null ? formatBytes(totalBytes) : "—",
    sizeBytes: totalBytes ?? downloadedBytes,
    progress: Math.round(percent),
    speed: active && speedMb >= 0.1 ? `${formatNumber(speedMb)} MB/s` : undefined,
    speedValue: speedMb > 0 ? speedMb : undefined,
    eta:
      etaMinutes === undefined
        ? undefined
        : etaMinutes < 60
          ? `${etaMinutes} phút`
          : `${Math.floor(etaMinutes / 60)} giờ ${etaMinutes % 60} phút`,
    etaMinutes,
    status: mapStatus(task.status),
    connections: active ? (progress?.connections ?? task.connections) : undefined,
    totalConnections: task.connections,
    hash: task.expectedSha256 ?? undefined,
    hashState: task.hashState,
    note: task.error?.message,
    errorCode: task.error?.code,
    tone: KIND_TONE[kind],
    date: dateFormatter.format(created),
    time: timeFormatter.format(created),
    sortTime: created.getTime(),
  }
}

function inferKind(fileName: string): DownloadKind {
  const extension = fileName.split(".").pop()?.toLowerCase() ?? ""
  return EXTENSION_KIND[extension] ?? "app"
}

function mapStatus(status: EngineTaskStatus): DownloadStatus {
  if (status === "resolving") return "downloading"
  return status
}

function describe(task: EngineTask, progress?: EngineProgress): string {
  if (task.status === "failed" || task.status === "quarantined") {
    return task.error?.message ?? "Đã dừng do lỗi"
  }
  if (task.status === "completed") {
    return task.hashState === "verified"
      ? "Đã xác minh SHA-256"
      : "Hoàn tất"
  }
  if (task.status === "verifying") return "Đang xác minh SHA-256"
  if (task.status === "paused") return "Tạm dừng · tiếp tục bất cứ lúc nào"
  if (task.status === "queued") return "Đang chờ lượt"
  if (task.status === "resolving") return "Đang kiểm tra máy chủ"
  const parts = progress?.segmentsTotal ?? 0
  const connections = progress?.connections ?? task.connections
  return parts > 1
    ? `${progress?.segmentsDone ?? 0}/${parts} phần · ${connections} kết nối`
    : "Đang tải một luồng"
}

function hostOf(value: string): string {
  try {
    return new URL(value).hostname.replace(/^www\./, "")
  } catch {
    return "máy cục bộ"
  }
}
