# Ra bản mới UTD (Windows + macOS)

Hướng dẫn ngắn cho chủ dự án. Máy CI trên GitHub làm hết phần khó cho **cả hai nền tảng**; bạn chỉ đổi số phiên bản, gắn tag rồi bấm Publish.

## Các bước

1. **Đổi số phiên bản** (ví dụ `0.2.0`) ở cả 3 chỗ, cùng một số:
   - `src-tauri/tauri.conf.json` → `"version"` (số này in lên bộ cài và app)
   - `Cargo.toml` ở thư mục gốc → `[workspace.package]` → `version`
   - `package.json` → `"version"` (hiện còn `0.0.1`, đặt cùng số cho gọn)
2. **Commit** thay đổi rồi **gắn tag** có chữ `v` đứng trước:
   ```
   git tag v0.2.0
   git push origin main v0.2.0
   ```
3. **CI tự chạy** (file `.github/workflows/ci.yml`, khoảng 20–30 phút):
   - kiểm tra trên Windows **và** macOS → kiểm số phiên bản và khoá ký bản cập nhật
   - dựng **song song**: bộ cài Windows (NSIS) và bản macOS (một file `.dmg` chạy cả Mac chip Apple lẫn Mac Intel)
   - viết `latest.json` (tự cập nhật cho cả hai nền tảng) → tính `SHA256SUMS.txt` → tạo **bản nháp** ở trang Releases.

   Tag khác số trong `tauri.conf.json` hoặc `Cargo.toml`, hoặc chưa có khoá ký bản cập nhật, thì CI dừng và báo lỗi (xem [Tự cập nhật](#tự-cập-nhật)).
4. **Mở trang Releases** trên GitHub, đọc lại ghi chú, bấm **Publish release**. Chưa bấm thì người ngoài chưa thấy, và app của người dùng cũng chưa thấy bản mới.
5. **Tự kiểm** (tuỳ chọn): tải file về rồi so mã với dòng tương ứng trong `SHA256SUMS.txt`:
   - Windows (PowerShell): `Get-FileHash .\UTD_0.2.0_x64-setup.exe`
   - macOS (Terminal): `shasum -a 256 UTD_0.2.0_universal.dmg`

Chạy lại job khi bản còn nháp thì file được thay bằng file mới; bản đã Publish thì CI không ghi đè.

### Bản nháp có 8 file (đừng xoá hay đổi tên)

| File | Cho ai |
|---|---|
| `UTD_0.2.0_x64-setup.exe` | Người dùng Windows tải về để cài |
| `UTD_0.2.0_x64-setup.exe.sig` | Chữ ký tự cập nhật của bộ cài Windows |
| `UTD_0.2.0_universal.dmg` | Người dùng Mac tải về để cài |
| `UTD_0.2.0_universal.app.tar.gz` | Gói app dùng cho tự cập nhật trên Mac (người dùng không cần tải) |
| `UTD_0.2.0_universal.app.tar.gz.sig` | Chữ ký tự cập nhật của gói Mac |
| `latest.json` | App đọc để biết có bản mới |
| `SHA256SUMS.txt` | Mã kiểm tra của mọi file trên |

(Trang Releases tự thêm 2 file mã nguồn `.zip`/`.tar.gz`, không tính.)

## Windows: bộ cài làm gì

- Cài cho người dùng hiện tại, không cần quyền admin, vào `%LOCALAPPDATA%\UTD\UTD.exe`.
- Gỡ cài: xoá app và tích hợp trình duyệt (đăng ký native messaging + `utd-native-host.exe`). **Giữ** lịch sử tải, cài đặt, khu cách ly và mọi file đã tải (AC-24).
- Ô **"Xoá dữ liệu ứng dụng"** ở trang gỡ: xoá thêm dữ liệu của app trong `%LOCALAPPDATA%\UTD-Data` (lịch sử, cài đặt, công cụ media, bản sao lưu, log), theo đúng tên tệp app tạo ra. **Giữ** khu cách ly `UTD-Data\quarantine` (file đã tải mà Download Guard giữ lại) và mọi tệp lạ; thư mục chỉ mất khi đã rỗng. Không bao giờ xoá thư mục tải về (app cũng không cho chọn thư mục tải nằm trong `UTD-Data`).
- Cập nhật đè bản mới (`/UPDATE`) thì không gỡ gì. Chọn "Gỡ trước khi cài" khi cài bản mới thì vẫn giữ tích hợp trình duyệt.
- Phần này nằm ở `src-tauri/windows/hooks.nsh`.

## macOS: cài, mở lần đầu, gỡ

Cần macOS 11 (Big Sur) trở lên, Mac chip Apple hay Intel đều được (một bản "universal").

**Cài**: mở file `.dmg` → kéo biểu tượng **UTD** vào thư mục **Applications** → tháo đĩa UTD (nút ⏏ trong Finder). Không có bước cài nào khác.

**Mở lần đầu** — UTD chưa được Apple "công chứng" (notarize: cần tài khoản Apple Developer, xem [Ký mã](#ký-mã-code-signing)), nên macOS chặn lần mở đầu tiên. Gửi người dùng đoạn này:

> 1. Mở **Applications**, nhấp đúp **UTD**. macOS báo không kiểm được UTD → bấm **Done** (đừng bấm Move to Trash).
> 2. Mở **System Settings** (Cài đặt hệ thống) → **Privacy & Security** (Quyền riêng tư & Bảo mật) → kéo xuống mục **Security**, thấy dòng *"UTD" was blocked…* → bấm **Open Anyway** (Vẫn mở) → nhập mật khẩu máy hoặc Touch ID → **Open**.
> 3. Từ lần sau mở bình thường. macOS 14 trở về trước còn cách nhanh: chuột phải (hoặc Control + bấm) vào UTD → **Open** → **Open**.
>
> Cách dùng Terminal (cho người quen): `xattr -dr com.apple.quarantine /Applications/UTD.app` rồi mở UTD. Terminal có thể xin quyền "quản lý app" lần đầu, bấm cho phép.

Lần tải đầu tiên vào `~/Downloads/UTD`, macOS hỏi *"UTD muốn truy cập tệp trong thư mục Tải về"* → bấm **Allow**. Vì chỉ ký ad-hoc, sau mỗi lần tự cập nhật macOS **có thể** hỏi lại câu này một lần.

**Mở UTD thẳng từ file .dmg** (không kéo vào Applications) vẫn chạy được, nhưng tích hợp trình duyệt sẽ báo "Kéo UTD vào thư mục Applications…" vì app nằm ở chỗ tạm sẽ biến mất.

**Dữ liệu và tích hợp trên Mac**:

| Gì | Ở đâu |
|---|---|
| App | `/Applications/UTD.app` |
| Lịch sử, cài đặt, công cụ media, khu cách ly, sao lưu | `~/Library/Application Support/UTD-Data` |
| Thư mục tải mặc định | `~/Downloads/UTD` |
| Tích hợp trình duyệt | script `UTD-Data/utd-native-host` + file `app.utd.desktop.json` trong `~/Library/Application Support/<Google/Chrome, Microsoft Edge, BraveSoftware/Brave-Browser, Chromium, Vivaldi, Mozilla>/NativeMessagingHosts/` |

**Gỡ**: thoát UTD (⌘Q) → kéo `UTD.app` từ Applications vào Thùng rác. Muốn xoá sạch thì xoá thêm thư mục `UTD-Data` và các file `app.utd.desktop.json` ở bảng trên (Finder → Go → Go to Folder…, dán đường dẫn). File đã tải và khu cách ly chỉ mất khi bạn tự xoá.

## Dựng thử trên máy

- **Windows**: `pnpm tauri build` → `target\release\UTD.exe` và bộ cài ở `target\release\bundle\nsis\`. `pnpm tauri dev` vẫn dùng `target\debug\app.exe`.
- **macOS**: cần Xcode Command Line Tools, Node + pnpm, Rust (rustup). `rust-toolchain.toml` ghim bản Windows, nên trên Mac luôn đặt `RUSTUP_TOOLCHAIN=stable`:
  ```
  RUSTUP_TOOLCHAIN=stable pnpm tauri build
  ```
  Ra `target/release/bundle/macos/UTD.app` và `target/release/bundle/dmg/UTD_<số>_aarch64.dmg` (chỉ chip của máy đang dựng). Muốn bản universal như CI: `rustup target add aarch64-apple-darwin x86_64-apple-darwin` rồi thêm `--target universal-apple-darwin`.
- Chạy thử song song với bản thật: đặt `UTD_DATA_DIR=<thư mục tuyệt đối>` (dữ liệu riêng). Trên Mac: `open -n --env UTD_DATA_DIR=$HOME/utd-thu target/release/bundle/macos/UTD.app`.
- Cấu hình riêng cho Mac nằm ở `src-tauri/tauri.macos.conf.json` (`.app` + `.dmg`, ký ad-hoc, macOS 11+, không gắn điều khoản giấy phép vào .dmg) và `src-tauri/Info.plist` (câu giải thích khi xin quyền thư mục).

## Ký mã (code signing)

Chưa ký thì hệ điều hành cảnh báo khi mở lần đầu. Cả hai **đang tắt**; **chữ ký tự cập nhật (mục dưới) không phụ thuộc vào ký mã** nên tự cập nhật vẫn chạy.

### Windows

Chưa ký thì Windows SmartScreen cảnh báo "Windows protected your PC" khi mở bộ cài (người dùng bấm *More info* → *Run anyway*). Hai cách bật:

| Cách | Chi phí | Cắm vào đâu |
|---|---|---|
| **SignPath Foundation** | Miễn phí cho dự án mã nguồn mở, phải nộp đơn và được duyệt | Thêm bước GitHub Action của SignPath trong job `build-windows`, sau "Build NSIS installer" và trước "Collect Windows files". Muốn ký cả `UTD.exe` bên trong thì tách build thành `pnpm tauri build --no-bundle` → ký `UTD.exe` → `pnpm tauri bundle --bundles nsis` → ký bộ cài. |
| **Azure Trusted Signing** | Khoảng 10 USD/tháng, cần xác minh danh tính | Dùng sẵn móc trong CI (dưới đây). Tauri tự ký `UTD.exe`, `uninstall.exe` và bộ cài. |

Móc ký có sẵn trong CI (tắt mặc định). Bật ở GitHub → *Settings* → *Secrets and variables* → *Actions*:

- Biến (Variables): `WINDOWS_SIGNING` = `true`
- Secret `WINDOWS_SIGN_COMMAND`: lệnh ký, `%1` là file cần ký. Với Azure:
  `trusted-signing-cli -e https://<vùng>.codesigning.azure.net -a <tài khoản> -c <hồ sơ chứng chỉ> -d UTD %1`
- Secret cho Azure: `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID` (lấy trong Azure, không ai nhập hộ bạn).
- Thêm một bước cài công cụ ký (`cargo install trusted-signing-cli`) trước "Build NSIS installer". Nhờ AI làm bước này khi có tài khoản.

Ký xong mới tính `SHA256SUMS.txt`, nên mã kiểm tra luôn khớp file đã ký.

Lưu ý với cách ký **sau** khi dựng (SignPath): ký mã làm đổi byte của bộ cài nên file `.sig` của tự cập nhật hết khớp và app sẽ từ chối bản đó. Khi thêm bước SignPath thì phải ký lại `.sig` (`pnpm tauri signer sign --app-version <số> <bộ cài>`, dùng 2 secret của mục Tự cập nhật) rồi mới chạy bước "Collect Windows files". Cách Azure qua `signCommand` không bị vướng: Tauri ký mã trước, ký `.sig` sau.

### macOS

Hiện app chỉ được ký **ad-hoc** (`signingIdentity: "-"`): đủ để chạy trên Mac chip Apple, nhưng Gatekeeper vẫn chặn lần mở đầu (người dùng làm theo [Mở lần đầu](#macos-cài-mở-lần-đầu-gỡ)). Muốn mở thẳng không cảnh báo:

1. Đăng ký **Apple Developer Program** (99 USD/năm), tạo chứng chỉ **Developer ID Application**.
2. Thêm secret trên GitHub: `APPLE_CERTIFICATE` (file .p12 dạng base64), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, và để công chứng: `APPLE_ID`, `APPLE_PASSWORD` (mật khẩu ứng dụng), `APPLE_TEAM_ID`.
3. Truyền các secret đó vào bước "Build macOS app and DMG" và bỏ `"signingIdentity": "-"` trong `src-tauri/tauri.macos.conf.json`. Tauri tự ký, công chứng và đóng dấu (staple). Nhờ AI làm bước này khi có tài khoản.

Tauri ký mã trước rồi mới ký `.sig` cho tự cập nhật, nên không vướng như SignPath ở Windows.

## Bảo mật trang (CSP)

`src-tauri/tauri.conf.json` → `app.security.csp` chỉ cho trang app tải đúng những gì cần:

| Mục | Giá trị | Vì sao |
|---|---|---|
| `default-src` | `'self'` | Mặc định chỉ file đóng gói trong app |
| `script-src` | `'self'` | Chỉ script đóng gói; Tauri tự thêm mã băm. Không `unsafe-eval`, không script ngoài mạng |
| `style-src` | `'self' 'unsafe-inline'` | CSS đóng gói + thuộc tính `style` mà Tailwind/Base UI gắn lúc chạy (định vị menu, hộp thoại) |
| `img-src` | `'self' data: https:` | `data:` cho icon file thật của Windows/macOS (`files/icons.rs` trả ảnh PNG dạng `data:image/png;base64,…`; ảnh không chạy được script); `https:` cho ảnh xem trước video (yt-dlp trả về link ảnh ở nhiều trang khác nhau) |
| `font-src` | `'self'` | Phông Inter đóng gói sẵn |
| `connect-src` | `ipc: http://ipc.localhost` | Kênh nói chuyện giữa giao diện và phần Rust của Tauri (macOS dùng `ipc:`, Windows dùng `http://ipc.localhost`) |
| `object-src`, `base-uri`, `form-action`, `frame-ancestors` | `'none'` | Không plugin, không đổi gốc link, không gửi form, không bị nhúng |

Thêm `<style>` vào `index.html` thì Tauri gắn nonce và `'unsafe-inline'` hết tác dụng: đặt CSS trong file `.css` thay vì vậy.

## Công cụ

- pnpm ghim `12.9.1` qua `"packageManager"` trong `package.json`; CI đọc từ đó (`pnpm/action-setup@v6.1.0`).
- Không cần server: GitHub Actions (kể cả máy macOS) và Releases miễn phí cho repo công khai.

## Tự cập nhật

App UTD tự tìm bản mới ở trang Releases của repo công khai https://github.com/1Scout-Co/UTD và chỉ cài gói **có chữ ký của bạn**. Một khoá ký dùng chung cho Windows và macOS.

### App làm gì

- Đọc đúng một file: `https://github.com/1Scout-Co/UTD/releases/latest/download/latest.json`. Lúc mở app (sau ~15 giây, nếu bật "Tự kiểm tra cập nhật" trong Cài đặt) và khi bấm **Kiểm tra**. Không gửi số phiên bản, mã máy hay thông tin download nào (P-03); GitHub chỉ thấy địa chỉ IP như khi mở một trang web.
- `latest.json` có 3 mục: `windows-x86_64` (bộ cài), `darwin-aarch64` và `darwin-x86_64` (cùng trỏ gói Mac universal). App tự chọn đúng mục của máy mình.
- Bấm **Cập nhật**: tải gói, **kiểm chữ ký** bằng khoá công khai gắn trong app (`src-tauri/tauri.conf.json` → `plugins.updater.pubkey`). Sai chữ ký thì từ chối, giữ nguyên bản đang dùng (AC-20). Chữ ký còn ghi số phiên bản (`requireSignedVersion`), nên không ai lấy gói cũ có chữ ký thật để ép hạ cấp.
- Chữ ký đúng: tạm dừng các download đang chạy, sao lưu lịch sử tải vào `UTD-Data/backup/utd-<bản cũ>.db` (giữ 3 bản gần nhất), rồi
  - **Windows**: chạy bộ cài ở chế độ chỉ hiện thanh tiến độ, bộ cài mở lại UTD;
  - **macOS**: thay `UTD.app` bằng bản mới ngay tại chỗ rồi mở lại;

  và UTD chạy tiếp các download vừa dừng.
- Bản này chưa có khoá hoặc cấu hình hỏng: app vẫn chạy bình thường, chỉ mục Cập nhật báo "chưa bật".

### Khoá ký (đọc kỹ)

| File | Là gì | Ai được thấy |
|---|---|---|
| `C:\Users\Admin\.tauri\utd-updater.key` | **Khoá bí mật**, dùng để ký mọi bản cập nhật | Chỉ bạn và secret trên GitHub. Không gửi cho ai, không đưa vào repo, không dán vào chat |
| `C:\Users\Admin\.tauri\utd-updater.key.pub` | Khoá công khai, đã chép vào `tauri.conf.json` | Ai cũng được thấy |

- **Sao lưu ngay** khoá bí mật và mật khẩu của nó (ví dụ trình quản lý mật khẩu + một USB cất riêng). **Mất khoá hoặc quên mật khẩu = mọi máy đang cài UTD (Windows lẫn Mac) không bao giờ nhận được bản cập nhật tự động nữa**; người dùng phải tự tải bản mới.
- Lộ khoá: người có khoá ký được bản giả mà app sẽ tin. Tạo khoá mới, ra ngay một bản mang khoá công khai mới (bản đó vẫn ký bằng khoá cũ), rồi từ bản sau mới ký bằng khoá mới. Nhờ AI làm cùng khi cần.

### Thêm khoá vào GitHub (một lần, bạn tự làm)

Cần [GitHub CLI](https://cli.github.com) đã đăng nhập (`gh auth login`). Mở **Command Prompt** (cmd) hoặc Git Bash rồi chạy:

```
gh secret set TAURI_SIGNING_PRIVATE_KEY -R 1Scout-Co/UTD < "C:\Users\Admin\.tauri\utd-updater.key"
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD -R 1Scout-Co/UTD
```

Lệnh thứ hai hỏi giá trị: dán mật khẩu đã đặt khi tạo khoá rồi Enter. Dùng PowerShell thì thay lệnh thứ nhất bằng:

```
Get-Content -Raw "C:\Users\Admin\.tauri\utd-updater.key" | gh secret set TAURI_SIGNING_PRIVATE_KEY -R 1Scout-Co/UTD
```

Lúc tạo khoá không đặt mật khẩu thì bỏ lệnh thứ hai, thay bằng `gh variable set UPDATER_KEY_NO_PASSWORD -R 1Scout-Co/UTD --body true`. Thiếu khoá thì CI dừng ở bước "Check updater signing key" và nói rõ thiếu gì; khoá không khớp khoá công khai thì bước dựng cảnh báo trong log. Hai secret này dùng cho cả hai job dựng (Windows và macOS), không cần thêm gì cho Mac.

### Phát bản cập nhật

1. Làm như [Các bước](#các-bước) ở trên. Muốn ghi chú hiện trong app thì trước khi gắn tag tạo file `docs/releases/v0.2.0.md` (vài dòng ngắn, tối đa ~1500 ký tự); không có file thì app hiện "UTD 0.2.0" kèm link trang Releases.
2. Kiểm bản nháp có đủ [8 file](#bản-nháp-có-8-file-đừng-xoá-hay-đổi-tên).
3. Bấm **Publish release** (để mặc định "Set as the latest release", không tick "pre-release"). Ngay lúc đó `releases/latest/download/latest.json` trỏ sang bản mới: app nào mở sau ~15 giây sẽ thấy, và nút **Kiểm tra** thấy ngay.

Ghi chú: `pnpm tauri build` trên máy bạn không ký `.sig` và không cần khoá (chỉ CI bật `src-tauri/tauri.updater.conf.json`).

### Quay lại khi bản mới có lỗi

- **Chặn người khác nhận bản lỗi**: mở bản đó trên trang Releases → *Edit* → tick **Set as a pre-release** rồi lưu. `latest` lập tức trỏ về bản trước, app không còn thấy bản lỗi (bỏ tick là phát lại được).
- **Máy đã lên bản lỗi**: app không tự hạ cấp (cố ý, để chống ép hạ cấp). Cách nhanh nhất cho mọi người: ra bản sửa có số **lớn hơn** (ví dụ `0.2.1` chứa code của bản tốt) là app tự cập nhật lên. Từng máy cũng có thể tải bản cũ ở Releases và cài đè (Mac: kéo `UTD.app` của bản cũ vào Applications, chọn Replace).
- **Dữ liệu**: lịch sử tải trước khi cập nhật nằm ở `UTD-Data/backup/utd-<bản cũ>.db` (Windows: `%LOCALAPPDATA%\UTD-Data`, Mac: `~/Library/Application Support/UTD-Data`). Cần quay lại thì đóng UTD, xoá `utd.db-wal` và `utd.db-shm` (nếu có) trong `UTD-Data`, chép file sao lưu đè lên `utd.db` rồi mở lại. File đã tải về không bị đụng tới.
