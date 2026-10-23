import { useState, type FormEvent } from "react"

import { Loader2, PenLine } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"

import type { DownloadItem } from "../model/types"

const INVALID = /[\\/:*?"<>|]/

function messageOf(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason)
}

type RenameProps = {
  item: DownloadItem
  onClose: () => void
  onSubmit: (id: string, name: string) => Promise<void>
}

/** Kiểm tên (không rỗng, không ký tự Windows cấm) rồi gửi; lỗi engine hiện ngay dưới ô nhập. */
function useRenameForm({ item, onClose, onSubmit }: RenameProps) {
  const [name, setName] = useState(item.name)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    const next = name.trim()
    if (!next) return setError("Nhập tên file.")
    if (INVALID.test(next))
      return setError('Tên không được có \\ / : * ? " < > |')
    if (next === item.name) return onClose()
    setBusy(true)
    setError(null)
    try {
      await onSubmit(item.id, next)
      onClose()
    } catch (reason) {
      setError(messageOf(reason))
    } finally {
      setBusy(false)
    }
  }

  return { name, setName, busy, error, submit }
}

function RenameForm(props: RenameProps) {
  const { item, onClose } = props
  const { name, setName, busy, error, submit } = useRenameForm(props)

  return (
    <form className="space-y-4" onSubmit={(event) => void submit(event)}>
      <Input
        aria-label="Tên file mới"
        value={name}
        autoFocus
        spellCheck={false}
        disabled={busy}
        onChange={(event) => setName(event.target.value)}
        // Chọn sẵn phần tên, chừa đuôi file như Explorer.
        onFocus={(event) => {
          const dot = item.name.lastIndexOf(".")
          event.currentTarget.setSelectionRange(
            0,
            dot > 0 ? dot : item.name.length
          )
        }}
      />
      {error ? (
        <p
          role="alert"
          className="rounded-lg bg-destructive/5 px-3 py-2 text-sm text-destructive"
        >
          {error}
        </p>
      ) : null}
      <div className="flex justify-end gap-2">
        <Button type="button" variant="ghost" disabled={busy} onClick={onClose}>
          Huỷ
        </Button>
        <Button data-primary type="submit" disabled={busy}>
          {busy ? <Loader2 className="animate-spin" /> : <PenLine />}
          Đổi tên
        </Button>
      </div>
    </form>
  )
}

/** Đổi tên file đã tải (F2 hoặc menu chuột phải). Không ghi đè file khác: engine báo lỗi nếu trùng. */
export function RenameDialog({
  item,
  onClose,
  onSubmit,
}: {
  item: DownloadItem | null
  onClose: () => void
  onSubmit: (id: string, name: string) => Promise<void>
}) {
  return (
    <Dialog
      open={item !== null}
      onOpenChange={(open) => (open ? null : onClose())}
    >
      <DialogContent data-variant="soniox" className="sm:max-w-[480px]">
        <DialogHeader>
          <DialogTitle>Đổi tên file</DialogTitle>
          <DialogDescription>File trên đĩa đổi tên theo.</DialogDescription>
        </DialogHeader>
        {item ? (
          <RenameForm
            key={item.id}
            item={item}
            onClose={onClose}
            onSubmit={onSubmit}
          />
        ) : null}
      </DialogContent>
    </Dialog>
  )
}
