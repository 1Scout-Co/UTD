# QA — nút "Tải video này" TikTok/Douyin + tải cả hồ sơ Douyin (nhánh claude/fb-media-4c7a19)

Commit kiểm thật lần cuối: `d173dcd` (lượt 5). Sau đó chỉ sửa cỡ hiển thị trên bảng (M-1 lượt 5, kiểm bằng test đơn vị) và ghi chú. Ngày 2026-10-27.

User duyệt giao diện: có (2026-10-27, xem ảnh bảng TikTok feed, bảng + nút Douyin, hộp Tải cả kênh Douyin, danh sách tải xong — lượt 1 và lại sau khi sửa ở lượt 2)

## Máy kiểm
- Test đủ bộ (dấu test): `cargo test` app 294, utd-engine 202, utd-mp4 19, clippy 3 crate `-D warnings`, `pnpm lint` (i18n 16 thứ tiếng), typecheck, build, Playwright 158/158.
- Bài `utd-engine --test adaptive` (`ramps_past_four_when_each_connection_is_throttled`) đỏ một lần khi máy đang chạy review song song. Chạy riêng 3/3 xanh, lần chạy đủ bộ sau cũng xanh. Nhánh không đụng bộ điều khiển băng thông (chỉ đổi `CookieJar::absorb_text`).
- flow-check: các mục KHÔNG ĐẠT đều có từ trước hoặc báo nhầm:
  - knip không hiểu điểm vào của tiện ích;
  - osv-scanner không đọc được đường dẫn tiếng Việt;
  - secret-build chỉ là file .pdb quá lớn, 0 khoá;
  - spec-codes và arch-size ở file nhánh không đụng;
  - 2 cảnh báo ast-grep ở `douyin-page.js`: `fetch` tới đường dẫn cố định cùng origin, và `catch` có đường lui rõ.

## Thử thật
App debug, UTD_DATA_DIR riêng, tiện ích bản chép nối app qua cầu nối 127.0.0.1:4621. Không đụng UTD thật, không đăng nhập, không giải CAPTCHA.

| Lượt | Commit | Mục chính | Kết quả |
|---|---|---|---|
| 1 | `b922f27` | S-74/S-75 TikTok feed/lưới, Douyin feed/`modal_id`/`/video/`, S-60/S-61 Douyin tải cả hồ sơ, hồi quy YouTube + TikTok cả kênh | ĐẠT. Lỗi tìm ra: TikTok chọn dòng chờ 11–14 s (MAJOR), nút đè thanh Douyin (MINOR) |
| 2 | `148475d` | TikTok `cached` 19–23 ms, byte đầu ≤ 0,5 s; nhãn cạnh ngắn; nút Douyin trong góc video; đồng hồ thời lượng; hồi quy YouTube watch + Shorts | ĐẠT |
| 3–4 | `51f52d8` | TikTok video dài tải ngay (`cached`), tạm dừng/tiếp tục không hỏi lại yt-dlp | ĐẠT. Lỗi tìm ra: file "Tốt nhất" có tiếng (nhạc nền `audio`) dài gấp 2,3 lần hình (MAJOR) |
| 5 | `d173dcd` | Chi tiết `qa-report-5.md`, xem bảng dưới | 8 ĐẠT, 1 MINOR (cỡ trên bảng), đã sửa |

Lượt 5:

| # | Mục | Kết quả |
|---|---|---|
| 1 | Rubik 2:55 "Tốt nhất" | ĐẠT. `cached`, byte đầu 0,15 s. 1 hevc 720x1280 + 1 aac, không có luồng hình thứ hai. Hình 174,97 / tiếng 174,89 s. Tiếng khớp bản 576p (tương quan 1,0000, lệch 0 s). Khung sạch |
| 2 | Ronaldo 4:36 | ĐẠT. `cached`, 1 hevc 1920x1080 + 1 aac, 276,30 / 276,32 s |
| 3 | 720p = Tốt nhất (trùng từng byte); 576p không đổi so với lượt 4 | ĐẠT |
| 4 | Âm thanh M4A / MP3 | ĐẠT. Chỉ có tiếng, 174,9 s, không câm |
| 5 | Tạm dừng 37% rồi Tiếp tục | ĐẠT. Không gọi lại yt-dlp, file trùng từng byte với mục 1 |
| 6 | Tạm dừng, tắt hẳn app, mở lại, Tiếp tục | ĐẠT. `refreshed source=fresh` 2,9–5,3 s, yt-dlp chạy kèm `--video-multistreams`, không 403. File trùng từng byte với mục 1–2 |
| 7 | Hồi quy: TikTok ngắn, YouTube Shorts (1440p av1 + opus), YouTube watch (240p và 4K) | ĐẠT. Mỗi file 1 hình + 1 tiếng, thời lượng khớp |

Đã sửa sau lượt 5: M-1. Bảng ghi Tốt nhất ≈ 42 MB nhưng file 11,9 MB, M4A ≈ 32 MB nhưng file 0,7 MB, vì bảng cộng cả hình của format gộp mà app bỏ đi. Nay Tốt nhất/720p ghi cỡ hình (≈ 11 MB), M4A không ghi cỡ (giống MP3). Test đơn vị đỏ trước, xanh sau.

## Còn lại (không chặn gộp, có từ trước nhánh này)
- YouTube Shorts có nhiều tiếng lồng tự động (`140-0`…`140-3` theo ngôn ngữ): selector Tốt nhất không ghim được tiếng.
  - Bảng ước lượng theo bản 360p (≈ 12 MB, file thật 1440p 36 MB).
  - Lượt tải phải hỏi lại yt-dlp (~6 s).
  - Bảng đọc bằng `/shorts/<id>` còn lượt tải gửi `/watch?v=` nên cũng trượt bộ nhớ Phân tích.
- Mỗi lần mở bảng hoặc làm mới có 2 tiến trình yt-dlp giống hệt chạy song song (lượt 5, M-3). Chưa rõ có từ trước hay không.
- TikTok: đọc bảng lần đầu ~5–13 s (yt-dlp đọc trang TikTok).
- Tiếng TikTok video dài là AAC ~33 kbps vì đó là luồng tiếng duy nhất của video. Dòng M4A tải cả bản gộp (33 MB) để lấy 0,7 MB tiếng.
- Không thử:
  - đường yt-dlp tải thẳng có `--ppa` (giao diện không ép được; coder đã chạy tay, ra đúng `hevc 720p + aac`);
  - lưới hồ sơ TikTok thật (CAPTCHA);
  - bài ảnh Douyin;
  - nhiều trang hồ sơ Douyin khi đã đăng nhập;
  - màn 375 px, bàn phím.
