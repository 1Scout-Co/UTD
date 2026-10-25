# Kế hoạch: tải video và tài liệu cần đăng nhập

Ngày 2026-10-25 · Nhánh `claude/auth-plan-6b2e91` (trên `main` bd1ef9c) · **Đã duyệt 2026-10-25:** chủ dự án chọn mọi
đề xuất ở §10 (Q1–Q10). Đang làm GĐ0. Các thử nghiệm nhỏ nằm ở `auth-thu/` (ghi rõ THỬ).

## 0. Tóm tắt

- **Nguồn cookie: chỉ extension** (`chrome.cookies` / `browser.cookies`). Extension chỉ đọc cookie bên trong một cú
  bấm của người dùng trên trình duyệt: menu chuột phải hoặc nút trong popup. App không bao giờ tự kéo cookie.
  Bất biến này giữ đúng P-02, và nó ngăn UTD thành cửa sau để mã độc lấy cookie mà Chrome đã khoá bằng
  App-Bound Encryption.
- **Quyền xin theo từng trang, khi cần.** Lần đầu một trang đòi đăng nhập, người dùng bấm "Dùng đăng nhập" trong
  popup. Chrome tự hiện hộp hỏi quyền cho trang đó. Từ lần sau trang ấy tự kèm cookie, và tắt được ngay trong
  popup. Phải xin quyền theo **tên miền gốc** (`*.google.com`), không theo host: đã thử thật, quyền một host
  làm mất cookie đăng nhập đặt cho cả miền (§4.2).
- **Cookie chỉ nằm trong RAM của app**, không vào journal, cài đặt, log hay giao diện WebView. Journal chỉ ghi
  dấu "task này cần đăng nhập ở google.com", không ghi giá trị. App khởi động lại thì task chờ "Cần đăng nhập":
  một lần bấm trong popup là tải tiếp.
- **Đưa cho yt-dlp bằng named pipe**, không ghi đĩa. Đã thử thật với yt-dlp.exe 2026.10.21: dùng
  `--cookies \\.\pipe\…`, thoát mã 0, site nhận đúng cookie. Quét `%TEMP%` không thấy giá trị cookie mồi.
  macOS dùng FIFO, cần thử trên Mac mini. Engine tự gửi header `Cookie` theo luật trình duyệt (domain, path,
  secure, SameSite) ở từng bước chuyển hướng, nên sang miền khác thì không mang theo.
- **Hai lỗ hổng có từ trước, sửa ngay ở giai đoạn 0:**
  - yt-dlp đang nạp file cấu hình của người dùng. Đã thử: một `yt-dlp.conf` có `--print-traffic` làm lộ dòng
    `Cookie:` ra stdout, và file cấu hình cũng có thể tự thêm `--cookies-from-browser`. Sửa: luôn chạy với
    `--ignore-config --no-plugin-dirs`.
  - Pipe IPC của app đang dùng quyền mặc định của Windows. Sửa: đặt DACL chỉ cho người dùng hiện tại.
- **Giai đoạn:**
  - 0: gia cố (½–1 ngày).
  - 1: MVP trên Windows cho Chromium và Firefox — link file trực tiếp, một video qua yt-dlp, khởi động lại, hết
    hạn.
  - 2: link trang Drive/OneDrive/Dropbox, hàng loạt và tải cả kênh dùng chung một lần cho phép, macOS, huy hiệu
    trên biểu tượng extension.
  - 3: takeover (AC-15) và nhập cookies.txt nếu chủ dự án muốn.
- **So với các repo khác** (Phụ lục C, đọc mã 23 repo): cách phổ biến lỏng hơn nhiều.
  - Trình quản lý tải có extension: xin quyền mọi trang, gửi cookie cho mọi download, lưu chuỗi cookie dạng chữ
    thường cạnh task, thường không bỏ cookie khi chuyển hướng sang miền khác.
  - App giao diện cho yt-dlp: đưa cookie bằng file cookies.txt nằm luôn trên đĩa.

  Kế hoạch học thêm 4 điểm: gửi kèm User-Agent (§4.2), kiểm yt-dlp đã thật sự đọc cookie (§5.1), danh sách miền
  nhạy cảm (Q10), takeover tạm dừng rồi trả lại trình duyệt (§4.4).
- Có **10 câu cần chủ dự án quyết** ở §10. Thử thật với YouTube giới hạn tuổi và Google Drive riêng cần chủ dự án
  tự đăng nhập: phiên này không đăng nhập tài khoản nào.

## 1. Phạm vi

| Loại | Ví dụ | Đường tải trong UTD | Cookie của miền | Ghi chú |
|---|---|---|---|---|
| Video giới hạn tuổi / hội viên | YouTube | yt-dlp `-J` → engine tải luồng googlevideo | `.youtube.com` (yt-dlp tự tính `SAPISIDHASH` từ `SAPISID`) | Luồng googlevideo là link ký, không cần cookie. Cookie Google bị DBSC buộc vào máy nên sống ngắn (§10, R3) |
| Video riêng tư / chỉ bạn bè | Facebook, Instagram, TikTok | yt-dlp → engine hoặc yt-dlp tải hết | `.facebook.com`, `.instagram.com`, `.tiktok.com` | Hỗ trợ của yt-dlp cho "chỉ bạn bè" không đều; rủi ro khoá tài khoản (R2) |
| File lưu trữ đám mây | Google Drive, OneDrive/SharePoint, Dropbox | Engine (link tải trực tiếp). Giai đoạn 2 thêm bộ đổi link trang → link tải | `.google.com`, `<tenant>.sharepoint.com`, `.dropbox.com` | Drive file lớn có trang "không quét được virus" (biểu mẫu `confirm`/`uuid`); Google Docs tải qua `export?format=` |
| Tài liệu trang nội bộ / link chỉ chạy khi đã đăng nhập | intranet, LMS, forum | Engine | host nội bộ, IP | NTLM/Kerberos ngoài phạm vi (AC-13); Basic auth để sau |
| Kênh / playlist riêng tư | TikTok riêng tư, playlist hội viên | yt-dlp liệt kê (S-60) → từng video | như dòng video | Một lần cho phép dùng chung cho cả kênh (§6) |
| DRM | Netflix… | — | — | Vẫn từ chối rõ (S-14, AC-17), cookie không đổi điều đó |

## 2. Ràng buộc trong SPEC và chỗ SPEC phải sửa

| Mục | Nội dung | Kế hoạch đáp ứng |
|---|---|---|
| P-02 | Extension chỉ thấy cookie của tab khi người dùng chủ động bấm tải; cookie chỉ ở bộ nhớ, không ghi đĩa/log, không gửi sang domain khác | Đọc cookie chỉ trong handler của cú bấm (§4). Cookie chỉ ở RAM: RAM của tiến trình app, RAM của native host trong vài mili giây, và yt-dlp nhận qua pipe. Engine gửi theo luật domain/path (§5.3) |
| P-04 | Log lọc cookie/token | Kiểu dữ liệu cookie không in được giá trị (`Debug` che). Test cookie mồi quét log, journal, file tạm (§9.3) |
| AC-14 | Chuột phải → nhận URL + headers/cookies theo opt-in | Tin nhắn native thêm trường `auth`, chỉ có khi trang đã được cho phép |
| AC-15 | Takeover hủy download của browser, chuyển sang UTD kèm cookie | Giai đoạn 3: `downloads` + cùng cơ chế cookie (§4.4) |
| AC-18 | Chưa opt-in thì không đọc/gửi cookie; opt-in rồi thì chỉ cho request đó, không ghi disk/log | Opt-in = cấp quyền cho trang trong popup. Trước đó extension không có quyền đọc, nên không thể đọc |
| AC-30 | Chứng minh bằng test: cookie không persist, log đã redact | Bộ test cookie mồi (§9.3) |
| S-14 | Lỗi đăng nhập/DRM: "Cho phép gửi cookie cho phiên này, mở trang đăng nhập, huỷ" | Giữ ba hành động; "cho phép" xảy ra trong popup (app không tự lấy được cookie) |
| S-63 | Kênh riêng tư: "UTD không gửi cookie" | Sửa: cùng luồng S-14 |
| §4 Guard | Strip Cookie/Authorization khi redirect sang domain khác | Cookie tính lại ở từng bước chuyển hướng theo domain-match; Authorization không nhận từ trình duyệt ở MVP |
| §2 Lưu trữ | "Không lưu cookie/token" | Journal chỉ có dấu `auth_sites`. Lưu ý: URL đã ký trong journal tự nó là token ngắn hạn (câu Q7) |

Sửa SPEC khi duyệt (chưa sửa trong phiên này):
- S-14 đổi "cho phiên này" theo câu Q1.
- Sửa S-63.
- Thêm S-70..S-73 (§8). Mã chọn ngoài khoảng S-40..S-59 đang chừa cho các phiên song song; chốt lại khi ghép.
- Thêm vào AC-18: "đọc cookie chỉ trong cú bấm trên trình duyệt".
- Bỏ câu trong `browser.manualNote` ("UTD không gửi cookie…").

## 3. Hiện trạng (main bd1ef9c)

| Phần | Hiện có | Liên quan tới đăng nhập |
|---|---|---|
| Extension `extensions/{chromium,firefox}` | MV3. Quyền `contextMenus, nativeMessaging, storage, activeTab, scripting`; không có `cookies`, không có quyền host. Gửi `{version:1, type:"download"\|"batch", url, pageUrl}` | `activeTab` không cho đọc cookie (thử thật §4.1). `runtime.onMessage` (`background.js:238`) không kiểm `sender` |
| Native host `src-tauri/src/native_host.rs` | Đọc 1 tin (≤ 1 MiB), kiểm schema, thêm token từ `ipc.json`, gửi một dòng JSON qua pipe; `log_line` chỉ ghi chữ lỗi vào `native-host.log` | Thêm trường `auth`. Không bao giờ ghi payload vào log |
| Pipe IPC `src-tauri/src/ipc.rs` | Windows: `ServerOptions::new()` (`ipc.rs:338`) không truyền security attributes ⇒ DACL mặc định của named pipe (Everyone/anonymous được đọc theo tài liệu Microsoft). Token 256 bit so sánh hằng thời gian. macOS: Unix socket 0600 | Trước khi cho cookie đi qua: DACL chỉ SID người dùng, `first_pipe_instance(true)`, `reject_remote_clients(true)` |
| Engine `crates/utd-engine` | `headers.rs` cấm `cookie`, `cookie2`, `authorization`, `proxy-*` (dòng 16–32). `request_headers` **lưu bền trong journal** (`db.rs:94`, cột `request_headers_json`). `transport.rs` gửi header riêng chỉ cùng origin, sang origin khác gửi lại không header | Cookie **không được** đi đường `request_headers` (sẽ vào SQLite). Cần kênh riêng chỉ ở RAM, tính theo domain-match chứ không theo origin (Drive: `drive.google.com` → `drive.usercontent.google.com` cần cookie `.google.com`) |
| Media `src-tauri/src/media/*` | yt-dlp gọi ở `probe.rs:83`, `resolve.rs:116`, `runner.rs:225`, `channel/mod.rs:185`, `channel/profile.rs:99`. Không có `--ignore-config`/`--no-plugin-dirs`. Lỗi đăng nhập: `classify_stderr` → `[media.login] … UTD chưa gửi cookie ở bản này.` (`probe.rs:565`) | `-J` có cookie thì mỗi format có trường **`cookies` chứa giá trị** (đã thử). `probe_cache` chỉ giữ `Stream` (không có cookie): giữ nguyên như vậy. `parts.rs:52` lọc header qua `allowed_request_headers` (bỏ Cookie): đúng |
| Tải cả kênh `media/channel` | `listing_args` không cookie; S-63 báo "riêng tư" | Thêm `--cookies <pipe>` khi có quyền |
| Log | `tauri_plugin_log` chỉ bật bản debug. Không chỗ nào log stdout/stderr của yt-dlp. Phiên "màn chi tiết" (`detail_log.rs`) bỏ query khỏi URL khi sao chép log | Thêm test: cookie mồi không có trong log/sự kiện chi tiết |

## 4. Lấy cookie thế nào

### 4.1 So sánh

| | (a) Extension đọc `cookies.getAll` lúc bấm | (b) yt-dlp `--cookies-from-browser` | (c) Người dùng nhập cookies.txt | (d) Takeover `chrome.downloads` |
|---|---|---|---|---|
| Quyền extension thêm | `cookies` (tuỳ chọn) + quyền host tuỳ chọn theo trang | Không | Không | `downloads`; muốn kèm cookie thì vẫn cần (a) |
| Cảnh báo Chrome | `cookies` một mình: không có. Một trang: "Read and change your data on {site}". Mọi trang: "Read and change all your data on all websites" | — | — | "Manage your downloads" |
| Firefox | `cookies` cấp im lặng. Quyền host: "Access your data for {domain}" / "for sites in the {domain} domain". Container: `storeId = tab.cookieStoreId`. Bật cách ly bên thứ nhất (FPI) thì bắt buộc `firstPartyDomain` | Chạy được: chép `cookies.sqlite` sang thư mục tạm (**ghi đĩa**), đọc **mọi** cookie | Như nhau | Không có `onDeterminingFilename`, chỉ `onCreated` |
| macOS | Như Windows (API của trình duyệt, không đụng Keychain) | Chrome: hỏi Keychain "Chrome Safe Storage". Safari: cần Full Disk Access | Như nhau | Như nhau |
| Chạy được hôm nay | **Có** (Chromium 156 thử thật: trả cả cookie HttpOnly; chỉ `activeTab` thì rỗng) | Chrome/Edge trên Windows: **không** (App-Bound Encryption từ Chrome 127, yt-dlp #10927 còn mở; khoá DB #7271 còn mở). Firefox: có | Có | Bắt download thì được; `DownloadItem` không có header/cookie, POST/blob không phát lại được |
| Phạm vi cookie | Chỉ cookie áp dụng cho URL đích | Toàn bộ hồ sơ trình duyệt | Cả file người dùng xuất | — |
| Đúng P-02 | Có | Không (đọc hết, ghi bản sao tạm) | Một phần (file nằm trên đĩa do người dùng tạo) | Có nếu đi kèm (a) |

**Chọn (a) làm đường chính.** (d) là cách *bắt* download, xây trên (a) ở giai đoạn 3. (c) chỉ làm nếu chủ dự án
muốn phục vụ người dùng không cài extension (Q4); khi đó đọc file vào RAM, không chép, và nhắc người dùng xoá file.
**Không làm (b)**: không chạy được với Chrome/Edge trên Windows, đọc toàn bộ cookie, ghi bản sao ra đĩa.

### 4.2 Chi tiết (a), đã thử thật

- Manifest dùng `optional_permissions: ["cookies"]` và `optional_host_permissions: ["https://*/*", "http://*/*"]`
  (Firefox MV3 cũng nhận khoá này; cần kiểm lại khi làm). Cài mới không hiện cảnh báo nào.
- **Phải xin theo tên miền gốc.** Chromium lọc cookie theo quyền với *miền của cookie*, không theo URL đang đọc.
  Bài `auth-thu/ext-domain.cjs`, trang đặt `dom` (`Domain=utd.test`) và `hostonly`:
  - Quyền `http://drive.utd.test/*` chỉ trả `hostonly@drive.utd.test`, mất `dom@.utd.test`.
  - Quyền `http://*.utd.test/*` trả cả hai, và trả `dom` khi đọc cho `usercontent.utd.test`.

  Cookie đăng nhập Google (`SID`, `__Secure-1PSID`…) đặt cho `.google.com`, nên phải xin
  `https://*.google.com/*`. Tên miền gốc (eTLD+1) cần danh sách hậu tố công khai (PSL). Extension không có API
  PSL, nên app tính bằng crate `psl` (thư viện mới, qua cổng `deps-check`) và đưa mẫu quyền cho popup.
  - `github.io`, `blogspot.com` (phần private của PSL) ⇒ `*.user.github.io`.
  - IP hoặc host nội bộ ⇒ đúng host đó.
- `permissions.request` bắt buộc có cú bấm. Đã thử: gọi trong service worker ngoài cú bấm bị lỗi "This function must
  be called during a user gesture". Bấm nút trong popup là trường hợp tài liệu ghi rõ, nên kế hoạch chỉ xin quyền
  từ popup. Gọi đồng bộ trong `contextMenus.onClicked` có thể cũng được (suy từ mã Chromium, chưa thử), nhưng
  không cần cho thiết kế này.
- Cách đọc trong extension:
  - Gọi `getAll({url: đích, storeId})`, và thêm `{url: pageUrl}` nếu cùng tên miền gốc.
  - `storeId` lấy từ cookie store của tab: Chrome dùng `getAllCookieStores` (tab ẩn danh có id `"1"`), Firefox
    dùng `tab.cookieStoreId`.
  - Không truyền `partitionKey`, nên chỉ có cookie không phân vùng: đúng cookie đăng nhập của trang chính.
  - Bỏ cookie `sameSite: "strict"` khi trang đang mở khác host với đích, vì trình duyệt cũng không gửi Strict khi
    điều hướng chéo trang.
  - Trần: 300 cookie, 64 KiB.
- **Gửi kèm User-Agent của trình duyệt** (`navigator.userAgent`; P-02 đã cho phép gửi User-Agent).
  - Lý do: một số trang gắn phiên đăng nhập với User-Agent; cookie đi với User-Agent khác có thể bị coi là phiên
    lạ. Seal làm đúng như vậy: `--cookies` đi cùng `--add-header User-Agent:<UA của WebView>` (Phụ lục C).
  - App dùng User-Agent đó làm header `User-Agent` của task trong engine, và `--user-agent` cho yt-dlp.
  - User-Agent không phải bí mật, nên được lưu cùng task như các header khác (`request_headers`). Nhờ vậy sau
    khi khởi động lại vẫn dùng đúng User-Agent đó.
- **Miền nhạy cảm (Q10):** nếu chủ dự án đồng ý thì làm như Motrix 2.x.
  - Có danh sách hậu tố tên miền: `gov`, `mil`, ngân hàng, `paypal.com`…
  - Các trang này không kèm cookie, kể cả khi trang đã được cho phép, trừ khi người dùng xác nhận trong popup
    cho đúng lượt đó.
  - Danh sách đặt trong app (Rust), có test; extension hỏi app như với mẫu quyền.
- Cửa sổ ẩn danh/riêng tư: extension chỉ chạy ở đó khi người dùng tự bật. Cookie từ đó được đánh dấu `private` và
  bỏ khi task xong (§6).

### 4.3 Vì sao không cho app tự xin cookie

Nếu app gửi yêu cầu qua native host mà extension tự đọc cookie, không cần cú bấm, thì mã độc chạy cùng user chỉ
cần đọc `ipc.json` là giả được app. Khi đó nó lấy được cookie của mọi trang đã cấp quyền, vượt qua App-Bound
Encryption (cơ chế Chrome dựng ra để chặn chính việc này). Vì vậy luật cứng là extension chỉ đọc cookie trong
handler `contextMenus.onClicked`, hoặc khi người dùng bấm nút trong popup / thông báo. Tin nhắn từ app chỉ được
*báo* là đang chờ, không bao giờ kích hoạt việc đọc. Test sẽ khoá luật này (§9.3, T-E2).

### 4.4 Takeover (giai đoạn 3, AC-15)

- **Bắt download:** `downloads.onCreated` (cả hai trình duyệt) → `downloads.pause(id)` → hỏi UTD.
  - Nếu UTD dò được file (kèm cookie khi trang đã cho phép): `cancel` + `erase` bản của trình duyệt.
  - Nếu không: `resume`, trình duyệt tải tiếp như thường. Cách này giữ được download POST, blob: và link
    dùng-một-lần, những thứ UTD không phát lại được.
- **Bỏ qua:** `blob:`, `data:`, file nhỏ hơn ngưỡng.
- **Bằng chứng từ repo khác** (Phụ lục C) cho cách tạm dừng rồi trả lại:
  - motrix-webextension (bản cũ, `f8bc93d`) làm đúng như vậy: tạm dừng, gửi sang aria2, aria2 lỗi thì cho trình
    duyệt tải tiếp (`background.js` L62–L90).
  - AB Download Manager (extension `658a0c7`) huỷ, xoá khỏi lịch sử và xoá file ngay
    (`DownloadLinkInterceptor.ts` L517). Trên MV3, nếu app lỗi sau khi đã huỷ thì chỉ có chú thích "NO-OP for
    now" (L558): **mất luôn download**.
  - Aria2-Explorer (`5e97685`) dự phòng bằng `downloads.download({url})`: tải lại bằng GET, nên mất download
    POST và link dùng-một-lần.
- **Quyền:** cần thêm `downloads`. Muốn kèm cookie cho mọi trang thì phải có quyền mọi trang (cảnh báo "all
  websites"), nên chủ dự án cần quyết (Q5).

## 5. Đưa cookie cho yt-dlp và engine

### 5.1 Các cách đưa cho yt-dlp (đã thử với yt-dlp.exe 2026.10.21)

| Cách | Kết quả thử / lý do | Chọn |
|---|---|---|
| `--cookies <file tạm>` | yt-dlp **ghi đè file lúc thoát** (thêm cookie máy chủ vừa đặt: `rot=1`; README: "read cookies from and dump cookie jar in"). Crash giữa chừng thì file sót lại | Không, trừ dự phòng có cờ (dưới) |
| `--cookies \\.\pipe\<ngẫu nhiên>` (Windows) | **Chạy.** Tiến trình con PyInstaller mở pipe 3 lần: `os.access` (nối rồi đóng), đọc, ghi lại lúc thoát. Điều kiện: tạo sẵn ≥ 3 instance (instance bận ⇒ `[Errno 22]`); sau khi ghi thì **đóng handle chứ không `DisconnectNamedPipe`** (disconnect làm `ReadFile` của client lỗi ⇒ `[Errno 22]`); phải nhận cả lần ghi lại (pipe chỉ-đọc ⇒ yt-dlp crash lúc lưu cookie, thoát 1). Thoát 0, site nhận `sid`; cookie không mang sang host khác khi chuyển hướng; cookie mồi không có trong file nào của `%TEMP%` (172 file mới) | **Có (Windows)** |
| FIFO trong thư mục tạm 0700 (macOS) | Thứ tự xác định: `os.access` không mở file; đọc (ta mở FIFO để ghi) rồi lưu lúc thoát (ta mở để đọc, bỏ dữ liệu). `/dev/fd/N` bị loại vì lần ghi lại sẽ crash như pipe chỉ-đọc | **Có, cần thử trên Mac mini** |
| `--add-header "Cookie: …"` | Nằm trên dòng lệnh, nên tiến trình khác cùng user đọc được (Win32_Process.CommandLine), và lọt vào log tạo tiến trình (Event 4688 kèm dòng lệnh, Sysmon EID 1). yt-dlp đánh dấu deprecated, gắn cookie vào miền của URL, không nhận `Domain=` | Không |
| `--config-locations -` (stdin) + `--add-header` | Không lộ dòng lệnh nhưng vẫn đi đường header deprecated, không có phạm vi miền | Không |
| Plugin / API Python | Plugin chỉ có loại extractor/postprocessor, không có móc cookie; yt-dlp.exe là PyInstaller, không import được | Không |

Thiết kế pipe Windows (`src-tauri/src/auth/pipe.rs`):
- **Tạo pipe:**
  - Tên `utd-ck-<128 bit ngẫu nhiên>`.
  - `create_with_security_attributes_raw`, DACL chỉ cấp cho SID người dùng.
  - `first_pipe_instance(true)`: ai chiếm tên trước thì tạo lỗi, không bị cướp tên.
  - `reject_remote_clients(true)`.
  - 4 instance.
- **Mỗi lần nối:**
  - Lấy `GetNamedPipeClientProcessId` và chỉ phục vụ khi client là tiến trình yt-dlp app vừa sinh hoặc con của nó.
    Cha của tiến trình lấy qua Toolhelp; PyInstaller chạy lại chính nó thành tiến trình con.
  - Phân biệt đọc hay ghi bằng cách đua: chờ drain xong ⇒ client đọc, đóng handle. Đọc được byte ⇒ client ghi,
    đọc tới EOF rồi bỏ.
- **Kiểm yt-dlp đã thật sự đọc cookie.**
  - Vấn đề: yt-dlp gọi `os.access(path, R_OK)` trước khi nạp. Nếu bước đó trả `False` thì **bỏ qua cookie, không
    báo lỗi** (`yt_dlp/cookies.py` L107, commit `fa98866`), và chạy tiếp như chưa đăng nhập. Trang công khai thì
    vẫn ra kết quả, nên lỗi pipe sẽ bị che mất.
  - Cách chặn: pipe giữ cờ `loaded`, chỉ bật khi một client hợp lệ đã đọc trọn nội dung (drain xong).
    - Lệnh `-J` (Phân tích, `resolve`): yt-dlp thoát mà `loaded` chưa bật ⇒ bỏ kết quả, báo `auth.pipe`.
    - Lệnh tải bằng yt-dlp (`runner`): yt-dlp chỉ nạp cookie ngay trước request đầu tiên (`cookiejar` là
      `cached_property`). Vì vậy tới dòng tiến độ đầu tiên mà `loaded` chưa bật ⇒ dừng cả cây tiến trình
      (`kill_tree`), báo `auth.pipe`.
  - Không bao giờ để task có cookie chạy tiếp mà không có cookie.
- **Đóng:** pipe sống đúng bằng tiến trình yt-dlp; nội dung (`Zeroizing<Vec<u8>>`) xoá khi tiến trình thoát.
- **Lỗi:** mã `auth.pipe`, không âm thầm rơi về file.
- **Dự phòng** (chỉ khi yt-dlp đổi cách mở file, tắt mặc định):
  - Thư mục `<data>\auth-tmp` ACL chỉ chủ, file xoá ngay khi yt-dlp thoát.
  - Mở app thì quét dọn thư mục này.
  - Test sót file chạy như bài pipe.

### 5.2 Tham số yt-dlp

- **Mọi lệnh gọi yt-dlp:** `--ignore-config --no-plugin-dirs`.
  - Đã thử: `yt-dlp.conf` cạnh exe có `--print-traffic` ⇒ stdout có `Cookie: canary=…`; thêm `--ignore-config`
    thì hết.
  - Bên cạnh đó, file cấu hình của người dùng (`%APPDATA%\yt-dlp\config.txt`) hoặc của bản cài qua
    WinGet/Scoop có thể tự thêm `--cookies-from-browser`. Như vậy hôm nay UTD có thể gửi cookie mà người dùng
    không opt-in (AC-18).
- **Lệnh có cookie:**
  - Thêm `--cookies <pipe>`.
  - Cấm `--verbose`, `--print-traffic`, `--write-pages`, `--dump-pages`, `--load-pages`; có test khoá.
  - stdout `-J` có trường `formats[].cookies`: chỉ đọc vào RAM, gộp vào hũ cookie của lần cho phép (§5.3), không
    bao giờ log.
  - Không đưa cookie cho ffmpeg (ghép tại chỗ, không cần).

### 5.3 Engine gửi cookie

- **Kiểu dữ liệu mới** trong `crates/utd-engine/src/cookies.rs`:
  - `BrowserCookie {name, value: Zeroizing<String>, domain, host_only, path, secure, http_only, same_site, expires}`.
  - `CookieJar`: chỉ ở RAM, không derive `Serialize`, `Debug` in "3 cookie cho google.com".
  - `Credentials {jar: Arc<CookieJar>, cross_site: bool}`.
- **Gắn vào request:**
  - `DownloadRequest.credentials: Option<Credentials>`, không serde.
  - `DownloadTask.auth: Option<AuthMarker {sites}>` thì lưu journal (cột mới `auth_json`), chỉ tên miền.
- **Header cho từng URL**, theo RFC 6265 §5.4:
  - domain-match (host-only phải trùng host);
  - path-match; `Secure` chỉ qua https; bỏ cookie hết hạn;
  - path dài xếp trước;
  - bỏ Strict khi `cross_site`.
  - `HeaderValue::set_sensitive(true)`: không hiện trong `Debug` của hyper, HTTP/2 HPACK đánh dấu không-index.
- **Chuyển hướng:**
  - Có credentials thì client không tự theo chuyển hướng (`Policy::none`). `Transport::send` tự theo tối đa 10
    bước và tính lại `Cookie` ở mỗi bước.
  - Hệ quả: Drive (`drive.google.com` → `drive.usercontent.google.com`) vẫn có cookie `.google.com`; trang lạ
    không nhận cookie nào.
  - `Authorization` (nếu sau này có, ví dụ Basic auth nội bộ) bỏ ngay khi đổi origin, theo luật fetch.
  - `request_headers` hiện có giữ luật cùng-origin như cũ.
- **`Set-Cookie`** máy chủ trả về thì gộp vào hũ trong RAM, như trình duyệt (ví dụ cookie xác nhận tải của Drive).
- **Nhận diện "cần đăng nhập"**, mã lỗi mới:
  - 401/403 khi không kèm cookie ⇒ `auth.required`.
  - Có kèm cookie mà vẫn bị từ chối ⇒ `auth.expired`.
  - Chuỗi chuyển hướng kết thúc ở trang đăng nhập (`accounts.google.com`, `login.microsoftonline.com`,
    `login.live.com`, `www.dropbox.com/login`, `facebook.com/login`…, đường dẫn chứa `login|signin|sso`) hoặc trả
    HTML thay vì file (đã có AC-06) ⇒ `auth.required`.
  - Danh sách đặt một chỗ, có test.
- **Tiếp tục mà không có credentials:** task có `auth` nhưng đang thiếu credentials thì không gửi request nào,
  trả `auth.required` ngay. App xếp nó vào chờ link mới (`RecoveryWait::Link`), cùng chỗ chờ với link hết hạn
  (Phụ lục B).

## 6. Vòng đời cookie

| Lúc | Cookie ở đâu | Việc xảy ra |
|---|---|---|
| Bấm trên trình duyệt | RAM extension → stdin native host → pipe IPC → RAM app | App tạo/làm mới `AuthGrant` của (tên miền gốc, cookie store). Lần cho phép mới thay hũ cũ cho mọi task cùng trang (cookie mới hơn) |
| Đang tải | `AuthGrant` (app); yt-dlp nhận bản sao qua pipe trong lúc chạy | Task giữ `Arc<AuthGrant>`; task con media dùng chung |
| Người dùng tạm dừng | Vẫn trong RAM | Tiếp tục không hỏi lại. Đề xuất giữ tới khi tắt app; thêm hạn hay không là câu Q3 |
| Link ký hết hạn (googlevideo `expire=`) | RAM | `resolve` chạy lại yt-dlp với cookie đang giữ; nếu đã mất cookie thì vào chờ `Login` trước |
| Máy chủ báo hết hạn đăng nhập | — | Đánh dấu grant cũ; task vào chờ `Login` (`auth.expired`); trình duyệt đã làm mới cookie (DBSC) nên một lần bấm popup là đủ |
| App tắt / crash / mở lại | Mất (đúng ý) | Journal còn `auth.sites` ⇒ task "Cần đăng nhập" (S-72). Popup → "Tiếp tục (n)" → cookie mới → chạy tiếp từ segment đã có |
| Hàng loạt (`batch`) | Một grant | Chỉ link **cùng tên miền gốc với trang** mới được kèm cookie; link sang trang khác tải không cookie (chống CSRF, §7) |
| Tải cả kênh | Một grant | Lần liệt kê (S-60) và mọi video xếp hàng dùng chung; app khởi động lại thì một lần bấm làm tiếp cả kênh |
| Task xong / lỗi hẳn / bị xoá | — | Thả `Arc`. Khi không còn task nào giữ ⇒ `zeroize` giá trị |
| Người dùng bấm "Quên cookie" (Cài đặt) | — | Xoá mọi grant ngay; task đang dùng chuyển sang chờ `Login` |
| Cửa sổ ẩn danh/riêng tư | RAM | Như trên, nhưng không bao giờ được làm mới sau khi app khởi động lại |

Phía extension không lưu cookie (không dùng `storage`). `storage.local` chỉ giữ thông tin "đang có n task chờ ở
google.com" để hiện popup nhanh.

## 7. Mô hình mối đe doạ

| # | Kẻ tấn công / đường lộ | Có thể làm gì | Chặn bằng | Rủi ro còn lại |
|---|---|---|---|---|
| T1 | Tiến trình khác **cùng user** (mã độc) | Đọc RAM của UTD, nối pipe, đọc `ipc.json`, journal | Cookie chỉ có sau cú bấm trên trình duyệt (§4.3), nên không thành cửa sau của App-Bound Encryption. Pipe cookie chỉ phục vụ đúng PID yt-dlp. Cookie chỉ sống trong RAM khi đang tải, phạm vi một trang | Kẻ đọc RAM đúng lúc đang tải vẫn lấy được. Chấp nhận: cùng mức với RAM của trình duyệt |
| T2 | User khác trên máy | Nối named pipe | DACL chỉ SID user cho **cả** pipe IPC (hiện là mặc định, phải sửa) và pipe cookie; `reject_remote_clients`; `first_pipe_instance` | Thấp |
| T3 | Log, Doctor, "Sao chép log" | Ghi lộ giá trị | Kiểu cookie không in được; không log stdout/stderr yt-dlp; native host chỉ log chữ lỗi; detail log bỏ query. Test cookie mồi | Thấp |
| T4 | Crash dump / WER | Dump có heap | Cookie không bao giờ sang WebView (giao diện chỉ biết "cần đăng nhập · google.com"); `Zeroizing`; thời gian sống ngắn | Bản sao trong buffer của hyper/reqwest (`Bytes`) không được zeroize: ghi rõ là rủi ro còn lại |
| T5 | Journal SQLite (+ WAL) | Persist | Không có cột nào giữ giá trị. Test quét byte của `utd.db`, `-wal`, `-shm` tìm cookie mồi | URL ký (token ngắn hạn) vẫn ở journal (Q7) |
| T6 | yt-dlp và cấu hình/plugin của nó | Cấu hình người dùng in traffic, plugin trộm cookie | `--ignore-config --no-plugin-dirs`; yt-dlp tải từ GitHub có so SHA-256 | Bản thân yt-dlp là mã bên thứ ba nhận cookie: ranh giới tin cậy đã chấp nhận |
| T7 | Extension giả | Gửi tin cho native host | `allowed_origins` / `allowed_extensions` của host manifest. Khoá `key` nằm công khai trong repo, nên ai cũng nạp được bản unpacked cùng ID (cần chế độ nhà phát triển + người dùng tự nạp) | Khi lên store: bản phát hành chỉ nhận ID của store (Q8) |
| T8 | Trang độc dụ tải kèm cookie (CSRF) | Đặt link tới trang nhạy cảm (`mail.google.com/…?action=delete`) để UTD GET kèm cookie | Chỉ GET, đúng như trình duyệt điều hướng. Bỏ cookie SameSite=Strict khi chéo trang. Hàng loạt chỉ kèm cookie cho link cùng trang. Chỉ trang đã cho phép mới có cookie. Miền nhạy cảm cần xác nhận từng lượt (Q10) | Ngang với người dùng tự bấm link đó |
| T9 | Trang độc chuyển hướng file sang miền của nó | Thu cookie | Cookie tính theo domain-match ở từng bước (đã thử với yt-dlp: không mang sang host khác). Test engine | Thấp |
| T10 | Trang gọi extension (postMessage) / extension khác (`onMessageExternal`) | Kích hoạt đọc cookie | Không có content script lắng nghe trang; không đăng ký `onMessageExternal`; `onMessage` kiểm `sender.id === runtime.id` và không có `sender.tab` | Thấp |
| T11 | Tin nhắn native giả cookie của miền khác | Gắn cookie lạ | App kiểm: domain của cookie phải domain-match URL đích hoặc trang; tên/giá trị đúng bảng ký tự; trần số lượng/kích thước | Thấp |
| T12 | Kênh app → extension (giai đoạn 2, cổng `connectNative` để hiện huy hiệu) | Giả app để gửi "đang chờ" | Tin nhắn chỉ đổi huy hiệu/danh sách chờ; mọi lần đọc cookie vẫn cần cú bấm | Có thể làm phiền (spam huy hiệu) |
| T13 | Tài khoản bị khoá vì dùng tự động | — | Ghi một dòng trong Cài đặt; chỉ gọi yt-dlp khi người dùng bấm | Không kiểm soát được (R2) |

## 8. UX ít chữ kiểu IDM

### 8.1 Luồng

1. **Trang đã cho phép** (thường gặp nhất): chuột phải → "Tải bằng UTD" là xong. Không có hộp hỏi nào; dòng tải
   có chấm nhỏ "đăng nhập". Một cú bấm, như IDM.
2. **Lần đầu, trang cần đăng nhập:**
   1. Người dùng bấm như thường; UTD dò ra cần đăng nhập ⇒ task (hoặc hộp Tải media S-14 / hộp kênh S-63) hiện
      **Cần đăng nhập · google.com** cùng dòng gợi ý "Bấm biểu tượng UTD trên trình duyệt".
   2. Popup mở ra (S-70): "google.com · 2 tải chờ" và nút chính **Dùng đăng nhập**.
   3. Chrome hỏi quyền cho google.com (hộp của chính trình duyệt, không giả được) → **Cho phép** → task chạy tiếp.
      Hộp media tự phân tích lại.

   Tổng cộng 3 cú bấm, và chỉ xảy ra một lần cho mỗi trang. Cải tiến nhỏ: link file trực tiếp thì app chờ lượt dò
   đầu ≤ 2,5 s rồi mới trả lời extension, để extension đặt huy hiệu "!" ngay lên biểu tượng.
3. **Sau khi khởi động lại / đăng nhập hết hạn:** dòng tải "Cần đăng nhập" (S-72), nút **Mở trang** → popup
   "Tiếp tục (n)". Hai cú bấm, không còn hộp của Chrome.
4. **Tắt cho một trang:** popup có công tắc "Đăng nhập trên trang này" (gọi `permissions.remove`). Chrome cũng tự
   có chỗ quản lý quyền theo trang.

### 8.2 Màn mới / sửa (mã tạm)

| Mã | Ở đâu | Khi | Thấy gì | Làm được gì |
|---|---|---|---|---|
| S-14 (sửa) | Hộp Tải media | yt-dlp báo cần đăng nhập | "Cần đăng nhập · youtube.com" + gợi ý một dòng | Mở trang, Huỷ; tự phân tích lại khi nhận cookie |
| S-63 (sửa) | Hộp Tải cả kênh | Kênh riêng tư | Như S-14 | Mở trang, Thử lại, Huỷ |
| S-70 | Popup extension | App có task chờ đăng nhập ở trang đang mở | "google.com · 2 tải chờ" | **Dùng đăng nhập** / **Tiếp tục (n)** |
| S-71 | Popup extension | Trình duyệt từ chối quyền, hoặc extension không được chạy ở cửa sổ riêng tư | Một câu lỗi | Thử lại |
| S-72 | Danh sách + Chi tiết | Task chờ `Login` | Nhãn "Cần đăng nhập"; Doctor (S-21): nguyên nhân + cách sửa | Mở trang, Huỷ |
| S-73 | Cài đặt › Tích hợp | Luôn | Công tắc "Dùng đăng nhập của trình duyệt", "Đang giữ cookie: 2 trang", một dòng cảnh báo khoá tài khoản | **Quên cookie** |

Công tắc chung ở S-73 (mặc định theo Q2) chỉ là cổng thứ hai. Cổng chính vẫn là quyền theo trang trong trình duyệt.

### 8.3 Câu chữ (16 ngôn ngữ)

- Khoá `channel.blocked.login` ("Cần đăng nhập") đã có đủ 16 thứ tiếng: dùng lại.
- Khoá mới ghi vào `src/shared/i18n/locales/*.json`. Câu của extension ghi vào `extensions/*/_locales/*/messages.json`,
  thư mục `zh_CN`, `pt_BR`.
- Mã lỗi Rust mới cần `err.<code>` ở đủ 16 file: `auth.required`, `auth.expired`, `auth.pipe`, `auth.denied`.
- Sửa câu cũ có ý "UTD không gửi cookie": `doctor.access.summary`, `browser.manualNote`, `channel.private`,
  `err.media.login`, `err.channel.private_all`.
- Từ ngữ theo bản dịch đã có (tiếng Anh dùng "sign-in" như `channel.blocked.login`).

| Ngôn ngữ | `auth.use` (nút) | `auth.openPage` | `auth.hint` | `auth.resume` | `auth.siteSwitch` | `settings.auth` | `auth.forget` | `err.auth.required` | `err.auth.expired` | `err.auth.denied` |
|---|---|---|---|---|---|---|---|---|---|---|
| vi | Dùng đăng nhập | Mở trang | Bấm biểu tượng UTD trên trình duyệt | Tiếp tục ({n}) | Đăng nhập trên trang này | Dùng đăng nhập của trình duyệt | Quên cookie | Trang này cần đăng nhập. | Đăng nhập đã hết hạn. | Trình duyệt chưa cho phép. |
| en | Use my sign-in | Open page | Click the UTD icon in your browser | Resume ({n}) | Sign-in on this site | Use browser sign-ins | Forget cookies | This page requires a sign-in. | Your sign-in has expired. | The browser didn't allow it. |
| zh-CN | 使用我的登录 | 打开页面 | 点击浏览器中的 UTD 图标 | 继续 ({n}) | 在此网站使用登录 | 使用浏览器的登录 | 清除 Cookie | 此页面需要登录。 | 登录已过期。 | 浏览器未允许。 |
| zh-TW | 使用我的登入 | 開啟頁面 | 點擊瀏覽器中的 UTD 圖示 | 繼續 ({n}) | 在此網站使用登入 | 使用瀏覽器的登入 | 清除 Cookie | 此頁面需要登入。 | 登入已過期。 | 瀏覽器未允許。 |
| ja | 自分のログインを使う | ページを開く | ブラウザーの UTD アイコンをクリック | 再開 ({n}) | このサイトでログインを使う | ブラウザーのログインを使う | Cookie を消去 | このページはログインが必要です。 | ログインの有効期限が切れました。 | ブラウザーで許可されませんでした。 |
| ko | 내 로그인 사용 | 페이지 열기 | 브라우저에서 UTD 아이콘을 클릭하세요 | 계속 ({n}) | 이 사이트에서 로그인 사용 | 브라우저 로그인 사용 | 쿠키 지우기 | 이 페이지는 로그인이 필요합니다. | 로그인이 만료되었습니다. | 브라우저가 허용하지 않았습니다. |
| th | ใช้การลงชื่อเข้าใช้ของฉัน | เปิดหน้า | คลิกไอคอน UTD ในเบราว์เซอร์ | ดำเนินการต่อ ({n}) | ใช้การลงชื่อเข้าใช้ในเว็บนี้ | ใช้การลงชื่อเข้าใช้ของเบราว์เซอร์ | ล้างคุกกี้ | หน้านี้ต้องลงชื่อเข้าใช้ | การลงชื่อเข้าใช้หมดอายุแล้ว | เบราว์เซอร์ไม่อนุญาต |
| id | Pakai login saya | Buka halaman | Klik ikon UTD di browser | Lanjutkan ({n}) | Login di situs ini | Pakai login browser | Hapus cookie | Halaman ini memerlukan login. | Login sudah kedaluwarsa. | Browser tidak mengizinkan. |
| hi | मेरा साइन-इन इस्तेमाल करें | पेज खोलें | ब्राउज़र में UTD आइकन पर क्लिक करें | जारी रखें ({n}) | इस साइट पर साइन-इन | ब्राउज़र का साइन-इन इस्तेमाल करें | कुकी हटाएँ | इस पेज के लिए साइन-इन ज़रूरी है। | साइन-इन की अवधि खत्म हो गई। | ब्राउज़र ने अनुमति नहीं दी। |
| es | Usar mi sesión | Abrir página | Haz clic en el icono de UTD en el navegador | Reanudar ({n}) | Sesión en este sitio | Usar sesiones del navegador | Olvidar cookies | Esta página requiere iniciar sesión. | Tu sesión ha caducado. | El navegador no lo permitió. |
| pt-BR | Usar meu login | Abrir página | Clique no ícone do UTD no navegador | Retomar ({n}) | Login neste site | Usar logins do navegador | Esquecer cookies | Esta página exige login. | Seu login expirou. | O navegador não permitiu. |
| fr | Utiliser ma connexion | Ouvrir la page | Cliquez sur l’icône UTD dans le navigateur | Reprendre ({n}) | Connexion sur ce site | Utiliser les connexions du navigateur | Oublier les cookies | Cette page nécessite une connexion. | Votre connexion a expiré. | Le navigateur ne l’a pas autorisé. |
| de | Meine Anmeldung nutzen | Seite öffnen | Klicken Sie im Browser auf das UTD-Symbol | Fortsetzen ({n}) | Anmeldung auf dieser Website | Browser-Anmeldungen nutzen | Cookies vergessen | Diese Seite erfordert eine Anmeldung. | Ihre Anmeldung ist abgelaufen. | Der Browser hat es nicht erlaubt. |
| ru | Использовать мой вход | Открыть страницу | Нажмите значок UTD в браузере | Продолжить ({n}) | Вход на этом сайте | Использовать вход из браузера | Забыть cookie | Для этой страницы нужен вход. | Срок входа истёк. | Браузер не разрешил. |
| tr | Oturumumu kullan | Sayfayı aç | Tarayıcıda UTD simgesine tıklayın | Devam et ({n}) | Bu sitede oturum | Tarayıcı oturumlarını kullan | Çerezleri unut | Bu sayfa oturum açmayı gerektiriyor. | Oturumun süresi doldu. | Tarayıcı izin vermedi. |
| ar | استخدم تسجيل دخولي | افتح الصفحة | انقر على أيقونة UTD في المتصفح | استئناف ({n}) | تسجيل الدخول في هذا الموقع | استخدم تسجيل الدخول من المتصفح | احذف ملفات تعريف الارتباط | تتطلب هذه الصفحة تسجيل الدخول. | انتهت صلاحية تسجيل الدخول. | لم يسمح المتصفح بذلك. |

Hộp hỏi quyền là của trình duyệt, tự theo ngôn ngữ trình duyệt.

## 9. Giai đoạn, file sẽ sửa, kế hoạch test

### 9.1 Giai đoạn

| GĐ | Nội dung | Xong khi |
|---|---|---|
| 0 — Gia cố (làm ngay, không phụ thuộc quyết định) | `--ignore-config --no-plugin-dirs` cho mọi lệnh yt-dlp; DACL + `first_pipe_instance` cho pipe IPC; `onMessage` kiểm sender; test cookie mồi cho log/journal (khung test dùng lại ở GĐ1) | Test xanh trên Windows + macOS CI |
| 1 — MVP Windows | Extension: quyền tuỳ chọn, đọc cookie trong cú bấm, popup S-70/S-71, trường `auth` trong tin nhắn v2. App: `auth/` (grant trong RAM, PSL, kiểm cookie), pipe cookie cho yt-dlp, `auth.*` + S-14/S-72/S-73. Engine: `cookies.rs`, `Transport` tự theo chuyển hướng, `AuthMarker` + migration. Chờ `Login` ghép với phiên khôi phục. Link file trực tiếp + một video | Test tự động §9.3 xanh; thử thật: trang fixture, YouTube giới hạn tuổi, file Drive riêng (link `drive.usercontent.google.com/download?id=…`), khởi động lại giữa chừng |
| 2 — Mở rộng | Bộ đổi link: Drive `/file/d/<id>/view` → link tải, trang xác nhận file lớn, Google Docs `export`; OneDrive/SharePoint `?download=1`; Dropbox `dl=1`. Hàng loạt + tải cả kênh dùng chung grant; macOS FIFO; Firefox container/FPI; huy hiệu đẩy từ app qua `connectNative` (tuỳ chọn) | Thử thật OneDrive/Dropbox/Facebook/Instagram/TikTok riêng với tài khoản thử của chủ dự án; Mac mini |
| 3 — Tuỳ chọn | Takeover (AC-15, §4.4); nhập cookies.txt (Q4) | Theo Q4/Q5 |

#### Kết quả GĐ0 (2026-10-25, nhánh này)

- **yt-dlp:** mọi lệnh đi qua `media::yt_dlp_command`: `--ignore-config` luôn có, `--no-plugin-dirs` khi bản
  yt-dlp ≥ 2025.03.21.
  - Đã thử với bản thật: 2025.02.19 báo "no such option", 2025.03.21 nhận cờ.
  - Lưu ý khi thử: `--version` thoát trước khi kiểm cờ, nên không dùng được để dò.
  - Thay 5 chỗ gọi: `probe`, `resolve`, `runner`, `channel/mod`, `channel/profile`.
  - Phân tích YouTube thật (09OGSRptb0M, yt-dlp 2026.10.21 + deno): cùng 50 định dạng / 34 luồng hình khi có và
    khi không có cờ.
- **Pipe IPC Windows** (`src-tauri/src/ipc/pipe_acl.rs`):
  - DACL `D:P(A;;GA;;;<SID người dùng>)`. Test đọc lại DACL thấy đúng một quyền.
  - Instance đầu `first_pipe_instance`: tên đã có thì lỗi. Có test.
  - `reject_remote_clients`.
  - Luôn tạo instance kế tiếp trước khi trao instance vừa nối, nên tên pipe không lúc nào trống.
  - Không dựng được DACL thì tắt hẳn kênh IPC thay vì mở pipe quyền mặc định.
  - Thử thật (`auth-thu/pipe-e2e.py`): app debug với `UTD_DATA_DIR` riêng nhận 4/4 tin native liên tiếp; token
    sai bị từ chối.
  - Crate `windows` bật thêm `Win32_Security`, `Win32_Security_Authorization`, `Win32_System_Threading` (không
    thêm thư viện mới).
- **Extension:** `runtime.onMessage` chỉ nhận từ popup của chính tiện ích (id + không có tab + URL
  `popup.html`). Thử thật (`auth-thu/ext-sender.cjs`): popup thật được nhận, tab mở `popup.html` bị bỏ.
- **Khung cookie mồi:**
  - `files_containing` (`crates/utd-engine/tests/kit`) quét mọi file kể cả `utd.db-wal`.
  - `tests/secrets.rs`: header bí mật từ nguồn ngoài không tới máy chủ, không xuống đĩa. Có bài tự kiểm để chắc
    bộ quét thấy bí mật trong WAL.
  - `native_host`: câu lỗi ghi vào `native-host.log` không lặp lại nội dung tin nhắn.
  - Test log của luồng cookie thật (T-L1) làm ở GĐ1 cùng code đăng nhập.
- **Kiểm:**
  - Windows: `cargo test -p app --lib` (209, 1 bỏ qua có sẵn), `cargo test -p utd-engine` (mọi bộ),
    `cargo clippy` 3 crate `-D warnings`, `pnpm lint`.
  - macOS chưa chạy. Phần chỉ-Windows đều gắn `#[cfg(windows)]`; phần còn lại là code chung.

#### Kết quả GĐ1 (2026-10-25, nhánh này)

Đã làm:
- **SPEC (Q9):** S-14, S-63 theo luồng đăng nhập; thêm S-70..S-73; AC-18 ghi rõ cookie chỉ đọc trong cú bấm.
- **Engine (`utd-engine/src/auth.rs`):**
  - `CookieJar` chỉ ở RAM, đi theo `DownloadControl::cookies` của từng lượt chạy.
  - Journal v4 chỉ ghi `auth_sites`. Task có `auth_sites` mà thiếu cookie thì dừng `auth.required` trước khi gửi
    yêu cầu nào.
  - Có cookie thì `Transport` tự theo chuyển hướng, tính lại `Cookie` từng bước, gộp `Set-Cookie`; 401/403 ⇒
    `auth.expired` ngay (không thử 8 lượt).
  - Chuyển hướng về trang đăng nhập ⇒ `auth.required` / `auth.expired`.
- **App (`src-tauri/src/auth/`):**
  - Lần cho phép (`Grant`) gắn task, thả khi xong/xoá.
  - `psl` 2.1.242; mẫu quyền `*://*.site/*`; miền nhạy cảm (Q10).
  - Kiểm cookie: đúng trang, trần 300 cookie / 64 KiB.
  - Pipe cookie cho yt-dlp (PID, `loaded`).
  - IPC/native host bản 2: `auth`, `auth-pending`, `auth-renew`, sự kiện `utd://auth-granted`.
  - Nối vào tải file, lô link (chỉ link cùng trang), Tải media (Phân tích, resolve, đường tải nhanh, yt-dlp
    tải), Tải cả kênh.
  - `--user-agent` của trình duyệt; Q7 (bỏ `final_url` khi xong).
  - Lệnh Cài đặt; lệnh `open_web_page` (chỉ http/https).
- **Tiện ích:**
  - `optional_permissions` / `optional_host_permissions`.
  - Đọc cookie trong cú bấm, theo kho cookie của tab; bỏ Strict khi chéo trang; Firefox FPI thử lại với
    `firstPartyDomain`.
  - Popup S-70/S-71: Dùng đăng nhập / Tiếp tục (n), công tắc theo trang.
  - 7 câu mới × 16 ngôn ngữ.
- **Giao diện:**
  - S-14 trong hộp Tải media và S-63 trong Tải cả kênh: tự phân tích/liệt kê lại khi popup cho phép.
  - S-72: nhãn "Cần đăng nhập" + Doctor "Mở trang".
  - S-73 trong Cài đặt.
  - 18 khoá mới + 5 câu cũ sửa × 16 ngôn ngữ. `scripts/i18n-check.mjs` quét thêm mã `auth.*`.

Khác kế hoạch (cố ý):
- SameSite chỉ lọc ở tiện ích lúc đọc; engine không lọc thêm.
- Engine không gộp trường `formats[].cookies` của yt-dlp: định dạng có trích dẫn kiểu Python, engine tự gộp
  `Set-Cookie` khi tải. Xem lại nếu có trang cần.
- Huy hiệu "!" trên biểu tượng tiện ích (§8.1, cải tiến nhỏ) để GĐ2.
- Firefox < 128 không có `optional_host_permissions`: popup báo "Trình duyệt chưa cho phép" (S-71), phần còn lại
  chạy như cũ.
- macOS: pipe cookie cho yt-dlp báo `auth.pipe` tới khi có FIFO (GĐ2). Tải file trực tiếp kèm cookie vẫn chạy.
- Task cần đăng nhập: `failed` với mã `auth.*`. Đã đổi khi ghép main: chờ `RecoveryWait::Link` (mục "Ghép main"
  bên dưới, Phụ lục B).

Kiểm (Windows):
- `cargo test` app 222 (+ 1 yt-dlp thật `#[ignore]` chạy tay: đạt), engine 159 (thêm `auth` 7 unit + 7 tích hợp).
- `clippy` 3 crate `-D warnings`; `pnpm lint` (eslint + i18n 601 khoá); `tsc`; `pnpm build` (JS 190,9 / 200 kB).
- e2e toàn bộ 76/76 (gồm `sign-in.spec.ts` 4 bài mới; bài S-63 cũ sửa theo SPEC mới). `ui-check` đạt.
- Thử thật:
  - `auth-thu/ext-auth.cjs`: tiện ích trong Chromium 156.
  - `auth-thu/auth-e2e.py`: trọn luồng trên app debug — đăng nhập, khởi động lại giữa chừng, cho phép lại, không
    sót cookie mồi.

#### Ghép main (2026-10-25, origin/main 16a882f: màn Chi tiết, tự cứu download, GĐ0)

- Journal: cột `auth_sites_json` thành migration **v6** (main có v4 `task_events`, v5 `recovery_json`); `save_task`
  ghi trong transaction của main.
- Cần đăng nhập lại đi chung đường chờ link mới `RecoveryWait::Link` (Phụ lục B), không có trạng thái chờ riêng:
  - `recovery::plan`: `auth.required`, `auth.expired`, `media.login` ⇒ `Link` (task gốc, cả media).
  - `auth.pipe` bỏ khỏi nhóm "cần đăng nhập" (đăng nhập lại không sửa được) ⇒ lỗi thường, có nút Thử lại.
  - Popup "Dùng đăng nhập": chỉ xếp lại task đang chờ `Link` của trang đó; hộp "Cập nhật link" đang mở cho task
    đó thì đóng (`utd://link-updated`).
  - Bấm tải lại trên trình duyệt (`capture_link`): `DownloadEngine::replace_link(…, signed)` dò link mới bằng cookie
    của cú bấm, ghi trang vào `auth_sites`; app gắn lần cho phép vào task trước khi xếp hàng.
  - Chẩn đoán (S-72) của download thường: nút chính "Cập nhật link…", thêm "Mở trang"; task media chỉ "Mở trang".
- IPC: link có cảnh báo trước khi tải (S-22, `Route::Confirm`) mang lần cho phép của cú bấm sang hộp Thêm hàng loạt.
- Bundle JS: main riêng 199,14 kB; ghép xong 200,09 kB / 200 kB (vượt 91 B) dù đã cắt: gộp ba hook nghe sự kiện
  (`use-app-event.ts`), S-73 chỉ hiện số trang (đúng SPEC), công tắc lên đầu mục, bỏ icon `LogIn`. Chủ dự án
  cho nâng giới hạn lên 220 kB (2026-10-25, ghi trong AI_RULES.md).
- Thêm bài thử: engine `a_sign_in_wait_takes_a_new_link_only_with_the_cookies`, unit `recovery` (mã đăng nhập ⇒
  `Link`, task con và `auth.pipe` thì không), app `a_sign_in_after_restart_waits_like_an_expired_link`.

Chưa làm, cần chủ dự án:
- Thử với tài khoản thật: YouTube giới hạn tuổi, file Google Drive riêng (Q6).
- Cần nạp tiện ích bản mới và chạy bản UTD mới. Native host đang đăng ký trên máy là của bản 0.1.0, chưa hiểu tin
  bản 2.

### 9.2 File sẽ sửa (GĐ0–1)

- **Extension:** `extensions/{chromium,firefox}/`
  - `manifest.json`: `optional_permissions`, `optional_host_permissions`.
  - `background.js`: đọc cookie trong `onClicked`; tin nhắn `version: 2` có `auth` (cookie + `userAgent`);
    kiểm `sender`.
  - `popup.html`, `popup.js`: S-70, S-71, công tắc theo trang.
  - `_locales/*/messages.json` (16 thư mục).
- **Native host:** `src-tauri/src/native_host.rs`
  - Schema v2: `auth.cookies[]`, kiểu tin `auth-pending`, `auth-renew`.
  - Giữ v1 chạy được.
  - Trần kích thước; không log payload.
- **IPC:** `src-tauri/src/ipc.rs`
  - DACL, `first_pipe_instance`.
  - `PipeRequest.auth`; route mới `AuthPending` / `AuthRenew`.
  - Kiểm token trước khi đọc cookie.
- **Module mới `src-tauri/src/auth/`:**
  - `mod.rs`: `AuthGrant`, kho grant trong `AppState`, hạn, quên.
  - `site.rs`: PSL ⇒ tên miền gốc và mẫu quyền.
  - `pipe.rs` (Windows) và `fifo.rs` (macOS, GĐ2): file cookies.txt ảo cho yt-dlp.
  - `netscape.rs`: dựng nội dung cookies.txt trong `Zeroizing`.
  - `tests.rs`.
- **Media:**
  - `src-tauri/src/media/{mod,probe,resolve,runner,fast,parts}.rs`: cờ chung, cookie, gộp `formats[].cookies`, mã
    `auth.*`.
  - `src-tauri/src/media/channel/{mod,profile}.rs`: liệt kê kèm cookie.
- **App:**
  - `src-tauri/src/{commands,state,lib,events}.rs`: lệnh `auth_status`, `auth_forget`; sự kiện
    `utd://auth-granted`.
  - `recover.rs` (của phiên khôi phục): chờ `Link` (không thêm `Login`, xem Phụ lục B).
- **Engine:**
  - `crates/utd-engine/src/cookies.rs` (mới).
  - `engine/{transport,session,mod}.rs`, `model.rs` (`AuthMarker`), `db.rs` (migration `auth_json`).
  - `engine/http.rs`: nhận diện trang đăng nhập.
  - `Cargo.toml`: `zeroize` (đã có trong `Cargo.lock`).
- **Thư viện mới:** `psl`, cho `src-tauri`, qua `deps-check`.
- **Giao diện:**
  - `src/features/home/ui/{media-picker-dialog, media-picker-parts, channel-dialog, channel-flow, soniox-table*, settings-browser}.tsx`.
  - `src/features/home/model/{types,doctor,download-mapper}.ts`.
  - `src/shared/i18n/locales/*.json` (16 file).
- **Tài liệu:** SPEC.md (§2), `progress.md`. Tạo `docs/PRIVACY.md`: mục "Xong nghĩa là gì" của SPEC đòi có, hiện
  chưa có. Ghi cookie dùng thế nào, giữ ở đâu, bao lâu.

### 9.3 Test

Tự động (chạy CI Windows + macOS trừ khi ghi khác):

| Mã | Test | Ở đâu |
|---|---|---|
| T-C1 | Hũ cookie: domain-match, host-only, path, Secure qua http, hết hạn, thứ tự, bỏ Strict khi chéo trang, trần, `Debug` không lộ giá trị | `utd-engine` unit |
| T-C2 | Chuyển hướng: cùng miền con có cookie `Domain=` ⇒ gửi; host-only sang miền con ⇒ không; sang miền khác ⇒ không Cookie, không Authorization; https→http ⇒ không cookie Secure; `Set-Cookie` được gộp | `utd-engine/tests/auth.rs` (server `kit`) |
| T-C3 | Không persist: tải xong và tạm dừng giữa chừng có credentials, rồi quét byte `utd.db`, `-wal`, `-shm` tìm cookie mồi ⇒ 0 | như trên |
| T-C4 | Tiếp tục khi thiếu credentials ⇒ `auth.required`, server không nhận request nào; 401 khi có cookie ⇒ `auth.expired` | như trên |
| T-P1 | Pipe Windows: yt-dlp thật (bỏ qua khi máy không có) đọc cookie mồi từ pipe, thoát 0, server fixture nhận cookie; quét `%TEMP%` + `UTD_DATA_DIR` ⇒ 0 file chứa mồi; client không phải con yt-dlp ⇒ bị từ chối; pipe biến mất sau khi yt-dlp thoát; không có lần đọc trọn nào (giả `os.access` hỏng) ⇒ `auth.pipe`, kết quả `-J` bị bỏ; lệnh tải bị dừng trước byte đầu | `src-tauri` test `#[cfg(windows)]` + `#[ignore]` khi không có yt-dlp; CI tải yt-dlp có SHA-256 |
| T-P2 | FIFO macOS như T-P1 | `#[cfg(target_os = "macos")]`, GĐ2 |
| T-Y1 | Mọi lệnh yt-dlp có `--ignore-config --no-plugin-dirs`; lệnh có cookie không có `-v/--print-traffic/--write-pages/--dump-pages` | unit trên hàm dựng tham số |
| T-L1 | Log: chạy cả luồng (IPC → grant → engine → yt-dlp giả) với logger thu vào bộ nhớ ⇒ không có cookie mồi; `native-host.log` không có mồi; "Sao chép log" và sự kiện chi tiết không có mồi | `src-tauri` |
| T-I1 | IPC: sai token thì không đọc `auth`; cookie miền lạ bị bỏ; quá trần ⇒ từ chối; tin v1 vẫn chạy | `ipc.rs`, `native_host.rs` |
| T-E1 | Extension (Playwright Chromium + `--load-extension`, như `auth-thu/`): `getAll` trả cookie HttpOnly; bỏ Strict khi chéo trang; chỉ đọc khi có quyền; tin nhắn có đúng cookie của trang | `e2e/extension.spec.ts` |
| T-E2 | Extension: tin từ app (`auth-pending`) không bao giờ dẫn tới `cookies.getAll` nếu không có cú bấm (spy API) | như trên |
| T-U1 | Giao diện: S-14/S-72/S-73 có `data-s`, axe sạch; WebView không bao giờ nhận trường giá trị cookie (kiểm kiểu sự kiện) | Playwright |

Thử thật (ghi lại trong `progress.md`):
1. Trang fixture "cần đăng nhập đơn giản" (`auth-thu/authsite.py`, chuyển thành fixture Rust): tải file, chuyển
   hướng sang host khác, khởi động lại giữa chừng ⇒ "Cần đăng nhập" ⇒ popup ⇒ tải tiếp đúng byte. Không cần tài
   khoản.
2. **YouTube giới hạn tuổi.** Chủ dự án đăng nhập Chrome bằng tài khoản đã xác minh tuổi; phiên làm việc chỉ lái
   UTD, không đụng mật khẩu. Kiểm: Phân tích ra định dạng, tải xong, sau đó quét đĩa không có cookie mồi. Không
   có mồi trong cookie thật, nên quét theo tên `__Secure-1PSID` và giá trị lấy từ RAM trong bản debug.
3. **Google Drive file riêng** của chủ dự án: một file nhỏ hơn 100 MB, một file lớn hơn 100 MB (trang xác nhận,
   GĐ2), một Google Docs xuất PDF (GĐ2).
4. Firefox (thường, container, cửa sổ riêng tư), Edge.
5. GĐ2: OneDrive/Dropbox riêng; Facebook/Instagram/TikTok riêng tư bằng tài khoản thử; Mac mini (Chrome + Firefox,
   FIFO).

## 10. Rủi ro và câu hỏi cần quyết

Rủi ro:
- **R1 — yt-dlp đổi cách mở file cookie** (bỏ `os.access`, đổi thứ tự) làm hỏng pipe. Chặn bằng T-P1 chạy với bản
  yt-dlp mới nhất trong CI (tự cập nhật yt-dlp đã có), lỗi thì báo `auth.pipe`, không rơi về đĩa.
- **R2 — Tài khoản bị khoá** khi dùng cookie với yt-dlp (wiki yt-dlp cảnh báo YouTube). Có một dòng cảnh báo ở S-73.
- **R3 — DBSC** (Chrome 145+ Windows, Google bật đại trà từ 2026-05): cookie Google sống ngắn. Suy luận, chưa
  nguồn nào xác nhận: phát lại chỉ dùng được tới khi hết hạn.
  - YouTube ít ảnh hưởng, vì yt-dlp chỉ cần cookie lúc đầu.
  - File Drive lớn tải lâu có thể gặp `auth.expired` giữa chừng. Đã thiết kế luồng làm mới một cú bấm; cần đo thời
    gian sống thật.
- **R4 — Cảnh báo quyền đáng sợ.** Hộp Chrome cho `*.google.com` ghi kiểu "dữ liệu của bạn trên các trang
  google.com", cần chụp lại khi làm. Hộp chỉ hiện một lần mỗi trang.
- **R5 — Lên store.**
  - Chrome Web Store: dùng `cookies` phải có chính sách quyền riêng tư (dữ liệu xác thực, "Limited Use").
  - Firefox AMO: extension mới phải khai `data_collection_permissions`; chưa rõ "gửi cho app trên cùng máy" có tính
    là thu thập không.
  - Kiểm lúc phát hành.
- **R6 — Firefox:** chưa thử thật `optional_host_permissions` của MV3 và FPI. Cuối GĐ1 thử bằng Firefox của Playwright
  hoặc `web-ext`.
- **R7 — macOS FIFO** chưa thử (Mac mini đang khoá màn hình theo `progress.md`).
- **R8 — Ghép nhánh:** đụng `ipc.rs`, `media/*`, `engine/{transport,model,db}.rs` cùng phiên khôi phục và phiên màn
  chi tiết (Phụ lục B). GĐ1 nên bắt đầu sau khi hai nhánh đó vào main.

Câu cần chủ dự án quyết. **Đã quyết 2026-10-25: theo đề xuất ở cả 10 câu** (nhớ theo trang; công tắc chung mặc
định Bật; giữ cookie tới khi tắt app; không làm cookies.txt và `--cookies-from-browser`; takeover xin quyền theo
trang; chủ dự án tự đăng nhập lúc thử thật; xoá query của task cần đăng nhập khi xong; bản phát hành chỉ nhận ID
store; sửa SPEC khi bắt đầu GĐ1; có danh sách miền nhạy cảm ở GĐ1).
1. **Q1 — Nhớ theo trang hay hỏi mỗi lần?**
   - Đề xuất: **nhớ theo trang**, dùng quyền theo trang của trình duyệt, tắt được trong popup. Chỉ cookie không bao
     giờ được lưu.
   - Cách kia: sau mỗi lượt tự `permissions.remove`, lần sau Chrome hỏi lại. Đúng chữ "cho phiên này" của S-14
     nhưng thêm 2 cú bấm mỗi lần.
2. **Q2 — Công tắc chung ở Cài đặt mặc định Bật hay Tắt?** Đề xuất **Bật**: vẫn phải cho phép từng trang trong
   trình duyệt, Tắt thì lần đầu thêm một bước vào Cài đặt.
3. **Q3 — Giữ cookie trong RAM bao lâu khi task tạm dừng?** Đề xuất **tới khi tắt app**. Cách khác: thêm hạn,
   ví dụ 12 giờ.
4. **Q4 — Có làm nhập cookies.txt (c) không?** Đề xuất **không** ở GĐ1–2. `--cookies-from-browser` (b) thì đề
   xuất **không làm**.
5. **Q5 — Takeover (AC-15) xin quyền thế nào?**
   - Đề xuất: theo trang, như §8: trang chưa cho phép thì takeover không kèm cookie, UTD dò không được thì trả lại
     trình duyệt.
   - Hay xin mọi trang một lần như IDM (cảnh báo "all websites")?
6. **Q6 — Tài khoản để thử thật:**
   - YouTube giới hạn tuổi (tài khoản đã xác minh tuổi) và file Google Drive riêng: chủ dự án tự đăng nhập trên
     Chrome của máy này lúc thử, hay cấp tài khoản thử?
   - GĐ2 cần thêm tài khoản thử Facebook/Instagram/TikTok.
7. **Q7 — URL có token (query) của task cần đăng nhập:** xoá query khỏi journal khi task xong? Đề xuất **có**;
   mất nút "tải lại từ link cũ" cho các task đó.
8. **Q8 — Khoá extension công khai trong repo:** khi lên store, bản phát hành chỉ nhận ID của store, ID dev chỉ
   dùng cho bản debug? Đề xuất **có**.
9. **Q9 — Đồng ý sửa SPEC** theo §2 (S-14, S-63, thêm S-70..S-73, AC-18) khi bắt đầu GĐ1?
10. **Q10 — Danh sách miền nhạy cảm** (`gov`, `mil`, ngân hàng, `paypal.com`…, học từ Motrix 2.x):
    - Đề xuất: **có**, làm ở GĐ1. Các trang này không tự kèm cookie dù đã được cho phép; mỗi lượt cần bấm xác nhận
      trong popup.
    - Cái giá: thêm một cú bấm mỗi lần tải từ ngân hàng/cổng chính phủ, và phải giữ danh sách.
    - Đồng ý thì thêm vào §8.3 hai câu chữ cho 16 ngôn ngữ: nhãn "Trang nhạy cảm" và nút xác nhận.

## Phụ lục A — Nguồn

Mã nguồn và tài liệu chính thức (tra 2026-10-25):
- Chrome `cookies` API (cần quyền host, `partitionKey`, `getAllCookieStores`):
  https://developer.chrome.com/docs/extensions/reference/api/cookies
- Chrome `permissions` (`optional_host_permissions`, cần cú bấm):
  https://developer.chrome.com/docs/extensions/reference/api/permissions
- Mã Chromium:
  - `activeTab` không áp cho cookies (`kUnknownTabId`):
    https://github.com/chromium/chromium/blob/main/extensions/browser/api/cookies/cookies_api.cc
  - Lỗi khi không có cú bấm:
    https://github.com/chromium/chromium/blob/main/extensions/browser/api/permissions/permissions_api.cc
  - Câu cảnh báo quyền: `chrome/app/generated_resources.grd`.
- Firefox:
  - `cookies` API (`firstPartyDomain`, `partitionKey`, `storeId`):
    https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/API/cookies
  - Quyền host MV3 (từ Firefox 127 cấp lúc cài):
    https://extensionworkshop.com/documentation/develop/manifest-v3-migration-guide/
  - User actions: https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/User_actions
- Native messaging:
  - Chrome: https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging
  - `connectNative` giữ service worker sống (Chrome 105+):
    https://developer.chrome.com/docs/extensions/develop/concepts/service-workers/lifecycle
  - Firefox: https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/Native_messaging
- `downloads` API (không có header/cookie): https://developer.chrome.com/docs/extensions/reference/api/downloads.
  `webRequest` + `extraHeaders`: https://developer.chrome.com/docs/extensions/reference/api/webRequest
- App-Bound Encryption: https://security.googleblog.com/2024/07/improving-security-of-chrome-cookies-on.html.
  Edge: https://learn.microsoft.com/en-us/deployedge/microsoft-edge-browser-policies/applicationboundencryptionenabled
- yt-dlp:
  - Issue #10927 (App-Bound, còn mở), #7271 (khoá DB cookie, còn mở).
  - Wiki "Exporting YouTube cookies": https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies
  - GHSA-v8mc-9377-rwjj (cookie tách khỏi `http_headers`, 2023.07.06).
  - `yt_dlp/cookies.py` (`load_cookies` gọi `os.access` rồi `open`; `save` mở `'w'`), `YoutubeDL.close()` →
    `save_cookies()`.
- DBSC: https://developer.chrome.com/docs/web-platform/device-bound-session-credentials ·
  https://developer.chrome.com/blog/dbsc-windows-announcement
- Quyền mặc định của named pipe: https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights
- Cách các trình quản lý tải khác làm (mã nguồn): XDM, Aria2-Explorer, Motrix WebExtension. Đều dùng
  `cookies.getAll` + `downloads`; XDM thêm `webRequest.onSendHeaders`.

Thử thật trong phiên này (Windows 11, yt-dlp.exe 2026.10.21 SHA-256 `33121d19…f555`, Chromium 156.0.8078.4 của
Playwright 1.64): `auth-thu/README.md`.

## Phụ lục B — Ghép với hai phiên song song

- **Khôi phục download** (`claude/recovery-9f3b1d`, chưa commit; đọc `notes/recovery.md` và `recovery.rs`):
  - Có `RecoveryWait::{Disk, Link, Network, Retry}`. Lúc lập kế hoạch đề xuất thêm `RecoveryWait::Login`.
  - **Đã làm khác khi ghép (2026-10-25, theo phiên chính):** không thêm `Login`. Cần đăng nhập lại đi chung đường
    chờ link mới `Link`, không có đường thứ hai:
    - `recovery::plan`: `auth.required`, `auth.expired`, `media.login` ⇒ `Link` (cả task media; task con thì không).
    - Mở lại app: task bị ngắt chạy lại, engine dừng ngay với `auth.required` (không gửi gì) ⇒ `Link`.
    - Hai cách gỡ, cùng một chỗ chờ: (1) popup "Dùng đăng nhập" xếp lại mọi task chờ `Link` của trang đó có mã
      cần đăng nhập; (2) bấm tải lại trên trình duyệt ⇒ `capture_link` gắn link vào task, dò bằng cookie của cú
      bấm, gắn lần cho phép rồi mới xếp hàng.
    - Hộp "Cập nhật link" đang mở (`await_download_link`) mà popup cho phép đăng nhập ⇒ báo `utd://link-updated`
      để hộp đóng.
- **Màn chi tiết + cảnh báo trước khi tải** (`claude/detail-screen-7c21e4`, chưa commit):
  - Doctor S-21 thêm nguyên nhân `auth.required` / `auth.expired`, cách sửa "Bấm biểu tượng UTD › Tiếp tục".
  - `detail_log.rs` đã bỏ query; thêm test T-L1 cho bảng sự kiện mới của phiên đó.
  - Hộp cảnh báo trước khi tải (S-22) có thể hiện chip "kèm đăng nhập · google.com" để người dùng thấy cookie sẽ
    được dùng.

## Phụ lục C — Các repo khác làm thế nào (đọc mã nguồn, 2026-10-25)

Hai agent clone nông (`--depth 1`) rồi đọc mã, không build và không chạy mã đã clone. Kết quả:
- **15 repo trình quản lý tải có extension.** Free Download Manager không công khai mã. JDownloader chỉ có bản
  extension MV3 của bên thứ ba.
- **8 app giao diện cho yt-dlp.** Hitomi Downloader bị chặn DMCA (HTTP 451). Stacher không có mã công khai.

Số dòng theo commit ghi trong bảng. Điều gì chưa kiểm trong mã thì ghi "chưa kiểm".

### C.1 Trình quản lý tải có extension

| Repo (commit) | Lấy cookie | Quyền | Đồng ý | Kênh extension → app | Lưu cookie? | Chuyển hướng sang miền khác |
|---|---|---|---|---|---|---|
| AB Download Manager (ext `658a0c7`, app `e329495`) | Tự bắt: `onSendHeaders` + `extraHeaders`; menu: `cookies.getAll({url})`, store mặc định | `*://*/*` bắt buộc, cookies, webRequest, downloads, nativeMessaging | Công tắc "Send headers" **mặc định bật** (`Config.ts` L58); đường tự bắt bỏ qua công tắc | Native messaging; dự phòng HTTP `127.0.0.1:15151`, `X-Api-Key` **mặc định tắt** (`DefaultAppSettings.kt` L43) | Có, chữ thường `downloadlist/<id>.json` | Vẫn gửi Cookie (OkHttp chỉ bỏ `Authorization`) |
| XDM 9 (`4f16d4d`) | Chép header thật (`extraHeaders`); menu: `getAll({url})`; lấy hàng loạt: `getAll({domain})` + `storeId` của tab | `*://*/*` bắt buộc, cookies, webRequest, downloads | Không hỏi | HTTP `127.0.0.1:8597`, không token; chỉ kiểm Host + Origin, chỉ POST (`BrowserIntegration.kt` L244–255) | Cookie chữ thường trong `task-<id>.info` (`TaskInfoDB.kt` L225); `Authorization` chỉ ở RAM (`SensitiveHeaders.kt`) | Vẫn gửi (OkHttp) |
| Gopeed (ext `d29bf1b`, app `b8ff5ff`) | `onBeforeSendHeaders` + `extraHeaders`, chỉ `main_frame`/`sub_frame`; không có quyền `cookies` | host http/https, downloads, webRequest, nativeMessaging | Không hỏi | Native messaging → socket / `\\.\pipe\gopeed_host`; REST: token tuỳ chọn, CORS `*` | Có, JSON trong bbolt (0600) | **Lộ:** nhớ URL cuối (`fetcher.go` L532) rồi gửi mọi header tới đó (`helper.go` L125–149) |
| motrix-webextension, bản cũ (`f8bc93d`) | `getAll({url: item.url})` → aria2 `header: Cookie:` | `<all_urls>`, cookies, downloads | Không hỏi; phải có API key mới takeover | aria2 RPC qua WS `127.0.0.1:16800`, bắt buộc secret | aria2 gốc lưu `header` vào file phiên (chữ thường) | aria2 gốc: không bỏ |
| Motrix 2.x + motrix-extension (`bcedf50`, app `a7a7c9a`) | `getAll({url})` → cookie có cấu trúc (domain/path/cờ) | `<all_urls>`, cookies, webRequest, downloads, nativeMessaging | **Takeover mặc định tắt** (`shared/takeover.ts` L56) + hộp đồng ý; **danh sách miền nhạy cảm** (`capture/sensitiveDomains.ts`) gửi không cookie | Native messaging + WS loopback 16802–16806, ghép cặp SPAKE2, AES-GCM | Hũ cookie riêng từng task trong SQLite của bản fork aria2 | **Bỏ** Cookie/`Authorization` khi khác origin (fork `HttpRequest.cc` L168–182) |
| Aria2-Explorer (`5e97685`) | `getAll({url, storeId})` + lần nữa với `partitionKey: {}`; `incognito: split` | `<all_urls>`, cookies, downloads | Chỉ gửi cookie khi RPC là localhost/https/wss | aria2 RPC, secret mặc định rỗng | Tuỳ cấu hình aria2 (chưa kiểm) | aria2 gốc: không bỏ |
| uGet (`b60810f`, integrator `94a13a7`) | `getAll({url: gốc origin})`: sót cookie có path | `<all_urls>`, cookies, webRequest, downloads, nativeMessaging (MV2) | Không hỏi | Native messaging → Python → CLI | File tạm tên cố định `$TMP/uget_cookie`, quyền mặc định, xoá sau khi CLI xong | chưa kiểm |
| Persepolis (`109e952`, ext `6ecc318`) | `getAll({url})` → `"a=b; c=d"` | `*://*/`, cookies, downloads, nativeMessaging | Không hỏi | Native messaging | Có, cột `load_cookies` trong SQLite; đọc mã thấy cookie bị rơi trên Linux/mac (sai tên khoá) | chưa kiểm |
| Video DownloadHelper coapp (`f3736ce`) | Extension đóng mã (chưa kiểm) | chưa kiểm | chưa kiểm | Native messaging, `allowed_origins` | **Không**: chỉ ở RAM, bỏ 60 s sau khi xong | **Bỏ** (got 11.8.6 xoá `cookie`/`authorization` khi đổi host); nhưng ffprobe nhận `-headers` trên dòng lệnh |
| MyJDownloader MV3, bên thứ ba (`fb045b6`) | Không xuất cookie: tự `fetch(credentials: 'include')` trong trình duyệt, chỉ gửi URL | `<all_urls>`, webRequest | — | Cloud MyJD / CNL `127.0.0.1:9666` | Chỉ URL | — |

### C.2 App giao diện cho yt-dlp

| App (commit) | Nguồn cookie | Đưa cho yt-dlp | Nơi lưu | Xoá? | Mặc định | `--ignore-config` |
|---|---|---|---|---|---|---|
| Parabolic (`1118e6a`) | File người dùng chọn, hoặc trình duyệt (Windows chỉ còn Firefox) | `--cookies <file người dùng>` hoặc `--cookies-from-browser` | File của người dùng; app lưu đường dẫn | Không | Tắt | **Có** (`YtdlpExecutableService.cs` L70, L132) |
| Open Video Downloader (`c402ee3`) | File chọn, hoặc danh sách trình duyệt (có Chrome) | Có thể truyền cả hai cờ | File của người dùng; mật khẩu/token lưu mã hoá (Stronghold), **cookie thì không** | Không | Tắt | Không |
| ytDownloader (`60f52ec`) | Người dùng dán chữ Netscape, hoặc trình duyệt | `--cookies <userData>/cookies.txt` | localStorage + file 0600, chữ thường, ghi lại mỗi lần mở cài đặt | Chỉ khi xoá ô chữ | Tắt | Không |
| Seal, Android (`7677f61`) | Đăng nhập trong WebView của app; lấy **mọi** cookie, không lọc host (`DownloadUtil.kt` L374–422) | `--cookies <cacheDir>/cookies.txt` + `--add-header User-Agent:<UA của WebView>` (L361–366) | Thư mục cache riêng của app, chữ thường | Không rõ (chưa kiểm) | Tắt | Không |
| Tartube (`823e873`) | Đường dẫn file, hoặc trình duyệt + keyring + profile | **Luôn** `--cookies <data_dir>/cookies.txt`, ghi rõ là để chặn yt-dlp ghi file vào thư mục làm việc (`mainapp.py` L764–769) | Thư mục dữ liệu, giữ mãi | Không | Trình duyệt tắt | Không |
| yt-dlg (`574ed2e`) | Không có, chỉ ô "tham số thêm" | — | — | — | — | **Có** (mặc định) |
| cobalt (`a636575`) | `cookies.json` do người chạy máy chủ cung cấp | Không dùng yt-dlp; tự gửi `Cookie`, gộp `Set-Cookie`, ghi lại file mỗi 60 s | File máy chủ, JSON chữ thường | Không | — | — |
| Media Downloader (`616c052`) | File chọn hoặc gõ tên trình duyệt | `--cookies-from-browser` và/hoặc `--cookies` | File của người dùng | Không | Tắt | **Có** |

Thêm hai ghi chú:
- **Lịch sử "Get cookies.txt":** extension cookies.txt gốc bị báo gửi URL và sau đó gửi cả cookie về máy chủ của
  tác giả ([ytdl-org/youtube-dl#31465](https://github.com/ytdl-org/youtube-dl/issues/31465), 2023). Wiki yt-dlp
  đổi sang khuyên dùng "Get cookies.txt LOCALLY" (commit wiki `1043ec2`, `7256bac`). Bản LOCALLY xin
  `<all_urls>` + cookies + downloads; trong mã không thấy gọi mạng.
- **Chrome trên Windows (App-Bound Encryption):** Parabolic bỏ Chrome/Edge khỏi danh sách trên Windows (commit
  `a95108bf5e`, "Manual txt files should be used instead"). Wiki ytDownloader ghi chỉ Firefox chạy được.
  Không app nào nhận diện hai lỗi tương ứng của yt-dlp.

### C.3 Cách phổ biến, và UTD khác ở đâu

| | Cách phổ biến | Kế hoạch UTD |
|---|---|---|
| Quyền extension | Mọi trang, bắt buộc ngay lúc cài | Tuỳ chọn, theo tên miền gốc, xin khi trang cần (§4.2) |
| Lúc đọc cookie | Mọi download, thường không hỏi | Chỉ trong cú bấm, chỉ trang đã cho phép (§4.3) |
| Định dạng | Một chuỗi `Cookie:` phẳng | Cookie có cấu trúc, gửi theo domain/path/secure/SameSite (§5.3), như Motrix 2.x |
| Lưu | Chữ thường cạnh task, để tải tiếp sau khi khởi động lại | Chỉ RAM; khởi động lại thì xin lại bằng 2 cú bấm (§6). Đây là điểm duy nhất UTD kém tiện hơn, do P-02 buộc |
| Đưa cho yt-dlp | File cookies.txt trên đĩa, hoặc `--cookies-from-browser` | Named pipe, kiểm đã đọc (§5.1) |
| Chuyển hướng sang miền khác | Đa số vẫn gửi Cookie | Tính lại theo miền ở mỗi bước (giống got, fork aria2 của Motrix) |
| Kênh extension → app | HTTP localhost, token tuỳ chọn hoặc không có | Native messaging + pipe có token; thêm DACL (GĐ0) |
| `--ignore-config` | 3/8 app | Mọi lệnh (GĐ0) |

**Đã học và đưa vào kế hoạch:**
1. Gửi kèm User-Agent của trình duyệt, như Seal (§4.2).
2. Kiểm yt-dlp đã thật sự đọc cookie, vì `os.access` sai thì yt-dlp bỏ qua không báo (§5.1, T-P1).
3. Danh sách miền nhạy cảm, như Motrix 2.x (Q10).
4. Takeover tạm dừng rồi trả lại trình duyệt, như motrix-webextension bản cũ; tránh lỗi mất download của AB
   Download Manager (§4.4).

**Cố ý không chép:**
- Đọc mọi request bằng `webRequest` + `extraHeaders` (XDM, AB Download Manager, Gopeed): phải có quyền mọi trang
  và thấy toàn bộ lưu lượng.
- Lưu chuỗi cookie cạnh task.
- File cookie cố định trên đĩa.
- HTTP localhost không token.
- Truyền header/cookie trên dòng lệnh (`ffprobe -headers` của Video DownloadHelper).

**Kiểm chéo:** agent thứ hai dựng lại pipe bằng CPython 3.11 + ctypes (không phải yt-dlp.exe) và ra cùng kết luận
với bài thử ở `auth-thu/`: `os.access` tốn một lần nối, lần nối thứ hai đọc được, lần thứ ba nhận phần ghi lại
(xuống dòng CRLF).
