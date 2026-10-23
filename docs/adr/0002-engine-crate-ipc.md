# ADR-0002 — Engine crate tách, in-process v1

## Bối cảnh

Engine tải cần test độc lập với GUI, có thể tách daemon/headless sau, nhưng v1 cần ít process và cài đặt đơn giản.

## Quyết định

Tạo crate `crates/utd-engine` độc lập với Tauri. V1 gọi engine **in-process** qua API Rust; Tauri command/event chỉ là lớp mỏng. Giữ boundary JSON/typed để sau này tách daemon hoặc CLI.

## Hệ quả

- Test engine bằng `cargo test` không cần mở app.
- Crash engine ở v1 làm sập app, nhưng journal SQLite phục hồi được; daemon tách sau nếu cần.
- Native messaging host là binary riêng, giao tiếp với app qua named pipe.

## Trạng thái

Accepted
