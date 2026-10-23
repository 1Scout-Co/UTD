# UTD

Trình quản lý tải xuống mã nguồn mở cho Windows, hướng tới thay thế IDM: tải nhanh file lớn, resume trên mạng yếu, kiểm tra an toàn và tải media từ trình duyệt.

## Đang có

- Engine HTTP(S) chia segment, tự dò số kết nối (1→16, tăng khi còn nhanh hơn, lùi khi máy chủ bóp/429/503, dò lại theo mạng), chia lại segment chậm, retry/backoff + `Retry-After`, resume bằng SQLite WAL, phát hiện ETag/kích thước đổi, SHA-256, pause/resume, không ghi đè file.
- Ứng dụng desktop Tauri 2 + React/TypeScript/shadcn (UI Soniox, Inter): bảng download thật, icon thật của file + logo nguồn media, menu chuột phải (mở file/thư mục, đổi tên, di chuyển, xoá vào Thùng rác), thư mục tải mặc định trong Cài đặt, dung lượng ổ.
- Download Guard (cách ly file chạy được/giả định dạng, MOTW) + Doctor theo mã lỗi.
- Tích hợp trình duyệt: extension Chromium + Firefox, context menu, native host + named pipe có token; gỡ cài thì gỡ đăng ký native host.
- Media: yt-dlp + ffmpeg (HLS/DASH/YouTube/TikTok/Facebook/direct URL), chọn định dạng, audio M4A/MP3, pause/resume, tên file Unicode, không DRM. Dùng bản có sẵn trên máy (PATH, WinGet, Scoop, Chocolatey); thiếu thì tải một chạm từ GitHub chính chủ, kiểm SHA-256.
- Journal SQLite v2 (`tasks`, `segments`, `media_jobs`, `settings`, `schema_migrations`), backup `.bak` trước migration.
- Tự cập nhật qua GitHub Releases: chỉ đọc `latest.json`, kiểm chữ ký trước khi cài, sai chữ ký thì giữ bản cũ ([docs/RELEASE.md](docs/RELEASE.md#tự-cập-nhật)).

## Yêu cầu phát triển

- Windows 10 21H2+/11 x64, WebView2.
- Rust stable MSVC, Node 22+, pnpm.
- Media: `yt-dlp.exe` và `ffmpeg.exe` trong `%LOCALAPPDATA%\UTD-Data\bin` hoặc `PATH`. Không bundle binary GPL vào bộ cài.

## Chạy

```bash
pnpm install
pnpm tauri dev
```

Frontend preview (mock data, không có engine):

```bash
pnpm dev
```

## Kiểm

```bash
cargo test -p utd-engine
cargo test -p app --lib
cargo clippy -p app --all-targets -- -D warnings
pnpm lint
pnpm build
pnpm test:e2e
```

## Thư mục dữ liệu

- `%LOCALAPPDATA%\UTD-Data\utd.db` — journal (WAL).
- `%LOCALAPPDATA%\UTD-Data\extension` — extension đã giải nén.
- `%LOCALAPPDATA%\UTD-Data\native-messaging` — manifest native host.
- `%LOCALAPPDATA%\UTD-Data\ipc.json` — token + pipe của phiên đang chạy.
- `%LOCALAPPDATA%\UTD-Data\backup` — bản sao journal trước mỗi lần tự cập nhật (giữ 3 bản).
- `Downloads\UTD` — thư mục tải mặc định.

Không telemetry, không cloud. Cookie/header đăng nhập chưa được gửi từ trình duyệt.

## Extension

Xem [extensions/README.md](extensions/README.md).

## Giấy phép

Apache-2.0, xem [LICENSE](LICENSE) và [NOTICE](NOTICE). yt-dlp/ffmpeg là phần mềm bên thứ ba, tải riêng theo giấy phép của chúng; UTD không phân phối các binary đó trong bộ cài.

## Chưa xong

- Ký số Authenticode cho bộ cài (CI có chỗ cắm sẵn, đang tắt) — SmartScreen sẽ cảnh báo bản chưa ký.
- Bản phát hành đầu tiên: thêm secret khoá cập nhật lên GitHub rồi tag `v0.1.0` (xem [docs/RELEASE.md](docs/RELEASE.md)).
- Màn chi tiết download (S-15..S-21), cảnh báo trước khi tải (S-22), onboarding (S-32), giới hạn tốc độ, proxy, VI/EN.
- Cookie/header đăng nhập từ trình duyệt, takeover download; Firefox test tự động; macOS.
