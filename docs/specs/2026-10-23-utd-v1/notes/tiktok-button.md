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

## Bài thử

- `e2e/video-button.spec.ts`: feed giả (2 mục `recommend-list-item-container` có `xgwrapper`) ⇒ đúng link từng video;
  lưới hồ sơ giả ⇒ đúng ô đang rê, rê qua không gửi gì, ô bài ảnh ⇒ link `/video/`.
- `e2e/fb-media.spec.ts`: nền đổi `channel_page: …` thành `media.failed`, không lộ câu thô.

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
