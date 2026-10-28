---
ma: tig802he
tom_tat: "Tải hàng loạt Facebook và Threads thu danh sách bằng cách hỏi thẳng trang (như Social Bulk Downloader / repo có sẵn), không tự cuộn hay chuyển tab"
pham_vi: 2026-10-23-utd-v1
trang_thai: dang-dung
luc: 2026-10-27 18:37
ai_quyet: ban
---

# [tig8] Tải hàng loạt Facebook và Threads thu danh sách bằng cách hỏi thẳng trang (như Social Bulk Downloader / repo có sẵn), không tự cuộn hay chuyển tab

## Vì sao
Cách cuộn trang chậm (Facebook ~7 ảnh/phút), chiếm tab, phải để tab trên màn hình và chưa thử thật; tiện ích mẫu chủ dự án gửi hỏi thẳng GraphQL cho cả Facebook lẫn Threads, Instagram đã làm vậy và chạy tốt.

## Áp dụng thế nào
Làm lại phần Facebook (ảnh, video, reels, Tin, Tin nổi bật, nhóm) và Threads trong social-bulk.js theo Social Bulk Downloader 1.8.8 và repo mở; mã câu hỏi để một chỗ như SOCIAL_IG_QUERIES; đổi mã ⇒ báo social.changed.

## Đã cân nhắc
- Giữ cách cuộn hiện tại rồi thử trước — chủ dự án bác: chậm, khổ
- Hỏi thẳng, hỏng thì quay về cuộn — nhiều việc nhất, chủ dự án không chọn
