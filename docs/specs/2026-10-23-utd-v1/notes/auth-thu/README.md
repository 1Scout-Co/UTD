# THỬ — bằng chứng cho `../auth-plan.md` (không phải code UTD)

Các script nhỏ dùng để thử giả định của kế hoạch tải cần đăng nhập (2026-10-25). Chỉ dùng cookie giả trên
máy (127.0.0.1, `*.utd.test`), không đăng nhập tài khoản thật nào. Khi làm thật, các bài này chuyển thành test
Rust/Playwright trong repo; xoá thư mục này sau đó.

| File | Thử gì | Kết quả (Windows 11, yt-dlp.exe 2026.10.21, Chromium 156 của Playwright 1.64) |
|---|---|---|
| `authsite.py` | Trang cần đăng nhập: `/file.mp4` chỉ trả file khi có cookie `sid=ok`, không thì 302 về `/login`; `/hop.mp4` 302 sang host khác; `/domlogin` đặt cookie `Domain=utd.test` | In mỗi request: host, **tên** cookie (không in giá trị) |
| `cookiepipe.ps1` + `run-pipe-test.ps1` | yt-dlp đọc cookie từ named pipe `\\.\pipe\utd-cookie-<ngẫu nhiên>` thay vì file | `-Mode InOut`: thoát 0, site nhận `sid`; yt-dlp mở pipe 3 lần (os.access, đọc, ghi lại lúc thoát) từ tiến trình con PyInstaller. `-Mode Out` (pipe chỉ cho đọc): lấy được dữ liệu nhưng **crash lúc lưu cookie, thoát 1** |
| (lệnh tay, xem kế hoạch §5.1) | `--cookies file.txt` thường | yt-dlp **ghi đè file** lúc thoát, thêm cookie máy chủ đặt (`rot=1`) |
| (lệnh tay) | Cookie mồi `UTDCANARYq8Zr41xK` qua pipe rồi quét `%TEMP%`, `~/.cache/yt-dlp` | 172 file mới trong `%TEMP%`, 0 file chứa giá trị mồi; không sót thư mục `_MEI*` |
| (lệnh tay) | `yt-dlp.conf` cạnh exe chứa `--print-traffic` | stdout có dòng `Cookie: canary=…`; thêm `--ignore-config` thì không còn |
| `ext-cookies.cjs` + `ext-a/b/c` | `chrome.cookies.getAll` với quyền host / chỉ `activeTab` / quyền tuỳ chọn chưa cấp; `permissions.request` ngoài cú bấm | Có quyền host: trả cả cookie HttpOnly, `sameSite`, đúng path. Chỉ `activeTab`: rỗng. Chưa cấp: `chrome.cookies` không tồn tại. `permissions.request` không có cú bấm: "This function must be called during a user gesture" |
| `ext-sender.cjs` (GĐ0) | `background.js` chỉ nhận lệnh từ popup thật. Bản chép tạm của extension bỏ `key` và `nativeMessaging` nên không chạm native host thật | Tab mở `popup.html`: bị bỏ (`undefined`). Popup thật (`action.openPopup()` + CDP): được nhận |
| `pipe-e2e.py` (GĐ0) | App debug chạy với `UTD_DATA_DIR` riêng; gửi tin nhắn native liên tiếp qua pipe IPC đã đặt DACL + "first instance" | 4/4 tin `batch` được nhận; nối thẳng pipe với token sai bị từ chối; sau đó vẫn nhận tiếp |
| `ext-domain.cjs` + `ext-d/e` | Quyền `http://drive.utd.test/*` so với `http://*.utd.test/*` khi trang đặt cookie `Domain=utd.test` | Quyền một host: **chỉ thấy cookie riêng host, mất cookie cả miền**. Quyền `*.utd.test`: thấy cả hai, và thấy cookie miền khi đọc cho `usercontent.utd.test` |

Chạy lại: chép thư mục này vào `<tạm>\scripts`, tạo `<tạm>\run` và `<tạm>\ytdlp-dl` (tải `yt-dlp.exe` +
`SHA2-256SUMS` từ GitHub releases, so SHA-256), tạo `run\c1.orig`:

```text
# Netscape HTTP Cookie File
127.0.0.1	FALSE	/	FALSE	0	sid	ok
```

rồi `py -3 -I scripts\authsite.py 8766` (nền), `powershell -File scripts\run-pipe-test.ps1`. Bài extension:
`py -3 -I scripts\authsite.py 8767` (nền), `node scripts\ext-domain.cjs <đường dẫn playwright-core> <tạm>`
(thư mục extension `ext-*` đặt ở `<tạm>`).
