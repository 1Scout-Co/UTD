# Việc (6): tải cả kênh TikTok (và kênh/playlist YouTube)

Ngày: 2026-10-24 · Nhánh: `claude/tiktok-channel-a531ef` · Trạng thái: xong, chờ phiên điều phối ghép.

## Kết quả ngắn

- Dán link kênh (`tiktok.com/@user`, kênh `youtube.com/@x`, `/channel/UC…`, `/c/…`, `/user/…`, thẻ
  `/videos` `/shorts` `/streams`, `playlist?list=`) ở hộp Thêm, hộp Tải media hoặc gửi từ extension ⇒ mở
  hộp **Tải cả kênh**. Link rút gọn `vm.tiktok.com`/`vt.tiktok.com` chưa biết là video hay kênh: đi
  đường Tải media như cũ, `media_probe` thấy yt-dlp trả playlist thì trả `channel_page: <link kênh>` và
  giao diện chuyển sang hộp Tải cả kênh.
- Hộp: ảnh đại diện, tên, @tên, số video đếm dần, người theo dõi; danh sách có ảnh nhỏ, tiêu đề, ngày,
  thời lượng, lượt xem; lọc Tất cả / Mới nhất N / Theo ngày, "Chỉ video mới"; Chọn tất cả / Bỏ chọn /
  từng ô; một chất lượng chung (Tốt nhất · H.264 · Nhẹ · M4A · MP3); thư mục con theo tên kênh (tắt
  được); nút "Tải N video". Kênh dài: video hiện dần theo đợt, nút **Dừng** giữ phần đã có, đóng hộp là
  dừng liệt kê.
- Mỗi video thành một task media qua lệnh có sẵn `add_media_download` (tên file `<YYYY-MM-DD> <tiêu đề>`).
- Nhớ id video đã tải theo kênh ⇒ mở lại kênh thì video cũ hiện "Đã tải", không được chọn mặc định,
  "Chỉ video mới" ẩn chúng.
- Không dùng cookie, không vượt bảo vệ: tài khoản riêng tư/cần đăng nhập/hội viên/DRM báo rõ (S-63, ô bị
  khoá kèm lý do).

## Nghiên cứu TikTokDownloader (JoeanAmier/TikTokDownloader, nhánh master, GPL-3.0)

Chỉ đọc mã để học cách làm; không chép mã, không chạy mã của họ.

1. **Lấy danh sách video của tài khoản TikTok** — `src/interface/account_tiktok.py`, lớp `AccountTikTok`:
   gọi `https://www.tiktok.com/api/post/item_list/` (thích: `/api/favorite/item_list/`) với `secUid`,
   `count=16`, `cursor` (bắt đầu 0), `coverFormat=2`, `needPinnedItemIds=true`, `video_encoding=dash`,
   cộng bộ tham số chung `APITikTok.params` (`src/interface/template.py`: `aid=1988`,
   `app_name=tiktok_web`, `device_platform=web_pc`, `region=US`, `device_id`, `msToken`…). Phân trang:
   `API.run_batch` lặp tới khi `finished`; `Account.check_response` lấy `itemList`, `cursor` mới từ phản
   hồi, `finished = not hasMore`. Dừng sớm theo ngày: `Account.early_stop` (cursor/1000 là thời điểm).
   `secUid` từ `@tên`: `ExtractorTikTok.user` (`src/link/extractor.py`) tải trang hồ sơ, lấy bằng regex
   `SEC_UID`; link rút gọn đi theo chuyển hướng ở `Requester.run` (`src/link/requester.py`).
2. **Chữ ký** — TikTok: `TikTokParams.sign_url` (`src/encrypt/tiktok_params.py`) gọi `sign()` trong
   `src/encrypt/tiktok_sign.py` (Python thuần, dịch ngược webmssdk 2.0.0.561) thêm `X-Dynosaur`,
   `msToken`, `X-Bogus=1`, `X-Gnarly` theo đúng thứ tự; `xBogus.py` cũ còn dùng để xin msToken. Douyin:
   `a_bogus` (`src/encrypt/aBogus.py`, SM3) + `x-secsdk-web-signature`. Có thể thay bộ ký bằng
   `encipher.py` ngoài. UA: `curl_cffi` giả `chrome146` (`src/tools/get_ua.py` đọc lại UA thật để ký
   khớp).
3. **Cookie** — wiki ghi `cookie_tiktok` và `device_id` là bắt buộc; đăng nhập = có `sessionid_ss`
   (`src/module/cookie.py`), chỉ cần cho tài khoản riêng tư và mục yêu thích. msToken lấy từ cookie (đoạn
   làm mới từ xa đã bị tắt); `ttwid` làm mới qua `TtWidTikTok.get_tt_wid`; vòng làm mới 15 phút trong
   `src/application/TikTokDownloader.py`.
4. **Bản không watermark** — `Extractor.__extract_video_download_tiktok` (`src/extract/extractor.py`)
   lấy `video.bitrateInfo[]`, xếp theo cạnh lớn nhất → bitrate → dung lượng, dùng `UrlList[0]`; không
   dùng `downloadAddr` (bản có watermark); không chọn codec (có thể ra H.265). Tải kèm `Referer:
   https://www.tiktok.com/explore` và cookie (`DOWNLOAD_HEADERS_TIKTOK` trong `src/custom/internal.py`).
5. **Chống bị chặn** — `wait()` (`src/custom/function.py`): nghỉ ngẫu nhiên kiểu log-normal ~6 s (1,5–33 s)
   sau mỗi lần gọi API; `suspend()` nghỉ 5 phút sau mỗi 10 tài khoản; `Retry.retry` (`src/tools/retry.py`)
   thử lại 5 lần, không backoff mũ; `Downloader.semaphore` = 4 file cùng lúc, khối 2 MiB, tải tiếp bằng
   `Range`; có proxy; tắt kiểm TLS (`verify=False`).
6. **Nhớ đã tải, đặt tên, chỉ bài mới** — SQLite `download_data(ID TEXT PRIMARY KEY)`
   (`src/manager/database.py`, `DownloadRecorder` trong `src/manager/recorder.py`); `Downloader.is_skip`
   bỏ qua nếu id đã ghi hoặc file đã có; ghi sau khi tải xong. Thư mục `UID{secUid}_{nickname}_…`, tên
   file theo mẫu `create_time type nickname desc`. Lọc ngày `earliest`/`latest` ở cursor và ở
   `createTime`.
7. **Rủi ro họ tự ghi** — thuật toán ký "không còn bảo trì", đổi SDK là hỏng im lặng (`item_list` trả 200
   rỗng); thiếu `ttwid` có thể bị hệ thống rủi ro gắn cờ; giá trị canvas giả là dấu hiệu bot rõ.

## yt-dlp trong app có đủ dùng không

Thử ngày 2026-10-24 với yt-dlp **2026.10.21** (bản chính chủ từ GitHub, SHA-256 khớp `SHA2-256SUMS`;
bản 2026.08.19 trong PATH máy này cũng chạy), **không cookie**:

- `yt-dlp --flat-playlist -J "https://www.tiktok.com/@tiktok"` (extractor `TikTokUser`) chạy được:
  40 video đầu trong ~9 s (gồm ~2–3 s khởi động exe). `--flat-playlist --lazy-playlist -j` in từng video
  ngay khi đọc trang: ~20 video/giây.
- Mỗi dòng có: `id`, `url` (trang video), `title` (mô tả cắt ~70 ký tự), `description`, `timestamp`,
  `upload_date`, `duration`, `view_count`, `like_count`, `comment_count`, `thumbnails` (`cover`,
  `dynamicCover`, `originCover`, không có kích thước), `channel` (tên hiển thị), `uploader` (@tên),
  `channel_id` (secUid), và `playlist_id` (secUid), `playlist_webpage_url`… Không có ảnh đại diện, số
  video, số người theo dõi.
- Cách yt-dlp làm (`yt_dlp/extractor/tiktok.py`, lớp `TikTokUserIE`, mã public domain): đọc trang hồ sơ
  (giả trình duyệt) lấy `secUid`, hỏng thì đọc trang nhúng `/embed/@tên`; liệt kê bằng
  `https://www.tiktok.com/api/creator/item_list/` với `type=1` (mới → cũ), `count=15`,
  `cursor` = thời điểm (ms) của video cuối trang, `device_id`/`verifyFp` ngẫu nhiên, **không có
  X-Bogus/a_bogus**; tự thử lại khi API trả trùng trang. Lỗi rõ: tài khoản riêng tư (cần đăng nhập),
  "does not have any videos", "Unable to extract secondary user ID".
- Bản không watermark: định dạng `download` (có watermark) bị yt-dlp xếp cuối; mặc định ra bản
  `bytevc1` (H.265) 1080p, có `h264_720p`. Vì máy Windows không có tiện ích HEVC mở H.265 khó, hộp có
  lựa chọn **H.264** (`b[vcodec^=h264]`…).
- YouTube: `@x` (trang chủ kênh) trả 3 thẻ lồng (Video/Live/Shorts) ⇒ app đổi sang `/@x/videos`. Thẻ kênh
  không có ngày đăng ⇒ thêm `--extractor-args youtubetab:approximate_date` (ngày ước lượng), `view_count`
  trống. Phần đầu (`-J -I 0`, ~1,5 s) có ảnh đại diện (`avatar_uncropped`), `channel_follower_count`.
- Video lẻ với `--flat-playlist -I 0` vẫn đủ định dạng ⇒ thêm hai cờ này vào `media_probe` không đổi
  gì cho link video, chỉ làm link kênh trả nhanh phần đầu (TikTok ~7 s) thay vì phân tích cả kênh.

**Quyết định:** dùng yt-dlp (ít bảo trì, cập nhật theo TikTok). Không tự gọi API web của TikTok: phải ký
X-Gnarly/X-Bogus, giả `msToken`/`ttwid`/`device_id`, dễ hỏng im lặng và dễ bị gắn cờ bot — đúng thứ
TikTokDownloader phải tự bảo trì. Ngoại lệ duy nhất: ảnh đại diện + tên + người theo dõi đọc từ **trang
nhúng công khai** `https://www.tiktok.com/embed/@<tên>` (chính yt-dlp cũng dùng trang này; không thử thách
chống bot, không chữ ký; một yêu cầu, hỏng thì chỉ thiếu ảnh). Trang hồ sơ `tiktok.com/@tên` trả thử
thách JS (WAF) — **không** giải nó.

Rủi ro còn lại: TikTok đổi API ⇒ chờ bản yt-dlp mới (nút cập nhật công cụ ở Cài đặt); trang nhúng đổi
cấu trúc ⇒ chỉ mất ảnh đại diện; liệt kê nhiều kênh liên tục có thể bị TikTok hạn chế (yt-dlp chỉ gọi tuần
tự từng trang, app chỉ chạy một lượt mỗi hộp).

## Thiết kế

- `src-tauri/src/media_pages.rs`: `is_channel_page`, `channel_listing_url` (bỏ tham số thừa, trang chủ
  kênh YouTube ⇒ `/videos`), `CHANNEL_PAGE_ERROR` (`channel_page: <link>`), `CHANNEL_PAGE_NOTE`. Bản TS:
  `isChannelPage` trong `src/features/home/model/media-pages.ts`; bộ ca chung trong `media_pages.rs` và
  `e2e/channel-download.spec.ts`.
- `add_download` gặp link kênh ⇒ lỗi `channel_page: <link>`; pipe extension (`ipc.rs`) có `Route::Channel`
  ⇒ phát cùng sự kiện `utd://media-request`, giao diện tự nhận ra link kênh. Extension không phải sửa.
- `src-tauri/src/media/channel/`:
  - `mod.rs`: lệnh `channel_list(url, listingId, onEvent: Channel)` trả ngay, chạy
    `yt-dlp --flat-playlist --lazy-playlist --dump-json` (YouTube thêm `approximate_date`), gửi sự kiện
    `head` · `profile` · `entries` (đợt 30 video hoặc 250 ms) · `done {total, complete, error}`;
    `channel_list_cancel(listingId)`. Tối đa 5 000 video; 120 s không có dòng mới ⇒ dừng; đọc hết stderr
    (giữ 64 KB cuối) để phân loại lỗi; hộp đóng (Channel hỏng) ⇒ dừng.
  - `entries.rs`: đọc một dòng JSON ⇒ `ChannelEntry` (bỏ dòng thiếu id/link, thẻ lồng; ảnh nhỏ: ảnh hẹp
    nhất ≥ 160 px hoặc `cover`; `availability`/`live_status` ⇒ lý do khoá) và `ChannelHead` (khoá
    `<trang>:<playlist_id>`, tên kênh hoặc tên playlist).
  - `profile.rs`: TikTok trang nhúng (`utd_engine::fetch::fetch_text`, chỉ host `www.tiktok.com`, ≤ 4 MB);
    YouTube `-J -I 0`.
  - `seen.rs`: nhớ id đã tải trong bảng `settings` của journal, khoá `channel.seen.<khoá kênh>` = JSON
    `{id video: id task}` (tối đa 20 000 id/kênh). **Không thêm bảng/migration** để khỏi đụng phiên khác.
    Video tính "đã có" khi task còn và chưa lỗi, hoặc task đã bị bỏ khỏi danh sách; task lỗi ⇒ lần sau
    được chọn lại. Sau khi ghép (cả kênh vào hàng đợi chung ngay, video được nhớ lúc xếp hàng): bỏ khỏi
    danh sách task **chưa tải xong** (vd xoá các task đang chờ để ngưng tải kênh) ⇒ `seen::forget` quên
    video đó (khoá ngược `channel.task.<id task>` = khoá kênh, xoá khi task bị bỏ); task đã xong vẫn nhớ.
- `add_media_download` (`AddMediaInput`) thêm 2 trường tuỳ chọn: `subfolder` (làm sạch bằng `safe_base`,
  một tầng) và `channel {key, videoId}` (ghi nhớ sau khi tạo task). Lệnh cũ gọi không có hai trường này
  vẫn y như trước.
- Giao diện (`src/features/home`): `model/channel.ts` (kiểu, lọc/chọn, chất lượng, tên file),
  `api/channel.ts`, `ui/channel-dialog.tsx` + `channel-parts.tsx`, `ui/use-channel-listing.ts`,
  `ui/use-channel-queue.ts`, `ui/channel-flow.tsx`; ngày dùng chung ở `src/shared/dates.ts`; thêm
  `src/components/ui/checkbox.tsx` (shadcn). Mã màn S-60–S-63 (SPEC, mục "Tải cả kênh").
- Chọn mặc định: mọi video chưa tải và không bị khoá, kể cả video còn đang liệt kê tới; "Chọn tất cả"
  chọn cả video đã tải đang hiện; danh sách dùng `content-visibility: auto` để kênh dài không đơ.

## Hàng đợi (cho phiên điều phối gộp với phiên 3)

Main chưa có hàng đợi chung giới hạn số download chạy cùng lúc. Tạm thời `ui/use-channel-queue.ts` tự
giới hạn ở giao diện: `CHANNEL_PARALLEL = 2` video của kênh chạy cùng lúc, video kế tiếp chỉ được gửi
`add_media_download` khi một video xong/lỗi/tạm dừng; thanh nhỏ dưới màn hình "Kênh: còn N video chờ ·
Dừng". Hàng chờ nằm ở màn chính (đóng hộp vẫn tải tiếp), **không** lưu xuống đĩa: đóng app thì phần chưa
gửi mất, nhưng lần sau "Chỉ video mới" chọn đúng phần còn thiếu. Khi ghép với hàng đợi chung của phiên 3:
đẩy hết `payloads` một lần (hoặc qua lệnh thêm hàng loạt của phiên 3) rồi bỏ `use-channel-queue.ts`.

## Chỗ dễ va khi ghép

- `src-tauri/src/media/mod.rs` (`AddMediaInput`, `enqueue_media`, hàm mới `media_destination`),
  `media/probe.rs` (2 cờ yt-dlp, nhánh `_type: playlist`, `classify_stderr` thành `pub(super)`),
  `commands.rs` (`enqueue_download`), `ipc.rs` (`Route::Channel`), `lib.rs` (2 lệnh).
- `home-screen.tsx`, `add-download-dialog.tsx`, `media-picker-dialog.tsx` (prop `onChannel` bắt buộc),
  `model/types.ts` (`AddMediaPayload`), `api/downloads.ts` (`channelPageUrl`).
- Chữ giao diện mới chưa qua đa ngôn ngữ (phiên 1): `channel-dialog.tsx`, `channel-parts.tsx`,
  `channel-flow.tsx`, câu lỗi trong `media/channel/mod.rs`.

## Kiểm thử

Tự động (2026-10-24, trên nhánh này):

- `cargo test -p app --lib`: 144 qua. Mới: `media_pages` (bộ ca link kênh, `channel_listing_url`, mã
  lỗi), `media::probe` (playlist ⇒ `channel_page:`), `media::channel::entries` (đọc fixture JSONL rút gọn
  từ yt-dlp thật TikTok + YouTube, ngày `upload_date`, ảnh nhỏ, lý do khoá, khoá kênh an toàn),
  `profile` (fixture trang nhúng TikTok, ảnh đại diện YouTube), `seen` (nhớ/bỏ qua task lỗi, khoá băm ≤ 64
  ký tự với secUid thật), `channel::tests` (đọc luồng, bỏ trùng, đánh dấu đã tải, Dừng/hộp đóng, phân loại
  lỗi, chỉ thử lại khi TikTok từ chối lúc chưa có video), `ipc` (`Route::Channel`).
- `cargo test -p utd-engine`, `cargo test -p utd-mp4`: xanh (không sửa hai crate này).
- `cargo clippy -p app --all-targets -- -D warnings`: sạch.
- `pnpm lint`, `pnpm build` (JS 179 kB gzip, dưới trần 200 kB): sạch. `knip`: không có export thừa mới.
- `pnpm exec playwright test` (PORT riêng): 36 qua, trong đó `e2e/channel-download.spec.ts` 7 bài: bộ ca
  `isChannelPage`, luật lọc/chọn/tên file (`model/channel.ts`), S-61 (hộp Thêm → kênh, lọc Mới nhất/Theo
  ngày, Chỉ video mới, Bỏ chọn/Chọn tất cả/bấm cả dòng, MP3, đúng payload `add_media_download`, chỉ 2
  video chạy cùng lúc, video thứ 3 vào khi một video xong), S-60 (liệt kê dần, Dừng giữ phần đã có, axe),
  S-63 (tài khoản riêng tư, axe), link rút gọn qua `media_probe` và link từ trình duyệt mở hộp kênh, bản
  xem trước báo cần app.

Thử thật (chế độ `UTD_DATA_DIR` trong scratchpad, cổng debug WebView2 9416, bản debug nhúng giao diện
`cargo build -p app --features tauri/custom-protocol`; bản thật của chủ dự án không bị đụng; đã tắt app
thử và xoá dữ liệu/thư mục tải thử):

- `@tiktok`: video đầu sau ~12 s (gồm khởi động yt-dlp), 327–342 video sau ~24 s thì bấm Dừng; tên,
  @tên, 96,4 Tr người theo dõi, ảnh đại diện (sau khi thêm thử lại trang nhúng). Lọc Mới nhất 4 + H.264 ⇒
  4 task trong thư mục `TikTok`, tối đa 2 chạy cùng lúc, xong cả 4 trong ~14 s. ffprobe: H.264 720×1280 +
  AAC, thời lượng khớp danh sách (49–107 s), `ffmpeg -f null` giải mã hết không lỗi; khung hình không có
  watermark TikTok. Mở lại kênh: "đã tải 4", 4 dòng nhãn "Đã tải", mặc định chọn 116/120 (bỏ 4 video cũ).
- `youtube.com/@YouTube`: ảnh đại diện, 46,5 Tr người theo dõi, 197 video (thẻ Video) có ngày ước lượng và
  thời lượng, liệt kê xong trong vài giây (không tải video YouTube nào).

Phát hiện khi thử thật (đã sửa):

1. Khoá bảng `settings` tối đa 64 ký tự; khoá kênh TikTok thật ~79 ký tự ⇒ lần đầu không nhớ được id.
   Sửa: khoá băm FNV-1a, giá trị mang khoá đầy đủ để kiểm.
2. Trang nhúng TikTok trả `503 overload-protect triggered` khoảng một nửa số lần. yt-dlp 2026.10.21 cũng
   cần trang này để tìm secUid (trang hồ sơ không còn cho secUid khi không cookie) ⇒ yt-dlp báo
   "Unable to extract secondary user ID" lúc được lúc không. Sửa: ảnh đại diện thử lại tối đa 3 lần
   (1,5 s, 3 s) và chỉ gọi sau khi yt-dlp đã đọc được đầu danh sách (không tranh nhau); lượt liệt kê tự
   thử lại tối đa 3 lần (nghỉ 3 s, 6 s) khi TikTok từ chối lúc chưa có video nào. Lần thử thật cuối không
   cần thử lại nên nhánh thử lại chỉ mới kiểm bằng unit test. `tiktokuser:<secUid>` của yt-dlp không dùng
   được để né bước này: yt-dlp chỉ nhận secUid dài 76 ký tự, `@tiktok` có secUid 55 ký tự.

Chưa sửa (ngoài phạm vi, báo để phiên điều phối biết): đường dẫn file > 260 ký tự làm yt-dlp (Python) báo
`unable to open for writing` và app chỉ hiện câu chung "Không tải được media". Gặp khi thư mục tải sâu
(thư mục thử ~170 ký tự + tên file ~90); với `Downloads\UTD\<kênh>` bình thường còn xa giới hạn.
Nên: `media/runner.rs::classify_failure` nhận ra câu này, hoặc cắt tên file theo độ dài đường dẫn.

## Việc còn lại

- Cookie/đăng nhập cho tài khoản riêng tư: chưa làm (S-14 của SPEC); hiện chỉ báo rõ.
- Kênh YouTube chỉ lấy thẻ Video khi dán trang chủ kênh; Shorts/Live phải dán link thẻ đó.
- Chưa có "liệt kê tiếp" sau khi bấm Dừng (mở lại hộp là liệt kê lại từ đầu).
- Bài ảnh (TikTok photo mode) trong kênh: chưa kiểm yt-dlp liệt kê/tải thế nào.
