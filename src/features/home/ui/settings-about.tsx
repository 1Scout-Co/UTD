import { t } from "@/shared/i18n/i18n"

import { MoreInfo, SettingsSection } from "./settings-section"

/** Công cụ UTD tự tải (không đóng gói trong bộ cài): nguồn chính chủ và giấy phép gốc (hàm: chữ theo ngôn ngữ). */
const TOOL_CREDITS = [
  {
    name: "yt-dlp",
    license: () => t("about.ytdlpLicense"),
    source: "github.com/yt-dlp/yt-dlp",
  },
  {
    name: "deno",
    license: () => "MIT",
    source: "github.com/denoland/deno",
  },
  {
    name: "ffmpeg",
    license: () => "GPLv3",
    source:
      "Windows: github.com/yt-dlp/FFmpeg-Builds · macOS: ffmpeg.martin-riedl.de",
  },
]

/** Mục "Giới thiệu": giấy phép của UTD, nguồn và giấy phép công cụ media. */
export function SettingsAbout({ version }: { version: string | null }) {
  return (
    <SettingsSection
      title={t("about.title")}
      description={t("about.description", {
        app: version ? `UTD ${version}` : "UTD",
      })}
    >
      <MoreInfo label={t("tools.title")}>
        <ul className="space-y-1.5">
          {TOOL_CREDITS.map((tool) => (
            <li key={tool.name}>
              <span className="font-semibold text-foreground">{tool.name}</span>{" "}
              · {tool.license()} ·{" "}
              <code className="break-all">{tool.source}</code>
            </li>
          ))}
        </ul>
        <p>{t("about.toolsNote")}</p>
      </MoreInfo>
    </SettingsSection>
  )
}
