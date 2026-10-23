import type { ReactNode, Ref } from "react"

import { ChevronRight } from "lucide-react"

import { cn } from "cn"

/** Một mục trong Cài đặt: tiêu đề + tối đa một dòng mô tả, rồi đúng một cụm hành động. */
export function SettingsSection({
  title,
  description,
  aside,
  ref,
  children,
}: {
  title: string
  description?: string
  aside?: ReactNode
  ref?: Ref<HTMLElement>
  children: ReactNode
}) {
  return (
    <section ref={ref} className="scroll-mt-4 py-5 first:pt-1 last:pb-1">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 className="text-md font-semibold">{title}</h3>
          {description ? (
            <p className="mt-0.5 text-sm text-muted-foreground">
              {description}
            </p>
          ) : null}
        </div>
        {aside}
      </div>
      <div className="mt-3">{children}</div>
    </section>
  )
}

/** Mục "An toàn & riêng tư": một dòng tóm tắt, chi tiết giấu sau "Chi tiết". */
export function SettingsPrivacy() {
  return (
    <SettingsSection
      title="An toàn & riêng tư"
      description="Không telemetry, không cloud. File đáng ngờ bị cách ly."
    >
      <MoreInfo label="Chi tiết">
        <p>
          Dữ liệu task nằm trong journal SQLite trên máy. File thực thi/script
          hoặc giả mạo định dạng bị Download Guard cách ly vào{" "}
          <code className="break-all">%LOCALAPPDATA%\UTD-Data\quarantine</code>.
          File an toàn được gắn Mark-of-the-Web. SHA-256 chỉ dùng khi bạn nhập.
        </p>
      </MoreInfo>
    </SettingsSection>
  )
}

/** Phần giải thích dài (ID, đường dẫn, các bước) giấu sau một dòng bấm mở. */
export function MoreInfo({
  label,
  className,
  children,
}: {
  label: string
  className?: string
  children: ReactNode
}) {
  return (
    <details className={cn("group", className)}>
      <summary className="inline-flex cursor-pointer list-none items-center gap-1 rounded-md text-sm font-medium text-primary outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 [&::-webkit-details-marker]:hidden">
        <ChevronRight className="size-3.5 transition-transform group-open:rotate-90" />
        {label}
      </summary>
      <div className="mt-2 space-y-3 rounded-xl bg-muted p-3.5 text-sm leading-relaxed text-muted-foreground">
        {children}
      </div>
    </details>
  )
}
