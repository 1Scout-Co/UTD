#!/usr/bin/env node
// Tạo latest.json cho tự cập nhật (tauri-plugin-updater) từ bộ cài NSIS và chữ ký `.sig` mà
// `tauri build` vừa tạo. App đọc file này qua
// https://github.com/<repo>/releases/latest/download/latest.json (chỉ khi bản phát hành đã Publish).
//
// Dùng (CI, job release):
//   node scripts/release/latest-json.mjs --installer <...-setup.exe> --tag v0.2.0 --out <thư mục>/latest.json
//     [--repo 1Scout-Co/UTD] [--notes-file docs/releases/v0.2.0.md] [--config src-tauri/tauri.conf.json] [--date <ISO>]
// Chỉ đọc chữ ký công khai (.sig), không bao giờ đụng khoá bí mật.

import { existsSync, readFileSync, writeFileSync } from "node:fs"
import { basename, dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { parseArgs } from "node:util"

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..")
const NOTES_MAX = 1500
const PLATFORM = "windows-x86_64"

function fail(message) {
  console.error(`::error::${message}`)
  process.exit(1)
}

function readArgs() {
  const { values } = parseArgs({
    options: {
      installer: { type: "string" },
      tag: { type: "string" },
      out: { type: "string" },
      repo: { type: "string", default: "1Scout-Co/UTD" },
      "notes-file": { type: "string" },
      config: { type: "string", default: resolve(ROOT, "src-tauri/tauri.conf.json") },
      date: { type: "string" },
    },
    strict: true,
  })
  for (const name of ["installer", "tag", "out"]) {
    if (!values[name]) fail(`Thiếu --${name}`)
  }
  if (!/^[\w.-]+\/[\w.-]+$/.test(values.repo)) fail(`--repo phải dạng chủ/tên, nhận "${values.repo}"`)
  return values
}

function appVersion(configPath) {
  if (!existsSync(configPath)) fail(`Không thấy ${configPath}`)
  const { version } = JSON.parse(readFileSync(configPath, "utf8"))
  if (typeof version !== "string" || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
    fail(`"version" trong ${configPath} không phải số phiên bản hợp lệ: ${version}`)
  }
  return version
}

/** Nội dung .sig (base64 của chữ ký minisign) phải khớp đúng bộ cài và đúng phiên bản. */
function readSignature(installer, version) {
  const sigPath = `${installer}.sig`
  if (!existsSync(sigPath)) {
    fail(`Không thấy ${basename(sigPath)}: build chưa bật createUpdaterArtifacts hoặc thiếu khoá ký`)
  }
  const signature = readFileSync(sigPath, "utf8").trim()
  const text = Buffer.from(signature, "base64").toString("utf8")
  const trusted = text.split("\n").find((line) => line.startsWith("trusted comment: "))
  if (!text.startsWith("untrusted comment:") || !trusted) fail(`${basename(sigPath)} không phải chữ ký minisign`)
  const fields = trusted.slice("trusted comment: ".length).split("\t")
  if (!fields.includes(`file:${basename(installer)}`)) fail(`${basename(sigPath)} ký cho file khác`)
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
  const installer = resolve(args.installer)
  const name = basename(installer)
  if (!existsSync(installer)) fail(`Không thấy bộ cài ${installer}`)
  if (!name.endsWith("-setup.exe")) fail(`Bộ cài phải là file *-setup.exe, nhận ${name}`)
  const version = appVersion(resolve(args.config))
  if (args.tag !== `v${version}`) fail(`Tag ${args.tag} khác phiên bản v${version} trong tauri.conf.json`)
  const pubDate = new Date(args.date ?? Date.now())
  if (Number.isNaN(pubDate.getTime())) fail(`--date không đọc được: ${args.date}`)
  const manifest = {
    version,
    notes: releaseNotes(args["notes-file"], version, args.repo, args.tag),
    pub_date: pubDate.toISOString().replace(/\.\d{3}Z$/, "Z"),
    platforms: {
      [PLATFORM]: {
        signature: readSignature(installer, version),
        url: `https://github.com/${args.repo}/releases/download/${encodeURIComponent(args.tag)}/${encodeURIComponent(name)}`,
      },
    },
  }
  writeFileSync(resolve(args.out), `${JSON.stringify(manifest, null, 2)}\n`)
  console.log(`latest.json: ${version} → ${manifest.platforms[PLATFORM].url}`)
}

main()
