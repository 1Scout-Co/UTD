---
dang_loi: e2e-trang-gia-bo-sot-mang-xa-hoi-that
tom_tat: "Tải hàng loạt: e2e với Facebook/Instagram giả đều xanh nhưng thử thật trên Chrome đã đăng nhập lộ 3 lỗi; cách tự thử thật"
tags: [social-bulk, extension, facebook, thu-that, cdp]
lan: 1
may_kiem: test
nguon: "claude/social-bulk-e821de 2026-10-28"
ngay: 2026-10-28
ra_soat: 2026-10-28
tai_phat: 0
vong_sua: 0
bao_nham: 0
vi_sao_khong_suy_ra: "Dữ liệu thật khác dữ liệu giả ở chỗ bài thử không mô phỏng: danh sách video Facebook chỉ kèm bản SD, người dùng bấm lại trên cùng tab (bộ đọc trong trang còn nhớ mục cũ), chrome.i18n cần truyền số. Cách nối vào Chrome thật của chủ dự án (cổng gỡ lỗi Chrome cho phép, DevToolsActivePort) không nằm trong repo."
---

# Tải hàng loạt: e2e với Facebook/Instagram giả đều xanh nhưng thử thật trên Chrome đã đăng nhập lộ 3 lỗi; cách tự thử thật

## Chuyện gì
e2e 19/19 xanh. Thử thật: bấm lần hai cùng tab ⇒ Instagram/Threads trống, video Facebook mất link; nhãn 'Đang thu  ảnh/video' thiếu số; video fanpage về 640×360 vì danh sách chỉ có SD. Đã sửa + thêm e2e mô phỏng đúng dữ liệu thật.

## Vì sao không suy ra được từ code
Dữ liệu thật khác dữ liệu giả ở chỗ bài thử không mô phỏng: danh sách video Facebook chỉ kèm bản SD, người dùng bấm lại trên cùng tab (bộ đọc trong trang còn nhớ mục cũ), chrome.i18n cần truyền số. Cách nối vào Chrome thật của chủ dự án (cổng gỡ lỗi Chrome cho phép, DevToolsActivePort) không nằm trong repo.

## Cách tránh
Trước khi hỏi gộp tính năng chạm mạng xã hội thật: thử thật. Chrome 144+ của chủ dự án bật chrome://inspect#remote-debugging ⇒ đọc %LOCALAPPDATA%/Google/Chrome/User Data/DevToolsActivePort, playwright chromium.connectOverCDP('ws://127.0.0.1:<port><path>') (chủ dự án bấm Allow một lần; giữ MỘT kết nối bằng script nền). App: WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333 rồi connectOverCDP. Gọi socialStart(tab) trong service worker tiện ích (mở popup.html rồi chrome.runtime.reload() để nạp bản mới). Chỉ mở tab mới có dấu #utd-thu, đóng khi xong; tắt app có cổng gỡ lỗi và mở lại bình thường.
