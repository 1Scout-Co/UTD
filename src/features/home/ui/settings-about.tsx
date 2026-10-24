import { MoreInfo, SettingsSection } from "./settings-section"

/** Công cụ UTD tự tải (không đóng gói trong bộ cài): nguồn chính chủ và giấy phép gốc. */
const TOOL_CREDITS = [
  {
    name: "yt-dlp",
    license: "Unlicense (bản chạy được gồm thành phần GPLv3+)",
    source: "github.com/yt-dlp/yt-dlp",
  },
  {
    name: "deno",
    license: "MIT",
    source: "github.com/denoland/deno",
  },
  {
    name: "ffmpeg",
    license: "GPLv3",
    source:
      "Windows: github.com/yt-dlp/FFmpeg-Builds · macOS: ffmpeg.martin-riedl.de",
  },
]

/** Mục "Giới thiệu": giấy phép của UTD, nguồn và giấy phép công cụ media. */
export function SettingsAbout({ version }: { version: string | null }) {
  return (
    <SettingsSection
      title="Giới thiệu"
      description={`${version ? `UTD ${version}` : "UTD"} · mã nguồn mở Apache-2.0.`}
    >
      <MoreInfo label="Công cụ media">
        <ul className="space-y-1.5">
          {TOOL_CREDITS.map((tool) => (
            <li key={tool.name}>
              <span className="font-semibold text-foreground">{tool.name}</span>{" "}
              · {tool.license} ·{" "}
              <code className="break-all">{tool.source}</code>
            </li>
          ))}
        </ul>
        <p>
          Bộ cài UTD không kèm các công cụ này. Lần đầu mở app, UTD tải bản chính
          chủ, kiểm SHA-256 rồi lưu riêng trong thư mục công cụ; giấy phép của
          ffmpeg được giữ cạnh file chạy.
        </p>
      </MoreInfo>
    </SettingsSection>
  )
}
