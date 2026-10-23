# ADR-0005 — Local-first security

## Bối cảnh

Ứng dụng nhận URL/cookie từ trình duyệt, ghi file vào ổ đĩa và có thể bị lợi dụng để mở file độc hại. Người dùng cần tin cậy, không tài khoản.

## Quyết định

- IPC: named pipe ACL theo user + token; extension chỉ được gọi host theo allowlist ID và schema.
- Cookie/referer/UA chỉ gửi khi opt-in cho đúng request; cookie memory-only, không ghi disk/log.
- Không telemetry, không server v1; TLS strict; không mở cổng localhost.
- Quarantine file nguy hiểm, giữ MOTW, hiện chữ ký/publisher, không auto-run; hash verify.
- Installer/updater ký; log redaction.

## Hệ quả

- Tăng ma sát ở extension/người dùng nhưng bảo vệ dữ liệu; Chrome Web Store cần privacy policy rõ.
- P-01..P-04 phải có test chặn quyền và bằng chứng "đỏ khi tắt" theo flow-qa.

## Trạng thái

Accepted
