# ADR-0006 — Apache-2.0 + open core

## Bối cảnh

Dự án cần được dùng/đóng góp rộng, tránh copy code GPL, nhưng vẫn có đường Pro cho team/remote/support sau này.

## Quyết định

- Core (engine, UI, extension, media, Guard, Doctor) phát hành **Apache-2.0**, DCO, giữ trademark/name.
- Không copy code GPL; tham khảo pattern từ AB/Rayburst/Motrix theo license tương ứng, giữ NOTICE.
- Pro sau này chỉ cho team policy, remote/headless web UI, send-to-device, support/SLA; không khoá tính năng an toàn.
- Sidecar GPL chạy ngoài, không bundle.

## Hệ quả

- Dễ adoption và tích hợp; có thể bị fork đóng, bảo vệ bằng trademark và chất lượng.
- Cần NOTICE/THIRD_PARTY_LICENSES và privacy policy cho extension.

## Trạng thái

Accepted
