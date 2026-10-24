import { useState } from "react"

import { ChevronDown, Languages, Loader2 } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Flag } from "@/shared/i18n/flag"
import { changeLanguage, t, useLanguage } from "@/shared/i18n/i18n"
import { isLanguageCode, LANGUAGES, languageOf } from "@/shared/i18n/languages"

import { SettingsSection } from "./settings-section"

/** Đổi ngôn ngữ: áp ngay cho cả app, lưu trong cài đặt của app. Lỗi lưu thì báo, chữ vẫn đã đổi. */
function useLanguagePicker() {
  const language = useLanguage()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(false)

  const pick = (value: unknown) => {
    if (!isLanguageCode(value) || value === language) return
    setBusy(true)
    setError(false)
    void changeLanguage(value)
      .catch(() => setError(true))
      .finally(() => setBusy(false))
  }

  return { language, busy, error, pick }
}

/** Mục "Ngôn ngữ" đầu Cài đặt: danh sách có cờ + tên bản địa (người không đọc được ngôn ngữ hiện tại vẫn nhận ra). */
export function SettingsLanguage() {
  const { language, busy, error, pick } = useLanguagePicker()
  const current = languageOf(language)

  return (
    <SettingsSection
      title={t("lang.title")}
      description={t("lang.description")}
    >
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button
              type="button"
              variant="outline"
              aria-label={`${t("lang.title")}: ${current.name}`}
              className="min-w-56 justify-start"
            />
          }
        >
          {busy ? <Loader2 className="animate-spin" /> : <Languages />}
          <Flag code={current.flag} />
          <span className="flex-1 text-start">{current.name}</span>
          <ChevronDown className="text-muted-foreground" />
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="max-h-80 w-60">
          <DropdownMenuRadioGroup value={language} onValueChange={pick}>
            {LANGUAGES.map((item) => (
              <DropdownMenuRadioItem
                key={item.code}
                value={item.code}
                lang={item.code}
                className="gap-2.5 py-1.5"
              >
                <Flag code={item.flag} />
                {item.name}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
      {error ? (
        <p
          role="alert"
          className="mt-2 rounded-lg bg-destructive/5 px-3 py-2 text-sm text-destructive"
        >
          {t("lang.saveFailed")}
        </p>
      ) : null}
    </SettingsSection>
  )
}
