import { Globe } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

import { SettingsBrowser } from "./settings-browser"
import { SettingsDownloadDir } from "./settings-download-dir"
import { SettingsMediaTools } from "./settings-media-tools"
import { SettingsQueue } from "./settings-queue"
import { SettingsSection as Section, SettingsPrivacy } from "./settings-section"
import { SettingsUpdates, useAppVersion } from "./settings-updates"

/** Chân cố định: số phiên bản + Đóng (nằm ngoài vùng cuộn). */
function SettingsFooter({
  version,
  onClose,
}: {
  version: string | null
  onClose: () => void
}) {
  return (
    <div className="-mx-6 -mb-6 flex items-center justify-between gap-3 rounded-b-2xl border-t border-border/60 px-6 py-3.5">
      <span className="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
        <Globe className="size-3.5" />
        {version ? `UTD ${version}` : "UTD"} · Apache-2.0
      </span>
      <Button variant="outline" type="button" onClick={onClose}>
        Đóng
      </Button>
    </div>
  )
}

type SettingsDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
  desktop: boolean
  defaultDir: string
  onDefaultDirChange: (dir: string) => void
  /** Mở thẳng tới một mục (bấm chấm "Mới" cạnh Cài đặt ⇒ mục Cập nhật). */
  focus?: "updates" | null
}

export function SettingsDialog(props: SettingsDialogProps) {
  const { open, onOpenChange, desktop, defaultDir, onDefaultDirChange } = props
  const version = useAppVersion(desktop)
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        data-s="S-27"
        data-variant="soniox"
        className="gap-0 sm:max-w-[640px]"
      >
        <DialogHeader>
          <DialogTitle>Cài đặt</DialogTitle>
          <DialogDescription>
            Thư mục tải, tiện ích trình duyệt, công cụ media và cập nhật.
          </DialogDescription>
        </DialogHeader>

        {/* Thân cuộn riêng, chân cố định: mở hộp thoại không bị nhảy xuống cuối vì focus nút Đóng. */}
        <div className="-mx-6 mt-4 max-h-[62vh] divide-y divide-border/60 overflow-y-auto border-t border-border/60 px-6 pt-4">
          <Section
            title="Tải xuống"
            description="Download mới sẽ lưu vào thư mục này."
          >
            <SettingsDownloadDir
              desktop={desktop}
              dir={defaultDir}
              onChange={onDefaultDirChange}
            />
          </Section>

          <SettingsQueue open={open} desktop={desktop} />

          <SettingsBrowser open={open} desktop={desktop} />

          <Section
            title="Công cụ media"
            description="yt-dlp tải video, ffmpeg ghép hình và tiếng."
          >
            <SettingsMediaTools desktop={desktop} />
          </Section>

          <SettingsUpdates
            open={open}
            desktop={desktop}
            version={version}
            focused={props.focus === "updates"}
          />

          <SettingsPrivacy />
        </div>

        <SettingsFooter
          version={version}
          onClose={() => onOpenChange(false)}
        />
      </DialogContent>
    </Dialog>
  )
}
