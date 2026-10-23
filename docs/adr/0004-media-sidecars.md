# ADR-0004 — Media qua sidecar yt-dlp/ffmpeg, không DRM

## Bối cảnh

Cần tải video YouTube/TikTok/Facebook và HLS/DASH, nhưng binary yt-dlp chính thức là GPLv3+ và ffmpeg full build là GPL; core UTD là Apache-2.0.

## Quyết định

- Không bundle yt-dlp/ffmpeg vào installer UTD.
- Phát hiện công cụ đã cài; nếu thiếu, hỏi người dùng rồi tải từ nguồn chính thức kèm checksum, license notice, rollback.
- Chạy như process riêng; HLS/DASH VOD không DRM dùng engine native; ffmpeg chỉ mux/remux `-c copy`.
- Từ chối DRM/paywall; chỉ tải nội dung người dùng có quyền.

## Hệ quả

- Installer sạch license; phụ thuộc phiên bản ngoài và cần nút cập nhật extractor.
- Một số site có thể gãy khi đổi cấu trúc; cần thông báo và fallback URL trực tiếp.
- Không có DRM hay tải nội dung trả phí.

## Trạng thái

Accepted
