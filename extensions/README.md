# UTD browser extension

Gửi link/video/ảnh từ trình duyệt sang UTD bằng menu chuột phải. Extension chỉ gửi URL HTTP/HTTPS; không gửi cookie, header đăng nhập hay nội dung trang.

## Bật tích hợp trong UTD

1. Mở UTD → **Cài đặt** → **Tích hợp trình duyệt** → **Bật tích hợp**.
2. Bấm **Mở thư mục tiện ích**. UTD giải nén extension vào `%LOCALAPPDATA%\UTD-Data\extension`.
3. Nạp extension:
   - **Chrome / Edge / Brave**: mở `chrome://extensions` hoặc `edge://extensions`, bật Developer mode, chọn **Load unpacked**, trỏ vào thư mục `chromium`.
   - **Firefox**: mở `about:debugging` → This Firefox → **Load Temporary Add-on**, chọn `firefox/manifest.json`.

Sau đó chuột phải vào link/video/ảnh → **Tải bằng UTD**.

## Thành phần

- `chromium/manifest.json` — MV3, ID cố định `aicoifnjolbpaollfoigidpecmpebeon`.
- `firefox/manifest.json` — MV3 event page, ID `utd@utd.app`.
- `background.js` — context menu + native messaging.
- `popup.html/js` — gửi trang đang mở; công tắc "Tự bắt link tải" (S-76).
- `takeover.js` — tự bắt link tải kiểu IDM: trình duyệt bắt đầu tải ⇒ tạm dừng ⇒ hỏi UTD; UTD nhận thì huỷ bản của trình duyệt, không thì trình duyệt tải tiếp. `takeover-keys.js` (đăng ký lúc chạy): giữ Alt khi bấm để trình duyệt tự tải lần đó.
- Native host `app.utd.desktop` do UTD cài ở `%LOCALAPPDATA%\UTD-Data\native-messaging`, trỏ tới `utd-native-host.exe` (bản sao của app, chạy ở chế độ host).
- Host đọc `%LOCALAPPDATA%\UTD-Data\ipc.json` (token + tên pipe ngẫu nhiên mỗi lần mở app), nối named pipe nội bộ của UTD, và UTD kiểm tra token trước khi nhận URL.

## Giới hạn v1

- Chưa có trên Chrome Web Store / addons.mozilla.org; phải nạp thủ công.
- Cookie đăng nhập chỉ gửi kèm cú bấm của người dùng (docs/specs/2026-10-23-utd-v1/notes/auth-plan.md); takeover (2026-10-26) theo cùng luật.
- Firefox chưa được kiểm thử tự động trong repo này; manifest và API dùng chung với bản Chromium.
