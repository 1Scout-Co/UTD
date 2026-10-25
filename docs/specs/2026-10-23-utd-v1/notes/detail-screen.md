# Màn Chi tiết download (S-15..S-21) + Cảnh báo trước khi tải (S-22)

Nhánh `claude/detail-screen-7c21e4`. SPEC: mục "Chi tiết download", "Cảnh báo trước khi tải"; AC-06, AC-09, AC-10, AC-12,
AC-26 (và AC-23 cho cách cập nhật trực tiếp).

## Người dùng thấy gì

**Mở**: nhấp đúp một dòng, phím **Enter** trên dòng đang chọn, hoặc **Chi tiết** (mục đầu của menu chuột phải / nút ⋯).
Nhấp đúp trước đây mở file đã xong; giờ là việc mặc định kiểu IDM (cửa sổ tiến độ), "Mở file" là nút chính của S-20.

| mã | khi | thấy | nút |
|---|---|---|---|
| S-15 | đang tải (cả đang chờ / kiểm máy chủ / xác minh / ghép) | % lớn, Tốc độ · Còn lại · Đã tải · Kết nối (đang chạy/trần), **bản đồ đoạn**: mỗi đoạn một khối rộng theo byte, tô phần đã có (xong · đang tải · tải dở · chưa tải · lỗi), bảng "Từng đoạn" gập sẵn | **Tạm dừng**, Mở thư mục |
| S-16 | task không còn trong journal | "Download này không còn" | **Về danh sách** |
| S-17 | đang đọc | khung xương + "Đang đọc chi tiết…" | — |
| S-18 | journal lỗi | "Không đọc được dữ liệu" + câu lỗi máy | **Thử lại**, Về danh sách |
| S-19 | tạm dừng | Dừng ở · Đoạn xong · Lý do (bạn tạm dừng / UTD bị đóng giữa chừng) · Còn thiếu, bản đồ đoạn | **Tiếp tục**, Huỷ tải (vào Thùng rác, khôi phục được), Mở thư mục |
| S-20 | hoàn tất | "SHA-256 khớp" hoặc **"Chưa kiểm hash — không khẳng định file an toàn"** (AC-26), đường dẫn, dung lượng, xong lúc, thời gian tải; file bị xoá/chuyển ngoài app thì báo | **Mở file**, Mở thư mục, Sao chép đường dẫn (ngay hàng Đường dẫn), Xoá khỏi danh sách |
| S-21 | lỗi / bị cách ly | Doctor có sẵn (`model/doctor.ts`): nguyên nhân, cách sửa; **Bằng chứng**: mã lỗi, lúc lỗi, đã tải tới đâu, số lần lỗi (kèm mã các lần trước), máy chủ | Nút chính theo Doctor: **Thử lại** (lỗi thử lại được), **Mở thư mục** (file cách ly — xem trong thư mục cách ly, không mở/chạy), **Đóng** (thử lại vô ích: nguồn đã gỡ, cần quyền, DRM…); thêm Sao chép log, Mở thư mục, Công cụ media (lỗi công cụ). Bản đồ đoạn chỉ hiện khi đã có đoạn và file không bị cách ly |

Task media (hình + tiếng là task con ẩn `MediaPart`) hiện một màn: bản đồ đoạn có hai hàng **Hình** / **Tiếng**; tải bằng
yt-dlp (không có task con) thì ghi "yt-dlp tải trọn video nên không có bản đồ đoạn".

**Chưa làm giới hạn tốc độ** (SPEC S-15/S-19 "đổi giới hạn tốc độ"): engine chưa có, nút bỏ hẳn. Việc sau: khi engine có
limiter theo task, thêm một ô ở S-15/S-19.

**S-22** — trước khi bắt đầu, nếu link có cảnh báo thì giữ lại và hỏi:

- Hộp **Thêm URL**: bấm Bắt đầu ⇒ dò link (`inspect_link`, tối đa 3 s) ⇒ `check_links`. Không cảnh báo ⇒ tải luôn, không
  hỏi. Có ⇒ khung cảnh báo thay ô nhập: mỗi cảnh báo một dòng (loại + chi tiết ngắn: `exe`, `pdf.exe`, `U+202E`, MIME,
  tên máy chủ); **Xem chi tiết** mở câu giải thích; **Vẫn tải** (đỏ, có ý thức); **Huỷ** là nút chính (an toàn), đóng hộp,
  không tạo task. Tên có ký tự đảo chiều hiện mã ký tự (`hoa-don[U+202E]fdp.exe`) thay vì tên lừa mắt.
- Hộp **Thêm hàng loạt** (gồm lô từ trình duyệt): bấm Bắt đầu ngay / Thêm vào hàng đợi ⇒ kiểm mọi link file đã chọn
  **một lần** (dùng kết quả dò sẵn của bảng). Có cảnh báo ⇒ một khung cho cả lô: "Link có cảnh báo: 2/120", đếm theo
  loại, Xem chi tiết = danh sách từng link + lý do; **Bỏ link cảnh báo** (nút chính: tải phần sạch), Vẫn tải cả lô, Huỷ
  (quay lại bảng). Cả lô đều cảnh báo thì Huỷ thành nút chính.
- **Một link từ trình duyệt** (chuột phải "Tải bằng UTD", takeover) trước đây tự bắt đầu ngay: nếu tên/link có cảnh báo
  thì app **không tự tải**, mở hộp Thêm hàng loạt với đúng link đó (cùng đường lô từ trình duyệt) để người dùng xem cảnh
  báo rồi quyết; extension nhận câu "Đã mở UTD để xác nhận file này". Link sạch vẫn tải ngay như cũ.
- Trang video/kênh không qua S-22 (tên file do UTD đặt).

Loại cảnh báo (nặng trước): ký tự ẩn/đảo chiều · tên giả đuôi (`hoa-don.pdf.exe`, nhiều khoảng trắng trước đuôi thật) ·
file chạy được (danh sách của Download Guard, gồm loại riêng macOS khi chạy trên mac) · MIME không khớp đuôi (server báo
chương trình mà tên không phải loại chạy được, hoặc hai nhóm chắc chắn khác nhau: ảnh/video-âm thanh/PDF/nén;
`octet-stream`, HTML và kiểu lạ không tính — HTML do engine chặn theo AC-06) · link `http://` (theo link sau chuyển hướng,
trừ chính máy này).

## Kỹ thuật

### Một bộ luật (Download Guard)
- `crates/utd-engine/src/safety/preflight.rs`: `warnings(&LinkFacts) -> Vec<Warning { kind, detail }>`, thuần, không mạng.
  Dùng lại `safety::dangerous_extension` (cùng danh sách cách ly sau khi tải). Luật tên giả đuôi + ký tự ẩn **chuyển từ**
  `src-tauri/src/files/paths.rs` vào đây (`safety::hidden_char`, `safety::spoofed_name`); hộp Đổi tên gọi lại đúng hai hàm
  này — không còn bản thứ hai.
- App: `src-tauri/src/preflight.rs` — lệnh `check_links(links)` (tối đa `batch::MAX_BATCH_ITEMS`), và IPC
  (`ipc.rs::route`) thêm nhánh `Route::Confirm` cho link đơn có cảnh báo.

### Journal: lịch sử task (migration v4)
- Bảng `task_events (task_id, at, status, code, message, downloaded_bytes)`, xoá theo task (khoá ngoại), giữ tối đa
  `MAX_TASK_EVENTS` = 50 mốc/task. `SCHEMA_VERSION` thành `pub const` = 4; test journal/migration so với hằng (không ghi
  số cứng nữa).
- Ghi tự động: `save_task` khi trạng thái đổi (hoặc mã lỗi đổi trong lúc lỗi/cách ly) — lưu lại cùng trạng thái (tiến độ,
  đổi tên) không thêm mốc; `set_task_status` (hàng đợi dừng/xếp lại); `mark_interrupted` ghi mốc `paused` + code
  `app.interrupted` (`INTERRUPTED_CODE`) ⇒ lý do dừng ở S-19. Đọc: `Journal::task_events(id)`.

### Lệnh Tauri mới (một khối trong `lib.rs`, sau `commands::inspect_link`)
- `download_detail(taskId) -> Option<DownloadDetail>`: task, `parts` (task HTTP: chính nó; media: task con theo thứ tự
  hình rồi tiếng, **không kèm link luồng** vì là link ký có hạn), đoạn từ journal, lịch sử, `targetPath`, `fileExists`
  (chỉ khi đã xong), tiêu đề media. `None` ⇒ S-16; `Err` ⇒ S-18.
- `watch_download(taskId | null)`: task đang mở ở màn Chi tiết (nhiều nhất một).
- `download_log(taskId)`: nhật ký chữ thuần (tiếng Anh, như log máy) cho "Sao chép log": link bỏ query/fragment (token),
  câu lỗi có link cũng che, thư mục người dùng thành `~`, không header (P-04).
- `check_links(links)`: xem trên.

### Cập nhật trực tiếp, không poll (AC-23)
- Tốc độ, còn lại, đã tải, kết nối: lấy từ `utd://progress` mà màn chính vốn đã nhận — màn Chi tiết không nghe thêm gì.
- Bản đồ đoạn: sự kiện mới **`utd://segments`** `{ taskId, parts }`. `detail::progressed(app, taskId)` được gọi sau mỗi
  lần báo tiến độ (`events::emit_engine_event` cho HTTP, `Sink for AppHandle::progress` cho media); chỉ task đang
  `watch_download` mới đọc journal + đẩy, tối đa ~1 lần/giây (bằng nhịp checkpoint của engine). 50 download chạy cùng lúc
  mà không mở chi tiết: mỗi lần báo tiến độ chỉ tốn một phép so chuỗi dưới mutex.
- Trạng thái task đổi (`utd://task`) ⇒ giao diện đọc lại `download_detail` một lần (lịch sử, kết quả mới), giữ màn cũ trong
  lúc đọc. Đóng hộp ⇒ `watch_download(null)`.

### Giao diện (`src/features/home`)
- `api/detail.ts` (lệnh + `preflightLink`), `model/detail.ts` (chọn màn theo trạng thái, khối bản đồ đoạn, lý do dừng,
  các lần lỗi…), `model/warnings.ts` (gộp cảnh báo, đếm theo loại, hiện mã ký tự ẩn), `ui/detail-dialog.tsx`,
  `ui/detail-states.tsx` (mỗi màn một hàm, gốc có `data-s`), `ui/detail-parts.tsx` (bản đồ đoạn, ô số liệu),
  `ui/detail-flow.tsx` (nối thao tác thật), `ui/use-download-detail.ts`, `ui/warning-panel.tsx` (S-22 một link / cả lô).
- Bản xem trước trên trình duyệt: chi tiết mẫu dựng từ dòng mẫu (`model/mock-data.ts::demoDetail`).
- `menu.*` / Doctor dùng lại khoá có sẵn; khoá mới chỉ `detail.*` (chèn sau khối `doctor.*`) và `warn.*` (sau khối
  `add.*`), đủ 16 ngôn ngữ (70 khoá).

### Chỗ chừa cho phiên "khôi phục download lỗi/gián đoạn"
- **Nút "Cập nhật link"** ở S-21: `DetailActions.failedExtra?: (detail) => ReactNode` (`ui/detail-states.tsx`) — vẽ trước
  nút Thử lại. Nối ở `ui/detail-flow.tsx` (`handlers.failedExtra`), vd hiện nút khi `detail.task.error.code` là link hết
  hạn (403/410 của link ký).
- Mốc lịch sử riêng (đã thử lại tự động, đã thay link): ghi thẳng vào `task_events` (code riêng, vd `retry.*`, `link.*`);
  S-21 "Số lần lỗi" và "Sao chép log" tự hiện.
- Đã có migration v4: thêm migration thì đánh số 5.

## Kiểm

- Rust: `utd-engine` (journal: ghi mốc khi đổi trạng thái/mã lỗi, không ghi khi chỉ lưu tiến độ, lý do dừng, giới hạn 50 mốc
  + xoá theo task; preflight: từng loại cảnh báo, link sau chuyển hướng, máy cục bộ, JSON cho giao diện), app (`detail`:
  task mất ⇒ None, task HTTP 1 luồng + lịch sử, media hình→tiếng không lộ link ký, file đã xong còn/mất, chỉ đẩy task đang
  mở và không dày hơn 1 nhịp; `detail_log`: che token + thư mục người dùng, ngày UTC; `preflight`; `ipc`: link đơn có
  cảnh báo ⇒ `Route::Confirm`).
- Playwright `e2e/detail.spec.ts` (15 bài: S-15 số liệu sống + `utd://segments` + bảng từng đoạn + Tạm dừng +
  watch/unwatch; mở bằng menu và Enter; media gộp; S-16; S-17; S-18 thử lại; S-19; S-20 hai biến thể; S-21 lỗi + cách ly;
  axe S-15/S-19/S-20/S-21) và `e2e/warning.spec.ts` (7 bài: một link — Vẫn tải / Huỷ / Xem chi tiết, tên đảo chiều, link
  sạch không hỏi; lô — bỏ link cảnh báo, cả lô cảnh báo; axe). `e2e/tauri-mock.ts` thêm `PENDING`, `failWith()`.
- Sửa tên 6 bài cũ trong `e2e/add-download.spec.ts` ghi nhầm mã: Cài đặt S-15 → S-27, Chẩn đoán S-16 → S-21.
- Bộ như CI (2026-10-25): `cargo test -p utd-engine` / `-p utd-mp4` / `-p app --lib` (215) xanh, clippy 3 crate
  `-D warnings` sạch, `pnpm lint` (16 ngôn ngữ × 653 khoá), `pnpm build`, `pnpm exec playwright test` 94/94.
- **App thật** (bản debug, `UTD_DATA_DIR` riêng trong scratchpad, WebView2 riêng; không đụng UTD-Data của chủ máy) với máy
  chủ thử trên máy (file 160 MB có Range, bóp 150 KB/s mỗi kết nối): S-15 bản đồ 16 đoạn tự chạy qua `utd://segments`
  (đo độ rộng khối đổi sau 3 s), Tạm dừng trong hộp ⇒ S-19 "Bạn tạm dừng"; file có SHA-256 ⇒ S-20 "SHA-256 khớp"; 404 ⇒
  S-21; "Sao chép log" của link `?X-Amz-Signature=…` ra `…/signed.bin?…`, thư mục người dùng thành `~`; Thêm URL
  `http://<IP LAN>/tools/setup.exe` ⇒ S-22 (file chạy được + link không mã hoá), Vẫn tải ⇒ Guard cách ly ⇒ S-21 "File bị
  Download Guard cách ly"; Thêm hàng loạt 4 link (dò thật) ⇒ S-22 "2/4", `invoice.pdf` do server trả
  `application/x-msdownload` bị bắt "Kiểu file không khớp tên", Bỏ link cảnh báo chỉ gửi 2 link sạch; gửi `setup.exe` qua
  **named pipe thật** (giả native host) ⇒ không tạo task, mở hộp hàng loạt, extension nhận "Đã mở UTD để xác nhận file
  này"; link sạch qua pipe vẫn tải ngay.

## Ảnh (`ui-demos/`)
App thật: `detail-s15-downloading.png`, `detail-s19-paused.png`, `detail-s20-completed.png`, `detail-s21-failed.png` (404),
`detail-s21-quarantined.png`, `warn-s22-single.png`, `warn-s22-batch.png`. App giả của e2e (khó dựng bằng app thật):
`detail-s15-media.png` (hình + tiếng), `detail-s16-gone.png`, `detail-s17-loading.png`, `detail-s18-read-failed.png`.

## Sửa kèm (lỗi có sẵn, lộ ra khi làm màn này)
- Màn chính: file nhỏ chạy xong (`utd://task`) **trước khi** lệnh thêm trả về thì kết quả lệnh ("đang chờ") đè trạng thái
  mới — dòng kẹt ở "Đang chờ" dù file đã xong/bị cách ly (thấy thật với `setup.exe` 200 KB). `home-screen.tsx::putFirst`:
  task vừa thêm lên đầu nhưng giữ bản mới hơn đã có. Màn Chi tiết cũng lấy bản mới hơn giữa danh sách và journal
  (`updatedAt`).

## Ghi chú khi ghép
- **Kích thước JS 197,8 / 200 kB** (gzip, `.size-limit.json`) sau đợt này: còn ~2 kB. Ghép thêm nhánh khác có giao diện
  dễ vượt; AI_RULES nói vượt thì cắt code, không nới.

## Việc sau / chưa làm
- Giới hạn tốc độ theo task (engine chưa có).
- `engine::http::sanitize_suggested_name` chưa bỏ ký tự bidi (U+202E…) khỏi tên server đề xuất như SPEC "Tên file: …bỏ
  control/bidi" — S-22 cảnh báo được, nhưng nếu người dùng Vẫn tải thì file lưu với tên đó. Nên sửa ở engine (AC-08).
- Menu "Chẩn đoán" vẫn mở hộp Doctor cũ; có thể cho mở thẳng S-21 của màn Chi tiết.
- Nút "Công cụ media" ở S-21 mở Cài đặt nhưng chưa cuộn tới mục Công cụ media (Cài đặt mới chỉ có `focus: "updates"`).
