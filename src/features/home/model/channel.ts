// Tải cả kênh (TikTok, kênh/playlist YouTube): kiểu dữ liệu app gửi qua `channel_list`, luật lọc/chọn video
// và cách biến mỗi video thành một lệnh `add_media_download`. Không biết giao diện.

import { dayStart, isoDay } from "@/shared/dates"

import type { AddMediaPayload, MediaAudioFormat } from "./types"

/** Một video trong kênh (src-tauri/src/media/channel/entries.rs). */
export interface ChannelEntry {
  id: string
  url: string
  title: string
  /** Giây Unix lúc đăng; YouTube chỉ là ngày ước lượng. */
  timestamp: number | null
  durationSeconds: number | null
  viewCount: number | null
  thumbnail: string | null
  /** Lý do không tải được (riêng tư, hội viên, đang phát trực tiếp). */
  blocked: string | null
  /** Đã tải ở lần trước (theo id video). */
  downloaded: boolean
}

export interface ChannelHead {
  /** Khoá nhớ video đã tải; null khi trang không cho id ổn định. */
  key: string | null
  name: string
  handle: string | null
}

export interface ChannelProfile {
  name: string | null
  avatar: string | null
  followers: number | null
  videoCount: number | null
  private: boolean
}

export type ListingEvent =
  | { kind: "head"; head: ChannelHead }
  | { kind: "profile"; profile: ChannelProfile }
  | { kind: "entries"; entries: ChannelEntry[] }
  | { kind: "done"; total: number; complete: boolean; error: string | null }

/** Một chất lượng chung cho cả loạt: không phân tích từng video nên selector phải có đường lui. */
export interface ChannelQuality {
  id: string
  label: string
  detail: string
  selector: string
  audioFormat?: MediaAudioFormat
}

export const CHANNEL_QUALITIES: ChannelQuality[] = [
  { id: "best", label: "Tốt nhất", detail: "Video + âm thanh, cao nhất", selector: "bv*+ba/b" },
  {
    id: "h264",
    label: "H.264",
    detail: "Dễ mở trên mọi máy (TikTok tốt nhất thường là H.265)",
    selector: "bv*[vcodec^=avc]+ba[ext=m4a]/b[vcodec^=avc]/b[vcodec^=h264]/bv*+ba/b",
  },
  {
    id: "small",
    label: "Nhẹ",
    detail: "Tối đa 720p, đỡ tốn dung lượng",
    selector: "bv*[height<=720]+ba/b[height<=720]/wv*+ba/w",
  },
  { id: "m4a", label: "M4A", detail: "Chỉ âm thanh", selector: "ba[ext=m4a]/ba/b", audioFormat: "m4a" },
  { id: "mp3", label: "MP3", detail: "Chỉ âm thanh, cần ffmpeg", selector: "ba/b", audioFormat: "mp3" },
]

export type ChannelRange = "all" | "latest" | "dates"

export interface ChannelFilter {
  range: ChannelRange
  /** Số video mới nhất khi `range = latest`. */
  latest: number
  /** `YYYY-MM-DD` (ô ngày); rỗng = không giới hạn đầu đó. */
  from: string
  to: string
  /** Ẩn video đã tải ở lần trước. */
  onlyNew: boolean
}

export const DEFAULT_FILTER: ChannelFilter = {
  range: "all",
  latest: 20,
  from: "",
  to: "",
  onlyNew: true,
}

/** Video hiện trong danh sách, theo thứ tự kênh trả (mới nhất trước). */
export function visibleEntries(entries: ChannelEntry[], filter: ChannelFilter): ChannelEntry[] {
  const shown = filter.onlyNew ? entries.filter((entry) => !entry.downloaded) : entries
  if (filter.range === "latest") return newest(shown, filter.latest)
  if (filter.range === "dates") return withinDates(shown, filter.from, filter.to)
  return shown
}

function newest(entries: ChannelEntry[], count: number): ChannelEntry[] {
  const limit = Math.max(1, Math.floor(count) || 1)
  const keep = new Set(
    entries
      .map((entry, index) => ({ entry, index }))
      // Thiếu ngày thì giữ thứ tự kênh trả (đã là mới nhất trước).
      .sort((a, b) => (b.entry.timestamp ?? -1) - (a.entry.timestamp ?? -1) || a.index - b.index)
      .slice(0, limit)
      .map(({ entry }) => entry.id),
  )
  return entries.filter((entry) => keep.has(entry.id))
}

function withinDates(entries: ChannelEntry[], from: string, to: string): ChannelEntry[] {
  const start = dayStart(from)
  const endDay = dayStart(to)
  const end = endDay === null ? null : endDay + 86_400
  if (start === null && end === null) return entries
  return entries.filter(
    (entry) =>
      entry.timestamp !== null &&
      (start === null || entry.timestamp >= start) &&
      (end === null || entry.timestamp < end),
  )
}

/**
 * Chọn video: mặc định chọn mọi video chưa tải (cả video còn đang liệt kê tới); `toggled` là các id người
 * dùng đã bấm đổi so với mặc định. Video bị chặn không bao giờ được chọn.
 */
export interface ChannelSelection {
  baseline: boolean
  toggled: ReadonlySet<string>
}

export const DEFAULT_SELECTION: ChannelSelection = { baseline: true, toggled: new Set() }

export function isSelected(entry: ChannelEntry, selection: ChannelSelection): boolean {
  if (entry.blocked) return false
  const byDefault = selection.baseline && !entry.downloaded
  return selection.toggled.has(entry.id) ? !byDefault : byDefault
}

export function toggleEntry(selection: ChannelSelection, id: string): ChannelSelection {
  const toggled = new Set(selection.toggled)
  if (!toggled.delete(id)) toggled.add(id)
  return { baseline: selection.baseline, toggled }
}

/** Chọn tất cả video đang hiện (kể cả video đã tải nếu đang hiện). */
export function selectAll(visible: ChannelEntry[]): ChannelSelection {
  return {
    baseline: true,
    toggled: new Set(visible.filter((entry) => entry.downloaded).map((entry) => entry.id)),
  }
}

export const SELECT_NONE: ChannelSelection = { baseline: false, toggled: new Set() }

export function pickedEntries(visible: ChannelEntry[], selection: ChannelSelection): ChannelEntry[] {
  return visible.filter((entry) => isSelected(entry, selection))
}

/** Tên file gọn `<ngày> <tiêu đề>` (app làm sạch ký tự cấm và cắt dài). */
function fileTitle(entry: ChannelEntry): string {
  return entry.timestamp === null ? entry.title : `${isoDay(entry.timestamp)} ${entry.title}`
}

/** Tên thư mục con: tên kênh (hoặc tên playlist). */
export function channelFolder(head: ChannelHead | null, profile: ChannelProfile | null): string | null {
  return head?.name || profile?.name || head?.handle || null
}

export function channelPayloads(
  entries: ChannelEntry[],
  quality: ChannelQuality,
  key: string | null,
  folder: string | null,
): AddMediaPayload[] {
  return entries.map((entry) => ({
    url: entry.url,
    title: fileTitle(entry),
    selector: quality.selector,
    audioFormat: quality.audioFormat,
    pageUrl: entry.url,
    subfolder: folder ?? undefined,
    channel: key ? { key, videoId: entry.id } : undefined,
  }))
}

type ListingStatus = "loading" | "done" | "stopped" | "error"

export interface ListingState {
  status: ListingStatus
  head: ChannelHead | null
  profile: ChannelProfile | null
  entries: ChannelEntry[]
  /** Lỗi hẳn (`error`), hoặc câu báo khi dừng giữa chừng mà vẫn còn danh sách đã nhận (`done`). */
  error: string | null
}

export const LISTING_START: ListingState = {
  status: "loading",
  head: null,
  profile: null,
  entries: [],
  error: null,
}

export function applyListingEvent(state: ListingState, event: ListingEvent): ListingState {
  switch (event.kind) {
    case "head":
      return { ...state, head: event.head }
    case "profile":
      return { ...state, profile: event.profile }
    case "entries":
      return { ...state, entries: [...state.entries, ...event.entries] }
    case "done": {
      const status: ListingStatus = event.complete
        ? "done"
        : event.error === null
          ? "stopped"
          : state.entries.length > 0
            ? "done"
            : "error"
      return { ...state, status, error: event.error }
    }
  }
}
