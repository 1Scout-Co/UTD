# Khôi phục download lỗi / bị gián đoạn

Nhánh `claude/recovery-9f3b1d`. Ngày 2026-10-25. Bám SPEC AC-02, AC-03, AC-04, AC-28, mục 9 và "Xong nghĩa là gì".

## 1. Khảo sát trước khi sửa (bản v0.1.0, commit bd1ef9c)

Mọi đường một download thành lỗi hoặc dừng ngang, và app đang xử lý thế nào.

### Engine — `crates/utd-engine/src/engine/*`

| # | Đường | Ở đâu | App làm gì (trước) | Vấn đề |
|---|---|---|---|---|
| E1 | Dò link (HEAD rồi GET `bytes=0-0`) lỗi mạng / 5xx / 408 / 425 / 429 | `session.rs::probe_with_retry` | thử tối đa 4 lượt, chờ `Retry-After` (≤ 300 s) hoặc 0,5 s × n, rồi **Lỗi** | hết 4 lượt là chết, người dùng phải bấm Thử lại |
| E2 | Máy chủ nhận kết nối rồi im | `http.rs::probe` (mỗi yêu cầu hạn `connect_timeout` 15 s) | HEAD 15 s + GET 15 s, × 4 lượt ⇒ đứng "đang dò" ~2–2,5 phút rồi **Lỗi** `network.timeout` | finding đã biết |
| E3 | Dò ra 401/403/407 (link ký hết hạn, cần đăng nhập) | `failure_from_status` lớp `User` | **Lỗi** ngay | file thường hỏng hẳn: Thử lại vẫn 403, chỉ còn xoá rồi tải lại từ đầu (finding đã biết) |
| E4 | 404/410/451 lúc dò | lớp `Permanent` | **Lỗi** ngay | đúng |
| E5 | Mở lại task mà ETag/Last-Modified/kích thước đổi | `session.rs::run` (`metadata_changed`) | **âm thầm xoá tiến độ, tải lại từ 0** | AC-04 đòi dừng và báo "file đã đổi" |
| E6 | Link là trang web / thân rỗng (AC-06) | `screen_page`, `finish` | **Lỗi** `content.not_a_file` | đúng |
| E7 | Tên đích đã có file | `run`, `place` | **Lỗi** `file.exists` (giữ `.part`) | đúng |
| E8 | Một kết nối rớt / 5xx / 429 giữa chừng | `worker.rs::run_segment` | thử lại tối đa 8 lượt/kết nối, giãn 0,5 s→30 s có jitter; có thêm byte thì đếm lại; 429 khi kết nối khác vẫn chạy thì không đếm | ổn cho chập chờn ngắn |
| E9 | **Mất mạng cả loạt** vài phút | như E8 | mỗi kết nối chờ `idle_timeout` 30 s × 8 lượt ⇒ sau ~2–5 phút cả lượt **Lỗi** `network.*` | AC-03/AC-28: mất mạng lâu thì thành lỗi, không tự nối lại |
| E10 | 429 kèm `Retry-After` dài | `http.rs::parse_retry_after` cắt còn 300 s, worker ngủ ngay trong lượt | giữ chỗ chạy trong lúc ngủ, 8 lượt là **Lỗi** | finding đã biết |
| E11 | 401/403/404 **giữa chừng** (link ký hết hạn khi đang tải) | `SegmentJob::request` trả `Retry` cho mọi mã ≠ 2xx | thử lại 8 lượt (~1 phút) dù không bao giờ khỏi, rồi **Lỗi** | phí thời gian; 403 như E3 |
| E12 | Content-Range sai / lệch đoạn / máy chủ bỏ qua Range | `request` | **Lỗi** ngay (`http.content_range`, `http.range_mismatch`, `http.range_ignored`) | đúng |
| E13 | Tổng kích thước đổi giữa chừng | `request` | **Lỗi** `http.size_mismatch` | đúng một nửa AC-04 |
| E14 | **ETag đổi giữa chừng, cùng kích thước** | không kiểm | ghép đoạn file cũ với file mới ⇒ **file hỏng âm thầm** (trừ khi có SHA-256) | vi phạm AC-04 |
| E15 | Ghi đĩa lỗi (đầy ổ, ổ rút, quyền) | `PartFile::write_at` | **Lỗi** `disk.write` lớp Permanent, không phân biệt đầy ổ | đầy ổ phải tạm dừng chờ chỗ |
| E16 | Mở/cấp phát `.part` lỗi (đầy ổ khi `set_len`, ổ USB đã rút, thư mục mất quyền) | `open_part(...)?` | trả `Err` lên app ⇒ app chỉ phát `utd://error`, **task kẹt ở "đang dò/đang tải"** trong journal tới lần mở app sau, rồi lại kẹt | lỗi thật, mọi `?` trong `run` đều vậy |
| E17 | `.part` hoặc thư mục tải bị xoá/đổi tên **lúc đang dừng** | `run` (`usable` sai) | âm thầm tạo lại thư mục, **tải lại từ 0**, không báo | phải báo rõ |
| E18 | `.part` ngắn hơn journal | `run` | xoá hết tiến độ, tải lại từ 0 | chỉ cần tải lại phần thiếu |
| E19 | `.part` bị xoá/đổi tên **lúc đang tải** | không kiểm | Windows: handle mở với `FILE_SHARE_DELETE` nên xoá được, engine ghi tiếp vào file đã vào Thùng rác; tới cuối `place` báo **Lỗi** `file.rename`, mất hết | phải phát hiện sớm |
| E20 | Một luồng (không Range / không rõ kích thước) | `run_single`, `StreamJob` | mỗi lượt chạy lại từ 0 (AC-05); trong lượt thì nối Range nếu máy chủ cho; máy chủ trả lại từ đầu > 3 lần ⇒ **Lỗi** `http.restart` | đúng theo SPEC |
| E21 | Tạm dừng / huỷ | `cancel` | checkpoint rồi **Tạm dừng** | đúng |
| E22 | Hash lệch / đọc lỗi lúc verify / Guard | `finish` | **Lỗi** (security) / cách ly | đúng |
| E23 | Đồng bộ trước khi đưa vào chỗ lỗi | `place` | **Lỗi** `disk.sync` | đúng |

### Journal và mở app — `db.rs`, `lib.rs`, `queue.rs`

| # | Đường | App làm gì (trước) | Vấn đề |
|---|---|---|---|
| J1 | App bị kill | checkpoint journal mỗi 1 s (WAL); mở app: `queue::interrupted` + `mark_interrupted` ⇒ task đang chạy lên đầu hàng, chạy tiếp | ổn với kill (bộ đệm hệ điều hành còn) |
| J2 | **Rút điện** | journal `synchronous=NORMAL`, **`.part` không bao giờ fsync trước checkpoint** | journal có thể đã nhận "đoạn X xong" trong khi byte của đoạn X còn nằm trong bộ đệm và mất ⇒ file cuối có vùng toàn 0, **hỏng âm thầm** |
| J3 | Đặt lại tiến độ rồi cắt `.part` | `reset_task_progress` (NORMAL, không fsync) rồi `set_len(0)` | rút điện giữa hai bước: journal cũ còn, file đã cắt ⇒ như J2 |
| J4 | Mở lại task | `usable` chỉ so tổng byte và độ dài file | không kiểm nội dung khớp journal |
| J5 | Task `failed` | đứng yên tới khi người dùng bấm Thử lại | **không có tự thử lại** nào ở mức task |

### Media — `src-tauri/src/media/*`

| # | Đường | App làm gì (trước) | Vấn đề |
|---|---|---|---|
| M1 | Link luồng hết hạn (`expire=`) lúc tiếp tục | `fast.rs::resume_plan` hỏi yt-dlp link mới trước | ổn |
| M2 | 401/403/410 giữa chừng (đã có byte) | `recover` lấy link mới ≤ 2 lần/lượt, `parts::reconcile` giữ đoạn nếu cùng kích thước | ổn |
| M3 | Link của lần "Phân tích" (`probe_cache`, chỉ trong RAM) hỏng trước byte đầu | bỏ cache, hỏi yt-dlp | ổn |
| M4 | Luồng con lỗi mạng giữa chừng (đã có byte) | `part_error` ⇒ **Lỗi** `media.network` "Bấm Tiếp tục" | không tự thử lại |
| M5 | yt-dlp lỗi (đường dự phòng) | `--retries 5 --fragment-retries 5 --socket-timeout 15`, rồi `classify_failure` ⇒ **Lỗi** | không tự thử lại; `media.rate_limited` không chờ |
| M6 | Luồng đã xong mà file luồng mất | `parts::repair_missing` tải lại luồng đó | ổn |
| M7 | Xoá task media dở | `parts::discard_parts` dọn `.part` của engine | **sót `*.f*.part`, `.ytdl`, `*.part-Frag*`, `*.temp.*` của yt-dlp** (finding đã biết) |
| M8 | App tắt ngang khi đang ghép / chuyển âm thanh | `mark_interrupted` ⇒ chạy lại, ghép lại | ổn |

### Kết luận khảo sát

Hỏng **âm thầm** (nguy hiểm nhất): E14, J2, J3. Kẹt trạng thái: E16. Chết vì lỗi tạm thời: E1, E2, E9, E10, M4,
M5, J5. Hỏng hẳn dù cứu được: E3/E11 (link ký hết hạn), E17/E18 (tải lại từ 0). Báo sai/không báo: E5, E15, E19.

## 2. Sau khi sửa

| # | Trước | Sau | Ở đâu |
|---|---|---|---|
| E1 | 4 lượt dò rồi **Lỗi** | dò tối đa 2 lượt/lần chạy; lỗi tạm thời ⇒ task **tạm dừng chờ tự thử lại** ("Sẽ thử lại lúc 14:32 · lý do"), giãn 5 s → 15 s → 30 s → 1 → 2 → 5 → 10 → 15 → 30 phút (±20%), tối đa 10 lần liền không thêm byte; có thêm byte thì đếm lại | `engine/session.rs::probe_with_retry`, `recovery.rs::plan`, `src-tauri/src/recover.rs` |
| E2 | đứng "đang dò" ~2,5 phút | HEAD tối đa 8 s, GET theo hạn kết nối; hết hạn chờ thì thôi luôn, không thử lại trong lượt ⇒ ≤ ~23 s rồi "Sẽ thử lại lúc…" (test: máy chủ im ⇒ ~6 s với hạn 3 s) | `http.rs::probe`, `session.rs` |
| E3/E11 | 403 thử 8 lượt rồi **hỏng hẳn** | 401/403/404/410… khi không còn kết nối nào khác chạy ⇒ dừng ngay (403 hết hạn: < 4 s thay vì ~1 phút); task HTTP 401/403/407/410 ⇒ lỗi kèm `recovery.wait = link` ⇒ **Cập nhật link…** (menu chuột phải, Doctor) hoặc tải lại file từ trình duyệt; giữ phần đã tải | `worker.rs::ends_the_run`, `relink.rs`, `recover.rs::capture_link`, `refresh-link-dialog.tsx` |
| E5 | âm thầm tải lại từ 0 | task gốc có Range: **dừng, báo `file.changed`** (AC-04), Doctor có nút **Tải lại từ đầu**; luồng con media vẫn tự bỏ đoạn cũ (app hỏi yt-dlp rồi ghép lại) | `session.rs` |
| E8 | như cũ | như cũ; Retry-After trong lượt chờ tối đa 30 s | `worker.rs::retry_delay` |
| E9 | mất mạng vài phút ⇒ **Lỗi** | engine hỏi bảng định tuyến mỗi giây (UDP `connect` tới địa chỉ dành cho tài liệu, không gửi gói nào): mất route ≥ 3 s mà không nhận byte ⇒ dừng lượt `network.offline`; app **chờ có mạng** (không lỗi, không đếm lần thử), có route lại là chạy ngay (và mỗi phút thử một lần, cho máy chỉ ra mạng qua proxy) | `engine/net.rs`, `recovery.rs` |
| AC-28 | kết nối cũ chết lặng ⇒ chờ hết hạn im 30 s | IP nguồn đổi (Wi-Fi↔4G, VPN) hoặc giờ máy nhảy > 5 s so với đồng hồ đơn điệu (ngủ, ngủ đông) ⇒ tăng `epoch`; kết nối đang chờ byte thấy trong ≤ 0,5 s ⇒ đóng, mở lại ngay | `net.rs`, `worker.rs::Watch` |
| E10 | ngủ trong lượt ≤ 300 s rồi **Lỗi** | Retry-After giữ tới 6 giờ; > 30 s mà không còn kết nối khác ⇒ dừng lượt ngay, engine ghi gợi ý `recovery.retryAt`, app hẹn đúng giờ đó (không giữ chỗ hàng đợi) | `http.rs::parse_retry_after`, `session.rs::fail_with` |
| E14 | ghép lẫn hai bản (hỏng âm thầm) | mỗi phản hồi 206 so ETag/Last-Modified với lúc bắt đầu lượt: mọi validator so được đều khác ⇒ dừng `file.changed`, không đưa file vào chỗ. ETag khác mà Last-Modified giống vẫn coi là một file (cụm Apache gắn inode vào ETag) | `worker.rs`, `http.rs::Validators` |
| E15 | `disk.write` lớp Permanent | đầy ổ (`ERROR_DISK_FULL`, `ENOSPC`, hết hạn mức) ⇒ `disk.full` ⇒ **tạm dừng kèm lý do** "Ổ đầy · cần trống thêm X · tự tải tiếp khi đủ chỗ"; app hỏi dung lượng mỗi 10 s, đủ (phần còn lại + 64 MiB) thì chạy tiếp; nút Tiếp tục vẫn có. Ổ nhỏ hơn phần còn phải tải ⇒ lỗi `disk.too_small` (không chờ vô ích) | `file.rs::disk_failure`, `recover.rs` |
| E16 | task kẹt "đang tải" | lỗi đĩa trong `run` ⇒ `fail` có mã; app: lượt kết thúc mà task còn đang chạy ⇒ `internal.run`, rồi theo luật tự thử lại | `session.rs::run`, `recover.rs::settle` |
| E17 | tạo lại thư mục, tải lại từ 0 | `.part`/thư mục mất ⇒ **lỗi `file.part_missing`**, Doctor: đổi lại như cũ rồi Thử lại, hoặc **Tải lại từ đầu**. Ổ chứa thư mục tải đã rút ⇒ `disk.unplugged`, app chờ ổ cắm lại rồi tự tải tiếp | `session.rs::missing_part`, `file.rs::volume_missing` |
| E18/J4 | xoá hết, tải lại từ 0 | mở lại task: đoạn journal nhận quá độ dài `.part` thì cắt bớt; đuôi đoạn là trang 4 KiB toàn byte 0 (tối đa 1 MiB/đoạn) thì tải lại ⇒ **chỉ tải lại phần sai** | `session.rs::reconcile_part` |
| E19 | ghi vào file trong Thùng rác, cuối cùng hỏng | kiểm `.part` mỗi nhịp: mất ⇒ dừng ngay `file.part_missing` | `session.rs::run_segmented` |
| J2/J3 | journal có thể đi trước dữ liệu khi rút điện | **fsync `.part` trước mỗi checkpoint** (chụp tiến độ → fsync → ghi journal), chạy nền không chặn vòng tải (đĩa chậm/USB: lượt trước chưa xong thì để nhịp sau); đổi cách chia đoạn chỉ ghi số byte đã fsync; journal `synchronous=FULL` | `session.rs::Saving`, `checkpoint_segments`, `db.rs` |
| J5 | không tự thử lại | `recover.rs`: vòng 2 s theo giờ máy (ngủ dậy là chạy ngay); lịch nằm trong journal (cột `recovery_json`, migration v5) nên mở lại app vẫn chờ đúng | `recover.rs`, `db.rs` |
| M4/M5 | lỗi mạng media ⇒ **Lỗi** | task media dùng chung luật: `media.network`, `media.rate_limited` thử lại tới 10 lần, `media.failed` 3 lần; mất mạng ⇒ chờ có mạng | `recovery.rs`, `media/mod.rs::spawn_media_run` |
| M7 | sót file tạm yt-dlp | xoá task media chưa xong dọn đúng mẫu `<tên>.f<mã>.<đuôi>[.part/.ytdl/.part-FragN]`, `<tên>.<đuôi>.part/.ytdl`, `<tên>.temp.<đuôi>`; không đụng file cuối | `media/parts.rs::discard_leftovers` |

Người dùng **Tạm dừng** một task đang chờ ⇒ thôi tự chạy lại; **Thử lại / Tiếp tục** ⇒ chạy ngay, đếm lại từ đầu.
Engine `pause` (người dùng, app tắt, cài bản mới) luôn xoá lịch.

### Trạng thái task chờ
- `paused` + `recovery {wait: retry|network|disk, retryAt?, attempts, progress, needBytes?}`: không giữ chỗ trong hàng
  đợi. Giao diện: "Đang chờ" (retry/network; Tạm dừng = thôi tự chạy) hoặc "Tạm dừng" (disk, nút Tiếp tục); cột
  "Còn lại" hiện "Thử lại 14:32" / "Chờ mạng" / "Chờ chỗ trống"; mô tả nêu lý do.
- `failed` + `recovery {wait: link}`: link hết hạn/cần đăng nhập; trình duyệt gửi tới link cùng tên file ⇒ app tự thử
  gắn (`capture_link`), không khớp thì tạo download mới như thường.
- Bản cũ mở journal v5 thì từ chối như mọi lần đổi schema (`utd.db.bak` có sẵn). Trạng thái task vẫn là chuỗi cũ.

### Cập nhật link (kiểu IDM "Refresh download address")
- Menu chuột phải **Cập nhật link…** (download thường chưa xong, không đang chạy) và nút trong Doctor cho
  401/403/407/410. Hộp thoại: dán link, hoặc bấm tải lại file trong trình duyệt — trong lúc hộp mở, link tải từ trình
  duyệt tới trước tiên thử gắn vào task đó (`await_download_link`).
- Nhận khi: cùng kích thước, máy chủ cho Range, và mọi ETag/Last-Modified so được đều giống. Validator cái giống cái
  khác, hoặc không có, thì so 3 đoạn 64 KiB đã tải (đầu, giữa, cuối). Khác ⇒ báo `[recover.size|changed|unverifiable|
  page|unreachable]`; `.part` và journal không đổi.
- Lệnh: `update_download_link(taskId, url)`, `await_download_link(taskId|null)`, `restart_download(taskId)`; sự kiện
  `utd://link-updated {taskId, error}`.

## 3. Test chaos — `crates/utd-engine/tests/chaos.rs`

Chạy cùng `cargo test -p utd-engine` (≈ 85 s, các bài chạy song song). In hạt giống ngẫu nhiên (`seed=…`);
chạy lại đúng kịch bản bằng `UTD_CHAOS_SEED=<số>`. Máy đo: Windows 11, 2026-10-25.

| Bài | Kịch bản | Kết quả |
|---|---|---|
| `a_hundred_network_drops_still_end_byte_exact` | 20 MiB, 4 kết nối × 128 KiB/s. **100 lần mất mạng** ngẫu nhiên: 96 lần rớt ngắn (40–120 ms, mọi kết nối bị cắt, kết nối mới bị reset), 4 lần mất hẳn 3,6 s (không còn route); 10 lần đổi Wi-Fi↔4G (IP nguồn đổi). Vòng chạy giống app (`recovery::plan`) | 100/100 lần rớt trong lúc tải; 4 lượt chạy dừng sớm, cả 4 được xếp **chờ có mạng** (`network.offline` ×2, `network.request` ×2), không lần nào thành lỗi; xong sau 76 s, **đúng byte, SHA-256 khớp** |
| `twenty_kills_and_five_power_cuts_keep_the_file_byte_exact` | 40 MiB, engine cấu hình thật (checkpoint 1 s) trong **tiến trình con thật**, 4 × 192 KiB/s. **20 lần kill** (TerminateProcess) sau 0,3–1,5 s tải; mỗi lần mở lại như app (`mark_interrupted` rồi tải tiếp). Rồi **5 lần rút điện** sau 1,5–3 s tải: kill + WAL rách 1–2 frame cuối, mất `-shm`, ghi rác vào mọi vùng `.part` journal chưa nhận (31,8 → 31,0 → 29,8 → 29,1 → 27,9 MB qua 5 lần: tiến độ vẫn giữ được qua mỗi lần rút điện); lần 2 và 4 thêm "ổ nói dối fsync": xoá trắng đuôi 64 KiB (theo trang 4 KiB) của mọi đoạn journal đã nhận (256 KiB) | **AC-02: mất tối đa 0,63–0,70 s tiến độ** mỗi lần kill qua 3 lần chạy (yêu cầu ≤ 5 s); journal qua `quick_check` sau mỗi lần rút điện; vùng đuôi bị xoá trắng được phát hiện và tải lại; file cuối **đúng byte, SHA-256 khớp**, không còn `.part` |
| `an_expired_signed_link_is_updated_and_keeps_the_downloaded_part` | 6 MiB, link ký `?sig=1`, mỗi yêu cầu ≤ 512 KiB; sau 1 MiB link hết hạn (403, cắt kết nối) | dừng `http.403` sau < 4 s (trước: ~1 phút), `plan` ⇒ chờ link; link khác kích thước ⇒ `Size`, khác file (ETag khác dù cùng Last-Modified ⇒ so byte) ⇒ `Changed`, link cũ ⇒ `Unreachable`; `.part` và journal không đổi. Link `?sig=2` ⇒ nhận, giữ phần đã tải, tải tiếp **đúng byte**, không tải lại phần đã có |
| `without_validators_the_new_link_is_checked_by_sampling_bytes` | như trên, máy chủ không gửi ETag/Last-Modified | link khác nội dung bị so byte phát hiện ⇒ `Changed`; đúng file ⇒ nhận, **đúng byte** |
| `a_file_replaced_mid_download_stops_without_mixing_versions` | máy chủ thay file cùng kích thước (ETag mới) sau ~1 MiB | dừng `file.changed`, không có file cuối (AC-04) |
| `a_long_retry_after_ends_the_run_with_a_scheduled_retry` | mọi GET nhận 429 `Retry-After: 1800` | dừng sau < 5 s (không ngủ trong lượt), gợi ý `retryAt` = +1800 s, `plan` hẹn ≥ giờ đó, ≤ 8 yêu cầu tới máy chủ |
| `a_server_that_accepts_then_stays_silent_is_given_up_quickly` | máy chủ nhận kết nối rồi im 120 s | `network.timeout` sau ~6 s (hạn kết nối 3 s), trước: ~2,5 phút |
| `going_offline_ends_the_run_fast_and_resumes_when_back` | kết nối mở mà im (gói tin không tới), rồi mất route; hạn im 30 s | dừng `network.offline` sau ~4 s, `plan` ⇒ chờ có mạng; có mạng lại ⇒ tải tiếp **đúng byte** |
| `switching_networks_reconnects_without_waiting_for_the_idle_timeout` | các kết nối của mạng cũ gửi 256 KiB rồi im mãi; 1,2 s sau đổi Wi-Fi→4G; hạn im 30 s | xong **đúng byte** sau vài giây (không chờ 30 s) |
| `a_part_file_deleted_mid_download_is_reported_then_downloaded_again` | xoá `.part` lúc đang tải | dừng ngay `file.part_missing`; tiếp tục vẫn báo (không âm thầm tải lại); "Tải lại từ đầu" ⇒ **đúng byte** |

Ngoài chaos: đơn vị engine (`recovery`, `net`, `relink`, `file`, `http::Validators`), journal (`keeps_the_recovery_
schedule_and_lists_waiting_tasks`), app (`recover_tests.rs`: hẹn giờ, chờ mạng, chờ ổ, ổ nhỏ, link, giới hạn lần
thử, task kẹt, Tạm dừng; `media::parts` dọn file tạm yt-dlp), e2e `e2e/recovery.spec.ts` (dòng chờ không hiện lỗi,
Cập nhật link dán/qua trình duyệt, Doctor Cập nhật link / Tải lại từ đầu, a11y).

Chưa thử thật: đầy ổ thật (không có ổ nhỏ để làm đầy trên máy dev; phân loại `ERROR_DISK_FULL`/`ENOSPC`, chờ chỗ và ổ
nhỏ hơn file có test đơn vị + test app), rút USB thật, ngủ máy thật (dò bằng giờ máy nhảy so với đồng hồ đơn điệu
có test đơn vị), app thật với trình duyệt gửi lại link (đường `capture_link` có test chọn task, phần IPC chưa chạy
thật).

**Thử trên app thật** (bản release riêng, `UTD_DATA_DIR` riêng, lái giao diện qua CDP; máy chủ Node cục bộ: file 48 MiB,
link ký `?sig=N`, 256 KiB/s mỗi kết nối):
1. Link hết hạn khi đã tải 9,2 MB ⇒ sau ~6 s dòng thành "Failed" (HTTP 403), `recovery.wait = link`, giữ nguyên 9 175 040
   byte. Chuột phải ⇒ "Update link…" ⇒ dán link mới ⇒ hộp đóng, **tải tiếp từ 9 175 040 byte**, xong 50 331 648 byte,
   SHA-256 khớp máy chủ.
2. Máy chủ bận (503, `Retry-After: 40`) giữa chừng ⇒ dòng "Queued", dưới tốc độ "Retry 14:04"; đúng giờ hẹn (+40 s)
   **tự chạy lại** từ 5 242 880 byte, xong, SHA-256 khớp.
Lỗi tìm được và đã sửa nhờ lần thử này: (a) sự kiện "failed" của engine đi qua kênh riêng nên có lúc tới giao diện
SAU sự kiện "chờ thử lại" ⇒ dòng kẹt "Failed" dù task đang chờ (`spawn_run` giờ chờ phát hết sự kiện engine, tối đa 5 s,
rồi mới xếp loại); (b) cửa sổ hẹp ghép thành "Retry 14:04 left" / "còn Thử lại 14:04" (ô Tốc độ giờ hiện nguyên giờ hẹn;
e2e có kiểm ở 1280 px).

**Chi phí fsync trước checkpoint:** tải 1,5 GiB từ máy chủ cục bộ không giới hạn (bản debug, ổ C: máy dev), 6 lượt
mỗi kiểu, xen kẽ: không fsync trung bình 1424 MB/s, có fsync 1410 MB/s — trong sai số. Sau đó checkpoint giữa lượt
chuyển hẳn ra nền (`Saving`): bản đầu chờ fsync ngay trong vòng lặp làm hai bài thử tốc độ (adaptive, chunked) đỏ khi
máy bận; chạy nền thì bộ `chunked` từ 4,2 s còn 1,9 s và cả bộ engine xanh.

## 4. Cho các phiên khác khi ghép

- **Phiên chính (adaptive.rs, không đụng):** `Watch::wait` giờ thức mỗi nhịp hỏi từ lúc bắt đầu (trước chỉ thức gần
  hạn treo) để thấy `NetEpoch` đổi; kết nối mở lại vì đổi mạng báo `network.stalled` cho `gate.report` như kết nối
  treo ⇒ cả loạt cùng lúc có thể làm bộ điều khiển giảm nửa. Gợi ý ghép sau: khi `NetEpoch` tăng thì bộ điều khiển
  dò lại từ đầu (mạng mới, tốc độ cũ vô nghĩa) thay vì coi là quá tải. `fill_idle`/`split_slowest` không còn ghi
  journal; `run_segmented` ghi cách chia mới qua `save_plan(Saving::durable_plan)` (chỉ byte đã fsync); checkpoint
  giữa lượt chạy nền (`Saving`), nên vòng lặp không còn đứng chờ ghi journal.
- **Phiên màn chi tiết:** `DownloadTask` có thêm `recovery?: {wait, retryAt?, attempts, progress, needBytes?}`. Nút
  "Cập nhật link" ở S-21: nối `DetailActions.failedExtra` (nhánh kia, `ui/detail-flow.tsx`) với `setRelinkId` của
  `home-screen.tsx` ⇒ mở `RefreshLinkDialog` (`src/features/home/ui/refresh-link-dialog.tsx`); "Tải lại từ đầu" ⇒
  `restartDownload` (`api/recover.ts`). `DoctorDialog` nhận thêm `onRefreshLink`, `onRestart`. Bảng `task_events` (v4 của
  nhánh kia) tự ghi mốc khi `recover::settle` đổi trạng thái (lưu qua `save_task`); muốn mốc riêng "đã tự thử lại",
  "đã thay link" thì ghi code `retry.*` / `link.*` trong `recover::due` và `recover::relink`.
- **Phiên tải cần đăng nhập:** chỗ chừa: `DownloadEngine::replace_link(task_id, url, headers)` (header thường, đã lọc)
  và `recover::capture_link(app, engine, queue, url, suggested_name)` gọi trong `ipc.rs` nhánh `Route::Download` trước
  khi tạo task mới. App khởi động lại ⇒ download cần đăng nhập gặp 401/403 ⇒ `failed` + chờ link; trình duyệt gửi
  lại (cookie theo đường RAM của phiên kia) ⇒ gắn vào task cũ, giữ phần đã tải. Hai phiên cùng sửa khối
  `Route::Download` của `ipc.rs`: giữ lời gọi `capture_link` trước nhánh tạo task.
- **Kế hoạch tải cần đăng nhập** (`claude/auth-plan-6b2e91`, đề xuất `RecoveryWait::Login`): thêm biến thể vào
  `utd_engine::RecoveryWait` (JSON `"login"`), nhánh `auth.*` trong `recovery::plan` trả `Wait(Login)` trước nhánh 401/403,
  `recover::due` coi `Login` như `Link` (không tự chạy theo giờ), sự kiện `utd://auth-granted` gọi `queue.submit` cho task
  `Login` của trang đó. Giao diện: `download-mapper.ts::waitingOf` đang chỉ loại `link` khỏi "đang chờ" — thêm `login`.
- **i18n:** khoá mới `recover.*` (chèn sau `desc.analyzing`, `menu.diagnose`, `rename.submit`, `doctor.disk.steps`) và
  `err.network.offline`, `err.disk.unplugged`, `err.disk.too_small`, `err.file.changed`, `err.file.part_missing`,
  `err.internal.run`, `err.recover.*` (chèn cạnh mã cùng nhóm).
- **Journal**: migration của nhánh này là **v5** (`recovery_json`), `SCHEMA_VERSION = 5`; v4 để cho bảng `task_events` của
  nhánh màn chi tiết. Nhánh này một mình nhảy 3 → 5 (không có v4); ghép thì giữ cả hai, test so với hằng số.
- **Kích thước JS:** main 189,65 kB → nhánh này 190,85 kB gzip (+1,2 kB). Nhánh màn chi tiết báo 197,8 kB ⇒ ghép hai
  nhánh ≈ 199 / 200 kB (`.size-limit.json`): sát trần.
- `src/features/home/ui/refresh-link-dialog.tsx` gắn `data-s="S-21"` (Doctor: áp dụng cách sửa); SPEC chưa có mã riêng.
