import { useState } from "react"

import { FolderOpen, Info, Loader2, RotateCcw } from "lucide-react"

import { Button } from "@/components/ui/button"
import { commandErrorText, t } from "@/shared/i18n/i18n"

import {
  pickFolder,
  resetDefaultDownloadDir,
  setDefaultDownloadDir,
} from "../api/downloads"

type Busy = "pick" | "reset" | null

/** Đổi / bỏ thư mục mặc định; báo lại thư mục mới qua `onChange`. */
function useDefaultDir(dir: string, onChange: (dir: string) => void) {
  const [busy, setBusy] = useState<Busy>(null)
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)

  const run = async (
    kind: Exclude<Busy, null>,
    task: () => Promise<string | null>,
  ) => {
    setBusy(kind)
    setError(null)
    setSaved(false)
    try {
      const next = await task()
      if (next) {
        onChange(next)
        setSaved(true)
      }
    } catch (reason) {
      setError(commandErrorText(reason))
    } finally {
      setBusy(null)
    }
  }

  const change = () =>
    run("pick", async () => {
      const picked = await pickFolder(dir)
      return picked ? setDefaultDownloadDir(picked) : null
    })
  const reset = () => run("reset", resetDefaultDownloadDir)

  return { busy, error, saved, change, reset }
}

function FolderPath({ dir }: { dir: string }) {
  return (
    <div
      className="flex h-9 min-w-48 flex-1 items-center gap-2 rounded-lg bg-muted px-3 text-sm"
      title={dir}
    >
      <FolderOpen className="size-4 shrink-0 text-muted-foreground" aria-hidden />
      <span className="sr-only">{t("dir.current")}</span>
      <span className="truncate">{dir || "Downloads\\UTD"}</span>
    </div>
  )
}

/** Dòng "Thư mục tải" trong Cài đặt (S-27): xem, đổi, quay về mặc định. */
export function SettingsDownloadDir({
  desktop,
  dir,
  onChange,
}: {
  desktop: boolean
  dir: string
  onChange: (dir: string) => void
}) {
  const { busy, error, saved, change, reset } = useDefaultDir(dir, onChange)
  const locked = !desktop || busy !== null
  const spinner = <Loader2 className="animate-spin" />

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <FolderPath dir={dir} />
        <Button
          type="button"
          variant="outline"
          disabled={locked}
          onClick={() => void change()}
        >
          {busy === "pick" ? spinner : <FolderOpen />}
          {t("dir.change")}
        </Button>
        <Button
          type="button"
          variant="ghost"
          disabled={locked}
          onClick={() => void reset()}
        >
          {busy === "reset" ? spinner : <RotateCcw />}
          {t("dir.reset")}
        </Button>
      </div>
      {!desktop ? (
        <p className="flex items-center gap-1.5 text-sm text-muted-foreground">
          <Info className="size-3.5 shrink-0" />
          {t("dir.desktopOnly")}
        </p>
      ) : null}
      <p role="status" className="text-sm text-emerald-700 empty:hidden">
        {saved ? t("dir.saved") : ""}
      </p>
      {error ? (
        <p
          role="alert"
          className="rounded-lg bg-destructive/5 px-3 py-2 text-sm text-destructive"
        >
          {error}
        </p>
      ) : null}
    </div>
  )
}
