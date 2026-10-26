# UTD v1 — Tải nhanh, tải sạch, không gãy

Ngày: 2026-10-23 · Trạng thái: chờ duyệt Cổng 1

## Bạn duyệt

### Ai dùng

- Người dùng Windows 10/11 tải file lớn (game, ISO, dataset, video) trên mạng chập chờn, cần resume và không muốn file hỏng.
- Người dùng phổ thông muốn tải video YouTube/TikTok/Facebook bằng chuột phải, không muốn app quảng cáo hay tài khoản.
- Người dùng cẩn thận về bảo mật: sợ file giả/malware, cần cảnh báo, quarantine và kiểm tra hash.

### Các màn hình

#### Màn hình chính — Danh sách tải

| mã | khi | thấy gì | làm được gì |
|---|---|---|---|
| S-01 | Có dữ liệu | Danh sách download nhóm Đang tải / Hoàn tất / Lỗi; tốc độ, ETA, tiến độ, trạng thái verify | Tạm dừng/tiếp tục, mở thư mục, xoá, xem chi tiết, thêm URL, tìm/lọc |
| S-02 | Trống | Hướng dẫn bắt đầu; nút "Thêm URL", "Dán từ clipboard", "Cài extension" | Thêm download đầu tiên hoặc mở hướng dẫn extension |
| S-03 | Đang tải | Skeleton danh sách, thanh tiến trình tải dữ liệu | Chờ; có thể mở cài đặt |
| S-04 | Lỗi | "Không tải được danh sách" kèm mã lỗi ngắn | Thử lại, mở log, khôi phục dữ liệu |

#### Thêm download

| mã | khi | thấy gì | làm được gì |
|---|---|---|---|
| S-05 | Trống | Ô URL trống, nơi lưu, nút bắt đầu bị mờ; gợi ý dán URL | Dán URL, chọn thư mục, bắt đầu |
| S-06 | Đang tải | Đang kiểm tra URL: tên file, kích thước, server có Range không | Huỷ |
| S-07 | Có dữ liệu | Thông tin file dự kiến, MIME, kích thước, Range; tuỳ chọn thư mục/số kết nối/giới hạn tốc độ/headers | Bắt đầu, sửa tên file, mở tuỳ chọn nâng cao |
| S-08 | Lỗi | URL sai, 404, TLS, không có quyền ghi, server chặn | Sửa URL, đổi thư mục, xem gợi ý Doctor, thử lại |
| S-09 | Có dữ liệu (trùng URL) | Cảnh báo đã có download cùng URL/ETag | Tiếp tục bản cũ, tải lại, giữ cả hai |

#### Chọn media (video)

| mã | khi | thấy gì | làm được gì |
|---|---|---|---|
| S-10 | Đang tải | Đang phân tích trang bằng extractor; tên trang, tiêu đề nếu có | Huỷ |
| S-11 | Có dữ liệu | Danh sách chất lượng (resolution, codec, bitrate, size); mặc định tốt nhất tương thích | Chọn chất lượng, chọn audio+video, bắt đầu tải |
| S-12 | Trống | Không tìm thấy media tải được | Mở trang gốc, dán URL trực tiếp, cập nhật extractor |
| S-13 | Lỗi | Extractor lỗi/hết hạn, mạng lỗi, trang đổi cấu trúc | Cập nhật extractor, thử lại, sao chép log |
| S-14 | Lỗi (đăng nhập/DRM) | "Cần đăng nhập · <trang>" + gợi ý "Bấm biểu tượng UTD trên trình duyệt" (2026-10-25: cho phép xảy ra trong popup, xem S-70); hoặc nội dung DRM không hỗ trợ | Mở trang, huỷ; nhận được đăng nhập thì tự phân tích lại; DRM thì từ chối rõ |

#### Chi tiết download

| mã | khi | thấy gì | làm được gì |
|---|---|---|---|
| S-15 | Có dữ liệu (đang tải) | Tốc độ, ETA, bản đồ segment, số kết nối, bytes đã tải | Tạm dừng, đổi giới hạn tốc độ, mở thư mục |
| S-16 | Trống | Download không còn tồn tại hoặc đã bị xoá | Quay về danh sách |
| S-17 | Đang tải | Đang đọc chi tiết download | Chờ |
| S-18 | Lỗi | Không đọc được dữ liệu download | Thử lại, quay về danh sách |
| S-19 | Có dữ liệu (tạm dừng) | Vị trí dừng, segment đã xong, lý do nếu có | Tiếp tục, đổi giới hạn, huỷ |
| S-20 | Có dữ liệu (hoàn tất) | Kết quả verify hash, đường dẫn, thời gian, dung lượng | Mở file/thư mục, sao chép đường dẫn, xoá lịch sử |
| S-21 | Lỗi (tải thất bại) | Doctor: nguyên nhân, bằng chứng, cách sửa; mã lỗi | Thử lại, áp dụng cách sửa, cập nhật extractor, sao chép log |

#### Cảnh báo trước khi tải

Không tải dữ liệu.

| mã | khi | thấy gì | làm được gì |
|---|---|---|---|
| S-22 | Cảnh báo | File nguy hiểm/HTTP/MIME không khớp/tên đáng ngờ; giải thích ngắn | Tiếp tục có ý thức, huỷ, xem chi tiết |

#### Quarantine và kết quả kiểm tra

| mã | khi | thấy gì | làm được gì |
|---|---|---|---|
| S-23 | Đang tải | Đang quét Defender/kiểm tra chữ ký/hash | Chờ hoặc huỷ quét (file vẫn bị giữ) |
| S-24 | Có dữ liệu | File đang bị giữ, publisher/chữ ký, kết quả quét, hash khớp/không khớp | Mở thư mục, xác nhận mở file, xoá file |
| S-25 | Trống | Không có Defender hoặc không có checksum để đối chiếu | Tự kiểm tra hash, mở thư mục, xoá |
| S-26 | Lỗi | Quét lỗi/không hoàn tất; file vẫn bị giữ | Thử quét lại, mở log, xoá file |

#### Cài đặt

| mã | khi | thấy gì | làm được gì |
|---|---|---|---|
| S-27 | Có dữ liệu | Tab Chung, Mạng, An toàn, Tích hợp, Giới thiệu; giá trị hiện tại | Sửa và lưu, khôi phục mặc định |
| S-28 | Trống | Chưa có cấu hình, dùng mặc định | Lưu cấu hình đầu tiên |
| S-29 | Đang tải | Đang đọc/ghi cấu hình | Chờ |
| S-30 | Lỗi | Không đọc/ghi được cấu hình | Thử lại, khôi phục mặc định, mở log |
| S-31 | Lỗi (thiếu thành phần) | Extension/native host chưa nối; yt-dlp/ffmpeg chưa có | Cài/sửa native host, tải công cụ kèm checksum, xem hướng dẫn |

#### Onboarding lần đầu

Không tải dữ liệu.

| mã | khi | thấy gì | làm được gì |
|---|---|---|---|
| S-32 | Lần đầu | Chọn thư mục tải, cài extension/native host, tải thử | Làm theo bước, bỏ qua bước không cần |
| S-33 | Lỗi cài đặt | Không cài được native host/extension | Thử lại, mở hướng dẫn, sao chép log |

#### Extension popup

| mã | khi | thấy gì | làm được gì |
|---|---|---|---|
| S-34 | Đang tải | Đang quét trang tìm media/download | Chờ, huỷ |
| S-35 | Có dữ liệu | Media/link phát hiện được; chất lượng ước tính | Gửi sang UTD, đổi chất lượng, tải link |
| S-36 | Trống | Không thấy media trên trang | Tải trang/link hiện tại, bật overlay cho site này |
| S-37 | Lỗi | App chưa chạy/chưa cài, thiếu quyền, native host lỗi | Mở app, cài/sửa native host, cấp quyền |

#### Nút overlay trên video

Không tải dữ liệu.

| mã | khi | thấy gì | làm được gì |
|---|---|---|---|
| S-38 | Có video | Nút "Tải" nhỏ khi rê chuột lên video | Mở bảng chọn chất lượng trong app |
| S-39 | Lỗi | App chưa cài/chưa chạy hoặc site bị tắt overlay | Mở app, bật overlay cho site, tải lại trang |

#### Tải cả kênh

Kênh TikTok (`@user`, link rút gọn), kênh/playlist YouTube; mở từ hộp Thêm, Tải media hoặc trình duyệt. Mã S-60–S-63 (chừa S-40–S-59 cho các việc làm song song).

| mã | khi | thấy gì | làm được gì |
|---|---|---|---|
| S-60 | Đang tải | Tên, ảnh đại diện kênh; video hiện dần, số video đếm dần | Dừng liệt kê (giữ phần đã có), huỷ |
| S-61 | Có dữ liệu | Video: ảnh nhỏ, tiêu đề, ngày, thời lượng, lượt xem; video đã tải lần trước đánh dấu "Đã tải" | Lọc mới nhất N / khoảng ngày / chỉ video mới, chọn tất cả / bỏ chọn, một chất lượng chung, thư mục riêng theo tên kênh, tải |
| S-62 | Trống | Kênh chưa có video công khai | Huỷ |
| S-63 | Lỗi | Tài khoản riêng tư/cần đăng nhập (cùng luồng S-14), kênh không có, mạng lỗi, thiếu yt-dlp | Mở trang, thử lại, huỷ |

**Tải hàng loạt Facebook / Instagram / Threads** (2026-10-26, `notes/social-bulk.md`): cùng hộp và mã S-60–S-63, mở khi
tiện ích gửi danh sách (chuột phải › "Tải hàng loạt bằng UTD" trên trang cá nhân / fanpage / nhóm / Tin). Khác kênh: tiêu đề
"Tải hàng loạt"; chip loại kèm số (Bài viết, Ảnh, Video, Reels, Tin, Tin nổi bật; Ảnh/Video cho Instagram, Threads) — bỏ chip
thì tiện ích thôi lấy loại đó; không chọn chất lượng (tải file gốc); không "Thử lại" (bấm lại trên trang); nút "Tải N mục".
Trên trang: nhãn nhỏ "UTD · Đang thu N ảnh/video · Dừng".

#### Đăng nhập (cookie của trình duyệt)

Kế hoạch và mô hình đe doạ: `notes/auth-plan.md` (duyệt 2026-10-25). Cookie chỉ đọc trong cú bấm trên trình duyệt, chỉ nằm trong RAM của app. Quyền xin một lần lúc cài tiện ích (mọi trang + cookies, Q11 chủ dự án quyết 2026-10-26); tắt riêng từng trang trong popup. Mã S-70–S-77 (ngoài khoảng S-40–S-59 đang chừa).

| mã | khi | thấy gì | làm được gì |
|---|---|---|---|
| S-70 | Có dữ liệu (popup extension) | Trang đang mở có task chờ đăng nhập: "google.com · 2 tải chờ"; trang nhạy cảm (ngân hàng, gov…) ghi rõ; công tắc "Hiện nút Tải video này trên trang" | Dùng đăng nhập / Tiếp tục (n); bật/tắt "Đăng nhập trên trang này" (mặc định bật), bật/tắt nút trên trang |
| S-71 | Lỗi (popup extension) | Trình duyệt chưa cho phép, hoặc tiện ích không được chạy ở cửa sổ riêng tư | Thử lại |
| S-72 | Lỗi (danh sách, chi tiết) | Task "Cần đăng nhập": chưa có cookie, đăng nhập hết hạn, hoặc app vừa mở lại. Chờ như link hết hạn (tự cứu, chờ link mới) | Mở trang, Cập nhật link… (download thường), huỷ |
| S-73 | Có dữ liệu (Cài đặt › Tích hợp) | Công tắc "Dùng đăng nhập của trình duyệt" (mặc định bật), số trang đang giữ cookie, cảnh báo khoá tài khoản | Quên cookie |
| S-74 | Có dữ liệu (trang web, tiện ích) | Rê chuột vào video (hoặc video vừa phát): nút nhỏ "⬇ Tải video này" ngay trên mép trên-phải video, không đè điều khiển của trình phát | Bấm ⇒ S-75 |
| S-75 | Đang tải / Có dữ liệu / Lỗi (trang web, tiện ích) | Bảng định dạng ngay trên trang: "Đang đọc định dạng…"; Tốt nhất · 2160p … 144p · Chỉ âm thanh (M4A) · MP3, mỗi dòng kèm dung lượng ước tính; lỗi ngắn (cần đăng nhập, không đọc được, UTD chưa chạy); "Đã thêm vào UTD". Chuột phải › UTD mở cùng bảng ở chỗ bấm | Chọn một dòng ⇒ UTD tải luôn (không mở hộp Tải media); Esc / bấm ra ngoài để đóng |
| S-76 | Có dữ liệu (popup extension) | Công tắc "Tự bắt link tải" (mặc định bật); "Tuỳ chọn" gập lại: bỏ qua file nhỏ hơn N MB (mặc định 1), đuôi không bắt; một dòng "Giữ Alt khi bấm: để trình duyệt tự tải" | Bật/tắt, sửa ngưỡng, sửa đuôi |
| S-77 | Có dữ liệu (cửa sổ UTD) | UTD vừa nhận download của trình duyệt: thông báo nhỏ 4 s "Nhận từ trình duyệt · tên file", không mở hộp nào; dòng tải mới hiện trong danh sách | — |

### Làm được gì

- Tải file lớn bằng nhiều kết nối, pause/resume, sống qua mất mạng, sleep, kill app và rút điện (S-01, S-15, S-19).
- Thêm download bằng URL, clipboard, kéo thả danh sách URL; tự đọc tên/kích thước/MIME và phát hiện trùng (S-05–S-09).
- Chuột phải link/trang/video trong Chrome, Edge, Firefox để tải; tự bắt link tải (takeover) download của browser, mặc định bật, tắt được trong popup (S-34–S-39, S-76, S-77).
- Tải video YouTube/TikTok/Facebook qua extractor; chọn chất lượng; tải HLS/DASH trực tiếp; ghép audio/video bằng ffmpeg (S-10–S-14).
- Download Guard: cảnh báo file nguy hiểm, tên giả, MIME giả; quarantine; kiểm tra chữ ký; giữ MOTW; verify hash (S-22–S-26).
- Doctor giải thích vì sao chậm/lỗi và đưa cách sửa; tự sửa khi an toàn (S-21).
- Quản lý hàng đợi, tìm/lọc, retry, giới hạn tốc độ toàn cục và theo tác vụ (S-01, S-27).
- Cài đặt mạng/proxy cơ bản, số kết nối, giới hạn theo host, ngôn ngữ VI/EN, auto-update (S-27–S-31).
- Onboarding cài native host/extension và tải thử (S-32–S-33).
- Tải nội dung cần đăng nhập (video giới hạn tuổi/riêng tư, file Drive…) bằng đăng nhập của trình duyệt, quyền xin một lần lúc cài, tắt riêng từng trang (S-14, S-70–S-73).
- Nút "Tải video này" trên mọi trang có video, chọn định dạng ngay trên trang rồi tải luôn, kiểu IDM (S-74, S-75).
- Dữ liệu local, không tài khoản, không telemetry; cookie chỉ dùng khi opt-in (P-01–P-04).

### Để sau

- Playlist/kênh, subtitle, audio-only, live stream.
- Torrent/magnet, FTP/SFTP/WebDAV, Metalink/mirror.
- Scheduler, RSS, watch folder.
- macOS, ARM64, Linux, mobile, portable mode.
- Remote daemon/web UI, cross-device, cloud sync.
- Plugin SDK/extractor bên thứ ba, archive extraction/preview.
- VirusTotal/AV cloud lookup, báo cáo cộng đồng.
- DRM: không làm, không thử bypass.

### Phí phát sinh

- Ký số Windows: SignPath.io miễn phí cho OSS nếu được duyệt; nếu không, Azure Trusted Signing khoảng 10 USD/tháng hoặc EV cert vài trăm USD/năm. Không ký thì SmartScreen/AV sẽ cảnh báo.
- Chrome Web Store: 5 USD một lần; Firefox Add-ons miễn phí.
- Apple Developer: 99 USD/năm khi làm macOS (sau v1).
- Server: không cần cho v1 (local-only); GitHub Releases/Actions miễn phí cho repo public.
- yt-dlp/ffmpeg: không bundle; người dùng hoặc app tải hộ, không phát sinh phí bắt buộc.
- Tuỳ chọn: VirusTotal API free tier; không bắt buộc và không nằm trong v1.

### Ai thấy/sửa được dữ liệu gì

- P-01: Người dùng Windows đang đăng nhập thấy, sửa và xoá danh sách download, cài đặt và log local của chính mình; file nằm trong thư mục người dùng chọn.
- P-02: Extension chỉ thấy URL, referer, user-agent và cookie của tab khi người dùng chủ động bấm tải; cookie chỉ nằm trong bộ nhớ, không ghi disk/log, không gửi sang domain khác.
- P-03: Không ai khác thấy dữ liệu; không có server UTD, không telemetry; auto-update chỉ kiểm tra phiên bản, không gửi nội dung tải.
- P-04: Log local được lọc cookie/token/query nhạy cảm; người dùng xoá log và dữ liệu ứng dụng bất kỳ lúc nào.

### Xong nghĩa là gì

- Toàn bộ AC kiểm được trên Windows 10 và 11 sạch; không còn lỗi P1.
- 100 lần mất mạng, 20 lần kill app, 5 lần mô phỏng rút điện: không file nào hỏng, resume đúng.
- YouTube/TikTok/Facebook + một mẫu HLS + một mẫu DASH tải được; nội dung DRM bị từ chối rõ.
- Installer ký, auto-update chạy; README, LICENSE, NOTICE, PRIVACY và hướng dẫn extension đầy đủ.

## Kỹ thuật

### 1. Mục tiêu và nguyên tắc

- Tải đúng trước, nhanh sau: không bao giờ âm thầm lưu file hỏng; mọi lỗi phải giải thích được.
- Local-first: không tài khoản, không server, không telemetry; dữ liệu nằm trên máy người dùng.
- Tách engine khỏi UI để có thể thay engine (Rust → Go) hoặc thêm daemon sau mà không đổi UI.
- Không copy code GPL; không bypass DRM/paywall; media phụ thuộc extractor và có thể gãy.

### 2. Kiến trúc

- **App Tauri 2**: Rust core + React/TS UI. Engine là crate `utd-core` chạy trong process app ở v1; boundary IPC đã tách để sau này thành daemon.
- **`utd-native-host`**: binary nhỏ do Chrome/Edge/Firefox khởi chạy; nói chuyện với app qua named pipe (Windows) với ACL theo user + token; chỉ chấp nhận extension ID trong allowlist.
- **Extension**: MV3 cho Chromium, WebExtensions cho Firefox; quyền tối thiểu, host permission optional theo site; right-click, toolbar popup, overlay; takeover mặc định bật (chủ dự án chọn như IDM, 2026-10-26), tắt trong popup.
- **Engine**: tokio + reqwest/hyper + rustls; HTTP/1.1 + HTTP/2; phân đoạn thích ứng; preallocate + positional writes; SQLite WAL cho journal; verify; speed limit; retry/backoff; Doctor.
- **Media**: yt-dlp (external) cho site extractor; ffmpeg (external) cho mux/remux; HLS/DASH VOD native cho URL trực tiếp. Không bundle binary GPL; quản lý phiên bản, checksum, license notice, rollback.
- **Lưu trữ**: SQLite `%LOCALAPPDATA%\UTD-Data\utd.db`; settings `%APPDATA%\UTD\settings.json`; log `%LOCALAPPDATA%\UTD-Data\logs`; download mặc định `%USERPROFILE%\Downloads\UTD`; quarantine `%LOCALAPPDATA%\UTD-Data\quarantine`. Không lưu cookie/token (journal chỉ ghi task nào cần đăng nhập ở trang nào, không ghi giá trị).
- **Phân phối**: NSIS installer + Tauri updater (chữ ký), WebView2 bootstrap; portable/ARM64 để sau.

### 3. Luồng chính

- **Thêm URL**: parse URL → probe HEAD/Range (fallback GET range 0-0) → filename/size/MIME/Range/ETag → nếu nguy hiểm thì cảnh báo → tạo task → engine tải.
- **Resume**: đọc journal → kiểm tra file/temp, ETag/Last-Modified → tiếp tục segment dở; nếu server đổi file thì dừng và hỏi tải lại.
- **Right-click**: extension gửi message → native host → app; nếu app chưa chạy thì khởi động; app mở đúng luồng (file/media/takeover).
- **Media**: URL trang → yt-dlp JSON → format picker → tải (yt-dlp hoặc engine) → ffmpeg mux → verify/quarantine.
- **Hoàn tất**: verify (nếu có checksum) → ghi MOTW → quarantine nếu nguy hiểm → đánh dấu hoàn tất → thông báo.

### 4. Download Guard

- Tên file: chuẩn hoá NFC, bỏ control/bidi, chặn reserved names/ADS/trailing dot/space, giới hạn path, chống trùng tên; mọi đường dẫn cuối phải nằm trong thư mục đã chọn.
- Nội dung: so magic bytes với đuôi/MIME; phát hiện HTML/JS trả về thay vì file; kiểm tra Content-Length/truncation; không tự extract archive.
- Nguy hiểm: quarantine `.exe/.msi/.scr/.js/.vbs/.cmd/.bat/.lnk/.ps1/.iso/.img/.dll` và file unsigned/không rõ nguồn; hiện publisher/chữ ký; không auto-run.
- MOTW: ghi `Zone.Identifier` với `ZoneId=3`, HostUrl, ReferrerUrl; không xoá cảnh báo của Windows.
- Quét: Defender CLI nếu có; nếu không có thì nói rõ "chưa quét", không giả vờ. Không upload file.
- Verify: server Digest, sidecar `.sha256`/`.sha512`, hoặc hash người dùng; tính SHA-256 + BLAKE3; mismatch → failed + quarantine.
- Cookie/header: chỉ gửi theo request đã opt-in; strip Authorization/Cookie khi redirect sang domain khác; redact log.

### 5. Doctor

- Kiểm tra: Range/Content-Length, MIME/magic, ETag/Last-Modified, 429/Retry-After, captive portal, TLS/proxy/DNS, IPv4/IPv6, disk full/quyền/AV lock, signed URL hết hạn, CDN throttle theo kết nối, extractor lỗi.
- Hiển thị: nguyên nhân, bằng chứng, hành động sửa; tự áp dụng khi an toàn (giảm kết nối, chuyển single-stream, retry, đổi DNS? không tự đổi DNS), còn lại hướng dẫn.
- Không bao giờ tự tắt bảo vệ để "cho tải được".

### 6. Bảo mật và quyền riêng tư

- Named pipe ACL theo SID user + token handshake; không mở cổng localhost ở v1.
- Extension allowlist ID; validate message schema; không nhận lệnh shell/path tuỳ ý từ extension.
- TLS nghiêm ngặt, dùng system trust store; không có nút bỏ qua cert.
- Không telemetry; network allowlist: update server + URL người dùng yêu cầu + tải công cụ có consent.
- Signed installer/updater; kiểm chữ ký gói update trước khi chạy; rollback bản cũ.
- Log redaction: cookie, Authorization, query token, đường dẫn nhạy cảm.

### 7. Tương thích

- Windows 10 21H2+ và Windows 11, x64; WebView2; NSIS.
- Chrome/Edge bản Chromium gần nhất, Firefox 115+; Safari để sau.
- Ngôn ngữ VI + EN; UTF-8; tên file tiếng Việt/emoji; high DPI; keyboard cho action chính.
- Không cần quyền admin để cài native host (HKCU).

### 8. Acceptance criteria

- AC-01: KHI server hỗ trợ Range và file ≥1GB THÌ HỆ THỐNG PHẢI tải xong đúng byte/SHA-256 với 4–16 kết nối, resume không tải lại segment đã xong.
- AC-02: KHI app bị kill/reboot giữa chừng THÌ HỆ THỐNG PHẢI mất tối đa 5 giây tiến độ và hoàn tất đúng sau khi mở lại.
- AC-03: KHI mất mạng rồi có lại THÌ HỆ THỐNG PHẢI tự reconnect với backoff và hoàn tất, không tạo file hỏng.
- AC-04: KHI ETag/Last-Modified đổi giữa chừng THÌ HỆ THỐNG PHẢI dừng, báo file đã đổi và không ghép sai.
- AC-05: KHI server không hỗ trợ Range THÌ HỆ THỐNG PHẢI chuyển single-stream, báo rõ và vẫn hoàn tất.
- AC-06: KHI server trả HTML 200 thay vì file hoặc Content-Length sai THÌ HỆ THỐNG PHẢI phát hiện và không lưu file cuối.
- AC-07: KHI đặt giới hạn tốc độ THÌ HỆ THỐNG PHẢI giữ tốc độ trung bình ≤110% giới hạn sau 10 giây.
- AC-08: KHI tên file chứa `..\`, reserved name, ADS, RTL/bidi hoặc đuôi kép THÌ HỆ THỐNG PHẢI lưu an toàn trong thư mục đã chọn.
- AC-09: KHI có checksum THÌ HỆ THỐNG PHẢI verify trước khi đánh dấu hoàn tất; mismatch THÌ phải failed + quarantine.
- AC-10: KHI file thuộc loại nguy hiểm THÌ HỆ THỐNG PHẢI giữ trong quarantine, hiện chữ ký/publisher và không tự mở/chạy.
- AC-11: KHI tải xong file THÌ HỆ THỐNG PHẢI giữ MOTW để SmartScreen/Protected View vẫn hoạt động.
- AC-12: KHI gặp lỗi phổ biến THÌ HỆ THỐNG PHẢI hiện Doctor với nguyên nhân và hành động sửa cụ thể.
- AC-13: KHI cấu hình proxy THÌ HỆ THỐNG PHẢI áp dụng cho mọi request và báo lỗi rõ nếu proxy sai; NTLM/Kerberos ngoài phạm vi v1.
- AC-14: KHI người dùng chuột phải link/trang trong Chrome/Edge/Firefox THÌ HỆ THỐNG PHẢI nhận URL + headers/cookies theo opt-in và mở đúng luồng.
- AC-15: KHI bật takeover THÌ HỆ THỐNG PHẢI hủy download của browser và chuyển sang UTD với headers/cookies; tắt thì browser tải như thường. (2026-10-26: tạm dừng bản của trình duyệt trước, chỉ huỷ khi UTD đã dò được file; UTD chưa chạy/không nhận/quá 20 s thì trình duyệt tải tiếp — không mất download nào. Mặc định bật, S-76/S-77, `notes/auth-plan.md` §9.1 "GĐ3 — Tự bắt link tải".)
- AC-16: KHI người dùng tải video từ trang được hỗ trợ THÌ HỆ THỐNG PHẢI hiện danh sách chất lượng và tải format đã chọn; extractor lỗi THÌ có nút cập nhật/thử lại.
- AC-17: KHI URL là HLS/DASH không DRM THÌ HỆ THỐNG PHẢI tải và ghép bằng ffmpeg; KHI phát hiện DRM THÌ phải từ chối rõ, không thử bypass.
- AC-18: KHI người dùng chưa opt-in cookie THÌ HỆ THỐNG PHẢI không đọc/gửi cookie; khi opt-in THÌ cookie chỉ dùng cho request đó và không ghi disk/log. Opt-in là cho phép theo từng trang trong popup; extension chỉ đọc cookie bên trong cú bấm của người dùng trên trình duyệt (menu chuột phải, nút popup), app không bao giờ tự xin cookie (2026-10-25).
- AC-19: KHI app chạy THÌ HỆ THỐNG PHẢI không gửi telemetry; chỉ có request update và request do người dùng yêu cầu.
- AC-20: KHI có bản update THÌ HỆ THỐNG PHẢI chỉ chấp nhận gói đã ký; sai chữ ký THÌ từ chối và giữ bản cũ.
- AC-21: KHI mở app lần đầu THÌ HỆ THỐNG PHẢI hướng dẫn cài native host/extension và tải thử; lỗi THÌ có cách sửa.
- AC-22: KHI hiển thị UI THÌ HỆ THỐNG PHẢI có VI/EN, xử lý tên file Unicode đúng và thao tác chính bằng bàn phím.
- AC-23: KHI có 50 download đồng thời THÌ HỆ THỐNG PHẢI giữ UI phản hồi và RAM dưới 500MB (máy test 8GB).
- AC-24: KHI gỡ cài đặt THÌ HỆ THỐNG PHẢI không xoá file đã tải; cấu hình/log chỉ xoá khi người dùng chọn.
- AC-25: KHI native host nhận lệnh THÌ HỆ THỐNG PHẢI chỉ chấp nhận extension ID trong allowlist, pipe ACL đúng user và message đúng schema.
- AC-26: KHI không có Defender/kết quả quét THÌ HỆ THỐNG PHẢI nói rõ "chưa quét", vẫn quarantine và không giả vờ an toàn.
- AC-27: KHI phát hiện download trùng URL/ETag THÌ HỆ THỐNG PHẢI hỏi tiếp tục/tải lại/giữ cả hai, không ghi đè âm thầm.
- AC-28: KHI mạng yếu hoặc đổi Wi-Fi↔4G THÌ HỆ THỐNG PHẢI giảm concurrency, backoff và resume thay vì fail.
- AC-29: KHI extractor hết hạn THÌ HỆ THỐNG PHẢI gợi ý cập nhật, có rollback và không làm hỏng download khác.
- AC-30: KHI kiểm tra quyền riêng tư THÌ HỆ THỐNG PHẢI chứng minh P-01–P-04 bằng test: không telemetry, cookie không persist, log đã redact, dữ liệu local đúng user.

### 9. Edge case phải xử lý

- Tên file: tiếng Việt, emoji, RTL/bidi, `CON/NUL/COM1`, đuôi kép, dấu `:` ADS, path >260, trùng tên, file đang mở.
- Server: no Range, gzip + Range, Content-Length sai, 429/Retry-After, redirect vòng, signed URL hết hạn, captive portal, TLS lỗi, IPv6-only, CDN bóp.
- Đĩa/quyền: full, read-only, AV lock, thư mục bị xoá/đổi tên, file bị xoá khi đang tải.
- Vòng đời: sleep/hibernate, mất điện, đổi mạng/VPN/proxy, đổi giờ, double-click, hai instance.
- Extension: app chưa cài/chưa chạy, private window, cookie bị chặn, Chrome cập nhật, nhiều browser.
- Media: DRM, login, extractor cũ, live stream (ngoài phạm vi), playlist (ngoài phạm vi), file rất lớn, thiếu ffmpeg.
- Update/DB: update gián đoạn, DB hỏng, downgrade; phải có backup/khôi phục.
- Bảo mật: HTML giả file, path traversal, symlink/junction, MOTW giả.

### 10. Yêu cầu phi chức năng

- Hiệu năng: khởi động ≤2 giây; UI 60fps khi 50 task; CPU thấp khi tải lớn; RAM ≤500MB với 50 task.
- Độ bền: checkpoint ≤5 giây; không file hỏng; retry có jitter; chịu kill/power-cut.
- Bảo mật: signed build/update; IPC ACL; TLS strict; path sanitization; quarantine; không telemetry.
- Tương thích: Windows 10 21H2+/11 x64; Chrome/Edge/Firefox; WebView2.
- i18n: VI/EN; UTF-8; không hard-code chuỗi.
- Bảo trì: update yt-dlp độc lập; log/Doctor đủ để debug; feature flag cho media.

### 11. Legal và license

- Core Apache-2.0; DCO; giữ trademark/name.
- Không bundle yt-dlp/ffmpeg GPL; chạy process riêng, tải có consent/checksum, hiển thị license/nguồn.
- Không DRM/paywall bypass; media chỉ cho nội dung người dùng có quyền; disclaimer YouTube/TikTok/Facebook ToS.
- NOTICE/THIRD_PARTY_LICENSES; privacy policy cho extension; không thu thập dữ liệu.

### 12. Open-core

- Miễn phí mãi: engine, UI, extension, media, Guard, Doctor, cập nhật.
- Pro sau này (không thuộc v1): quản lý team/policy, remote/headless web UI, send-to-device, support/SLA. Không khoá tính năng an toàn.
