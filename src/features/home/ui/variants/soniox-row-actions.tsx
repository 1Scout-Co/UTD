import { Fragment, type ComponentType } from "react"

import {
  ExternalLink,
  FolderInput,
  FolderOpen,
  Link,
  ListX,
  MoreHorizontal,
  Pause,
  PenLine,
  Play,
  RotateCcw,
  Stethoscope,
  Trash2,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
} from "@/components/ui/context-menu"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { t } from "@/shared/i18n/i18n"

import type { DownloadItem, HomeScreenProps } from "../../model/types"

/** Những gì một hàng cần để chạy lệnh: thao tác file (app desktop) + thao tác task có sẵn. */
export type RowContext = Pick<
  HomeScreenProps,
  "onPause" | "onResume" | "onRemove" | "onDiagnose"
> & {
  desktop: boolean
  hasLink: (id: string) => boolean
  onOpen: (id: string) => void
  onReveal: (id: string) => void
  onRename: (item: DownloadItem) => void
  onMove: (id: string) => void
  onCopyLink: (id: string) => void
  onTrash: (id: string) => void
}

export type RowCommand = {
  key: string
  label: string
  icon: ComponentType<{ className?: string }>
  shortcut?: string
  destructive?: boolean
  disabled: boolean
  run: () => void
}

function fileCommands(item: DownloadItem, ctx: RowContext): RowCommand[] {
  const { id, status } = item
  const busy = status === "downloading" || status === "verifying"
  return [
    // File bị cách ly không mở được; "Mở thư mục" thì vẫn được.
    {
      key: "open",
      label: t("menu.open"),
      icon: ExternalLink,
      disabled: !ctx.desktop || status !== "completed",
      run: () => ctx.onOpen(id),
    },
    {
      key: "reveal",
      label: t("menu.reveal"),
      icon: FolderOpen,
      disabled: !ctx.desktop,
      run: () => ctx.onReveal(id),
    },
    {
      key: "rename",
      label: t("menu.rename"),
      icon: PenLine,
      shortcut: "F2",
      disabled: !ctx.desktop || status === "quarantined",
      run: () => ctx.onRename(item),
    },
    {
      key: "move",
      label: t("menu.move"),
      icon: FolderInput,
      disabled: !ctx.desktop || busy || status === "quarantined",
      run: () => ctx.onMove(id),
    },
    {
      key: "copy",
      label: t("menu.copyLink"),
      icon: Link,
      disabled: !ctx.hasLink(id),
      run: () => ctx.onCopyLink(id),
    },
  ]
}

function taskCommands(item: DownloadItem, ctx: RowContext): RowCommand[] {
  const { id, status } = item
  const pausable =
    status === "downloading" || status === "queued" || status === "verifying"
  const toggle: RowCommand = pausable
    ? {
        key: "pause",
        label: t("menu.pause"),
        icon: Pause,
        disabled: !ctx.desktop,
        run: () => ctx.onPause(id),
      }
    : {
        key: "resume",
        label: t("menu.resume"),
        icon: Play,
        disabled: !ctx.desktop || status !== "paused",
        run: () => ctx.onResume(id),
      }
  return [
    toggle,
    {
      key: "retry",
      label: t("menu.retry"),
      icon: RotateCcw,
      disabled: !ctx.desktop || status !== "failed",
      run: () => ctx.onResume(id),
    },
    {
      key: "diagnose",
      label: t("menu.diagnose"),
      icon: Stethoscope,
      disabled: status !== "failed" && status !== "quarantined",
      run: () => ctx.onDiagnose(id),
    },
  ]
}

function removeCommands(item: DownloadItem, ctx: RowContext): RowCommand[] {
  const { id, status } = item
  // Task đang chờ trong hàng đợi chưa chạy: bỏ khỏi danh sách được ngay (app bỏ nó khỏi hàng).
  const busy = status === "downloading" || status === "verifying"
  return [
    {
      key: "remove",
      label: t("menu.remove"),
      icon: ListX,
      disabled: !ctx.desktop || busy,
      run: () => ctx.onRemove(id),
    },
    {
      key: "trash",
      label: t("menu.trash"),
      icon: Trash2,
      destructive: true,
      disabled:
        !ctx.desktop || status === "downloading" || status === "verifying",
      run: () => ctx.onTrash(id),
    },
  ]
}

/** Menu kiểu IDM, chia 3 nhóm: file · task · xoá. Mục không hợp trạng thái thì mờ, không ẩn. */
export function rowCommands(
  item: DownloadItem,
  ctx: RowContext
): RowCommand[][] {
  return [
    fileCommands(item, ctx),
    taskCommands(item, ctx),
    removeCommands(item, ctx),
  ]
}

const PARTS = {
  context: {
    Item: ContextMenuItem,
    Separator: ContextMenuSeparator,
    Shortcut: ContextMenuShortcut,
  },
  dropdown: {
    Item: DropdownMenuItem,
    Separator: DropdownMenuSeparator,
    Shortcut: DropdownMenuShortcut,
  },
}

/** Vẽ cùng một danh sách lệnh cho menu chuột phải và menu của nút ⋯. */
export function RowMenuItems({
  groups,
  kind,
}: {
  groups: RowCommand[][]
  kind: keyof typeof PARTS
}) {
  const { Item, Separator, Shortcut } = PARTS[kind]
  return groups.map((group, index) => (
    <Fragment key={group[0]?.key ?? index}>
      {index > 0 ? <Separator /> : null}
      {group.map((command) => (
        <Item
          key={command.key}
          disabled={command.disabled}
          variant={command.destructive ? "destructive" : "default"}
          onClick={command.run}
          className="gap-2.5 px-2 py-1.5"
        >
          <command.icon />
          {command.label}
          {command.shortcut ? <Shortcut>{command.shortcut}</Shortcut> : null}
        </Item>
      ))}
    </Fragment>
  ))
}

/** Nút ⋯ cuối hàng: mở đúng menu như khi chuột phải vào hàng. */
export function SonioxRowActions({
  item,
  groups,
}: {
  item: DownloadItem
  groups: RowCommand[][]
}) {
  const trigger = (
    <Button
      variant="ghost"
      size="icon-sm"
      className="rounded-md text-muted-foreground hover:bg-card hover:text-foreground"
      aria-label={t("menu.optionsFor", { name: item.name })}
      title={t("table.options")}
    />
  )

  return (
    <DropdownMenu>
      <DropdownMenuTrigger render={trigger}>
        <MoreHorizontal className="size-4" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-60">
        <RowMenuItems groups={groups} kind="dropdown" />
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
