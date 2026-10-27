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

### Thử thật (2026-10-26)

- Chromium riêng (Playwright, phiên mới, chưa đăng nhập) nạp tiện ích của nhánh này: `douyinCall(tab, "posts")` trên hồ
  sơ 奋斗的希瓜皮 ⇒ 19 bài kèm bản 2160/1440/1080/720p trong 2,3 s; `media(url)` ⇒ 2160…576p.
- **Chưa đăng nhập chỉ xem được trang đầu:** trang 1 có 19 bài + `has_more=1` (hồ sơ có 28 bài, lưới hiện ô
  "看更多最新作品 · 登录"), trang 2 trả rỗng + `has_more=0`; cuộn lưới cũng không tải thêm. Tiện ích báo `login` cho
  trường hợp này (giữ các bài đã lấy, hộp kênh báo cần đăng nhập để xem tiếp).
- **Có phiên chưa đăng nhập bị Douyin hạn chế hẳn** (Chrome của chủ dự án): lời gọi đã được ký đủ (`msToken, a_bogus,
  verifyFp, fp, uifid, x-secsdk-web-signature`), mượn cả bộ tham số thật của trang (`version_name=17.4.0`, `webid`,
  `browser_*`…) vẫn nhận 200 + 0 byte, không header xác minh. Lưới vẫn hiện vì trang lấy lúc mở.
- Đã thử "nghe" lời gọi của chính trang thay vì tự gọi, không dùng được: SDK của Douyin thay `XMLHttpRequest` và tự gửi
  lại yêu cầu (bọc `open`/fetch không thấy `load`); bọc `JSON.parse` từ `document_start` thì trang không tải (Douyin phát
  hiện); ô lưới (React `itemInfo`) chỉ có mã, mô tả, ảnh bìa, không có link MP4.

### Luật cookie

Không đọc cookie ở đâu cả cho Douyin: `fetch` trong trang tự kèm cookie của trình duyệt, tin `channel-entries` và
`media-streams` không có trường `auth` (native host bỏ nếu có). Lời gọi chỉ chạy trong cú bấm của người dùng.

### Sửa sau review 2026-10-27

- **Chỉ chạy API Douyin trong đúng tài liệu Douyin.** `executeScript` thế giới MAIN chạy trong tài liệu *hiện tại* của
  tab, mà trang lạ tự định nghĩa được `window[Symbol.for("utd.douyin")].posts` (douyin-page.js giữ nguyên khi đã có
  sẵn) ⇒ người dùng mở website khác trong cùng tab giữa hai lượt (nghỉ 2,5 s) thì trang đó trả được mục Douyin hợp lệ
  kèm `variants[].url` ở máy chủ lạ. Nay `douyinCall` (dùng cho cả lật hồ sơ lẫn nút "Tải video này") mỗi lần gọi:
  1. chạy một `executeScript` **không đặt `world`** (ISOLATED, trang không sửa được hàm gốc ở đó) đọc
     `location.hostname`; không phải `douyin.com` (https) ⇒ ném lỗi, không chạy gì trong trang;
  2. lấy `documentId` của kết quả đó (Chromium ≥ 106) và nhắm các lời gọi MAIN vào `target: { tabId, documentIds }`;
     tab đã sang tài liệu khác ⇒ trình duyệt từ chối. Không có `documentId` (Firefox có thể không có) ⇒ vẫn `frameIds:
     [0]` kèm kiểm host ở bước 1, còn một khe hở vài ms giữa kiểm và chạy;
  3. cả một lượt tải hồ sơ dùng chung một `run`: tài liệu của lần gọi đầu được ghi lại, lần sau khác tài liệu ⇒ ném lỗi
     ⇒ vòng đẩy `done: true, error: "blocked"` (câu có sẵn "Douyin không trả thêm danh sách…") rồi dừng.
- **Dừng/đóng hộp phải dừng vòng lật; bấm lại phải mở hộp.** Trước đây `douyinRunning` (Set) giữ chỗ suốt vòng (tới 5000
  video × 2,5 s) và vòng bỏ qua kết quả `push`: Dừng/đóng hộp trong app mà tiện ích vẫn lật tiếp, bấm chuột phải lại
  thì `return true` ngay (không gửi `download`) nên hộp không mở. Nay:
  - Mỗi lượt có một `run`; `douyinRuns` (Map theo `sec_uid`) giữ lượt hiện tại. Bấm lại ⇒ lượt mới thay lượt cũ (vòng cũ
    thấy mình không còn là lượt hiện tại sau mỗi `await` và thoát, không đẩy thêm gì), lượt mới `push({})` rồi gửi
    `download` như lần đầu ⇒ hộp mở.
  - Vòng dừng khi `push` trả không `ok`. App báo điều đó bằng `douyin::push` trả lỗi (`Hộp Tải cả kênh đã dừng.`, không
    có mã dịch vì người dùng không thấy) khi lượt đã bị dừng: `Feed.stopped` đặt lúc `list` kết thúc vì huỷ
    (`channel_list_cancel`: nút Dừng, đóng hộp) hoặc vì cửa sổ không nhận sự kiện nữa. Hộp chưa mở thì chưa phải dừng
    (trang đầu có thể tới trước khi hộp gọi `channel_list`). Phần đầu rỗng (`push({})`, người dùng bấm lại) tạo `Feed`
    mới nên nhận lại bình thường. Danh sách đã nhận vẫn giữ trong RAM 30 phút như cũ.
  - Chưa làm: mở lại hộp (cùng hồ sơ) trên lượt đã dừng sẽ chờ tới `IDLE` (120 s) rồi báo "danh sách có thể chưa đủ";
    đường vào hộp thật là bấm trên trang Douyin nên luôn bắt đầu lượt mới.
- **Đường dự phòng `window.player` (camelCase)** cũng bỏ bản `gearName` chứa `bytevc2` và nhận cờ HEVC `isBytevc1` (cùng
  `is_bytevc1`/`is_h265`/`isH265`), để cùng mức thì H.264 đứng trước như đường API.
- Chưa thử trên Firefox thật: `scripting.executeScript` trả `documentId` và nhận `target.documentIds` hay không; mã đã
  chừa đường lùi khi kết quả không có `documentId`, nhưng nếu Firefox trả `documentId` mà từ chối `documentIds` thì
  Douyin trên Firefox báo "blocked" ngay (không chạy sang trang lạ).

## Bài thử

- `e2e/douyin.spec.ts`: API giả — chi tiết video (H.264 trước, bỏ DASH/watermark, không tự điền chữ ký, `modal_id`), bị
  chặn ⇒ `window.player` (cả camelCase: bỏ `bytevc2`, cờ `isBytevc1`), một trang `aweme/post` (link, thời lượng, ảnh bìa,
  bản MP4; bài ảnh không có bản nào), tải cả hồ sơ trong nền (báo trước, mở hộp, lật 2 trang, bỏ mục/link lạ, không
  cookie, đọc host trước mỗi lời gọi và nhắm `documentIds`). Sửa sau review: mở website khác trong cùng tab giữa hai trang
  (có và không có `documentId`) ⇒ không nhận mục của trang lạ, báo `blocked`; sang tài liệu Douyin khác ⇒ dừng; tab rời
  đi ngay sau lúc kiểm host ⇒ lời gọi bị từ chối; nút trên trang không chạy gì khi tab đã sang website khác; app báo hộp
  đã dừng ⇒ không lật trang tiếp; bấm lại giữa lúc lật ⇒ lượt cũ dừng và hộp mở lại.
- `e2e/video-button.spec.ts`: feed (`data-e2e-vid`), lưới (link bọc và cạnh video), `modal_id` + báo gọn khi chưa đọc được.
  Sửa sau review: trang có module (`tiktok.js`, `douyin.js`) mà module không tìm được link của video (thẻ quảng cáo) ⇒
  `analysisUrl` trả `null`, bảng báo "UTD không đọc được video này." và không gửi link trang cho yt-dlp.
- Rust: `channel::douyin` (chọn bản theo chất lượng; phát danh sách đẩy dần, không trùng, lỗi bị chặn; sau Dừng/đóng hộp
  `push` từ chối đến khi có phần đầu mới), `native_host`
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
