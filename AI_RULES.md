# AI_RULES — webapp utd

Luật cho AI sửa app này. Người dùng không viết code: giữ giao diện ít chữ, mỗi màn một việc chính (CLAUDE.md của người dùng).

## Nền tảng — đã ghim, KHÔNG đổi, KHÔNG thêm framework khác
- Vite + React + TypeScript, Tailwind CSS v4, shadcn/ui (nền Base UI), icon lucide-react.
- Kiểm thử: Playwright + axe (`pnpm test:e2e`). Gói quản lý: pnpm.
- Thêm thư viện mới (npm hoặc Rust crate): người dùng đã cho phép thêm khi có lợi rõ ràng, không cần hỏi lại (2026-10-23). Vẫn phải: ưu tiên thứ đã có sẵn (kể cả phụ thuộc gián tiếp trong `Cargo.lock`/`pnpm-lock.yaml`), cho qua cổng thư viện của kit (`node ~/.claude/hooks/deps-check.mjs --vet npm:<tên>`), và ghi lý do vào báo cáo.
- Người dùng cho phép AI sửa luật trong file này khi cần (2026-10-23); sửa thì ghi ngày + lý do ngay tại dòng đổi.

## Dùng gì cho việc gì
| Việc | Dùng | Không |
|---|---|---|
| Nút, ô nhập, hộp thoại | `src/components/ui/*` (shadcn). Thêm: `pnpm dlx shadcn@4.21.4 add <tên>` | tự viết lại, thêm thư viện giao diện khác |
| Icon | `lucide-react`, luôn kèm nhãn chữ (trừ nhà · in · tìm kiếm · đóng (X)) | icon trần |
| Kiểu dáng | class Tailwind | CSS tự viết, style inline |
| Gọi dữ liệu / dịch vụ ngoài | `src/features/<tên>/api` | gọi từ `ui` |
| Tiền, ngày | hàm định dạng chung trong `src/shared` (tạo khi cần) | định dạng rải rác |

## Cấu trúc (docs/arch.json là luật máy đọc)
- Mỗi tính năng một thư mục `src/features/<tên>/{ui,model,api}`; tính năng KHÔNG import tính năng khác; dùng chung đặt ở `src/shared`.
- Màn đầu `src/features/home` (S-01) chỉ là chỗ giữ chỗ, không có dữ liệu mẫu: làm màn thật thì thay nó, không dựng thêm bên cạnh.
- Thêm tính năng ⇒ thêm module vào `docs/arch.json` + một dòng Codemap trong `docs/ARCHITECTURE.md`, rồi `node ~/.claude/hooks/arch.mjs sync`.

## Giao diện
- Mỗi màn đúng MỘT nút chính (`data-primary` hoặc nút shadcn mặc định); nhãn 1–3 chữ, động từ ("Lưu", "Thêm từ"); không "OK/Có/Không/Xác nhận".
- Việc không quay lại được (xoá, tiền, quyền): nút nêu rõ ("Xoá từ"); việc hoàn tác được: nút Hoàn tác, không hộp xác nhận.
- Mỗi trạng thái màn trong SPEC có mã `S-nn`: gắn `data-s="S-nn"` vào phần tử gốc của trạng thái và ghi mã trong tên test e2e.
- Kiểm: `node ~/.claude/hooks/ui-check.mjs` (đọc `.claude/ui.json`).

## Bí mật và header
- Khoá bí mật chỉ ở `.env.local` / biến của nền tảng; biến `VITE_*` vào trình duyệt nên KHÔNG đặt khoá bí mật ở đó. Người dùng tự dán khoá, AI không nhập hộ.
- `public/_headers` là hàng rào trình duyệt: thêm tên miền dữ liệu vào `connect-src` khi cần; KHÔNG nới `script-src`, KHÔNG thêm `'unsafe-eval'`.
- Dữ liệu từ người dùng: kiểm ở máy chủ, truy vấn tham số hoá; không ghép chuỗi vào SQL.
- Ghi nhật ký chỉ qua `safeLog` (`src/shared/safe-log.ts`: chỉ trường trong danh sách cho phép được ghi, phần còn lại bị che). KHÔNG `console.log` thẳng mật khẩu, token, khoá, email hay chữ người dùng nhập; code máy chủ cũng ghi log qua hàm tương đương. Máy kiểm `log-secrets` quét log của bài thử.

## Lệnh
- Chạy thử: `pnpm dev` (http://localhost:5173) · Test: `pnpm test:e2e` · Kiểm tổng: `node ~/.claude/hooks/flow-check.mjs --dir .`
- Cổng đọc từ biến `PORT` (mặc định 5173): chạy nhiều bản song song thì mỗi bản một `PORT`.
- Bài thử nhanh sau phát hành: test gắn `{ tag: '@smoke' }` CHỈ ĐỌC (mở màn, nhìn; không bấm/nhập/ghi dữ liệu). Chạy vào bản đã đưa lên: `BASE_URL=<địa chỉ>` (và `SMOKE_EXTRA_HTTP_HEADERS='{"tên":"giá trị"}'` nếu cần header thêm) rồi `pnpm exec playwright test --grep @smoke`. Màn thật thay màn đầu ⇒ giữ ít nhất một test `@smoke` cho màn đó.
- Máy kiểm đã cấu hình sẵn (`eslint.config.js`, `knip.jsonc`, `.size-limit.json`, `osv-scanner.toml`): sửa code cho đạt, đừng nới cấu hình để lách; cần nới thì hỏi người dùng.
  - 2026-10-23: nâng giới hạn JS trong `.size-limit.json` 150 → 200 kB (gzip), người dùng cho phép sửa luật. Lý do: app desktop nạp mã từ đĩa chứ không qua mạng; giới hạn cộng mọi file JS nên tách lazy không giảm được; bản trước đợt này đã 158 kB. Vượt 200 kB thì cắt code, không nới tiếp.
