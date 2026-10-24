import { useEffect, useState } from "react"

import { formatNumber } from "@/shared/i18n/format"
import { commandErrorText, t } from "@/shared/i18n/i18n"

import { queueSettings, setQueueLimit } from "../api/downloads"
import { Segmented } from "./batch-dialog-parts"
import { SettingsSection } from "./settings-section"

/** Mặc định của hàng đợi (src-tauri/src/queue.rs `DEFAULT_LIMIT`). */
const DEFAULT_LIMIT = 3
const CHOICES = [1, 2, 3, 4, 5, 6, 8, 10]

/** Mục "Tải cùng lúc" (S-27): số download chạy một lúc, áp cho mọi download kể cả media. */
export function SettingsQueue({ open, desktop }: { open: boolean; desktop: boolean }) {
  const [limit, setLimit] = useState(DEFAULT_LIMIT)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open || !desktop) return
    let live = true
    queueSettings()
      .then((settings) => live && setLimit(settings.limit))
      .catch((reason: unknown) => live && setError(commandErrorText(reason)))
    return () => {
      live = false
    }
  }, [open, desktop])

  const change = (value: string) => {
    const next = Number(value)
    setLimit(next)
    setError(null)
    setQueueLimit(next)
      .then((settings) => setLimit(settings.limit))
      .catch((reason: unknown) => setError(commandErrorText(reason)))
  }

  return (
    <SettingsSection
      title={t("queue.title")}
      description={t("queue.description")}
    >
      <Segmented
        label={t("queue.limitLabel")}
        value={String(limit)}
        options={CHOICES.map((choice) => ({
          id: String(choice),
          label: formatNumber(choice, 0),
        }))}
        onChange={change}
        disabled={!desktop}
      />
      {error ? (
        <p role="alert" className="mt-2 rounded-lg bg-destructive/5 px-3 py-2 text-sm text-destructive">
          {error}
        </p>
      ) : null}
    </SettingsSection>
  )
}
