# YouTube Shorts có lồng tiếng: "Tốt nhất" và hai tiến trình yt-dlp (2026-10-27)

Nhánh `claude/yt-probe-5d81c2` (dựng trên `claude/fb-media-4c7a19` @ `e72ee30`). Hai mục "Còn lại" trong
`qa-report.md` lượt 5 (M-2, M-3).

## M-2: Shorts lồng tiếng — bảng ghi sai cỡ, lượt tải chậm thêm ~6 s

- Tái hiện: `https://www.youtube.com/shorts/Zhz7bQb9saM` (tiếng Việt gốc + lồng tiếng Anh). Bảng: Tốt nhất ≈ 12 MB,
  1440p ≈ 52 MB; file thật 1440p ≈ 36 MB; lượt tải `resolved source=fresh ms=6367`.
- Nguyên nhân: YouTube trả **mỗi tiếng hai lần** (hai client, chỉ khác cờ "default", cỡ trùng từng byte): `140-2` vi và
  `140-3` vi, `140-0`/`140-1` en-US… `pin` thấy ngôn ngữ trùng trong nhóm `140-*` nên không ghim ⇒ selector
  `bv*+ba/b` ⇒ cỡ ước lượng theo format gộp 360p (`18`), dòng 1440p cộng webm `271` + tiếng lớn nhất, và
  `probe_cache::lookup` không đọc được mã ⇒ lượt tải chạy yt-dlp lần hai.
- Sửa (`probe.rs::pin`): cùng ngôn ngữ mà **cùng `filesize`** ⇒ coi là một tiếng, ghim `140-3[language=vi]`. Khác cỡ
  (vd. bản thuyết minh cùng ngôn ngữ) hoặc thiếu cỡ ⇒ vẫn không ghim như cũ. Mã lệch lúc yt-dlp chạy lại thì bộ lọc
  `[language=…]` vẫn giữ đúng tiếng; hai bản cùng ngôn ngữ là cùng một luồng.
- Kết quả trên JSON thật: Tốt nhất = 1440p `400+140-3[language=vi]` ≈ 35,8 MB (= dòng 1440p), M4A `140-3`, MP3
  `251-3`; bộ nhớ Phân tích trả đủ 2 luồng. yt-dlp nhận selector: `400+140-3 | vi | 1440x2560`.
- Shorts không trùng ngôn ngữ (MrBeast `mND4T0lzg_g`, 22 thứ tiếng lồng, 2160p) vốn đã đúng: `401+140-21[language=en]`.
- Link `/shorts/<id>` (bảng) và `/watch?v=<id>` (lượt tải) không làm trượt bộ nhớ: `probe_cache::remember` lưu cả
  `webpage_url`.
- Test: `pins_the_original_language_when_youtube_lists_each_track_twice` (đỏ trước, xanh sau).

## M-3: "Mỗi lần mở bảng có 2 tiến trình yt-dlp giống hệt" — không phải lỗi

`yt-dlp.exe` (bản Windows một file, PyInstaller) tự mở một tiến trình con cùng dòng lệnh: chạy `yt-dlp.exe -J …` đúng
một lần ⇒ `Get-CimInstance Win32_Process` thấy 2 dòng, con có `ParentProcessId` = cha (cha 8 MB, con 79 MB). Lượt 5 bắt
được 2 dòng cách nhau 20 ms (`watchproc.ps1` không ghi tiến trình cha). Phía app vẫn chỉ phân tích một lần:
`content.js` và `background.js` gộp yêu cầu trùng, `video_button::formats` nhớ kết quả 10 phút. Không sửa.

## QA thật (commit `817fef1`, 2026-10-27)

App debug dữ liệu riêng, tiện ích bản chép qua cầu nối 4621 (bộ công cụ của lượt 5), Chromium profile mới, không đăng nhập.

| # | Mục | Kết quả |
|---|---|---|
| 1 | `Zhz7bQb9saM` Tốt nhất | ĐẠT. Bảng Tốt nhất ≈ 34 MB = dòng 1440p (trước: 12 / 52). `resolved source=cached ms=30`. File 35.750.764 B, av1 1440x2560 161,07 s + 1 aac 161,15 s |
| 2 | Tiếng của file mục 1 | ĐẠT. MD5 tiếng đã giải mã trùng `-f 140-3` (vi), khác `-f 140-1` (en-US) |
| 3 | Âm thanh M4A | ĐẠT. `cached ms=24`, chỉ 1 luồng aac, trùng 140-3 |
| 4 | Hồi quy: MrBeast `mND4T0lzg_g`, YouTube `jNQXAC9IVRw`, TikTok Rubik | ĐẠT. Đều `cached`; MrBeast 2160x3840 + tiếng Anh gốc (trùng `140-21`), bảng Tốt nhất = 2160p; Rubik trùng từng byte lượt 5 |
| 5 | Mở bảng lạnh (3 link), ghi `yt-dlp.exe` kèm tiến trình cha | ĐẠT. Mỗi bảng đúng một lần chạy: cha (cha là `app.exe`) + con cùng dòng lệnh; cầu nối 1 yêu cầu `media-formats` mỗi bảng |

Cả lượt 5 dòng `resolved`, đều `cached`; không ERROR/panic/403. Còn lại (có từ trước, không do nhánh này): bảng Shorts
dòng 480p (≈ 11 MB, avc1) nặng hơn 608p (≈ 7,9 MB, av1) vì mỗi mức chọn mp4 cuối danh sách.
