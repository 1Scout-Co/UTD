---
ma: e5z3wac7
tom_tat: "Nhánh nút tải TikTok/Douyin: giao diện đã duyệt; sửa luôn TikTok chờ ~12 s trước khi tải và 4 điểm nhỏ rồi mới gộp"
pham_vi: 2026-10-23-utd-v1
trang_thai: dang-dung
luc: 2026-10-27 10:51
ai_quyet: ban
---

# [e5z3] Nhánh nút tải TikTok/Douyin: giao diện đã duyệt; sửa luôn TikTok chờ ~12 s trước khi tải và 4 điểm nhỏ rồi mới gộp

## Vì sao
Nhánh hứa "TikTok bắt đầu tải ngay" nhưng thử thật vẫn chờ 11–13 s vì app hỏi yt-dlp lần hai (định dạng TikTok gộp sẵn hình + tiếng nên không dùng lại kết quả Phân tích). Chủ dự án muốn sửa trước khi gộp, kèm các điểm nhỏ QA thấy.

## Áp dụng thế nào
Trong claude/fb-media-4c7a19: (1) chọn chất lượng TikTok dùng lại kết quả Phân tích, có byte trong ~1–2 s; (2) nút "Tải video này" không đè thanh đầu trang Douyin; (3) nhãn mức theo cạnh ngắn (video dọc 1080×1920 ghi 1080p) cho mọi trang; (4) bảng không có hai dòng trùng (Tốt nhất và mức duy nhất cùng dung lượng); (5) hộp Tải cả kênh: thời lượng có biểu tượng đồng hồ, không nhầm giờ đăng. Sửa xong cho chủ dự án xem lại, review rồi hỏi gộp.

## Đã cân nhắc
- Gộp trước, sửa sau: nút vào bản chính sớm hơn nhưng TikTok vẫn chờ ~12 s — không chọn
- Chỉ sửa TikTok chờ, để các điểm nhỏ sau — chủ dự án chọn sửa luôn cả 4
