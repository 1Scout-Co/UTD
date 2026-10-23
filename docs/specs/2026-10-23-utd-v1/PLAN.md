# PLAN — UTD v1

Ngày: 2026-10-23 · Trạng thái: đã chốt; M0 + M1 core xong (engine, Tauri bridge, UI thật); tiếp theo M2

## Kiến trúc — 9 câu

1. **Ranh giới module:** `crates/utd-engine` (Rust thuần, không biết Tauri) sở hữu transport, segment, journal, verify, Guard/Doctor; `src-tauri` là shell Tauri + command/event + native host + sidecar manager; `src/features/home` là UI gọi command/event; `extensions/` là Chromium/Firefox gửi message qua native host. Không module nào gọi ngược vào UI; UI không chứa logic tải.
2. **Hợp đồng:** Rust serde types (`DownloadTask`, `Segment`, `DoctorReport`, `MediaFormat`, `Settings`) là nguồn sự thật; TypeScript mirror trong `src/features/home/model/types.ts`; Tauri command/event có prefix `utd://`; native messaging schema có `version/type/url/pageUrl/headers` và host validate trước khi chuyển.
3. **Dữ liệu:** SQLite WAL tại `%LOCALAPPDATA%\UTD-Data\utd.db` với bảng tasks/segments/settings/migrations; chỉ `store` được ghi; migration có backup `.bak`; file tải nằm trong thư mục người dùng chọn, temp `.part`, quarantine riêng; không cloud.
4. **Lỗi và biên:** parse URL/header/cookie tại biên, chặn scheme lạ/path traversal/tên file nguy hiểm; lỗi phân loại `User/Transient/Permanent/Security`; UI chỉ thấy message ngắn + mã Doctor, log nội bộ đã redact.
5. **Tăng trưởng 10x:** SQLite index theo status/date + batch checkpoint; giới hạn connection theo host và toàn cục; event throttle 4–10 Hz; UI table ảo hoá khi >200 dòng; test 1.000 task; migration schema có version.
6. **Quan sát:** log local có cấu trúc + redaction; mỗi task có metrics speed/retry/segment/Doctor; nút "Xuất chẩn đoán"; không telemetry; cảnh báo disk full/DB hỏng ngay trong app.
7. **Quyền/riêng tư:** named pipe ACL theo user + token; extension allowlist + schema; cookie opt-in, memory-only; file ACL theo user; không mở cổng localhost; không gửi dữ liệu ra ngoài ngoài update/tải do người dùng yêu cầu.
8. **Chi phí trần:** không server; signing SignPath OSS hoặc Azure Trusted Signing (~10 USD/tháng); Chrome Web Store 5 USD; CI GitHub Actions; sidecar do người dùng tải; không phát sinh phí runtime.
9. **Đường quay lại:** updater có chữ ký + giữ bản trước; DB migration có backup; engine có kill switch/tắt tính năng media; journal phục hồi sau crash; không migration phá dữ liệu.

## ADR

- [ADR-0001](../../adr/0001-tauri-react-rust.md) — Tauri 2 + React + Rust.
- [ADR-0002](../../adr/0002-engine-crate-ipc.md) — engine crate tách, in-process v1.
- [ADR-0003](../../adr/0003-sqlite-pwrite-journal.md) — SQLite WAL + positional writes.
- [ADR-0004](../../adr/0004-media-sidecars.md) — media sidecar, không DRM.
- [ADR-0005](../../adr/0005-local-first-security.md) — bảo mật local-first.
- [ADR-0006](../../adr/0006-license-open-core.md) — Apache-2.0 + open core.

## Mốc

- **M0:** workspace + `utd-engine` + model/DB + segmented HTTP + resume + verify + chaos tests.
- **M1:** Tauri commands/events + nối UI table/add dialog + settings + installer ký.
- **M2:** extension + native host + right-click/takeover; HLS/DASH + sidecar media.
- **M3:** Guard/Doctor + MOTW/quarantine + updater + beta.
- **M4:** macOS/notarization + v1.1.

## Làn

| lane | worker | files | ops | depends_on | accept |
|---|---|---|---|---|---|
| S | captain | Cargo.toml, crates/utd-engine/Cargo.toml, crates/utd-engine/src/lib.rs, src-tauri/Cargo.toml | new, extend | - | cargo check -p utd-engine |
| A | coder | crates/utd-engine/src/model.rs, crates/utd-engine/src/db.rs, src/features/home/model/types.ts | new, extend | S | cargo test -p utd-engine contracts |
| B | coder | crates/utd-engine/src/engine/** | new | S, A | cargo test -p utd-engine engine |
| G | coder | crates/utd-engine/src/safety/**, crates/utd-engine/src/doctor/** | new | A | cargo test -p utd-engine safety doctor |
| C | coder | src-tauri/src/lib.rs, src-tauri/src/commands.rs, src-tauri/src/events.rs | extend, new | A, B | cargo check -p utd |
| D | coder | src/features/home/ui/**, src/features/home/model/mock-data.ts | extend | A, C | pnpm typecheck && pnpm test:e2e |
| E | coder | extensions/**, src-tauri/src/native_host.rs | new | A, C | pnpm -C extensions test |
| F | coder | crates/utd-engine/src/media/**, src-tauri/src/sidecars.rs | new | B | cargo test -p utd-engine media |
| H | captain | src-tauri/tauri.conf.json, .github/workflows/**, package.json, pnpm-lock.yaml | extend | S, A, B, C, D, E, F, G | pnpm build && pnpm tauri build --debug |

Lưu ý: đây là path đích của greenfield; làn S tạo skeleton trước, các làn sau mới có file thật. `src/features/home/model/types.ts` do A sở hữu để tránh D sửa contract khi đang nối UI.

## Test plan

- Engine unit: parse Range/206/Content-Range, ETag đổi, no-Range, retry/backoff, filename sanitize, hash.
- Integration/chaos: HTTP server cục bộ (slow, drop, 429, redirect, HTML giả file, ETag change); 100 lần mất mạng, 20 lần kill, 5 lần power-cut; không file hỏng.
- Tauri/IPC: command contract, pipe ACL/token, native messaging allowlist/schema.
- UI: Playwright smoke S-01 + axe; dialog add/media; search/sort/filter; keyboard.
- Security: P-01..P-04 test chặn quyền và "đỏ khi tắt"; không telemetry; cookie không persist; log redaction.
- Media: YouTube/TikTok/Facebook + HLS/DASH mẫu; DRM bị từ chối; thiếu sidecar có hướng dẫn.

## QA charter

- Vai: người dùng tải file 20 GB trên Wi-Fi chập chờn, tải video bằng chuột phải, và người dùng mở file lạ.
- Trạng thái đầu: app sạch, chưa có task, chưa cài extension; sau đó có 3 task đang tải và 1 file quarantine.
- Dữ liệu: file lớn, ETag đổi, server no-Range, 429, tên file tiếng Việt/emoji; video cần cookie.
- Bằng chứng: ảnh segment/Doctor, hash khớp, resume sau kill, không file rác ngoài thư mục chọn, axe xanh.

## Phát hiện bất ngờ

- 2026-10-23: Tauri template không có trong flow kit (chỉ web/Wails), nên scaffold web shadcn rồi gắn Tauri 2 thủ công; đã build xanh.
- 2026-10-23: Soniox docs dùng Figtree nhưng người dùng chốt Inter toàn app; đã đổi và xoá font cũ.
- 2026-10-23: toolchain GNU/MSYS2 không build được Tauri vì đường dẫn dự án có tiếng Việt (`Dự án mở`) — linker/windres lỗi; chuyển sang **MSVC** (`rust-toolchain.toml`), engine test và Tauri check đều xanh, không cần target-dir ASCII.
- 2026-10-23: UI đã chốt và dọn 8 bản demo; git repo đã init (chưa commit).
- 2026-10-23: yt-dlp trên Windows in đường dẫn theo cp1252 → tên tiếng Việt làm mất dòng `UTD_FILE` (task 8,9 GB báo lỗi dù đã tải xong); `--print` còn ngầm tắt tiến độ. Sửa bằng `--encoding utf-8` + `--progress` + đọc byte/lossy + tìm file theo tên.
- 2026-10-23: thư mục cài NSIS currentUser là `%LOCALAPPDATA%\UTD` — trùng thư mục dữ liệu cũ; dữ liệu chuyển sang `UTD-Data` (migration một lần). Uninstaller của Tauri không xoá đệ quy `$INSTDIR`.
- 2026-10-23: HTTP/2 gộp mọi "kết nối" vào một TCP → engine dùng HTTP/1.1 để nhiều kết nối thật sự vượt giới hạn mỗi kết nối của máy chủ.
- 2026-10-23: CSP mới chặn ảnh `data:` → icon file mất trong bản đóng gói; cả 3 người rà soát bắt được, đã thêm `data:` vào img-src.

## Nhật ký quyết định

- 2026-10-23: chốt UI bản 9 Soniox + Inter; bỏ 8 bản demo, demo bar và state pills.
- 2026-10-23: chọn table chuyên nghiệp với Tên/Mô tả/Kích thước/Trạng thái/Tiến độ/Tốc độ tải xuống/Thời gian còn lại/Ngày tải; search Ctrl K, sort header, segmented tabs.
- 2026-10-23: chủ dự án chốt: phát hành chưa ký Authenticode; tự cập nhật qua repo public 1Scout-Co/UTD (khoá do chủ dự án tạo/giữ); dùng yt-dlp/ffmpeg có sẵn trên máy, chỉ tải khi thiếu; bỏ ô chọn số kết nối (engine tự dò như IDM); cho phép thêm thư viện khi tiện và sửa AI_RULES.md.
- 2026-10-23: UI vòng 2 "bóng mềm" + vòng 3 (chữ ≥12px, thanh cuộn mảnh, Cài đặt mỗi mục một hành động, menu chuột phải, logo nguồn/icon file thật) — làm ở phiên riêng utd-ui-refresh.

## Thay đổi plan

- 2026-10-23 15:10 | toàn bộ UI | đổi từ 9 demo sang 1 UI Soniox | người dùng chốt “ok ổn rồi” | người dùng thấy UI sạch hơn | user.

## Kết quả

- S xong: workspace Rust + `crates/utd-engine` + Tauri shell; `cargo test -p utd-engine` và `cargo check -p app` xanh bằng MSVC.
- UI đã chốt Soniox/Inter và dọn demo; smoke + axe xanh.
- Tiếp: A (model/DB/journal) → B (segmented download + resume tests).
