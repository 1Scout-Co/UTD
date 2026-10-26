# Tải Tin / video Facebook bằng nút "Tải video này" (2026-10-26)

Nhánh `claude/fb-media-4c7a19` (cắt từ `claude/takeover-787f20` @ `884565e`). Chủ dự án báo: nút bấm được trên Tin
Facebook nhưng bảng báo "UTD không đọc được video này".

## Tóm tắt

- **Vì sao hỏng:** yt-dlp 2026.10.21 không có extractor cho `facebook.com/stories/…` (yt-dlp#5763 mở từ 2022-12).
  Video trong Tin phát bằng `blob:` (MSE) nên `video.currentSrc` vô dụng.
- **Cách chọn (học từ 11 dự án, Phụ lục A):** đọc link file thật trong dữ liệu JSON của chính trang Facebook, tìm
  đúng Tin đang xem bằng mã thẻ trong địa chỉ. Tin hiện có sẵn **file MP4 trọn có tiếng** (HD/SD) trong
  `progressive_urls` ⇒ thường không cần ghép. Manifest DASH cho bản nét hơn (1080p): hình và tiếng tách rời ⇒ UTD
  tải song song rồi ffmpeg ghép (đường tải nhanh sẵn có).
- **Đường dự phòng kiểu IDM:** link trong props React của trình phát; rồi các yêu cầu mạng trang đã gửi (đoạn fbcdn
  `bytestart`/`byteend` ⇒ bỏ hai tham số là cả luồng). Trang khác yt-dlp không đọc được: manifest HLS/DASH hoặc file
  media trang đã tải.
- **Không cookie:** link file đã có chữ ký của Facebook (`oh=`, `oe=`), tin `media-streams` không mang cookie. Không
  đăng nhập, không gọi API của Facebook, không gửi gì đi khi người dùng chưa bấm.

## Cách làm

### Ba mảnh trong tiện ích (`extensions/chromium` = `extensions/firefox`)

1. **`fb-media.js`** — thế giới MAIN, `document_start`, chỉ `https://*.facebook.com/*`. Nền đăng ký lúc chạy
   (`scripting.registerContentScripts`, như `takeover-keys.js`).
   - Bọc `JSON.parse` (cách của HushFacebookWeb): khối `<script type="application/json" data-sjs>` lúc mở trang và
     câu trả lời `/api/graphql/` khi chuyển trang trong app đều đi qua đây. Lọc rẻ bằng một regex trên chuỗi, duyệt
     kết quả sau (`setTimeout`), không đổi giá trị trả về.
   - Ghi nhớ (giới hạn 400 video): mã video ⇒ `{hd, sd, manifest, reps}`; mã thẻ Tin ⇒ mã video + tên chủ Tin.
   - Ghi thêm các yêu cầu đoạn luồng bằng `PerformanceObserver` (bộ đệm Resource Timing chỉ 250 mục).
   - Tiện ích nạp sau khi trang đã mở: `scanScripts()` đọc lại các khối JSON có sẵn.
2. **`page-media.js`** — hàm `utdFindPageMedia(x, y)`, nền chạy bằng `scripting.executeScript({world: "MAIN"})` khi
   người dùng bấm nút (Firefox < 128 không có MAIN: chạy như content script, đọc qua `wrappedJSObject`). Thứ tự:
   1. Video tại điểm bấm (giữa video đang gắn nút), không có thì video đang phát lớn nhất.
   2. Mã thẻ Tin từ địa chỉ `/stories/<bucket>/<thẻ>`; mã video từ `/reel/<mã>`, `/videos/<mã>`, `?v=`, props React
      (`videoId`, `videoFBID`…), link gần thẻ `<video>` nhất (bảng tin). Tra chỗ ghi nhớ của `fb-media.js`.
   3. Không có ⇒ link trong props trình phát (`hdSrc`/`sdSrc`/`playable_url`…).
   4. Không có ⇒ yêu cầu mạng: gộp theo `xpv_asset_id` (trong `efg`, base64 JSON), lấy asset tải nhiều đoạn nhất
      (video đang phát, không phải tin tải trước), luồng `vencode_tag` đuôi `audio` là tiếng, bỏ `.ad.`/`-ads`.
3. **`content.js` / `background.js`** — bảng chọn:
   - Trên facebook.com: hỏi `page-media.js` **trước** yt-dlp (Tin: yt-dlp chắc chắn hỏng và chậm). Có định dạng ⇒ hiện
     ngay. Không có ⇒ hỏi yt-dlp như cũ. Vẫn không được ⇒ "Chưa thấy luồng video. Cho video phát rồi bấm lại."
   - Trang khác: yt-dlp trước; yt-dlp báo lỗi có mã (UTD đang chạy) ⇒ hỏi `page-media.js`: định dạng, hoặc manifest
     (đưa cho yt-dlp đọc), hoặc file media ("Tải file video").
   - Không đọc sẵn (prefetch) bằng yt-dlp trên facebook.com nữa.
   - Nền kiểm lại kết quả từ trang (không tin hẳn): mã định dạng `[\w-]{1,16}`, link `https`, 1 luồng có hình hoặc
     hình + tiếng, codecs `[\w.-]{1,64}`; tối đa 8 dòng.

### Bảng định dạng

| Có gì | Các dòng |
|---|---|
| File trọn HD/SD + DASH | DASH cao hơn 720p (vd "1080p", ghép) · "HD" · "SD" |
| Chỉ DASH | Mỗi chiều cao một dòng (tối đa 6), hình + tiếng tốt nhất |
| Chỉ file trọn | "HD" · "SD" |
| Chỉ yêu cầu mạng | "Tải file video" (hình + tiếng của asset đang phát) |

Cùng chiều cao thì chọn H.264 (`avc1`) trước AV1/VP9 (downloadcraft, yobi: trình phát Windows hay hỏng với AV1).
Dung lượng ước theo `bandwidth × thời lượng` (MPD `mediaPresentationDuration`, hoặc `video.duration`).

### Phía app (`src-tauri`)

- Tin native `media-streams` (bản 2): `url` (trang), `title`, `streams` [1–2 × `{url, kind: video|audio|both,
  codecs?, sizeBytes?}`]. `native_host.rs` kiểm và **không chuyển `auth`**; `ipc.rs` → `video_button::download_streams`.
- `media/page_streams.rs`: kiểm luồng, lưu link vào journal (`page-streams:<task>`, xoá cùng task) để tải tiếp sau
  khi mở lại app. Job media có selector `utd-page`.
- `media/runner.rs`: job `utd-page` dùng `page_streams::Fixed` thay yt-dlp trong đường tải nhanh (`fast.rs`): hai luồng
  ⇒ tải song song nhiều kết nối, ffmpeg ghép MP4; một luồng ⇒ file thường. Gửi `Referer` là gốc trang
  (`https://www.facebook.com/`) như trình duyệt. Link hỏng/hết hạn ⇒ lỗi `media.page_expired` ("Link video trên
  trang đã hết hạn. Mở lại trang rồi bấm Tải video này."), không chạy yt-dlp.

### Reels, video thường, video trong bảng tin

- `/reel/<mã>`, `/watch/?v=`, `/<trang>/videos/<mã>`: yt-dlp đọc được, nhưng trên facebook.com bảng hỏi trong trang
  trước (nhanh, không cần cookie, đúng video đang xem). Mã video lấy từ địa chỉ / props / link quanh `<video>`.
- Video trong bảng tin (địa chỉ là trang chủ, yt-dlp báo Unsupported URL): link `/reel/` hoặc `/videos/` gần nhất
  quanh `<video>` (cách HushFacebookWeb), rồi props React, rồi yêu cầu mạng.

## Rủi ro / chưa làm

- Facebook đổi tên khoá JSON: đã đọc cả hai dạng (`progressive_urls` mới; `playable_url_quality_hd`,
  `browser_native_*` cũ). Dự phòng yêu cầu mạng không phụ thuộc khoá JSON.
- Firefox < 128: không có thế giới MAIN ⇒ không có `fb-media.js`; còn props (qua `wrappedJSObject`) và yêu cầu mạng.
- Tin ảnh (Photo) chưa hỗ trợ (nút chỉ hiện trên `<video>`).
- Link fbcdn hết hạn sau vài ngày (`oe=`): tải tiếp sau đó phải bấm lại trên trang (lỗi `media.page_expired`).
- Chưa thử thật trên Facebook (phiên này không đăng nhập): chủ dự án thử trên Chrome của mình.

## Bài thử

- `e2e/fb-media.spec.ts` (7): Tin mở thẳng (đúng thẻ, 1080p H.264 + tiếng, HD, SD, tên chủ Tin); chuyển Tin bằng
  GraphQL; tiện ích nạp sau; props React; yêu cầu mạng fbcdn (asset đang phát, bỏ quảng cáo/tải trước); manifest HLS
  ở trang khác; nền kiểm kết quả và gửi `media-streams` không cookie.
- `e2e/video-button.spec.ts` (+3): bảng trên Tin ra 1080p/HD/SD, chọn ⇒ `utd.video-streams`; chưa có luồng ⇒ báo gọn;
  trang khác ⇒ manifest đưa cho yt-dlp.
- Rust: `page_streams` (kiểm, journal), `native_host` (`media-streams`), `fast_tests::page_streams_download_and_merge_
  without_yt_dlp` (server 127.0.0.1 + ffmpeg thật: tải hai luồng, ghép, Referer chỉ gốc; link hỏng ⇒ không yt-dlp).

## Phụ lục A — Các dự án khác làm thế nào (đọc mã, 2026-10-26)

Đọc mã qua HTTP, không chạy mã của họ. "(đoán)" = suy luận, chưa kiểm trong mã.

| Dự án (commit) | Lấy link thế nào | Hình + tiếng | Chạy ở đâu |
|---|---|---|---|
| yt-dlp `facebook.py` (`fa98866`) | L34–58 `_VALID_URL` không có `/stories/`. L574–590 regex `data-sjs` → `RelayPrefetchedStreamCache`. L604–649 `parse_graphql_video`: `playable_url(_quality_hd)`, `browser_native_*`, `dash_manifest`/`manifest_xml`, mới: `videoDeliveryResponseFragment.videoDeliveryResponseResult.{progressive_urls, dash_manifests, dash_manifest_urls}`. L570 UA `facebookexternalhit/1.1` | Cả hai: file trọn + DASH | Python, đọc HTML trang |
| HushFacebookWeb (`3444636`, 2026-10-25) | `manifest.mjs` L16 MAIN + `document_start`. `net/hooks.js` L1105–1131 bọc `JSON.parse` (khối `data-sjs` không qua fetch/XHR). `media-index.js` L80–110 `progressive_urls` HD trước, bỏ `failure_reason`; L170–182 thẻ ⇒ `attachments[].media.id`. `find.js` L37–53 regex `^/stories/[^/]+/([^/?#]+)` ⇒ mã thẻ; L59–93 link `/reel/`, `/videos/` quanh `<video>` | File trọn; DASH thì ghép trong tab (`mp4-merge.js`) | MAIN world |
| downloadcraft (`75dccab`, 2026-10-24) | `page-hook.js` L67–99 bọc fetch/XHR, chuyển câu trả lời có `all_video_dash_prefetch_representations`/`progressive_urls`. `rank.js` L496–603 quét JSON; ưu tiên `avc1`. `content.js` L1145–1160 chọn video: mã reel trong địa chỉ, không thì file fbcdn trình phát tải cuối (so đường dẫn) | DASH (ghép sau) + file trọn | MAIN + nền webRequest |
| storiee (`0c6f128`, 2025-05) | `story.ts` L346–542 đọc HTML trang Tin (Puppeteer): `attachments[0].media.browser_native_*`; L479–514 `all_video_dash_prefetch_representations` **ghép theo vị trí mảng (dễ nhầm)**. Cách cũ L16–275: bắt `.mp4`, bỏ `bytestart`/`byteend`, `efg` → `vencode_tag` | Cả hai | Máy chủ |
| FSE (`e9d6ffa`, 2025-06) | L11–57 regex `<script type="application/json">` trên mã nguồn trang Tin đã lưu, tìm `video_id` + `representations`; L111–122 ffmpeg ghép | DASH tách rời | Python |
| useful-script (`d739535`) | `fb_storySaver.js` L18–79 fiber React của `<video>` ⇒ `implementations[1].data.hdSrc/sdSrc` (rất phụ thuộc bố cục). `fb_GLOBAL.js` L704–796 gọi GraphQL `doc_id` kèm `fb_dtsg` | File trọn | MAIN |
| GreasyFork 477748 v1.5 (2026-05) | Giả `__REACT_DEVTOOLS_GLOBAL_HOOK__` lấy cây fiber (L21–66), tìm `memoizedProps.video.id` (L207–437), rồi `video_data_async`/GraphQL | File trọn | Userscript |
| monokai fb-story-downloader (`1f3e888`, 2022) | `interceptor.js` L30–54 bọc XHR, đọc thân `storiesUpdateSeenStateMutation` ⇒ `bucket_id` + `story_id` Tin đang xem; L62–93 GraphQL ⇒ `playable_url_quality_hd` | File trọn | MAIN (MV2) |
| facebook-post-downloader (`9024b11`, 2025-12) | `graphql.js` L250–350 bọc XHR `/api/graphql`; `story.js` L108–126 `progressive_urls`; L803–857 lấy luồng **không phải tiếng** băng thông cao nhất ⇒ **file câm** (bẫy cần tránh) | Lẫn | MAIN |
| XDM (`a44f9c6`) | `SplitTrackRules.kt` L31–72: host `*.fbcdn.net`, có `bytestart`/`byteend`, `efg` → `vencode_tag` có `dash`, đuôi `audio` = tiếng, `.ad.`/`-ads` = quảng cáo; gộp `xpv_asset_id`, xếp `bitrate`; bỏ hai tham số = cả file | DASH (ghép) | App, nhận từ webRequest |
| Ghost-Downloader-3 (`e8cfeb1`, 2026-10-23) | `url-classify.ts` L19–35 bỏ `bytestart`/`byteend`/`_nc_rmd`, L60–106 `efg`. `instagram.ts` L23–67 (cả facebook.com): gộp asset, "video đang phát có nhiều đoạn nhất". `mse-probe.ts` L34–63 bọc `createObjectURL`/`appendBuffer` để gắn luồng vào đúng `<video>` | DASH (ghép) | MAIN + nền |
| yobi (`4e13118`, 2026-10-24) | `facebookReader.ts` L274–303 tải HTML trang Tin (đã đăng nhập); `facebookMedia.ts` L21–46 index mọi object JSON theo `id`, L72–97 các khoá file trọn; `facebookStories.ts` L112–118 `atob(thẻ)` = `S:_ISC:<số>`, L48–58 thời lượng từ MPD | File trọn | Electron |

Dữ liệu Tin thật (mẫu GraphQL đã che của HushFacebookWeb, `StoriesSuspenseContentPaneRootWithEntryPointQuery`,
2026-10): `data.bucket.unified_stories_with_notes.edges[].node` = thẻ (`id` base64, `story_card_info`,
`attachments[0].media`); media `Video` có `progressive_urls` HD + SD (file trọn có tiếng), `dash_manifests`,
`playable_url` (SD); **không có** `playable_url_quality_hd`/`browser_native_*` trong mẫu này (bản 2022–2025 có).
`extensions.all_video_dash_prefetch_representations` có 19 video, chỉ 1 thuộc bucket đang xem ⇒ không chọn theo vị trí.
