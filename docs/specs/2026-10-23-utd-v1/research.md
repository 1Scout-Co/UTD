# Research — UTD v1 (2026-10-23)

## Câu hỏi

- Đã có repo nào làm tốt bài này, license là gì, học được gì và không được lấy gì?
- Stack nào hợp với scope: tải file lớn + media + extension + an toàn file, Windows-first?
- Có thể tái dùng thư viện/công cụ nào? Có rủi ro license nào phải chốt trước khi viết SPEC?
- Giới hạn kỹ thuật/pháp lý nào cần nói rõ với người dùng ngay từ đầu?

## Kết luận ngắn

1. Không làm "IDM clone" thuần. Định vị: **tải nhanh + an toàn + bền trên mạng yếu + bắt media bằng chuột phải**.
2. Stack: **Tauri 2 + React/TS/shadcn + Rust engine**, extension MV3/Firefox qua native messaging. Native UI không còn là ưu tiên.
3. **Không bundle yt-dlp/ffmpeg bản GPL vào installer Apache-2.0.** Quản lý runtime, tải từ nguồn chính thức kèm checksum + license notice, chạy như process riêng.
4. Không copy code GPL (Gopeed, XDM). AB (Apache-2.0), Rayburst/Motrix (MIT) chỉ học pattern hoặc tái dùng có ghi công.
5. V1: HTTP(S) segmented + resume/verify + Download Guard + Doctor + extension + YouTube/TikTok/Facebook + HLS/DASH + ffmpeg. Không torrent, playlist, DRM, live stream.

## 1. Cạnh tranh

| Repo | Stack / license | Học được | Không nên đối đầu trực diện |
|---|---|---|---|
| [Gopeed](https://github.com/GopeedLab/gopeed) | Go + Flutter · GPL-3.0 · ~26k sao | Breadth (HTTP/BT/ed2k), JS plugin, REST API, CLI, MCP; headless | Breadth và automation; họ đã mạnh |
| [AB Download Manager](https://github.com/amir1376/ab-download-manager) | Kotlin + Compose · Apache-2.0 · ~18k sao | UX giống IDM, queue/scheduler, extension Chrome/Firefox | UX cơ bản; họ đã làm tốt |
| [Rayburst](https://github.com/AnInsomniacy/rayburst) (Motrix Next) | Tauri 2 + aria2-next · MIT | Tauri 2 khả thi cho app loại này; HLS/DASH; browser integration | Mới đổi tên 10/2026, chưa có bản sắc ổn định |
| [XDM](https://github.com/subhra74/xdm) | Java/Swing · GPL-3.0 | HLS/DASH lâu năm, xử lý site extractor | UI/kiến trúc cũ |
| [Motrix](https://github.com/agalwood/Motrix) v2 | Electron + React/TS + aria2 fork · MIT | Đóng gói đa nền tảng, cộng đồng | Electron nặng; v2 beta, installer Windows chưa ký |
| aria2 | C++ · GPL-2.0 | Tham khảo thuật toán segment, RPC | Không dùng làm engine: GPL + kiến trúc cũ, khó kiểm soát resume/verify |

**Bài học chung**

- Extension + chuột phải là moat của IDM; thiếu nó thì không thay được IDM.
- HLS/DASH và site extractor là tính năng người dùng mong đợi; nhưng là chi phí bảo trì thường xuyên.
- Chưa ai làm tử tế lớp **an toàn sau khi tải** (tên giả, MIME giả, quarantine, hash, MOTW) và **chẩn đoán lỗi** (Doctor). Đây là khoảng trống chính.
- Độ bền trên mạng yếu (mất Wi-Fi, sleep, rút điện) là lợi thế kể chuyện được, đặc biệt ở VN/SEA.

## 2. License và phân phối

### yt-dlp

- Source: **Unlicense**. Binary standalone chính thức (`yt-dlp.exe`) được đóng gói bằng PyInstaller và **phát hành dưới GPLv3+** vì chứa code GPL.
- Vì UTD là Apache-2.0, **không đưa binary yt-dlp vào installer**. Cách làm:
  - Ưu tiên phát hiện yt-dlp người dùng đã cài.
  - Nếu chưa có, hỏi người dùng rồi tải từ GitHub Releases chính thức, xác minh `SHA2-512SUMS`, hiển thị license GPLv3+ và nguồn.
  - Chạy bằng process riêng; không link code.
  - Có kênh cập nhật/rollback vì extractor gãy thường xuyên.
- Không dùng yt-dlp để vượt DRM/paywall.

### ffmpeg

- Nền **LGPL-2.1+**; nếu build bật thành phần GPL thì toàn bản thành GPL. Build "full" phổ biến trên Windows (Gyan) là GPL.
- Không bundle build GPL. Cách làm: phát hiện ffmpeg có sẵn; hoặc tải bản LGPL từ nguồn tin cậy kèm checksum/notice; chạy process riêng; chỉ dùng `-c copy` để mux/remux, không re-encode mặc định.
- Nếu sau này phân phối kèm ffmpeg, phải kèm license, thông báo và nghĩa vụ source tương ứng.

### Phần còn lại

- Tauri 2: MIT/Apache-2.0; React, shadcn/ui, Tailwind, Lucide: MIT; SQLite: public domain.
- Rust crates: chọn MIT/Apache-2.0; kiểm license từng crate trước khi thêm (đặc biệt crate HLS/DASH).
- Không copy code từ Gopeed/XDM (GPL-3.0). AB (Apache-2.0) có thể tham khảo pattern và phải giữ NOTICE nếu dùng code; Rayburst/Motrix (MIT) dùng được nếu ghi công.
- Chrome Web Store: phí một lần 5 USD; Firefox Add-ons miễn phí. Apple Developer 99 USD/năm khi làm macOS.

## 3. Browser integration

- Chrome native messaging: host manifest cần `name`, `description`, `path`, `type: stdio`, `allowed_origins`; Windows đăng ký ở HKCU (không cần admin), host được Chrome khởi chạy và nói chuyện qua stdin/stdout; có giới hạn kích thước message nên chỉ gửi URL/metadata, **không truyền file**.
- Firefox dùng manifest khác (`allowed_extensions`) và registry path khác. Edge dùng cơ chế Chromium với registry riêng — kiểm khi implement.
- MV3: `contextMenus`, `nativeMessaging`, `downloads` (takeover), `cookies` (nhạy cảm), `storage`, `scripting`/`activeTab`; host permission nên **optional theo site**.
- Không dùng remote code trong extension; giữ CSP chặt; chuẩn bị privacy policy và lý do cho từng quyền khi review store.
- YouTube chặn context menu của player → cần **nút overlay** trên video (content script, chỉ bật khi người dùng cho phép site đó) + menu trên page/link + toolbar popup.
- Takeover: hủy download của browser rồi chuyển URL/headers/cookies sang app. Phải opt-in, tránh tải đúp; test kỹ với file lớn và redirect.
- Cookie: chỉ đọc khi người dùng chủ động tải nội dung cần đăng nhập; giữ trong bộ nhớ, không ghi disk/log, không gửi sang domain khác.

## 4. Media pipeline

- Site extractor (YouTube/TikTok/Facebook): yt-dlp `--dump-single-json --no-playlist` để lấy metadata/format; truyền cookie opt-in; tải bằng yt-dlp hoặc URL trực tiếp khi phù hợp; ffmpeg mux audio+video.
- HLS/DASH trực tiếp: parse master playlist/MPD, chọn variant, tải segment song song, xử lý AES-128 (HLS thường, **không phải DRM**), ghép/remux bằng ffmpeg.
- DRM (Widevine/PlayReady/FairPlay) và nội dung trả phí: **từ chối rõ ràng**, không thử bypass.
- V1 chỉ VOD; live stream, playlist, subtitle, audio-only để sau.
- Chất lượng: hiển thị resolution/bitrate/codec/size; mặc định "tốt nhất tương thích"; cảnh báo nếu cần tải video-only + audio riêng.
- YouTube/TikTok/Facebook có thể vi phạm ToS/điều kiện nền tảng; app cần disclaimer, chỉ dành cho nội dung người dùng có quyền.

## 5. Engine và độ bền

- HTTP Range: kiểm tra 206/Content-Range, gửi `Accept-Encoding: identity` khi tải range, xác thực `ETag`/`Last-Modified`/`Content-Length`; fallback 200 single-stream nếu server không hỗ trợ Range.
- Quirk phải xử lý: server trả HTML 200 thay vì file, Content-Length sai, 429/Retry-After, redirect vòng, signed URL hết hạn, captive portal, TLS lỗi, IPv6-only, CDN bóp tốc độ theo kết nối.
- Ghi file: preallocate + ghi tại vị trí (`pwrite`/`seek_write`), không merge file; checkpoint SQLite WAL định kỳ; temp file + rename khi xong; chịu được kill/power-cut.
- Segment: thích ứng 4→16 kết nối, chia segment chậm nhất, giới hạn theo host, backoff + jitter, token bucket cho speed limit.
- Verify: SHA-256/BLAKE3; ưu tiên server `Digest`, file `.sha256`/`.sha512` cạnh URL, hoặc người dùng nhập.
- Doctor: chẩn đoán no-range, ETag đổi, 429, captive portal, proxy/TLS, disk full/quyền, AV lock, CDN throttle, extractor lỗi.

## 6. Windows: an toàn file và phân phối

- Tên file: reserved names (`CON`, `NUL`, `COM1`...), dấu chấm/khoảng trắng cuối, dấu `:` (ADS), path separator, ký tự điều khiển, RTL/bidi, Unicode normalize, path dài, trùng tên.
- **MOTW**: giữ/ghi `Zone.Identifier` (ZoneId=3) để SmartScreen/Office Protected View tiếp tục hoạt động.
- Authenticode: `WinVerifyTrust` để hiện publisher/chữ ký; file unsigned phải cảnh báo rõ.
- Quarantine: file nguy hiểm vào thư mục riêng, không tự mở/chạy; hiện kết quả Defender nếu có, không giả vờ đã quét khi không có.
- Code signing: SignPath.io miễn phí cho OSS nếu được duyệt; nếu không, Azure Trusted Signing (~10 USD/tháng) hoặc EV cert. Không ký thì SmartScreen/AV sẽ làm mất niềm tin.
- IPC: named pipe ACL theo user + token; v1 không mở cổng localhost. Extension chỉ được gọi host qua allowlist ID.
- WebView2 + NSIS installer; auto-update ký bằng Tauri updater.

## 7. Bản đồ tái dùng

| Thành phần | Lựa chọn | License | Ghi chú |
|---|---|---|---|
| UI shell | Tauri 2 + React + TS + shadcn/ui + Tailwind | MIT/Apache | Tray, updater, single-instance, autostart |
| UI state/list | TanStack Query + TanStack Virtual + Zustand | MIT | Danh sách lớn, cập nhật progress |
| i18n | i18next + react-i18next | MIT | VI + EN |
| HTTP engine | tokio + reqwest/hyper + rustls | MIT/Apache | Transport tách trait để thay libcurl sau |
| Storage | rusqlite (SQLite WAL) | MIT | Journal task/segment |
| Windows API | windows-rs | MIT/Apache | Preallocate, MOTW, WinVerifyTrust, pipe ACL |
| Hash | sha2 + blake3 | MIT/Apache | Verify |
| HLS/DASH | m3u8-rs, dash-mpd (kiểm license/activity) | cần xác nhận | Parser; engine tự viết |
| Extractor | yt-dlp (external) | Unlicense source / GPLv3+ binary | Không bundle; tải có consent/checksum |
| Mux | ffmpeg (external) | LGPL/GPL tùy build | Không bundle bản GPL; chạy process riêng |
| Test | cargo test + Vitest + Playwright + HTTP chaos server | MIT/Apache | Bắt buộc cho resume/verify |

## 8. Rủi ro

- **yt-dlp gãy/ToS**: bảo trì thường xuyên, nút update extractor, fallback URL trực tiếp; không hứa "luôn tải được YouTube".
- **Chrome Web Store**: quyền `cookies`/`downloads` + overlay dễ bị soi; giảm quyền, optional theo site, privacy policy rõ.
- **GPL sidecar**: không bundle; nếu bundle phải tuân thủ GPL cho binary đó.
- **Proxy doanh nghiệp**: NTLM/Kerberos/PAC phức tạp; v1 chỉ proxy cơ bản, ghi rõ giới hạn.
- **SmartScreen/AV false positive**: ký từ build đầu; nộp mẫu nếu bị chặn.
- **Rust velocity**: nếu chậm, thay engine bằng Go sidecar sau IPC, giữ nguyên UI.
- **Scope creep**: torrent, playlist, live, macOS, CLI dễ làm loãng v1.

## Nguồn

- [Gopeed](https://github.com/GopeedLab/gopeed)
- [AB Download Manager](https://github.com/amir1376/ab-download-manager)
- [Rayburst (Motrix Next)](https://github.com/AnInsomniacy/rayburst)
- [XDM](https://github.com/subhra74/xdm)
- [Motrix](https://github.com/agalwood/Motrix)
- [awesome-download-manager](https://github.com/tamld/awesome-download-manager)
- [yt-dlp](https://github.com/yt-dlp/yt-dlp)
- [ffmpeg legal](https://ffmpeg.org/legal.html)
- [Tauri sidecar](https://v2.tauri.app/develop/sidecar/)
- [Chrome native messaging](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)
- [Chrome Web Store policies](https://developer.chrome.com/docs/webstore/program-policies/)
