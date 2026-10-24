# Tải hàng loạt + hàng đợi chung (việc 3)

Nhánh `claude/batch-download-4d17fc`. Ngày 2026-10-24.

## Đã làm

### 1. Hàng đợi chung ở backend — `src-tauri/src/queue.rs`
- MỌI download đi qua `AppState.queue` (`DownloadQueue`): thêm lẻ (`add_download`), từ trình duyệt (IPC),
  media (`add_media_download`), lô (`add_batch_downloads`), Tiếp tục / Thử lại (`resume_download`).
- Chạy cùng lúc tối đa `limit` task (Cài đặt → "Tải cùng lúc", mặc định **3**, chọn 1–6, 8, 10; lưu
  trong settings `queue_limit`). Giảm giới hạn không dừng task đang chạy.
- Task chờ = trạng thái `queued` trong journal. Một lượt chạy kết thúc (xong, tạm dừng, lỗi, cách ly)
  ⇒ `release` ⇒ chạy task kế. Task media giữ MỘT chỗ (luồng hình/tiếng ẩn không qua hàng đợi).
- Thứ tự chờ lưu ở settings `queue_order` (JSON `[{id, rename?}]`, đang chạy rồi đang chờ) mỗi lần đổi.
- Mở app: (1) task dừng để cài bản mới (`update_resume`, `updater::take_resume_ids`), (2) task đang chạy
  lúc app tắt ngang (`queue::interrupted`, đọc trước `mark_interrupted`) lên **đầu hàng**, rồi tới thứ tự
  đã lưu, rồi task `queued` cũ không có trong thứ tự (theo giờ tạo). Task đã Tạm dừng vẫn dừng.
  → Thay đổi hành vi: trước đây app tắt ngang thì download thành "Tạm dừng", nay tự chạy tiếp.
- Tạm dừng task đang chờ ⇒ bỏ khỏi hàng, `paused` (phát `utd://task`). Task đang chạy ⇒ huỷ token như cũ.
- Xoá khỏi danh sách: task đang chờ xoá được ngay (`forget` rồi xoá); giao diện bật lại mục này cho task `queued`.
- Cài bản mới: `freeze()` trước khi dừng download (task kế không chen vào), cài lỗi thì `thaw()` + chạy lại.
- Task đang bận (đổi tên/chuyển thư mục giữ chỗ trong `RunningTasks`) ⇒ để lại đầu hàng, thử lại sau 500 ms.

### 2. API cho các phiên khác (nhất là việc 6 — tải cả kênh TikTok)
- Chỉ cần gọi `add_media_download` cho từng video; hàng đợi tự giới hạn, KHÔNG cần tự điều tiết:
  ```ts
  invoke("add_media_download", { input: {
    url,                 // link trang video
    title: "",           // rỗng ⇒ tên tạm "tiktok.com <mã>", đổi theo tiêu đề thật khi Phân tích
    quality: "best",     // best | <chiều cao 144–4320, vd 1080, 720> | audio-m4a | audio-mp3
    later: true,         // true = xếp CUỐI hàng (lô tự động); mặc định false = chen ĐẦU hàng
    destDir?: string,    // tuỳ chọn, kiểm như Cài đặt; không gửi = thư mục mặc định
  }})
  ```
  Gửi tuần tự với `later: true` ⇒ chạy đúng thứ tự gửi. Muốn thêm cả lô một lần (một lệnh, đúng thứ tự,
  tự đổi tên file trùng): `add_batch_downloads({ input: { urls, quality, later, destDir? } })` → `{ tasks, failed[] }`.
- `quality` thay cho `selector`: job lưu dấu `utd:<chất lượng>` (không đổi schema DB); tới lượt chạy,
  `media/preset.rs::settle` chạy `probe::probe` (điền `probe_cache` ⇒ đường tải nhanh không gọi yt-dlp
  lần hai), chọn định dạng probe trả: đúng mức, thiếu ⇒ mức thấp hơn gần nhất, không có mức thấp hơn ⇒ mức
  thấp nhất, trang chỉ có tiếng ⇒ "Tốt nhất"; MP3 cần ffmpeg. Lỗi Phân tích ⇒ task `failed`
  (`media.probe_failed`), Tiếp tục ⇒ Phân tích lại. Gửi `selector` như cũ vẫn chạy y nguyên.
- `add_download` nhận thêm `later?: boolean`.
- Lệnh mới: `queue_settings()` / `set_queue_limit(limit)` → `{ limit, maxLimit }`; `inspect_link(url)` →
  `RemoteFile { url, fileName, totalBytes, contentType, page }` (engine `DownloadEngine::inspect`: HEAD hoặc
  GET 1 byte, không tạo task); `add_batch_downloads`; `take_batch_request`.

### 3. Hộp "Thêm hàng loạt" (S-05 trống / S-07 có link)
- Mở: nút "Hàng loạt" cạnh "Thêm URL", **Ctrl+B**, dán ≥ 2 link vào ô URL của "Thêm download", hoặc lô từ trình duyệt.
- Nguồn: "Danh sách" (mỗi dòng một link; bỏ trùng, bỏ dòng không phải link, báo số đã bỏ; nút Dán, Nhập .txt ≤ 5 MB)
  hoặc "Mẫu" kiểu IDM `https://site/img[001-120].jpg`, `[a-z]`, `[A-Z]`, nhiều ngoặc (ngoặc đầu đổi chậm nhất),
  khoảng ngược `[3-1]`; hiện trước số link; chặn > 10.000.
- Bảng xem trước: tick từng dòng / tất cả / theo nhóm (đuôi file, Video, Trang web), tên + kích thước (dò 6 link
  một lúc, tối đa 500 link mỗi lần mở hộp, không chặn giao diện), loại File / Video / Trang web (server trả HTML
  ⇒ bỏ tick). Bảng chỉ vẽ các dòng đang thấy (10.000 dòng vẫn mượt).
- Có link trang video ⇒ một chất lượng chung: Tốt nhất · 1080p · 720p · M4A · MP3 (không mở hộp chọn từng link).
- Chọn thư mục; "Thêm vào hàng đợi" (cuối hàng) hoặc "Bắt đầu ngay" (nút chính, đầu hàng, giữ thứ tự lô).
  Link lỗi khi thêm ⇒ hộp giữ lại đúng các link đó kèm câu lỗi.
- File trùng tên trong lô hoặc đã có trong thư mục ⇒ `tên (1).đuôi` (`batch.rs::free_name`).

### 4. Extension — "Tải tất cả link bằng UTD"
- Menu chuột phải mới (trang, link, ảnh, vùng chọn). Có vùng chọn ⇒ chỉ lấy link trong vùng chọn.
- Gom `<a href>` có đuôi tệp quen thuộc hoặc là trang xem video (regex gần đúng luật media_pages, app kiểm lại),
  và mọi `img/video/audio/source` http(s); bỏ `#…`, bỏ trùng; tối đa 2000 link, ~900 KB.
- Thêm quyền `scripting` (không hiện cảnh báo quyền mới; dùng cùng `activeTab`).
- Tin nhắn mới `{version:1, type:"batch", urls, pageUrl}` → native host kiểm (`pipe_payload`) → pipe
  `{token, url, pageUrl, batch}` → `ipc::route` kiểm token rồi `batch::clean_links` (http/https, ≤ 8192 ký tự,
  bỏ trùng, ≤ 2000) → `utd://batch-request` (giữ lô mới nhất nếu giao diện chưa nghe, `take_batch_request`).
  Pipe đọc tối đa 2 MiB một dòng (trước 64 KiB). Không tạo task nào: người dùng chọn trong hộp rồi mới tải.

## Kiểm
- `cargo test -p utd-engine` (thêm `tests/inspect.rs`), `cargo test -p utd-mp4`, `cargo test -p app --lib`
  (thêm: `queue_tests.rs` gồm giới hạn 3, thứ tự, chen đầu giữ thứ tự lô, tạm dừng, xoá, task bận, đổi giới hạn,
  đóng băng khi cập nhật, mở lại app, và một bài engine thật + server cục bộ chậm: đỉnh đúng 3, chạy đúng thứ tự;
  `batch.rs`, `media/preset.rs`, `ipc.rs`, `native_host.rs`).
- `cargo clippy -p app --all-targets -- -D warnings`, `-p utd-engine` sạch.
- `pnpm lint`, `pnpm build` (JS 177,6 kB / 200 kB gzip), `pnpm exec playwright test` 42/42 (mới: `e2e/batch.spec.ts`
  13 bài: tách link, mẫu, hộp hàng loạt, cửa sổ 1080×680, a11y, Cài đặt, desktop giả lập: gửi lô một lần + dò song
  song ≤ 6, lô từ trình duyệt, link lỗi giữ lại, lô tới sớm, extension gom/lọc link), `ui-check` ĐẠT, arch size đạt.

### Thử thật (2026-10-24, `UTD_DATA_DIR` riêng, WebView2 debug 9413, bản `tauri build --debug --no-bundle`)
- Lô 6 file thật (ovh 10 MB + 1 MB, 2 link Cloudflare cùng tên `__down`, dummy.pdf, SHA2-256SUMS trên GitHub) + 1 dòng
  rác + 1 link trùng + 2 link YouTube, giới hạn 2, chất lượng 720p: bảng hiện đủ tên/kích thước, "Đã bỏ 1 trùng,
  1 dòng không phải link"; tối đa 2 chạy cùng lúc; `__down` + `__down (1)`; "Me at the zoo.mp4" tự Phân tích và đặt
  tên theo tiêu đề. Video thứ hai (`BaW_jenozKc`) báo lỗi rõ — video đã bị YouTube gỡ (`yt-dlp`: "This video is
  unavailable"), không phải lỗi app. Lô MP3 với `C0DPdy98e4c` ⇒ "TEST VIDEO.mp3".
- Giới hạn 1, "Thêm vào hàng đợi" 3 link, tạm dừng link thứ 3 khi đang chờ, thêm lẻ một link khi đầy chỗ (⇒ `queued`),
  rồi **tắt ngang** app (Stop-Process) khi 100Mb.dat đang tải: mở lại ⇒ 100Mb.dat tự lên đầu, chạy tiếp từ byte đã có,
  rồi link thêm lẻ, rồi link kế; link đã tạm dừng vẫn dừng; Tiếp tục ⇒ chạy và xong. Luôn đúng 1 chạy cùng lúc.
  (100Mb.dat và 1Mb (1).dat sau đó báo "connection timed out": proof.ovh.net ngừng trả lời cả với curl, lỗi phía máy chủ.)
- Native host thật (`UTD.exe --native-host` nhận tin nhắn từ stdin như trình duyệt, cùng `UTD_DATA_DIR`): tin `batch`
  ⇒ "Đã mở UTD để chọn link", hộp hàng loạt mở với đúng link (bỏ `javascript:` và link trùng, link không rõ là file bỏ
  tick sẵn); tin chỉ có `blob:` ⇒ từ chối "Trang này không có link tải được.". "Thêm vào hàng đợi" ⇒ `__down (2)`,
  "TEST VIDEO (1).mp4".
- Chưa thử: nạp extension vào Chrome/Firefox thật (không bật tích hợp trình duyệt của bản thật); logic gom/lọc link của
  extension chỉ kiểm bằng e2e với API trình duyệt giả.
- Đã tắt app thử (đúng PID) và xoá thư mục dữ liệu/tải thử.

## Ghi chú cho phiên điều phối
- Chuỗi giao diện mới viết thẳng tiếng Việt như code xung quanh (phiên đa ngôn ngữ sẽ gom).
- Thêm 2 thành phần shadcn: `components/ui/textarea.tsx`, `components/ui/checkbox.tsx` (`pnpm dlx shadcn@4.21.4 add`).
- `.claude/launch.json` (cổng 5413) chỉ để xem trước trong phiên này.
- Đụng file chung: `commands.rs` (tách `create_download`, `download_dir_for`, `resume_task`), `media/mod.rs`
  (`AddMediaInput` thêm trường, tách `create_media`), `media/runner.rs` (3 dòng gọi `preset::settle`),
  `updater.rs` (bỏ `resume_after_update`, thêm `take_resume_ids`, freeze/thaw), `lib.rs`, `ipc.rs`, `native_host.rs`,
  `home-screen.tsx`, `add-download-dialog.tsx`, `settings-dialog.tsx`, `soniox*.tsx`, `download-mapper.ts`.
- Mã màn: hộp hàng loạt dùng `data-s` S-05 (chưa có link) / S-07 (có link) của nhóm "Thêm download" trong SPEC.

## Việc còn lại
- Chưa có kéo-thả thứ tự trong hàng đợi, chưa có lịch hẹn giờ chạy hàng đợi như IDM.
- Hai video trùng tiêu đề trong cùng thư mục chạy cùng lúc có thể trùng tên (đặt tên lúc Phân tích chỉ xem đĩa).
- Dò kích thước chỉ 500 link đầu mỗi lần mở hộp; link sau hiện "—".
- Firefox: chưa thử tay menu mới (API `scripting` dùng chung với Chromium).
