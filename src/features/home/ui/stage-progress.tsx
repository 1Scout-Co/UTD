import { Progress } from "@/components/ui/progress"

// Base UI không đặt bề rộng khi chưa biết % ⇒ rãnh trống trông như đứng im: hiện vạch ngắn nhấp nháy.
const INDETERMINATE =
  "[&_[data-slot=progress-indicator][data-indeterminate]]:w-1/3 [&_[data-slot=progress-indicator][data-indeterminate]]:animate-pulse"

/** Thanh tiến độ tải/cài trong Cài đặt; `percent` null = chưa biết dung lượng. */
export function StageProgress({
  percent,
  label,
}: {
  percent: number | null
  label: string
}) {
  return <Progress value={percent} aria-label={label} className={INDETERMINATE} />
}

/** % đã tải; null khi máy chủ không báo dung lượng. */
export function percentOf(done: number, total: number | null): number | null {
  if (total === null || total <= 0) return null
  return Math.min(100, Math.round((done / total) * 100))
}
