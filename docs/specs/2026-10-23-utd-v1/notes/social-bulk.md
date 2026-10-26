# Tải hàng loạt Facebook / Instagram / Threads (2026-10-26)

Nhánh `claude/social-bulk-e821de` (cắt từ main `93ed9a3`). Chủ dự án yêu cầu (tham khảo tiện ích Social Bulk Downloader):

- Facebook: ảnh, video, reels, Tin, Tin nổi bật của người dùng / fanpage; ảnh, video của nhóm.
- Instagram: ảnh/video trong bài viết, reels, Tin, Tin nổi bật của người dùng.
- Threads: ảnh/video trong bài viết của người dùng.

## Tóm tắt

- Trên trang cá nhân / fanpage / nhóm / Tin: chuột phải › **Tải hàng loạt bằng UTD**. Tiện ích thu danh sách **ngay trong
  tab đó**, bằng phiên đăng nhập sẵn có của trình duyệt (không đọc, không gửi cookie). Góc dưới trang có nhãn nhỏ
  "UTD · Đang thu N ảnh/video · Dừng".
- UTD mở hộp **Tải cả kênh** ở chế độ hàng loạt (không dựng màn mới): chip loại kèm số (Bài viết, Ảnh, Video, Reels, Tin,
  Tin nổi bật; Ảnh/Video cho Instagram, Threads), lọc Mới nhất N / Theo ngày / Chỉ mục mới, chọn từng ô, thư mục riêng theo
  tên trang, nút "Tải N mục". Mục đã tải lần trước hiện "Đã tải".
- Bỏ một chip hoặc đặt "từ ngày" lúc tiện ích còn thu ⇒ tiện ích bỏ phần đó (đỡ gọi trang thừa). Nút **Dừng** ở hộp hoặc
  trên trang ⇒ thôi thu, giữ phần đã có.
- Mỗi ảnh/video có link file trên CDN (đã có chữ ký của Facebook/Instagram) ⇒ task HTTP thường trong hàng đợi chung, tên
  `<YYYY-MM-DD> <chú thích ngắn> <id>.<đuôi>`. Video Facebook không có link file ⇒ task media (yt-dlp) từ trang video.
- Nhịp gọi: nghỉ ngẫu nhiên **2–4 s** giữa hai yêu cầu, **≤ 70 yêu cầu / 10 phút** (mỗi lần cuộn trang tính một yêu cầu);
  bị chặn tạm / cần đăng nhập ⇒ dừng ngay, báo rõ, phần đã thu vẫn tải được.

## Cách lấy danh sách — các dự án khác (đọc mã, 2026-10-26)

Đọc mã qua HTTP, không chạy mã của họ, không chép mã. SHA là commit cuối chạm vào file. "(chưa kiểm)" = chưa thấy trong mã.

| Dự án (commit) | Làm thế nào |
|---|---|
| gallery-dl `instagram.py` (`35f9167`) | API REST web: header `X-IG-App-ID: 936619743392459`, `X-ASBD-ID: 129477`, `X-CSRFToken` (cookie `csrftoken`), `X-IG-WWW-Claim` (L1113–1133). Bài viết `GET /api/v1/feed/user/{id}/?count=30&max_id=` → `items`, `more_available`, `next_max_id` (L1135–1152). Reels `POST /api/v1/clips/user/` (`target_user_id`, `page_size=50`, `max_id`) → `items[].media`, `paging_info` (L1154–1167). Tin `GET /api/v1/feed/reels_media/?reel_ids=<id>` (L945–951); tin nổi bật `GET /api/v1/highlights/{id}/highlights_tray/` rồi `reel_ids=highlight:<n>` **5 cái một lần** (L918–937). Ảnh lớn nhất `image_versions2.candidates`, video `max(video_versions)`, carousel `carousel_media`, ngày `taken_at` (L198–330). Nghỉ mặc định 6–12 s/yêu cầu (L31); chuyển hướng tới `/accounts/login/`, `/challenge/` ⇒ dừng (L153–179). |
| instaloader (`ca44660` structures.py, `0cf00a6` instaloadercontext.py) | GraphQL `doc_id` (bài viết đăng nhập `28975909992013618`, reels `7845543455542541`; L1374–1484). Giới hạn tự đặt: 75 yêu cầu thường / 11 phút, 275 GraphQL / 10 phút (ctx L788–900); `checkpoint_required`, `challenge_required`, `feedback_required` ⇒ dừng hẳn (L470–479). Ghi: `web_profile_info` có lúc trả 400/401/429 (structures L983–997). |
| yt-dlp `instagram.py` (`1f1101d`) | `InstagramUserIE` đánh dấu không chạy (L632–633): yt-dlp **không** liệt kê được hồ sơ Instagram. Tin/tin nổi bật `reels_media` (L702–760). |
| Open-IGDownload (`99c83be`) | Tiện ích MV3 "tải cả hồ sơ": chạy trong thế giới của trang, lấy `doc_id` sống bằng `require("<Op>_instagramRelayOperation")` (page-bridge.js L220–296); nghỉ 1,2 s giữa các trang (content.js L700–750); tải file `credentials: "omit"` (L769). |
| hoaianle/Instagram-Downloader (`2a2ea07`) | GraphQL `doc_id` viết cứng; header `x-ig-www-claim` từ `sessionStorage['www-claim-v2']` (utils.js L55–73); độ phân giải ảnh theo cookie `wd`/`dpr` (L42–53). |
| scrapfly threads-scraper (`8311140`) | Threads: dữ liệu nhúng `<script type="application/json" data-sjs>` → `thread_items[].post` (L222–262); phân trang GraphQL lấy `queryID` từ `expectedPreloaders` của trang, `lsd`, `x-ig-app-id: 238260118697367` (L95–174). Tên miền đã đổi `threads.net` → `threads.com`. |
| go-threads (`8303908`) | Ghi rõ `doc_id` hồ sơ Threads **đã đổi** ("user data is null"), nên dùng dữ liệu nhúng (graphql.go L24–41); đường dẫn `mediaData.edges[].node.thread_items[].post` (parsers.go L262–290). |
| vdite/threads-scraper (`8e84bc2`) | Cuộn trang, đọc câu trả lời `graphql`/`api/v1` trang tự gọi (L243–261) — không cần `doc_id`. |
| gallery-dl `facebook.py` (`473617f`) | Chỉ đọc HTML: `/<hồ sơ>/photos_by` → bộ ảnh → đi lần lượt `/photo/?fbid=<id>&set=<bộ>`, lấy bản gốc `"image":{"uri"` và ảnh kế `nextMediaAfterNodeId` (L89–123, L251–317). Trang có `CometErrorRoot.react` = "bị chặn tạm xem ảnh" (L229–249). **Không** có nhóm, Tin, tin nổi bật. |
| yt-dlp `facebook.py` (`e244458`) | Một video/reel: khối `data-sjs` → `__bbox.result.data` (L472–476, L574–590); khoá link `playable_url(_quality_hd)`, `browser_native_*`, `progressive_urls` (L604–641). |
| facebook-gql-scraper (`85849e2`) | GraphQL với `doc_id` tìm trong mã JS của trang; `ProfileCometAppCollectionListRendererPaginationQuery`, `...ReelsRendererPaginationQuery`, nhóm `GroupsCometFeedRegularStoriesPaginationQuery` (L612–676, L1003). Báo "GraphQL document with ID … was not found" = `doc_id` đã đổi. |
| facebook-cli (`f50ef61`) | "Không bao giờ viết cứng doc id… biến `__relay_internal__pv__*` bắt buộc, không đoán được" (graphql.go L14–25); mã 1675004 "Rate limit exceeded" ở HTTP 200 (L27–34). |
| HushFacebookWeb, notes/fb-media.md | Tin Facebook: `data.bucket.unified_stories_with_notes.edges[].node.attachments[0].media` (Photo/Video); dữ liệu tới qua `JSON.parse` (khối `data-sjs` + câu trả lời GraphQL) — UTD đã dùng và thử thật trong `fb-media.js`. |
| Social Bulk Downloader (Chrome Web Store `cnejldbhpclimaappoekdhmeieehfdpb`, v1.8.8) | Mô tả công khai: tải ảnh, video, reels, Tin, Tin nổi bật từ hồ sơ, trang, nhóm, bài viết; lọc theo ngày; huỷ giữa chừng. **Đọc mã bản đã cài trên máy chủ dự án (2026-10-26, không chép)**, `Instagram.f1e86f7f.js`: nền dùng `webRequest` bắt header/thân thật của trang (`x-ig-app-id`, `x-csrftoken`, `fb_dtsg`) rồi gửi `POST https://www.instagram.com/graphql/query` (hoặc `/api/graphql`) với `doc_id` + `variables` + `x-fb-friendly-name`. Mã hồ sơ: `"profile_id"` trong HTML trang, dự phòng `ProfileRootContentQuery` (`27981003384861049`, `xig_user_by_username.pk`). Bài viết `27672504985785333` (`xdt_api__v1__feed__user_timeline_graphql_connection`, 12/trang); reels `28143376935350124` (`fetch__XDTUserDict.clips_connection`, chỉ có mã bài ⇒ hỏi từng reel); Tin `28284433337819802` (`xdt_api__v1__feed__reels_media.reels_media[0].items`); danh sách tin nổi bật `26970053832668570` (`/api/graphql`, `highlights.edges`); từng tin nổi bật `38282502831363383` (`xdt_api__v1__feed__reels_media__connection`). |

Nghiên cứu đầy đủ (từng dòng, header, đường dẫn JSON): phụ lục ở cuối.

## Cách UTD làm cho từng loại

| Loại | Cách lấy | Ghi chú |
|---|---|---|
| Instagram — bài viết (ảnh, video, carousel) | GraphQL `PolarisProfilePostsTabContentQuery_connection` gọi từ trong trang | carousel tách từng ảnh/video; chỉ giữ bài của chính người đó |
| Instagram — reels | GraphQL `PolarisProfileReelsTabContentQuery_connection` | bỏ reel hợp tác của người khác; danh sách chỉ có ảnh bìa (thử thật: `media_type: 2`, không `video_versions`) ⇒ mục video không link file, yt-dlp tải từ `/reel/<mã>/` |
| Instagram — Tin | GraphQL `PolarisStoriesV3ReelPageStandaloneQuery` | lấy trước (hết hạn sau 24 giờ) |
| Instagram — Tin nổi bật | GraphQL `PolarisProfileStoryHighlightsTrayContentQuery` rồi `PolarisStoriesV3HighlightsPageQuery` từng cái | tên tin nổi bật ⇒ album (tên file, nhãn) |
| Instagram — người dùng | `"profile_id"` trong HTML mới tải của `/<tên>/` + thẻ OG; dự phòng GraphQL `ProfileRootContentQuery` | tên, ảnh đại diện |
| Threads — bài viết | dữ liệu nhúng của trang hồ sơ + **tự cuộn**, đọc JSON trang tự nạp | bỏ bài đăng lại / trích dẫn của người khác |
| Facebook — ảnh | mở thẻ `photos_by` (`&sk=photos_by`) + tự cuộn; ảnh lưới nhỏ (< 900 px) ⇒ mở `/photo/?fbid=` lấy bản gốc | như gallery-dl; không lấy được bản gốc thì giữ ảnh lưới |
| Facebook — video | thẻ `videos` + tự cuộn; `progressive_urls` (HD trước) / `playable_url*` / `browser_native_*` | không có link file ⇒ yt-dlp tải từ `/watch/?v=` |
| Facebook — reels | thẻ `reels` (`&sk=reels_tab`) + tự cuộn | |
| Facebook — Tin, Tin nổi bật | link `/stories/<bucket>/` trên trang hồ sơ (vòng tròn); mở từng cái, đọc bucket trong JSON | đang xem Tin (`/stories/…`) ⇒ chỉ bucket đó; bucket của người khác bị bỏ |
| Facebook — nhóm (ảnh, video) | `/groups/<id>/media/photos`, `/media/videos` + tự cuộn | |

Vì sao không gọi GraphQL bằng `doc_id` cho Facebook/Threads: `doc_id` và biến `__relay_internal__pv__*` đổi thường xuyên
(go-threads, facebook-cli, instaloader đều ghi). Để trang tự gọi (bằng mã đang dùng) rồi đọc theo `JSON.parse` thì không
phụ thuộc chúng — chỉ phụ thuộc hình dạng object media (`Photo`/`Video`, `image_versions2`/`video_versions`), ổn định hơn
nhiều. Instagram gọi thẳng GraphQL như Social Bulk Downloader (nhanh, đủ hơn cuộn); `doc_id` để một chỗ
(`SOCIAL_IG_QUERIES` trong social-bulk.js), Instagram đổi thì phần đó báo `social.changed` và sửa mã ở đó.

Thử thật với chủ dự án (2026-10-26, đã đăng nhập): `GET /api/v1/users/web_profile_info/` trả **429** ngay lần đầu;
`/api/v1/feed/user/…`, `/api/v1/clips/user/`, `highlights_tray` gọi từ tiện ích trả **200 kèm trang HTML** của web
(không phải JSON); `/api/v1/feed/reels_media/` trả 500 ⇒ bỏ hẳn REST `/api/v1/`. Thử GraphQL trên instagram.com không đăng
nhập (hồ sơ công khai): `ProfileRootContentQuery`, tin nổi bật, reels trả 200 JSON đúng khoá; bài viết trả 401
`require_login` (cần đăng nhập). Câu trả lời có thể mở đầu bằng `for (;;);` (CSRFGuard của web Instagram) ⇒ bóc trước khi đọc.

## Thiết kế

### Tiện ích (`extensions/chromium` = `extensions/firefox`)

- `social-bulk.js` (nền; Chromium `importScripts`, Firefox theo manifest). Tên có tiền tố `social`, dùng chung `api`, `t`,
  `sendNative`, `fromHost` của `background.js` (như `takeover.js`).
  - `socialPlan(url)`: trang nào làm gì (Instagram `/<user>/`, `/stories/<user>/`; Threads `/@user`; Facebook `/<vanity>`,
    `profile.php?id=`, `people/<tên>/<id>`, `groups/<id>`, `stories/<bucket>`). Trang khác ⇒ nhãn "Mở trang cá nhân,
    fanpage, nhóm hoặc Tin rồi bấm lại.", không gửi gì.
  - `SocialJob`: nhịp gọi (`pace`), cửa sổ 70 yêu cầu / 10 phút, gửi UTD từng đợt ≤ 150 mục / ≤ 600 KB (tin native ≤ 1 MiB),
    ít nhất mỗi ~9 s một đợt khi đang nghỉ để biết người dùng có Dừng không. Bấm menu lần nữa khi đang chạy = Dừng.
  - Mọi yêu cầu chạy trong tab bằng `scripting.executeScript` (thế giới MAIN; trình duyệt không cho thì content script, Firefox
    dùng `content.fetch` để mang nguồn của trang). Facebook/Threads: `tabs.update` chuyển thẻ, chờ `complete`, xong thì trả
    tab về trang ban đầu.
- `social-harvest.js` (nạp vào trang lúc bấm, thế giới MAIN): bọc `JSON.parse` + đọc lại khối `data-sjs` có sẵn, gom
  Facebook `Photo`/`Video`/bucket Tin, Instagram/Threads bài dạng REST; ảnh lớn nhất, ảnh nhỏ ~320 px; giữ thứ tự trong
  trang; tối đa 25 000 mục. Không gửi gì, không đổi kết quả của trang. Chỉ nạp khi người dùng bấm (khác `fb-media.js`).
- `background.js`: thêm 1 mục menu (chỉ hiện trên 4 tên miền, `documentUrlPatterns`) + 1 dòng điều phối + `importScripts`.
- 7 khoá chữ mới × 16 ngôn ngữ (`menuSocial`, `socialCollecting`, `socialStop`, `socialHint`, `socialDone`,
  `socialUnsupported`, `socialFailed`).

### Tin nhắn native `social-list` (bản 2)

```json
{ "version": 2, "type": "social-list", "url": "<trang đang mở>",
  "social": { "listingId": "sb-<24 hex>", "part": 0,
    "head": { "id": "528817151", "name": "NASA", "handle": "nasa", "avatar": "https://…cdninstagram.com/…", "followers": 97000000, "private": false },
    "entries": [{ "id": "7001002", "source": "posts", "kind": "video", "url": "https://…cdninstagram.com/….mp4",
      "page": "https://www.instagram.com/p/P1/", "thumb": "https://…", "timestamp": 1770000000, "title": "…", "album": null }],
    "done": { "complete": true, "error": null } } }
```

UTD trả lời trong trường `media` (native host chuyển nguyên): `{ "go": true, "sources": ["reels","posts"] | null, "since": null }`.
Đợt đầu kèm câu "Đã mở UTD để chọn ảnh/video". `error` chỉ là mã cố định (`social.rate_limited`, `social.login`,
`social.private`, `social.changed`, `social.not_found`, `social.partial`); app đổi sang câu của mình, mã lạ ⇒ câu chung,
không bao giờ hiện chữ do trang gửi.

### App (`src-tauri/src/social/`)

- `entries.rs`: kiểm từng mục — id `[A-Za-z0-9_:.-]{1,128}`, nhóm/loại trong danh sách, link file **https và chỉ CDN của
  Meta** (`*.fbcdn.net`, `*.cdninstagram.com`, `*.fbsbx.com`), trang bài viết đúng mạng của tin nhắn, ảnh bắt buộc có file,
  ngày trong khoảng hợp lý, chữ gộp khoảng trắng + cắt ngắn; khoá nhớ `<trang>:<id>` (`facebook.com:`, `instagram.com:`,
  `threads.com:`).
- `mod.rs`: sổ danh sách trong RAM (tối đa 8 danh sách, 20 000 mục, hết hạn sau 30 phút không đụng), bỏ trùng theo id, đánh
  dấu "Đã tải" bằng `media::channel::seen` (cùng bảng `settings`, không migration). Đợt đầu ⇒ sự kiện `utd://social-request`
  (giữ lại nếu giao diện chưa nghe: `take_social_request`). Đợt cuối ⇒ đưa cửa sổ UTD lên trước (lúc còn thu thì không,
  vì cửa sổ che trình duyệt làm Chrome coi tab là ẩn và trang thôi nạp thêm khi cuộn). Lệnh: `social_watch` (Channel; gửi lại
  phần đã có rồi nhận tiếp), `social_control` (Dừng / nhóm / từ ngày), `social_close`.
- `download.rs`: `add_social_downloads {listingId, ids, subfolder, later}` — mục lấy từ sổ (giao diện chỉ gửi id): có file
  ⇒ `commands::create_download` với tên đặt sẵn (trùng ⇒ `(1)`), không file ⇒ `media::create_media` (yt-dlp, "Tốt nhất");
  nhớ id đã tải; cả lô vào hàng đợi một lần.
- Đụng file chung (ít dòng): `ipc.rs` (trường `social`, `Route::SocialList`), `native_host.rs` (`social-list`), `lib.rs`
  (5 lệnh), `browser_integration.rs` (2 file tiện ích), `media/mod.rs` (`safe_base`, `with_subfolder` thành `pub(crate)`),
  `media/channel/mod.rs` (xuất `Downloaded`).

### Giao diện (`src/features/home`)

- Dùng lại `ChannelDialog` (S-60–S-63) với prop `social`: tiêu đề "Tải hàng loạt", chip loại (`SourceChips` trong
  `channel-parts.tsx`), không có chọn chất lượng, không "Thử lại" (danh sách do trang gửi), biểu tượng ▶ trên ảnh nhỏ của
  video, mục không chú thích hiện "Tin nổi bật · <tên>".
- `model/social.ts`, `api/social.ts`, `ui/use-social-listing.ts`; `channel-flow.tsx` nghe `utd://social-request`, đóng hộp ⇒
  `social_close`, "Tải N mục" ⇒ `add_social_downloads` (cuối hàng đợi).
- 18 khoá giao diện + 11 câu lỗi `err.social.*` × 16 ngôn ngữ. JS 201,7 kB / 220 kB (gzip).

## Giới hạn và rủi ro

- **Cần đăng nhập** trên trình duyệt cho Tin, tin nổi bật, tài khoản riêng tư, phần lớn Facebook. Tiện ích không đăng nhập hộ.
- **Instagram đổi `doc_id`**: phần đó báo "danh sách có thể chưa đủ", các phần khác vẫn lấy; cập nhật `SOCIAL_IG_QUERIES`.
- **Bị khoá tạm vì gọi nhiều**: Instagram 429 / `checkpoint_required` / `challenge_required` / "Please wait a few minutes";
  Facebook `CometErrorRoot.react`. UTD dừng ngay, không thử lại; nhịp mặc định chậm (2–4 s, ≤ 70/10 phút) — hồ sơ rất lớn
  sẽ lâu (Facebook: mỗi ảnh lưới nhỏ thêm 1 yêu cầu ⇒ ~7 ảnh/phút). Có thể bấm Dừng bất cứ lúc nào và tải phần đã có.
- **Facebook / Threads đổi giao diện**: tên thẻ (`photos_by`, `reels`, `media/photos`) hoặc hình dạng JSON đổi ⇒ thiếu mục
  hoặc báo `social.changed`. Phụ thuộc ít nhất có thể (không `doc_id`), nhưng chưa thử thật — chủ dự án thử trên Chrome.
- Ảnh lưới Facebook có thể đã đủ lớn (khi đó không mở trang ảnh). Mẫu `"image":{"uri":…,"width":…,"height":…}` của trang
  ảnh lấy theo gallery-dl; trang đổi thì giữ ảnh lưới.
- **Tab phải mở và đang hiện** khi thu Facebook/Threads (trang chỉ nạp thêm khi thấy cuộn). Nhãn trên trang nhắc việc này.
- Link CDN hết hạn sau vài ngày (`oe=`): nên tải ngay sau khi thu.
- Firefox < 128 không có thế giới MAIN: Instagram vẫn chạy; Facebook/Threads chỉ thấy phần nhúng sẵn của trang đầu.
- Tin Facebook: lấy cả bucket qua JSON của trang Tin; nếu Facebook chỉ nạp thẻ khi xem tới thì chỉ được các thẻ đã nạp.

## Bài thử

- Rust (`cargo test -p app --lib social`, 15): kiểm mục (host CDN, trang đúng mạng, ảnh bắt buộc file, ngày, chữ), mã lỗi cố
  định, sổ danh sách (đợt đầu mở hộp, phát lại khi hộp mở sau, bỏ trùng, Dừng/nhóm/từ ngày tới tiện ích ở đợt sau, hộp đóng
  / hết hạn ⇒ bảo dừng, hộp biến mất ⇒ dừng, "Đã tải" theo bộ nhớ hồ sơ, trần 8 danh sách / 20 000 mục, chỉ đợt cuối đưa
  UTD lên trước), tên file theo ngày UTC, `ipc` (route), `native_host` (bản 2, không chuyển cookie).
- Playwright `e2e/social-bulk.spec.ts` (9): Instagram đủ 4 loại qua API giả (đúng header, CSRF từ cookie trang, carousel,
  reel người khác bị bỏ, tin nổi bật có tên); UTD bỏ nhóm ⇒ không gọi + 429 ⇒ dừng giữ phần đã có; UTD Dừng ⇒ thôi gọi;
  trang không hỗ trợ + bảng `socialPlan`; Threads (nhúng + cuộn, bỏ bài người khác); Facebook đủ thẻ (ảnh lưới nhỏ ⇒ bản gốc,
  video có/không link, reels, tin nổi bật, bucket lạ bị bỏ, tab trở về); Facebook bị chặn tạm; hộp S-61 (chip, Ảnh/Video,
  báo tiện ích, Dừng, gửi đúng id, đóng ⇒ `social_close`); S-63 (lỗi rõ, không Thử lại, axe).

## Phụ lục — ghi chép nghiên cứu chi tiết

Instagram (gallery-dl `35f9167`): header mỗi lần gọi `X-CSRFToken`, `X-IG-App-ID 936619743392459`, `X-ASBD-ID 129477`,
`X-IG-WWW-Claim` (bắt đầu "0", cập nhật từ header `x-ig-set-www-claim`), `X-Requested-With: XMLHttpRequest`. Tìm id: topsearch,
`"profile_id":"(\d+)"` trong trang, hoặc `web_profile_info`. Ảnh: `candidates[0]` thường lớn nhất (tính max w×h cho chắc);
cảnh báo khi nhỏ hơn `original_width`. instaloader `ca44660`: `do_sleep()` ~ `min(expovariate(0.6), 15)` s; 429 ⇒ chờ rồi
thử; bị đăng xuất ⇒ "please wait some time". Open-IGDownload `99c83be`: `require("PolarisConfig").getIGAppID()`,
`require("PolarisWWWClaim").getWWWClaim()`; `require("PolarisInstapi").apiGet("/api/v1/feed/reels_media/?reel_ids=")`.

Threads (scrapfly `8311140`): ảnh `carousel_media[].image_versions2.candidates`, video `video_versions[].url`, `taken_at`,
`code`, `user.username`; LSD regex `\["LSD",\[\],\{"token":"([^"]+)"\}`. go-threads `8303908`: base `https://www.threads.com`,
`x-asbd-id 359341`; 403 ⇒ LSD cũ. hasyaapp userscript `9cc7055`: cuộn DOM, nghỉ 1800 ms.

Facebook (gallery-dl `473617f`): `Sec-Fetch-Mode: navigate`; album `/<hồ sơ>/photos_albums`; video đọc MPD nhúng (hình +
tiếng tách rời); người dùng: JSON giữa `","user":{"` và `},"viewer":{`. facebook-gql-scraper: video
`video_grid_renderer.video.videoDeliveryResponseFragment.videoDeliveryResponseResult.progressive_urls[]`. Tin: chưa dự án mở
nào đọc JSON Tin ngoài HushFacebookWeb (fixture 2026-10 trong notes/fb-media.md); hai công cụ Tin hàng loạt
(tyhallcsu `321b8ad`, oscar370 `04e9805`) đọc props React và bấm ArrowRight.
