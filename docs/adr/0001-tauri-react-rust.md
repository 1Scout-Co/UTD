# ADR-0001 — Tauri 2 + React UI + Rust engine

## Bối cảnh

UTD cần app desktop Windows-first, macOS sau; UI nhiều dữ liệu (table, media, dialog), extension trình duyệt, engine tải file lớn và yêu cầu native UI không bắt buộc.

## Quyết định

Dùng **Tauri 2** làm shell desktop, **React + TypeScript + Tailwind + shadcn** cho UI, **Rust** cho engine. Thiết kế đã chốt theo hướng Soniox/Inter: nền sáng xanh nhạt, panel trắng, table chuyên nghiệp, segmented tabs.

## Hệ quả

- UI phát triển nhanh, dùng WebView2 trên Windows và WKWebView trên macOS.
- Engine native, không phụ thuộc runtime Electron; installer nhẹ hơn.
- Không chọn Qt/Avalonia/Wails cho v1; nếu Rust làm chậm tiến độ có thể thay engine bằng Go sidecar sau IPC mà không đổi UI.
- Phải ký installer/updater và xử lý WebView2 runtime.

## Trạng thái

Accepted
