// Ngôn ngữ của giao diện. Thêm ngôn ngữ: thêm một dòng ở đây + file locales/<code>.json (đủ khoá như vi.json)
// + cờ flags/<flag>.svg; scripts/i18n-check.mjs kiểm cả ba (docs/specs/2026-10-23-utd-v1/notes/i18n.md).

export const LANGUAGES = [
  { code: "vi", name: "Tiếng Việt", flag: "VN", locale: "vi-VN" },
  { code: "en", name: "English", flag: "US", locale: "en-US" },
  { code: "zh-CN", name: "简体中文", flag: "CN", locale: "zh-CN" },
  { code: "zh-TW", name: "繁體中文", flag: "TW", locale: "zh-TW" },
  { code: "ja", name: "日本語", flag: "JP", locale: "ja-JP" },
  { code: "ko", name: "한국어", flag: "KR", locale: "ko-KR" },
  { code: "th", name: "ไทย", flag: "TH", locale: "th-TH" },
  { code: "id", name: "Bahasa Indonesia", flag: "ID", locale: "id-ID" },
  { code: "hi", name: "हिन्दी", flag: "IN", locale: "hi-IN" },
  { code: "es", name: "Español", flag: "ES", locale: "es-ES" },
  { code: "pt-BR", name: "Português (Brasil)", flag: "BR", locale: "pt-BR" },
  { code: "fr", name: "Français", flag: "FR", locale: "fr-FR" },
  { code: "de", name: "Deutsch", flag: "DE", locale: "de-DE" },
  { code: "ru", name: "Русский", flag: "RU", locale: "ru-RU" },
  { code: "tr", name: "Türkçe", flag: "TR", locale: "tr-TR" },
  // "ar" chứ không "ar-SA": ar-SA đổi ngày sang lịch Hijri.
  { code: "ar", name: "العربية", flag: "SA", locale: "ar", rtl: true },
] as const

export type Language = (typeof LANGUAGES)[number]
export type LanguageCode = Language["code"]

/** Không khớp ngôn ngữ nào của hệ điều hành ⇒ English. */
export const FALLBACK_LANGUAGE: LanguageCode = "en"

export function languageOf(code: LanguageCode): Language {
  return LANGUAGES.find((language) => language.code === code) ?? LANGUAGES[0]
}

export function isLanguageCode(value: unknown): value is LanguageCode {
  return LANGUAGES.some((language) => language.code === value)
}

export function isRtl(code: LanguageCode): boolean {
  return "rtl" in languageOf(code)
}

// Tiếng Trung chia theo chữ: Giản thể (CN, SG, Hans) / Phồn thể (TW, HK, MO, Hant).
function chinese(tag: string): LanguageCode {
  return /-(tw|hk|mo|hant)\b/.test(tag) ? "zh-TW" : "zh-CN"
}

/** Một thẻ ngôn ngữ của hệ điều hành (`en-GB`, `zh-Hant-TW`, `pt-PT`…) → ngôn ngữ UTD gần nhất. */
function matchTag(raw: string): LanguageCode | null {
  const tag = raw.toLowerCase().replace(/_/g, "-")
  const primary = tag.split("-")[0]
  if (primary === "zh") return chinese(tag)
  if (primary === "pt") return "pt-BR"
  // "in" là mã cũ của tiếng Indonesia (Java/Android đời đầu).
  if (primary === "in") return "id"
  return LANGUAGES.find((language) => language.code === primary)?.code ?? null
}

/** Lần đầu mở app: theo thứ tự ngôn ngữ ưa thích của hệ điều hành; không khớp thì English. */
export function detectLanguage(tags: readonly string[]): LanguageCode {
  for (const tag of tags) {
    const match = matchTag(tag)
    if (match) return match
  }
  return FALLBACK_LANGUAGE
}
