# QA — Tải hàng loạt Facebook / Instagram / Threads (nhánh claude/social-bulk-e821de)

Commit đã kiểm bằng máy: `1edb841` (2026-10-28). Thử thật trên Chrome của chủ dự án: bản dựng `290b3d5` (nhịp Facebook
mới). `1edb841` chỉ đổi cách dựng nhãn trên trang (DOM thay innerHTML), thêm log gỡ lỗi ở các `catch`, tách `UrlPreview`
sang file riêng; không đổi hành vi.

User duyệt giao diện: có (2026-10-28, hộp "Bulk download" khi lấy danh sách trang Facebook NASA: đầu hộp + Stop, chip
Photos, lọc All/Latest/By date, danh sách có ảnh nhỏ, nút Download; chủ dự án chọn "Nhanh hơn, giao diện ổn")

## Máy kiểm
- Test đủ bộ (dấu test, `PORT=5174` vì cổng 5173 của máy đang có app khác): `cargo test` app 311, utd-engine, utd-mp4
  19, clippy 3 crate `-D warnings`, `pnpm lint` (i18n 16 thứ tiếng), typecheck, build, Playwright 177/177.
- e2e `social-bulk.spec.ts` 19/19: Instagram (Tin, tin nổi bật, reels, bài viết, bỏ chọn nhóm, 429, 401, lỗi một phần,
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

## Chưa thử thật
Nhóm Facebook, Threads, Instagram trên bản `290b3d5` trở đi (chỉ có e2e với trang giả). Rủi ro còn lại ghi ở
`notes/social-bulk.md` (Meta đổi `doc_id` / cờ ⇒ phần đó báo danh sách có thể chưa đủ).
