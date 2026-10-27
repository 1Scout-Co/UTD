# Nút "Tải video này" trên TikTok: feed và lưới hồ sơ (2026-10-26)

Nhánh `claude/fb-media-4c7a19` (đã ghép main @ `93ed9a3`). Chủ dự án báo:
1. Feed (For You…, kênh David Nguyen Jr.): bảng tên "TikTok - Make Your Day", chỉ một dòng "Tải file video" ⇒ địa chỉ
   trang không phải link video, yt-dlp không đọc được, bảng rơi về đường dự phòng.
2. Lưới "Videos" trên `https://www.tiktok.com/@_littledav_`: bảng báo lỗi thô "channel_page: https://…" ⇒ đã gửi link
   kênh thay vì link của ô đang rê.

## Cách làm

- **`extensions/*/tiktok.js`** (content script riêng, chỉ `https://*.tiktok.com/*`, manifest): đăng ký vào
  `globalThis.__utdVideoSites` một hàm `videoUrl(video)`; `content.js` hỏi hàm này trước khi gửi link cho yt-dlp
  (`analysisUrl(video)`, cả lúc bấm nút lẫn chuột phải › UTD). Thứ tự (học từ mediago, Just-download-it, omarchy-yeet,
  vot.js — Phụ lục A):
  1. Ô lưới (hồ sơ, khám phá, tìm kiếm): link `/@kênh/video/<mã>` bọc video, hoặc nằm trong cùng ô `user-post-item`.
  2. Feed: mã ở khung trình phát `xgwrapper-<số>-<mã>` (feed không có link tới trang video), kênh lấy từ link tác giả
     của cùng mục (`video-author-avatar`, hoặc `a[href^="/@"]` đầu tiên). Bỏ thẻ quảng cáo.
  3. Trang xem một video: chính địa chỉ trang.
  4. Link bài đăng trong tổ tiên gần nhất có link; nhiều bài khác nhau ⇒ không đoán.
- **Link gửi yt-dlp luôn là `https://www.tiktok.com/@<kênh hoặc _>/video/<mã>`.** yt-dlp (`tiktok.py` L709) không nhận
  `/video/<mã>` thiếu `@`, `/photo/`, hay host khác `www.`; kênh trống thì yt-dlp tự dùng `@_` (L107–108). Bài ảnh
  (`/photo/`) cũng gửi dạng `/video/`: yt-dlp không tải ảnh (issue #15469), chỉ ra nhạc của bài (#15764).
- **Không đọc sẵn khi rê qua lưới:** `content.js` chỉ đọc sẵn (prefetch) video của chính trang, không chạy yt-dlp cho
  từng ô.
- **Không bao giờ hiện câu lỗi thô:** `background.js` đổi câu lỗi không mã mà tiện ích không biết (vd
  `channel_page: <link>`) thành mã chung `media.failed` ⇒ bảng hiện "UTD không đọc được video này."
- yt-dlp đọc TikTok công khai không cần cookie; bản trước 2026-08-18 hỏng TikTok, UTD đang dùng 2026.10.21.
- **Bắt đầu tải chậm, "Đang tải 0 B/s" (sửa 2026-10-26):** CDN TikTok (`v19-webapp-prime.tiktok.com`, link có
  `tk=tt_chain_token`) trả 403 nếu yêu cầu thiếu cookie `tt_chain_token` mà yt-dlp nhận lúc đọc trang (đo bằng curl:
  thiếu ⇒ 403, có ⇒ 206; giá trị còn ngoặc kép như trong JSON ⇒ vẫn 403). yt-dlp ghi cookie này ở trường `cookies`
  của format; đường tải nhanh trước đây bỏ qua nên bị 403, hỏi yt-dlp lại, lại 403, rồi mới để yt-dlp tự tải.
  Nay `resolve::StreamCookies` đọc trường đó (bỏ ngoặc như yt-dlp gửi), `fast::cookies_for` gộp vào hũ cookie của
  lượt tải: chỉ ở RAM, engine chỉ gửi tới đúng miền, không vào journal.

## Bài thử

- `e2e/video-button.spec.ts`: feed giả (2 mục `recommend-list-item-container` có `xgwrapper`) ⇒ đúng link từng video;
  lưới hồ sơ giả ⇒ đúng ô đang rê, rê qua không gửi gì, ô bài ảnh ⇒ link `/video/`.
- `e2e/fb-media.spec.ts`: nền đổi `channel_page: …` thành `media.failed`, không lộ câu thô.
- Rust: `fast::tests::sends_the_cookies_yt_dlp_set_for_the_stream` (máy chủ giả trả 403 nếu thiếu `tt_chain_token=abc`;
  tải xong ngay lượt đầu, không hỏi yt-dlp lại), `resolve::tests::splits_yt_dlp_stream_cookies_and_unquotes_values`.

## Chọn chất lượng là tải ngay (2026-10-27)

Chủ dự án báo (sổ chốt e5z3): chọn một dòng trong bảng "Tải video này" hoặc hộp Tải media vẫn đứng "Đang tải 0 B/s"
~12 s (log `media.timing … resolved source=fresh ms≈11000–13000`), bảng ghi "1920p", và hiện hai dòng trùng
("Tốt nhất ≈3,2 MB" cùng "1920p ≈3,2 MB").

- **Nguyên nhân:** format TikTok của yt-dlp đều gộp sẵn hình + tiếng, mã có số thứ tự vì hai CDN mirror
  (`bytevc1_1080p_1146746-0`, `-1`). `probe::merge_choice` chỉ ghim được luồng tách (hình + tiếng riêng) ⇒ selector
  của "Tốt nhất" là từ khoá `bv*+ba/b` ⇒ `probe_cache::lookup` không tìm được mã nào ⇒ `fast.rs` hỏi yt-dlp lần hai
  (`fresh`). Nhãn mức lấy theo `height` (cạnh dài của video dọc); bảng luôn thêm dòng mức dù chỉ có một mức.
- **Cách sửa** (`media/probe.rs`; `fast.rs` không đổi: một luồng có cả hình + tiếng đã đi đường tải nhanh một file,
  `Layout::Single`, và vẫn mang cookie luồng `tt_chain_token` qua `cookies_for`):
  1. `merge_choice` = `split_choice` (luồng tách, như cũ) rồi `combined_choice`: bậc `preference` cao nhất, format
     gộp `plain` cao nhất trong giới hạn mức, chỉ khi không có luồng nào cao hơn (cùng luật không tụt độ phân giải);
     ở mức đó lấy mp4 cuối danh sách (yt-dlp xếp kém → tốt) ⇒ `bytevc1_1080p_1146746-1/bv*+ba/b`.
  2. Mã đánh số mà cả nhóm cùng một ngôn ngữ (mirror) ghim thẳng (`mirror_pin`), chỉ cho format gộp; tiếng riêng vẫn
     theo `pin` (lồng tiếng YouTube ghim kèm `[language=…]`). Mirror lệch thì yt-dlp rơi về phần sau `/`.
  3. Mức tính theo **cạnh ngắn** (`Format::level`, như `res` của yt-dlp): 1080×1920 ⇒ "1080p", 720×1280 ⇒ "720p",
     576×1024 ⇒ "576p"; `id` của dòng, nhãn, "tối đa {N}p" đều theo mức, nên `preset::Quality::Height` (lô link) và
     huy hiệu "Phổ biến" (1080) khớp với video dọc. Bộ lọc dự phòng vẫn theo `height` thật: dọc 1080 ⇒
     `bv*[height<=1920]+ba/b[height<=1920]`; ngang giữ nguyên selector cũ.
  4. Chỉ một mức ⇒ không thêm dòng mức (bảng chỉ có "Tốt nhất", M4A, MP3). Hộp Tải media liệt kê mức quen thuộc nên
     còn một dòng cũng bỏ; bảng trên trang (`every_height`) liệt kê mọi mức có thật từ 144p (TikTok dọc ra thêm 576p).
- **Đổi so với trước, chủ ý:** trang chỉ có format gộp (hoặc mức chỉ có format gộp) nay ghim mã format gộp ở đầu
  selector, phần dự phòng sau `/` giữ nguyên (ba chỗ kỳ vọng trong `probe.rs` thêm tiền tố mã: `18/…`, `22/…`, `37/…`).
- **Chưa kiểm với TikTok thật:** các bài thử dùng JSON dựng theo mô tả format yt-dlp 2026.10.26 (mã, kích thước, cookie).
  Cần mở bảng trên một video TikTok thật và đọc log `media.timing … resolved source=cached`.

Bài thử: `probe::tests::{pins_the_best_combined_tiktok_format_so_the_download_reuses_the_analysed_stream,
names_levels_by_the_short_side_so_vertical_videos_read_like_landscape_ones, lists_no_level_row_that_would_repeat_the_only_level,
labels_vertical_dash_videos_by_the_short_side_and_keeps_the_pins, a_combined_format_never_hides_a_taller_stream_or_a_better_container,
pins_numbered_combined_ids_directly_only_when_the_whole_group_is_mirrors}`,
`probe_cache::tests::returns_the_one_combined_stream_with_the_cookies_yt_dlp_set`,
`fast::tests::{plans_one_cached_stream_with_its_cookies_for_a_combined_only_page,
downloads_a_combined_only_page_from_the_analysed_stream_without_asking_yt_dlp}` (JSON kiểu TikTok ⇒ `probe` ⇒
`probe_cache::remember` ⇒ chọn "Tốt nhất" ⇒ kế hoạch `Source::Cached`, một luồng `Single`, cookie `tt_chain_token` gửi
đi, mirror `-0` không bị đụng, không gọi resolver).

## Sửa sau review 2026-10-27

- **Tiếp tục task đã tạm dừng phải mang cookie luồng.** `fast::resume_plan` đặt cookie của kế hoạch là `fast.cookies`
  (chỉ hũ đăng nhập) nên cookie `tt_chain_token` của luồng (chỉ ở RAM, không vào journal — luật giữ nguyên) mất sau khi
  tạm dừng: CDN trả 403, task đứng 0 B/s rồi `recover` hỏi yt-dlp (~12 s). Nay lượt tiếp tục hỏi
  `Resolver::cached(job)`: luồng của lần "Phân tích" còn trong `probe_cache` ⇒ `cookies_for(fast.cookies, &streams)`
  (link vẫn là của task con trong journal); không còn (mở lại app, cache đầy) ⇒ như cũ, đường `recover` lo.
  Bài thử: `fast::tests::resume_keeps_the_stream_cookies_of_a_paused_combined_only_page` (tạm dừng giữa chừng một luồng
  gộp TikTok có cổng cookie, tiếp tục ⇒ tải xong, không gọi yt-dlp, mọi yêu cầu mang cookie). `tiktok_json` của bài thử
  nay khai `downloader_options.http_chunk_size` như yt-dlp để file chia nhiều yêu cầu.
- **Thẻ quảng cáo trong feed không gửi link feed cho yt-dlp.** `tiktok.js` trả `null` cho thẻ quảng cáo nhưng
  `analysisUrl` (content.js) rơi về `location.href` (link feed) ⇒ một lượt yt-dlp vô ích rồi bảng báo lỗi chung. Nay trang
  có module mà module trả `null` ⇒ `analysisUrl` trả `null`, `open()` báo luôn "UTD không đọc được video này." (không
  hỏi nền). Bài thử: `e2e/video-button.spec.ts` › "TikTok thẻ quảng cáo trong feed…".

## Phụ lục A — Các dự án khác làm thế nào (đọc mã, 2026-10-26)

| Dự án (commit) | Cách lấy mã video | Chạy ở |
|---|---|---|
| yt-dlp `tiktok.py` (`fa98866`) | L709 `_VALID_URL` `www\.tiktokv?\.com/(?:embed\|(?:share\|@(?P<user_id>[\w\.-]+)?)/video)/(?P<id>\d+)`; L107–108 dựng `@{user or "_"}`; L275–339 đọc `__UNIVERSAL_DATA_FOR_REHYDRATION__`, không cần cookie | Python |
| mediago (`3e206c3`) `site-adapters/short-video/adapter.ts` L5–16, L108–142, L164–182 | Địa chỉ (chỉ trang chi tiết) → link đầu tiên trong thẻ → `[id^="xgwrapper-"]` + `a[data-e2e="video-author-avatar"]`; bỏ `ad-card`/`feed-ad`/`promoted-label` | Content script |
| Just-download-it (`955446e`) `handlers/tiktok.js` L40–96 | `closest('a[href]')` → `xgwrapper` → link trong thẻ (`user-post-item`, `article`…) → địa chỉ | Content script |
| omarchy-yeet (`9b42121`) `tiktok-video-url.js` L17–42 | Ghi rõ feed không có link tới trang video; `closest('a')` (lưới bọc ô trong link) → `article` → `xgwrapper`, kênh `a[href*="/@"]`, trống ⇒ `@_` | Content script |
| vot.js (`9a54383`) `helpers/tiktok.ts` L65–94 | Địa chỉ → `.tiktok-web-player` id `xgwrapper-\d+-(\d+)`, kênh `video-author-avatar` | Content script |
| sixem/tiktok-download-buttons (`250074e`) `item-data/feed.ts` L13–57, `grid.ts` L9–46 | Feed: link trong mục, không thì `xgwrapper`; lưới: link trong `DivItemContainer`. Dự phòng `tiktok.com/video/<mã>` — **yt-dlp từ chối** (bẫy) | Content script |
| PreMiD TikTok (`548c232`) `presence.ts` L112–121 | `recommend-list-item-container` + `video-author-uniqueid` + id `xgwrapper` | Presence |
| Embed-Link-Copier (`b47de46`) `content-tiktok.js` L12–45 | `article` → `xgwrapper`; chọn bài hiện nhiều nhất trong khung nhìn | Content script |
| dinoosauro/tiktok-to-ytdlp (`1f3e738`) `script.js` L102–116 | Ô `user-post-item`… → `a` có `/video/` hoặc `/photo/` | Trang |
