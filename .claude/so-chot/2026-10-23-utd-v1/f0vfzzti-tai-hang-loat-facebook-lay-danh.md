---
ma: f0vfzzti
tom_tat: "Tải hàng loạt Facebook lấy danh sách nhanh như tiện ích mẫu: nghỉ ~0,3–0,6 giây giữa các lần hỏi, không trần 200 lần/10 phút; Instagram và Threads giữ nhịp 1–2 giây + trần 200/10 phút"
pham_vi: 2026-10-23-utd-v1
trang_thai: dang-dung
luc: 2026-10-28 10:55
ai_quyet: ban
lien_quan: [tig802he]
---

# [f0vf] Tải hàng loạt Facebook lấy danh sách nhanh như tiện ích mẫu: nghỉ ~0,3–0,6 giây giữa các lần hỏi, không trần 200 lần/10 phút; Instagram và Threads giữ nhịp 1–2 giây + trần 200/10 phút

## Vì sao
Chủ dự án thử trang NASA thấy danh sách hiện chậm (~150–200 ảnh/phút, trang lớn chững lại sau ~1.600 ảnh). Social Bulk Downloader mặc định không nghỉ, SCrawler nghỉ 1 s, cả hai không đặt trần cho Facebook; Instagram chặn gắt hơn nên giữ nhịp cũ.

## Áp dụng thế nào
social-bulk.js: SocialJob.pace() với nền tảng facebook nghỉ SOCIAL_FB_GAP_MS–2×SOCIAL_FB_GAP_MS (300–600 ms) và bỏ qua cửa sổ 200 yêu cầu/10 phút; Facebook báo tạm chặn (1675004/429) vẫn dừng ngay, giữ phần đã lấy.

## Đã cân nhắc
- Vừa phải: nghỉ 1 s như SCrawler, giữ trần 200/10 phút — nhanh ~1,5 lần, trang lớn vẫn chững; không chọn
- Giữ nhịp 1–2 s + trần — an toàn nhất nhưng chậm; không chọn
