import { Fragment, useSyncExternalStore, type ReactNode } from "react"

import { formatNumber, setFormatLocale } from "./format"
import {
  detectLanguage,
  isLanguageCode,
  isRtl,
  languageOf,
  type LanguageCode,
} from "./languages"
import { savedLanguage, saveLanguage } from "./storage"

// Chuỗi giao diện: khoá phẳng "nhóm.tên" trong locales/<code>.json, tiếng Việt (vi.json) là nguồn.
// Số nhiều: "khoá#one", "khoá#other"… theo Intl.PluralRules, gọi t("khoá", { count }).
// Khoá sai là lỗi lúc build vì kiểu MessageKey lấy từ vi.json.

type Source = typeof import("./locales/vi.json")
type Base<K> = K extends `${infer B}#${string}` ? B : K
export type MessageKey = Base<keyof Source & string>
type Messages = Record<string, string>
type Vars = Record<string, string | number>

// Mỗi file dịch là một tệp riêng trong dist/assets (không nằm trong JS): chỉ tải file đang dùng.
const FILES = import.meta.glob<string>("./locales/*.json", {
  eager: true,
  query: "?url",
  import: "default",
})

let current: LanguageCode = "vi"
let messages: Messages = {}
// Thiếu khoá (file dịch chưa kịp cập nhật) thì lấy tiếng Anh trước khi hiện khoá trần.
let fallback: Messages = {}
let plural = new Intl.PluralRules("vi-VN")
const listeners = new Set<() => void>()
const loaded = new Map<LanguageCode, Promise<Messages>>()

function load(code: LanguageCode): Promise<Messages> {
  let pending = loaded.get(code)
  if (!pending) {
    const url = FILES[`./locales/${code}.json`]
    pending = fetch(url).then((response) => {
      if (!response.ok) throw new Error(`locale ${code}: ${response.status}`)
      return response.json() as Promise<Messages>
    })
    pending.catch(() => loaded.delete(code))
    loaded.set(code, pending)
  }
  return pending
}

/** Tải file dịch rồi áp ngay: chữ, hướng viết (RTL), định dạng số/ngày. */
async function apply(code: LanguageCode) {
  const [main, backup] = await Promise.all([
    load(code),
    code === "en" ? Promise.resolve({}) : load("en").catch(() => ({})),
  ])
  const { locale } = languageOf(code)
  current = code
  messages = main
  fallback = backup
  plural = new Intl.PluralRules(locale)
  setFormatLocale(locale)
  document.documentElement.lang = code
  document.documentElement.dir = isRtl(code) ? "rtl" : "ltr"
  listeners.forEach((listener) => listener())
}

/** Gọi một lần trước khi vẽ app: ngôn ngữ đã lưu, chưa có thì theo hệ điều hành (không khớp ⇒ English). */
export async function startI18n() {
  const saved = await savedLanguage().catch(() => null)
  const code = isLanguageCode(saved)
    ? saved
    : detectLanguage(navigator.languages)
  await apply(code).catch(() => apply("en"))
}

/** Người dùng chọn trong Cài đặt: áp ngay, lưu bền. */
export async function changeLanguage(code: LanguageCode) {
  await apply(code)
  await saveLanguage(code)
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

/** Ngôn ngữ đang dùng; component gọi hook này thì vẽ lại khi đổi ngôn ngữ. */
export function useLanguage(): LanguageCode {
  return useSyncExternalStore(subscribe, () => current)
}

function pick(dict: Messages, key: string, count: unknown): string | undefined {
  if (typeof count === "number") {
    return (
      dict[`${key}#${plural.select(count)}`] ??
      dict[`${key}#other`] ??
      dict[key]
    )
  }
  return dict[key]
}

function fill(text: string, vars: Vars): string {
  return text.replace(/\{(\w+)\}/g, (whole, name: string) => {
    const value = vars[name]
    if (value === undefined) return whole
    return typeof value === "number" ? formatNumber(value, 2) : value
  })
}

/** Khoá ghép lúc chạy (mã lỗi từ Rust…): undefined khi không có. */
function lookup(key: string, vars?: Vars): string | undefined {
  const text =
    pick(messages, key, vars?.count) ?? pick(fallback, key, vars?.count)
  return text === undefined ? undefined : vars ? fill(text, vars) : text
}

export function t(key: MessageKey, vars?: Vars): string {
  return lookup(key, vars) ?? key
}

/** Như t() nhưng chỗ {tên} là thành phần React (chữ in đậm, <code>…). */
export function tx(
  key: MessageKey,
  nodes: Record<string, ReactNode>
): ReactNode[] {
  return t(key)
    .split(/\{(\w+)\}/)
    .map((part, index) =>
      index % 2 === 1 ? (
        <Fragment key={index}>{nodes[part] ?? `{${part}}`}</Fragment>
      ) : (
        part
      )
    )
}

/**
 * Câu lỗi từ Rust: dịch theo `code` ổn định (err.<code> trong file dịch, mã http.<số> dùng chung một câu);
 * mã chưa có câu dịch thì hiện nguyên câu gốc.
 */
export function errorText(
  code: string | null | undefined,
  message?: string | null
): string {
  if (code) {
    const known = lookup(`err.${code}`)
    if (known) return known
    const status = /^http\.(\d{3})$/.exec(code)?.[1]
    const http = status ? lookup("err.http", { status }) : undefined
    if (http) return http
  }
  return message ?? ""
}
