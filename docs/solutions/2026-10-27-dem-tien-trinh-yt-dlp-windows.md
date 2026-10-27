---
dang_loi: dem-tien-trinh-yt-dlp-windows
tom_tat: "yt-dlp.exe trên Windows (PyInstaller một file) chạy 1 lần hiện 2 tiến trình cùng dòng lệnh (cha + con) — đếm tiến trình phải xét ParentProcessId"
tags: [yt-dlp, qa, windows]
lan: 1
may_kiem: none
nguon: "QA lượt 5 nhánh fb-media (M-3), kiểm lại 2026-10-27 nhánh yt-probe"
ngay: 2026-10-27
ra_soat: 2026-10-27
tai_phat: 0
vong_sua: 0
bao_nham: 0
vi_sao_khong_suy_ra: "Code app chỉ spawn yt-dlp một lần; việc bản .exe tự mở tiến trình con là hành vi của bộ đóng gói PyInstaller bên ngoài repo, không thấy được khi đọc code hay test."
---

# yt-dlp.exe trên Windows (PyInstaller một file) chạy 1 lần hiện 2 tiến trình cùng dòng lệnh (cha + con) — đếm tiến trình phải xét ParentProcessId

## Chuyện gì
QA ghi 'mỗi lần mở bảng có 2 tiến trình yt-dlp giống hệt chạy song song' (cách nhau ~20 ms) và đề xuất thành việc sửa riêng; thật ra là cha (≈8 MB) + con (≈79 MB) của cùng một lần chạy.

## Vì sao không suy ra được từ code
Code app chỉ spawn yt-dlp một lần; việc bản .exe tự mở tiến trình con là hành vi của bộ đóng gói PyInstaller bên ngoài repo, không thấy được khi đọc code hay test.

## Cách tránh
Khi bắt tiến trình yt-dlp (watchproc, Get-CimInstance Win32_Process) luôn ghi ParentProcessId; chỉ tính là chạy trùng khi có hai tiến trình mà cha đều là app.exe.
