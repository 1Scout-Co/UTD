import { errorText, t } from "@/shared/i18n/i18n"
import {
  formatBytes,
  formatDate,
  formatMinutes,
  formatSpeed,
  formatTime,
} from "@/shared/i18n/format"

import type {
  DownloadItem,
  DownloadKind,
  DownloadStatus,
  EngineProgress,
  EngineTask,
  EngineTaskStatus,
} from "./types"

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
    subtitle: `${host} · ${t(`kind.${kind}`)}`,
    description: describe(task, progress),
    kind,
    size: totalBytes !== null ? formatBytes(totalBytes) : "—",
    sizeBytes: totalBytes ?? downloadedBytes,
    progress: Math.round(percent),
    speed: active && speedMb >= 0.1 ? formatSpeed(speedMb) : undefined,
    speedValue: speedMb > 0 ? speedMb : undefined,
    eta: etaMinutes === undefined ? undefined : formatMinutes(etaMinutes),
    etaMinutes,
    status: mapStatus(task.status),
    connections: active ? (progress?.connections ?? task.connections) : undefined,
    totalConnections: task.connections,
    hash: task.expectedSha256 ?? undefined,
    hashState: task.hashState,
    note: task.error?.message,
    errorCode: task.error?.code,
    tone: KIND_TONE[kind],
    date: formatDate(created.getTime()),
    time: formatTime(created.getTime()),
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

// Media "verifying" = ffmpeg đang ghép hình + tiếng hoặc chuyển định dạng âm thanh sau khi tải xong.
function verifyingLabel(task: EngineTask): string {
  if (task.kind !== "media") return t("desc.verifyingHash")
  return /\.(mp3|m4a)$/i.test(task.fileName)
    ? t("desc.convertingAudio")
    : t("desc.merging")
}

function describe(task: EngineTask, progress?: EngineProgress): string {
  if (task.status === "failed" || task.status === "quarantined") {
    return task.error
      ? errorText(task.error.code, task.error.message)
      : t("desc.stopped")
  }
  if (task.status === "completed") {
    return task.hashState === "verified" ? t("desc.verified") : t("desc.done")
  }
  if (task.status === "verifying") return verifyingLabel(task)
  if (task.status === "paused") return t("desc.paused")
  if (task.status === "queued") return t("desc.queued")
  if (task.status === "resolving") {
    return task.kind === "media" ? t("desc.analyzing") : t("desc.resolving")
  }
  const parts = progress?.segmentsTotal ?? 0
  const connections = progress?.connections ?? task.connections
  return parts > 1
    ? t("desc.parts", {
        done: progress?.segmentsDone ?? 0,
        total: parts,
        count: connections,
      })
    : t("desc.single")
}

function hostOf(value: string): string {
  try {
    return new URL(value).hostname.replace(/^www\./, "")
  } catch {
    return t("desc.localHost")
  }
}
