#!/usr/bin/env node
// Kiểm đa ngôn ngữ (chạy trong `pnpm lint` và CI). Lỗi nào cũng in cách sửa; xem thêm
// docs/specs/2026-10-23-utd-v1/notes/i18n.md.
//   1. Mỗi ngôn ngữ trong src/shared/i18n/languages.ts có file dịch + cờ; không có file thừa.
//   2. Mọi file dịch có đúng bộ khoá của vi.json (nguồn): không thiếu, không thừa, đủ dạng số nhiều
//      theo Intl.PluralRules, cùng {chỗ trống}.
//   3. Mã lỗi Rust (`media.forbidden`, `http.not_a_file`…) có câu dịch err.<mã>.
//   4. Tiện ích trình duyệt: _locales của Chromium và Firefox đủ ngôn ngữ, đủ khoá như en, hai bên giống nhau.
//   5. Không còn chuỗi tiếng Việt viết cứng trong src/ (ngoài chú thích).

import { existsSync, readdirSync, readFileSync } from "node:fs"
import { join, relative } from "node:path"
import { fileURLToPath } from "node:url"

const root = fileURLToPath(new URL("..", import.meta.url))
const I18N = join(root, "src/shared/i18n")
const errors = []
const fail = (message) => errors.push(message)
const read = (path) => readFileSync(path, "utf8")

function readLanguages() {
  const source = read(join(I18N, "languages.ts"))
  const pattern = /code: "([^"]+)", name: "[^"]+", flag: "([A-Z]{2})", locale: "([^"]+)"/g
  return [...source.matchAll(pattern)].map(([, code, flag, locale]) => ({ code, flag, locale }))
}

const placeholders = (text) => [...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort()
const baseOf = (key) => key.split("#")[0]

function checkPlural(code, locale, key, messages) {
  const wanted = new Intl.PluralRules(locale).resolvedOptions().pluralCategories
  const have = Object.keys(messages).filter((name) => baseOf(name) === key)
  for (const category of wanted) {
    if (!have.includes(`${key}#${category}`)) {
      fail(`${code}.json: thiếu dạng số nhiều "${key}#${category}" (${locale} cần: ${wanted.join(", ")}).`)
    }
  }
  for (const name of have) {
    const category = name.split("#")[1]
    if (!wanted.includes(category)) fail(`${code}.json: "${name}" thừa — ${locale} chỉ có ${wanted.join(", ")}.`)
  }
}

function checkPlaceholders(code, key, text, source) {
  const want = placeholders(source)
  const got = placeholders(text)
  const plural = key.includes("#") && !key.endsWith("#other")
  // Dạng "một" có thể viết chữ thay cho {count} ("one item").
  const ok = plural
    ? got.every((name) => want.includes(name)) && want.every((name) => name === "count" || got.includes(name))
    : want.join() === got.join()
  if (!ok) fail(`${code}.json: "${key}" phải giữ đúng {chỗ trống} như vi.json: ${want.map((n) => `{${n}}`).join(" ") || "(không có)"}.`)
}

function checkLocale(language, source) {
  const path = join(I18N, "locales", `${language.code}.json`)
  if (!existsSync(path)) return fail(`Thiếu ${relative(root, path)}: chép vi.json rồi dịch.`)
  let messages
  try {
    messages = JSON.parse(read(path))
  } catch (error) {
    return fail(`${language.code}.json không phải JSON hợp lệ: ${error.message}`)
  }
  const sourceBases = new Set(Object.keys(source).map(baseOf))
  const plural = new Set(Object.keys(source).filter((key) => key.endsWith("#other")).map(baseOf))
  for (const key of sourceBases) {
    if (plural.has(key)) checkPlural(language.code, language.locale, key, messages)
    else if (!(key in messages)) fail(`${language.code}.json: thiếu khoá "${key}".`)
  }
  for (const [key, text] of Object.entries(messages)) {
    const base = baseOf(key)
    if (!sourceBases.has(base)) {
      fail(`${language.code}.json: khoá thừa "${key}" (không có trong vi.json): xoá đi.`)
      continue
    }
    if (typeof text !== "string" || !text.trim()) {
      fail(`${language.code}.json: "${key}" rỗng.`)
      continue
    }
    if (key.includes("#") !== plural.has(base)) {
      fail(`${language.code}.json: "${key}" sai dạng — ${plural.has(base) ? "khoá số nhiều cần đuôi #<dạng>" : "khoá thường không có đuôi #"}.`)
      continue
    }
    const origin = source[key] ?? source[`${base}#other`]
    if (origin !== undefined) checkPlaceholders(language.code, key, text, origin)
  }
}

function checkLanguages(languages) {
  if (!languages.some((language) => language.code === "vi")) fail("languages.ts: không đọc được danh sách ngôn ngữ.")
  const source = JSON.parse(read(join(I18N, "locales/vi.json")))
  for (const language of languages) {
    checkLocale(language, source)
    if (!existsSync(join(I18N, "flags", `${language.flag}.svg`))) {
      fail(`Thiếu cờ src/shared/i18n/flags/${language.flag}.svg (chép từ country-flag-icons 3x2).`)
    }
  }
  const codes = new Set(languages.map((language) => language.code))
  for (const file of readdirSync(join(I18N, "locales"))) {
    if (!codes.has(file.replace(/\.json$/, ""))) fail(`locales/${file} không có trong languages.ts: thêm ngôn ngữ hoặc xoá file.`)
  }
  return source
}

// Phần test (`#[cfg(test)]` trở đi) có tên file mẫu kiểu "tool.exe", không phải mã lỗi.
function rustCodes() {
  const codes = new Set()
  const pattern = /(starts_with\()?"((?:http|network|disk|file|guard|verify|tool|internal|media)\.[a-z0-9_]+)"/g
  const walk = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name)
      if (entry.isDirectory()) walk(path)
      else if (entry.name.endsWith(".rs") && !entry.name.endsWith("_tests.rs")) {
        const code = read(path).split("#[cfg(test)]")[0]
        for (const [, prefix, name] of code.matchAll(pattern)) {
          if (!prefix && !/^http\.\d+$/.test(name)) codes.add(name)
        }
      }
    }
  }
  walk(join(root, "src-tauri/src"))
  for (const crate of readdirSync(join(root, "crates"))) walk(join(root, "crates", crate, "src"))
  return codes
}

function checkRustCodes(source) {
  for (const code of rustCodes()) {
    if (!(`err.${code}` in source)) {
      fail(`Mã lỗi Rust "${code}" chưa có câu dịch: thêm "err.${code}" vào mọi file locales (giao diện dịch lỗi theo mã).`)
    }
  }
}

function checkExtension(languages) {
  const folders = languages.map((language) => language.code.replace("-", "_"))
  const base = join(root, "extensions/chromium/_locales")
  const keys = (path) => Object.keys(JSON.parse(read(path))).sort().join()
  const reference = keys(join(base, "en/messages.json"))
  for (const browser of ["chromium", "firefox"]) {
    const manifest = JSON.parse(read(join(root, "extensions", browser, "manifest.json")))
    if (manifest.default_locale !== "en") fail(`extensions/${browser}/manifest.json: cần "default_locale": "en".`)
    const directory = join(root, "extensions", browser, "_locales")
    const present = existsSync(directory) ? readdirSync(directory) : []
    for (const extra of present.filter((folder) => !folders.includes(folder))) {
      fail(`extensions/${browser}/_locales/${extra} không có trong languages.ts.`)
    }
  }
  for (const folder of folders) {
    const chromium = join(base, folder, "messages.json")
    const firefox = join(root, "extensions/firefox/_locales", folder, "messages.json")
    if (!existsSync(chromium)) {
      fail(`Thiếu extensions/chromium/_locales/${folder}/messages.json (dịch từ en).`)
      continue
    }
    if (keys(chromium) !== reference) fail(`extensions/chromium/_locales/${folder}: khoá phải giống hệt en.`)
    if (!existsSync(firefox) || read(firefox) !== read(chromium)) {
      fail(`extensions/firefox/_locales/${folder}/messages.json phải giống hệt bản chromium (chép sang).`)
    }
  }
}

// Chuỗi người dùng thấy phải đi qua t(); cho phép tên riêng/tên file mẫu ở các file liệt kê.
const VIETNAMESE = /[àáạảãâầấậẩẫăằắặẳẵèéẹẻẽêềếệểễìíịỉĩòóọỏõôồốộổỗơờớợởỡùúụủũưừứựửữỳýỵỷỹđ]/i
const ALLOW = ["src/shared/i18n/languages.ts", "src/features/home/model/mock-data.ts"]

function checkHardcoded() {
  const walk = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name)
      const name = relative(root, path).replaceAll("\\", "/")
      if (entry.isDirectory()) walk(path)
      else if (/\.tsx?$/.test(entry.name) && !ALLOW.includes(name)) {
        const code = read(path)
          .replace(/\/\*[\s\S]*?\*\//g, (block) => block.replace(/[^\n]/g, " "))
          .split(/\r?\n/)
        code.forEach((line, index) => {
          const text = line.replace(/\/\/.*$/, "")
          if (VIETNAMESE.test(text)) {
            fail(`${name}:${index + 1}: chuỗi tiếng Việt viết cứng — đưa vào locales/vi.json rồi gọi t("khoá").`)
          }
        })
      }
    }
  }
  walk(join(root, "src"))
}

const languages = readLanguages()
const source = checkLanguages(languages)
checkRustCodes(source)
checkExtension(languages)
checkHardcoded()

if (errors.length > 0) {
  console.error(`i18n: ${errors.length} lỗi\n${errors.map((line) => `  - ${line}`).join("\n")}`)
  process.exit(1)
}
console.log(`i18n: ${languages.length} ngôn ngữ, ${Object.keys(source).length} khoá — đủ.`)
