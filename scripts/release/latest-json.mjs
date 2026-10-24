#!/usr/bin/env node
// Tạo latest.json cho tự cập nhật (tauri-plugin-updater) từ các gói mà `tauri build` vừa tạo, mỗi gói
// kèm chữ ký `.sig`: bộ cài NSIS cho Windows, `.app.tar.gz` cho macOS. App đọc file này qua
// https://github.com/<repo>/releases/latest/download/latest.json (chỉ khi bản phát hành đã Publish).
//
// Dùng (CI, job release):
//   node scripts/release/latest-json.mjs --tag v0.2.0 --out <thư mục>/latest.json \
//     --asset windows-x86_64=<...-setup.exe> \
//     --asset darwin-aarch64=<...universal.app.tar.gz> --asset darwin-x86_64=<...universal.app.tar.gz>
//     [--repo 1Scout-Co/UTD] [--notes-file docs/releases/v0.2.0.md] [--config src-tauri/tauri.conf.json] [--date <ISO>]
// Chỉ đọc chữ ký công khai (.sig), không bao giờ đụng khoá bí mật.

import { existsSync, readFileSync, writeFileSync } from "node:fs"
import { basename, dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { parseArgs } from "node:util"

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..")
const NOTES_MAX = 1500
// Nền tảng app hỏi (`<os>-<arch>` của tauri-plugin-updater) → đuôi gói hợp lệ cho nền tảng đó.
const PLATFORMS = {
  "windows-x86_64": "-setup.exe",
  "darwin-aarch64": ".app.tar.gz",
  "darwin-x86_64": ".app.tar.gz",
}

function fail(message) {
  console.error(`::error::${message}`)
  process.exit(1)
}

function readArgs() {
  const { values } = parseArgs({
    options: {
      asset: { type: "string", multiple: true },
      tag: { type: "string" },
      out: { type: "string" },
      repo: { type: "string", default: "1Scout-Co/UTD" },
      "notes-file": { type: "string" },
      config: { type: "string", default: resolve(ROOT, "src-tauri/tauri.conf.json") },
      date: { type: "string" },
    },
    strict: true,
  })
  for (const name of ["asset", "tag", "out"]) {
    if (!values[name]) fail(`Thiếu --${name}`)
  }
  if (!/^[\w.-]+\/[\w.-]+$/.test(values.repo)) fail(`--repo phải dạng chủ/tên, nhận "${values.repo}"`)
  return values
}

/** `--asset nền-tảng=đường-dẫn` → [{ platform, file }], mỗi nền tảng một lần, đúng loại gói. */
function readAssets(entries) {
  const assets = []
  for (const entry of entries) {
    const split = entry.indexOf("=")
    const platform = entry.slice(0, split)
    const file = resolve(entry.slice(split + 1))
    if (split < 1 || !(platform in PLATFORMS)) {
      fail(`--asset phải dạng <${Object.keys(PLATFORMS).join("|")}>=<file>, nhận "${entry}"`)
    }
    if (assets.some((asset) => asset.platform === platform)) fail(`--asset ${platform} bị lặp`)
    if (!existsSync(file)) fail(`Không thấy gói ${file}`)
    if (!basename(file).endsWith(PLATFORMS[platform])) {
      fail(`Gói cho ${platform} phải là file *${PLATFORMS[platform]}, nhận ${basename(file)}`)
    }
    assets.push({ platform, file })
  }
  return assets
}

function appVersion(configPath) {
  if (!existsSync(configPath)) fail(`Không thấy ${configPath}`)
  const { version } = JSON.parse(readFileSync(configPath, "utf8"))
  if (typeof version !== "string" || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
    fail(`"version" trong ${configPath} không phải số phiên bản hợp lệ: ${version}`)
  }
  return version
}

/** Nội dung .sig (base64 của chữ ký minisign) phải khớp đúng gói và đúng phiên bản. */
function readSignature(file, version) {
  const sigPath = `${file}.sig`
  const name = basename(file)
  if (!existsSync(sigPath)) {
    fail(`Không thấy ${basename(sigPath)}: build chưa bật createUpdaterArtifacts hoặc thiếu khoá ký`)
  }
  const signature = readFileSync(sigPath, "utf8").trim()
  const text = Buffer.from(signature, "base64").toString("utf8")
  const trusted = text.split("\n").find((line) => line.startsWith("trusted comment: "))
  if (!text.startsWith("untrusted comment:") || !trusted) fail(`${basename(sigPath)} không phải chữ ký minisign`)
  const fields = trusted.slice("trusted comment: ".length).split("\t")
  const signedName = fields.find((field) => field.startsWith("file:"))?.slice("file:".length)
  // Gói macOS Tauri đặt tên `UTD.app.tar.gz`; CI đổi tên kèm phiên bản khi đăng (chữ ký vẫn của nội dung đó).
  const sameFile = name.endsWith(".app.tar.gz") ? signedName?.endsWith(".app.tar.gz") : signedName === name
  if (!sameFile) fail(`${basename(sigPath)} ký cho file khác (${signedName})`)
  // App bật requireSignedVersion: chữ ký thiếu/lệch phiên bản sẽ bị app từ chối.
  if (!fields.includes(`version:${version}`)) fail(`${basename(sigPath)} không ghi version:${version}`)
  return signature
}

function releaseNotes(notesFile, version, repo, tag) {
  const fallback = `UTD ${version}. Ghi chú đầy đủ: https://github.com/${repo}/releases/tag/${tag}`
  if (!notesFile || !existsSync(notesFile)) return fallback
  const notes = readFileSync(notesFile, "utf8").replace(/\r\n/g, "\n").trim()
  return notes ? notes.slice(0, NOTES_MAX) : fallback
}

function main() {
  const args = readArgs()
  const assets = readAssets(args.asset)
  const version = appVersion(resolve(args.config))
  if (args.tag !== `v${version}`) fail(`Tag ${args.tag} khác phiên bản v${version} trong tauri.conf.json`)
  const pubDate = new Date(args.date ?? Date.now())
  if (Number.isNaN(pubDate.getTime())) fail(`--date không đọc được: ${args.date}`)
  const platforms = {}
  for (const { platform, file } of assets) {
    platforms[platform] = {
      signature: readSignature(file, version),
      url: `https://github.com/${args.repo}/releases/download/${encodeURIComponent(args.tag)}/${encodeURIComponent(basename(file))}`,
    }
  }
  const manifest = {
    version,
    notes: releaseNotes(args["notes-file"], version, args.repo, args.tag),
    pub_date: pubDate.toISOString().replace(/\.\d{3}Z$/, "Z"),
    platforms,
  }
  writeFileSync(resolve(args.out), `${JSON.stringify(manifest, null, 2)}\n`)
  for (const [platform, { url }] of Object.entries(platforms)) {
    console.log(`latest.json: ${version} ${platform} → ${url}`)
  }
}

main()
