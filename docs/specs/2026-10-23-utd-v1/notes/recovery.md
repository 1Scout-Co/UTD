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
