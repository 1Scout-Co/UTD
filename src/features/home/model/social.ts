// Tải hàng loạt Facebook / Instagram / Threads (notes/social-bulk.md): tiện ích liệt kê ảnh/video trong trang đã đăng
// nhập, app giữ danh sách (src-tauri/src/social) và mở hộp Tải cả kênh ở chế độ hàng loạt. Kiểu dữ liệu và luật lọc
// riêng của chế độ này; phần lọc ngày / chọn dùng chung với kênh (model/channel.ts). Không biết giao diện.

import { dayStart } from "@/shared/dates"
import type { MessageKey } from "@/shared/i18n/i18n"

import type { ChannelEntry, ChannelFilter } from "./channel"

type SocialPlatform = "facebook" | "instagram" | "threads"
export type SocialSource = "posts" | "photos" | "videos" | "reels" | "stories" | "highlights"
export type SocialKind = "photo" | "video"

/** Thứ tự chip trong hộp (tiện ích lấy Tin trước vì Tin hết hạn sau 24 giờ). */
export const SOCIAL_SOURCES: SocialSource[] = ["posts", "photos", "videos", "reels", "stories", "highlights"]

export const SOURCE_LABELS: Record<SocialSource, MessageKey> = {
  posts: "social.source.posts",
  photos: "social.source.photos",
  videos: "social.source.videos",
  reels: "social.source.reels",
  stories: "social.source.stories",
  highlights: "social.source.highlights",
}

/** Một ảnh/video (src-tauri/src/social/entries.rs): như video của kênh, cộng nhóm, loại và link file. */
export interface SocialEntry extends ChannelEntry {
  source: SocialSource
  kind: SocialKind
  /** Link file trên CDN (tải thẳng); null ⇒ video tải bằng yt-dlp từ `page`. */
  file: string | null
  page: string | null
  /** Tên tin nổi bật. */
  album: string | null
}

/** Danh sách mới từ trình duyệt: hộp mở với mã này. */
export interface SocialRequest {
  listingId: string
  url: string
  platform: SocialPlatform
}

export const isSocialEntry = (entry: ChannelEntry): entry is SocialEntry => "source" in entry && "kind" in entry

/** Số mục theo nhóm, theo thứ tự chip; nhóm trống không có. */
export function sourceCounts(entries: SocialEntry[]): [SocialSource, number][] {
  const counts = new Map<SocialSource, number>()
  for (const entry of entries) counts.set(entry.source, (counts.get(entry.source) ?? 0) + 1)
  return SOCIAL_SOURCES.filter((source) => counts.has(source)).map((source) => [source, counts.get(source) ?? 0])
}

export function kindCounts(entries: SocialEntry[]): Record<SocialKind, number> {
  const counts = { photo: 0, video: 0 }
  for (const entry of entries) counts[entry.kind] += 1
  return counts
}

/** Bấm một chip: chỉ lọc theo các chip đang bật; `null` (không chip nào bật) = mọi nhóm/loại. */
export function toggleIn<T>(current: ReadonlySet<T> | null, value: T): ReadonlySet<T> | null {
  const next = new Set(current)
  if (!next.delete(value)) next.add(value)
  return next.size === 0 ? null : next
}

/** Mục thuộc nhóm và loại đang chọn. */
export function inScope(
  entries: SocialEntry[],
  sources: ReadonlySet<SocialSource> | null,
  kinds: ReadonlySet<SocialKind> | null,
): SocialEntry[] {
  if (!sources && !kinds) return entries
  return entries.filter((entry) => (!sources || sources.has(entry.source)) && (!kinds || kinds.has(entry.kind)))
}

/** Mốc "từ ngày" gửi tiện ích để thôi lấy mục cũ hơn. */
export function sinceOf(filter: ChannelFilter): number | null {
  return filter.range === "dates" ? dayStart(filter.from) : null
}

/** Mục không có chú thích (ảnh, Tin): tên hiện là nhóm (+ tên tin nổi bật). */
export function withTitles(entries: SocialEntry[], label: (source: SocialSource) => string): SocialEntry[] {
  return entries.map((entry) =>
    entry.title ? entry : { ...entry, title: [label(entry.source), entry.album].filter(Boolean).join(" · ") },
  )
}
