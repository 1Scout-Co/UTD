# QA — Tải hàng loạt Facebook / Instagram / Threads (nhánh claude/social-bulk-e821de)

Commit đã kiểm bằng máy: `e06cfc7` (2026-10-28; sau `20540f2` sửa 3 lỗi tìm ra khi thử thật, xem bảng dưới). Thử thật trên Chrome của chủ dự án: bản dựng `290b3d5` (nhịp Facebook
mới). Sau đó: `1edb841` chỉ đổi cách dựng nhãn trên trang (DOM thay innerHTML), thêm log gỡ lỗi ở các `catch`, tách
`UrlPreview` sang file riêng, không đổi hành vi; `20540f2` sửa 3 MAJOR + 1 MINOR của review (nhận đúng hồ sơ / nhóm theo
đường dẫn, 429 khi tải trang hồ sơ ⇒ bị chặn tạm, cờ Dừng của nhãn không dính sang lượt sau, hộp báo tiện ích khi quay về
tất cả) — mỗi lỗi có bài e2e đỏ trước khi sửa, không đổi giao diện.

User duyệt giao diện: có (2026-10-28, hộp "Bulk download" khi lấy danh sách trang Facebook NASA: đầu hộp + Stop, chip
Photos, lọc All/Latest/By date, danh sách có ảnh nhỏ, nút Download; chủ dự án chọn "Nhanh hơn, giao diện ổn")

## Máy kiểm
- Test đủ bộ (dấu test, `PORT=5174` vì cổng 5173 của máy đang có app khác): ở `1edb841` `cargo test` app 311, utd-engine,
  utd-mp4 19, clippy 3 crate `-D warnings`, `pnpm lint` (i18n 16 thứ tiếng), typecheck, build, Playwright 177/177; chạy
  lại đủ bộ cho commit cuối khi đóng dấu.
- e2e `social-bulk.spec.ts` 22/22 ở `e06cfc7` (thêm: nhận đúng hồ sơ / nhóm + 429, nhãn Dừng; S-61 kiểm báo tiện ích khi
  quay về tất cả; bấm lần hai cùng tab đủ danh sách + nhãn có số; video fanpage danh sách SD ⇒ hỏi lấy HD): Instagram (Tin, tin nổi bật, reels, bài viết, bỏ chọn nhóm, 429, 401, lỗi một phần,
  Dừng), Threads (hovercard + trang bài, không chuyển trang), Facebook hồ sơ (Tin, tin nổi bật, ảnh, reels, video fanpage,
  NDJSON + `for (;;);`), Facebook nhóm (ảnh qua trang ảnh, 1675004 ⇒ dừng, giữ phần đã có), nhịp gọi (Facebook 0,3–0,6 s
  không trần; Instagram / Threads 1–2 s, đủ 200 / 10 phút thì chờ), S-05 / S-61 / S-63 phía app.
- flow-check (cổng riêng): axe, ui-rules, eslint, tsc, depcruise, arch-sync, size-limit, web-headers, log-secrets ĐẠT.
  KHÔNG ĐẠT đều có từ trước trên main hoặc do máy, không ở file nhánh:
  - knip: file trong `notes/auth-thu`;
  - jscpd: đoạn trùng trong `.github/workflows/ci.yml`;
  - osv-scanner: không tìm thấy nguồn gói;
  - secret-build: file .pdb quá lớn, 0 khoá;
  - sast-ast-grep: `src/shared/i18n/storage.ts`, `takeover-keys.js`, `background.js` (catch có sẵn trên main);
  - spec-codes: S-03…S-09 v.v. của SPEC chung;
  - arch-size: 4 file nhánh không đụng.
  Nhánh đã tự sửa phần của mình: 7 chỗ SAST trong `social-bulk.js`, `add-download-dialog.tsx` 416 → 334 dòng.
- deps-check: không có thư viện mới.

## Thử thật (chủ dự án, Chrome đã đăng nhập)

| Ngày | Bản | Mục | Kết quả |
|---|---|---|---|
| 2026-10-26 | trước khi làm lại | Instagram hồ sơ | ĐẠT |
| 2026-10-28 | `dc0a8dd` | Facebook NASA: danh sách ảnh hiện dần, chọn rồi Download | ĐẠT nhưng chậm (~150–200 ảnh/phút) ⇒ chốt [f0vf] |
| 2026-10-28 | `290b3d5` | Facebook NASA sau khi đổi nhịp | ĐẠT, nhanh hơn; giao diện ổn |

## Thử thật do captain chạy (chủ dự án nhờ "tự test", 2026-10-28)
Chrome đã đăng nhập của chủ dự án qua cổng gỡ lỗi Chrome cho phép (chỉ mở tab mới, đóng lại sau; tab của chủ dự án
không đụng), app release mở với cổng WebView2 riêng để đọc / bấm hộp danh sách; tắt cả hai sau khi thử. Gọi đúng hàm
menu "Tải hàng loạt bằng UTD" gọi (`socialStart`) cho tab thử.

| Bản | Mục | Kết quả |
|---|---|---|
| `20540f2` | Threads @nasa | ĐẠT: 206 bài (190 ảnh, 16 video) / 86 s, không báo thiếu; tải 3 ảnh gốc 3024×4032, 1279×1919, 4096×3891 |
| `20540f2` | Nhóm Facebook `aiartworksvn` | ĐẠT: nhận đúng nhóm (tên dạng chữ), 60 ảnh / 90 s có ngày; tải 3 ảnh 1542×2048, 1103×1426, 1080×2340 |
| `20540f2` | Instagram nasa | ĐẠT: 740 mục / 2 phút (3 Tin, 89 tin nổi bật, 648 reels); tải 2 video 720×1280 + 1 ảnh 1320×2346 |
| `20540f2` | Fanpage NASA (chỉ Video) | LỖI: nhãn trên trang thiếu số (MINOR); video lấy bản SD 640×360 (MAJOR); bấm lần hai cùng tab mất link (MAJOR) |
| `e06cfc7` | Fanpage NASA, bấm hai lần trên cùng tab | ĐẠT: cả hai lần 49/49 video có link HD, ngày, chú thích; nhãn "Đã gửi 65 ảnh/video"; tải video 720×1280 tên `2026-10-20 There's a lot of ground… 1863947697952668.mp4` |

Giới hạn thấy khi thử (ghi ở `notes/social-bulk.md`): fanpage NASA không có thẻ Reels; "Tất cả video" Facebook báo hết
trang ở 49; video Facebook tải thẳng tối đa 720p (bản cao hơn chỉ có dạng DASH, tiện ích mẫu cũng vậy).
