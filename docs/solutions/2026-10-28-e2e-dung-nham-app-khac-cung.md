---
dang_loi: e2e-dung-nham-app-khac-cung-cong
tom_tat: "Playwright tái dùng dev server cổng 5173 ⇒ chạy nhầm app khác đang mở trên máy, bài giao diện đỏ giả"
tags: [playwright, e2e, flow-check, port]
lan: 1
may_kiem: none
nguon: "claude/social-bulk-e821de 2026-10-28"
ngay: 2026-10-28
ra_soat: 2026-10-28
tai_phat: 0
vong_sua: 0
bao_nham: 0
vi_sao_khong_suy_ra: "playwright.config.ts đúng (reuseExistingServer + PORT mặc định 5173); lỗi chỉ do máy đang có app khác (\"runto english\") nghe cổng 5173 — đọc code không thấy, bài đỏ trông như lỗi thật (không thấy heading, axe báo nút không tên)."
---

# Playwright tái dùng dev server cổng 5173 ⇒ chạy nhầm app khác đang mở trên máy, bài giao diện đỏ giả

## Chuyện gì
4 bài S-05/S-61/S-63 đỏ vì không thấy hộp 'Tải hàng loạt'; flow-check axe + ui-rules đỏ ở titlebar/settings. error-context.md cho thấy trang là app khác. Chạy lại với PORT=5174 thì 19/19 và axe/ui-rules đạt.

## Vì sao không suy ra được từ code
playwright.config.ts đúng (reuseExistingServer + PORT mặc định 5173); lỗi chỉ do máy đang có app khác ("runto english") nghe cổng 5173 — đọc code không thấy, bài đỏ trông như lỗi thật (không thấy heading, axe báo nút không tên).

## Cách tránh
Bài giao diện đỏ hàng loạt ở chỗ không đụng ⇒ mở error-context.md xem trang có phải UTD không; kiểm cổng: Get-NetTCPConnection -State Listen -LocalPort 5173. Có app lạ ⇒ đặt PORT=<cổng trống> cho cả pnpm test:e2e lẫn flow-check (và stamp test).
