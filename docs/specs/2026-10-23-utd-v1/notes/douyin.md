# Douyin: nút "Tải video này" và tải cả hồ sơ (2026-10-26)

Nhánh `claude/fb-media-4c7a19`. Chủ dự án: "thêm cái download douyin luôn đi, i chang như tiktok luôn", tham khảo
https://github.com/jiji262/douyin-downloader.

## Vì sao không làm y như TikTok (yt-dlp)

- **Từ 2026-09-14 Douyin chặn mọi lời gọi API không mang chữ ký của chính trang** (`ArgusSecurityPlugin`: thiếu
  `uifid` + `timestamp` + `x-secsdk-web-signature`, trả 403). Cookie mới thôi không đủ (jiji262 `AGENTS.md` L55,
  `core/api_client.py` L52–57; Evil0ctal `signing/protection.py` L18–24, L51–68).
- **yt-dlp `DouyinIE` hiện luôn hỏng** ("Fresh cookies (not necessarily logged in) are needed", `tiktok.py` L1503–1506,
  issue #9667; bản sửa PR #17826 chưa ghép) và **yt-dlp không có extractor hồ sơ Douyin** (`_extractors.py` L1971).
- **`fetch` chạy trong trang (thế giới MAIN) được SDK của Douyin tự ký** (dYm `page.ts` L10–16, L183–186;
  social-media-copilot `background/index.ts` L17–28). UTD không tự làm a_bogus/X-Bogus/chữ ký, không đọc cookie
  (trình duyệt tự kèm), và không tự điền các tham số trang tự thêm (`msToken`, `a_bogus`, `uifid`, `verifyFp`… — trùng
  là bị "Sign Invalid", dYm L34–45).

## Cách làm

### Nút "Tải video này" (trang xem, feed, khung `modal_id`, lưới hồ sơ)

1. `extensions/*/douyin.js` (content script, chỉ douyin.com) đăng ký vào `__utdVideoSites` như `tiktok.js`: link
   `https://www.douyin.com/video/<mã>` của đúng video — ô lưới (link bọc/cùng ô `li`), feed (`[data-e2e="feed-active-video"]`
   mang `data-e2e-vid`; chỉ lấy tổ tiên của video vì mục cũ còn sót trong trang), `?modal_id=`, địa chỉ `/video|note/<mã>`.
2. `content.js`: trên douyin.com hỏi trong trang trước yt-dlp (như Facebook), gửi kèm link đó.
3. `background.js` nạp `douyin-page.js` vào trang (MAIN) rồi gọi `media(url)`: `fetch` tới
   `/aweme/v1/web/aweme/detail/?aweme_id=<mã>&aid=6383&device_platform=webapp&channel=channel_pc_web…` (bài ảnh trả
   rỗng thì thử `aid=1128`, jiji262 L902–945). Bị chặn ⇒ dùng bài đang phát trong `window.player` (douyin-dl-user-js
   `PlayerAdapter.ts` L29–61).
4. Bảng: mỗi cạnh ngắn một dòng ("1080p", "720p"…) từ `video.bit_rate[]`; bỏ bản `format == "dash"` (chỉ có hình) và
   `bytevc2`; cùng mức thì H.264 trước H.265; link máy chủ CDN (`douyinvod`) trước `/aweme/v1/play/`, bỏ link watermark
   (`playwm`, `watermark=1`) — jiji262 `downloader_base.py` L1141–1213, L1436–1505, L1663–1674. Chọn một dòng ⇒ tin
   `media-streams` như Tin Facebook: UTD tải thẳng MP4 có tiếng, Referer `https://www.douyin.com/`, không yt-dlp.

### Tải cả hồ sơ

1. Chuột phải trên trang `douyin.com/user/<sec_uid>` › Tải bằng UTD (hoặc nút "Gửi trang này" trong popup).
2. `douyin-channel.js` (nền): báo UTD có lượt mới (`channel-entries` rỗng), mở hộp Tải cả kênh (tin `download` với link
   hồ sơ ⇒ app nhận ra trang kênh), rồi lật từng trang `aweme/post` (`sec_user_id`, `max_cursor`, `count=18`…) bằng
   `posts()` của `douyin-page.js` trong tab, nghỉ 2,5 s giữa hai trang, đẩy dần sang UTD. Dừng khi `has_more = 0`, con
   trỏ không đổi, trang rỗng (Douyin có khi vẫn báo còn — jiji262 `post_strategy.py` L190–207, L454–461), quá 5000 video,
   hoặc bị chặn/đòi đăng nhập (báo lỗi có mã).
3. App (`src-tauri/src/media/channel/douyin.rs`): giữ danh sách trong RAM 30 phút; `channel_list` với link hồ sơ Douyin
   phát danh sách này (không chạy yt-dlp), khoá nhớ video đã tải `douyin.com:<sec_uid>`. Bài ảnh hiện "Bài ảnh", không chọn
   được.
4. Thêm task từ hộp kênh: `enqueue_media` thấy video Douyin có link MP4 trong danh sách ⇒ đổi sang luồng đọc từ trang
   (`page_streams`), chọn bản theo chất lượng của hộp (Tốt nhất / H.264 / ≤720p / M4A / MP3).
   Link CDN hết hạn sau vài giờ: task để lâu mới chạy sẽ báo "Link video trên trang đã hết hạn" — bấm lại trên trang.

### Luật cookie

Không đọc cookie ở đâu cả cho Douyin: `fetch` trong trang tự kèm cookie của trình duyệt, tin `channel-entries` và
`media-streams` không có trường `auth` (native host bỏ nếu có). Lời gọi chỉ chạy trong cú bấm của người dùng.

## Bài thử

- `e2e/douyin.spec.ts`: API giả — chi tiết video (H.264 trước, bỏ DASH/watermark, không tự điền chữ ký, `modal_id`), bị
  chặn ⇒ `window.player`, một trang `aweme/post` (link, thời lượng, ảnh bìa, bản MP4; bài ảnh không có bản nào), tải cả
  hồ sơ trong nền (báo trước, mở hộp, lật 2 trang, bỏ mục/link lạ, không cookie).
- `e2e/video-button.spec.ts`: feed (`data-e2e-vid`), lưới (link bọc và cạnh video), `modal_id` + báo gọn khi chưa đọc được.
- Rust: `channel::douyin` (chọn bản theo chất lượng; phát danh sách đẩy dần, không trùng, lỗi bị chặn), `native_host`
  (`channel-entries` bản 2, tối đa 100 mục, không cookie), `media_pages` (hồ sơ Douyin là trang kênh).

## Phụ lục A — Các dự án khác làm thế nào (đọc mã, 2026-10-26)

| Dự án (commit, giấy phép) | Điều học được |
|---|---|
| jiji262/douyin-downloader (`ed86361`, MIT) | `core/api_client.py` L439–472 tham số chung (`aid=6383`, `device_platform=webapp`…), L888–962 `aweme/post` + `max_cursor`/`has_more`, L902–945 `aweme/detail` (+`aid=1128` cho bài ảnh); `downloader_base.py` L1141–1213 xếp link CDN, L1436–1505 chọn theo cạnh ngắn, L1663–1674 nhận diện watermark, L320–328 header tải (Referer). README L39–41: bản dòng lệnh không còn tải được video lẻ vì bị chặn |
| yt-dlp `tiktok.py` (`fa98866`, Unlicense) | L1372 `_VALID_URL` chỉ `/video/<mã>`; L1495–1508 gọi chi tiết không chữ ký ⇒ "Fresh cookies…"; L406–493 định dạng từ `play_addr`/`bit_rate` (không watermark tốt nhất); không có extractor hồ sơ |
| Everless321/dYm (`d4d12bf`, GPL-3.0 — chỉ học ý) | `page.ts` L10–16, L183–186: gọi trong trang để được ký; L34–45 không tự điền tham số trang thêm; `user-post.ts` L88–99 lật `aweme/post`, nghỉ 5 s |
| iszhouhua/social-media-copilot (`bcbb586`, GPL-3.0 — chỉ học ý) | `background/index.ts` L17–28 `executeScript({world:"MAIN"})` + `window.fetch`; `video-detail.tsx` L102–131 `feed-active-video`/`data-e2e-vid` |
| Evil0ctal/Douyin_TikTok_Download_API (`3d540db`, Apache-2.0) | `signing/protection.py` L3–24, L51–68: Argus, `webSign` viết lại URL của yêu cầu |
| ChenKun1997/douyin-video-download (`47f4c8b`, không giấy phép — chỉ đọc) | `content.ts` L38–101: video đang phát ⇒ `closest('[data-e2e="feed-active-video"]')`, mục cũ còn sót |
| zhzLuke96/douyin-dl-user-js (`721d14f`, MIT) | `PlayerAdapter.ts` L29–61 `window.player` awemeInfo; `MediaHandler.tsx` L181–182 bỏ bản `dash` |
| mediago (`3e206c3`, MIT) | `short-video/adapter.ts` L73–84 `modal_id` ⇒ `/video/<mã>`, L145–151 `[data-aweme-id]` |
| zeeschuimer (`366df71`, MPL-2.0) | `modules/douyin.js` L21–126: `RENDER_DATA` mã hoá URL, `__pace_f`; dữ liệu nhúng dạng camelCase |
| lingji-cut (`59a2fc9`, Apache-2.0) | `dom-extractor.ts` L1–10, L269–276: lưới `[data-e2e="user-post-list"] li a`; hồ sơ chuyển sang SSR nên không bắt thụ động được ⇒ gọi `aweme/post` chủ động |
