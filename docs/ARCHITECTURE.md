# Kiến trúc

> Bản đồ để người mới (và AI) nắm dự án trong 5 phút. Giữ ≤ 80 dòng, chỉ gọi tên file, không dán link.
> Luật máy đọc nằm ở `docs/arch.json`. Đổi module thì sửa CẢ HAI chỗ rồi chạy
> `node ~/.claude/hooks/arch.mjs sync` (kiểm hai bên khớp nhau và khớp thư mục thật).

## Bài toán

Webapp mới tạo: màn đầu (S-01) còn trống, chưa có dữ liệu hay tính năng nào. Thay bằng bài toán thật của app khi viết SPEC.

## Codemap

Mỗi module một dòng: đường dẫn trong dấu huyền · làm gì · KHÔNG làm gì. Thêm module mới = thêm một dòng ở đây
và một mục trong `docs/arch.json`.

<!-- codemap:start -->
- `src/components` — Thành phần giao diện của shadcn (nút, ô nhập…); KHÔNG chứa logic nghiệp vụ.
- `src/features/home` — Màn đầu (S-01, giữ chỗ): chỉ hiện tên app; thay bằng màn thật khi có SPEC; KHÔNG gọi dịch vụ ngoài từ ui, KHÔNG import tính năng khác.
- `src/lib` — Hàm tiện ích của shadcn (cn); KHÔNG chứa logic nghiệp vụ.
- `src/shared` — Code dùng chung (kiểu dữ liệu, tiện ích, thành phần giao diện chung). KHÔNG biết tính năng nào.
<!-- codemap:end -->

## Bất biến

Những điều luôn đúng, nêu bằng sự vắng mặt (máy kiểm trong `docs/arch.json`).
- Tính năng / miền này không biết tính năng / miền kia, trừ khi `mayDependOn` ghi rõ.
- Lõi nghiệp vụ không biết database, HTTP, giao diện.
- (Thêm điều bất biến riêng của dự án.)

## Ranh giới

- Vào: (giao diện, API, lệnh… ai gọi vào hệ thống.)
- Ra: (database, dịch vụ ngoài… hệ thống gọi ra đâu; chỉ gọi trong lớp api / store.)
- Hợp đồng: (nơi định nghĩa kiểu dữ liệu và API mà hai bên cùng tin.)

## Cắt ngang

- Đăng nhập / quyền: (ở đâu.)
- Ghi log và lỗi: (ở đâu, định dạng nào.)
- Cấu hình và khoá bí mật: (đọc từ đâu; không ghi vào code.)

## Vỡ đầu tiên ở 10x

(Nếu lượng người dùng / dữ liệu tăng gấp 10 lần, chỗ nào vỡ trước và cách xử lý dự kiến.)
