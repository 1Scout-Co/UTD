# Việc (5): UTD trên macOS + phát hành Windows/macOS

Nhánh `claude/macos-ab0b40` (từ `main` @ 9503965). Kiểm trên Mac mini thật: macOS 27.0.1, Apple Silicon (arm64),
Rust 1.99.1 (rustup bản người dùng), Node 24 + pnpm, Xcode Command Line Tools.

## Đã làm

| Phần | macOS làm gì | File |
|---|---|---|
| Thư mục dữ liệu | `~/Library/Application Support/UTD-Data`; chế độ thử `UTD_DATA_DIR` giữ nguyên | `ipc.rs::data_dir` |
| Thư mục tải mặc định | `~/Downloads/UTD` (Tauri `download_dir`, có sẵn); ổ ngoài `/Volumes/<tên>` rút ra ⇒ về mặc định | `commands.rs::volume_root` |
| IPC native host ↔ app | Unix domain socket `<data>/ipc.sock` quyền 0600 (thư mục dữ liệu dài quá `sun_path` 104 byte, vd bản thử trong `$TMPDIR` ⇒ `<thư mục tạm riêng của người dùng>/utd-<băm>.sock`), socket sót lại thì thay, bind lỗi thì thử lại; `ipc.json` tạo quyền 0600; vẫn kiểm token như named pipe | `ipc.rs`, `native_host.rs` |
| Native host | script `<data>/utd-native-host` (`UTD_NATIVE_HOST=1 exec …/UTD.app/Contents/MacOS/UTD "$@"`), không chép binary ra ngoài bundle (giữ chữ ký, cập nhật xong vẫn đúng bản). App bị kéo sang chỗ khác ⇒ lúc mở viết lại script. Chạy từ .dmg hay AppTranslocation (ổ chỉ đọc) ⇒ báo "Kéo UTD vào Applications…" | `browser_integration/mac.rs` |
| Đăng ký trình duyệt | chép manifest `app.utd.desktop.json` vào `~/Library/Application Support/<Google/Chrome, Chrome Beta, Chrome Canary, Chromium, Microsoft Edge, BraveSoftware/Brave-Browser, Vivaldi, Mozilla>/NativeMessagingHosts/` — chỉ trình duyệt đã cài | `browser_integration/mac.rs` |
| Mở file / mở thư mục chứa | `/usr/bin/open` / `open -R` (chọn sẵn file trong Finder); thư mục dạng gói (`X.app`) thì chỉ chọn sẵn, không chạy | `files/shell/mac.rs` |
| Thùng rác | `NSFileManager.trashItemAtURL` (objc2), ổ không có Thùng rác ⇒ báo, không bao giờ xoá hẳn | `files/shell/mac.rs` |
| Icon thật | `NSWorkspace.iconForFile` / theo đuôi, vẽ ra bitmap RGBA đúng cỡ (32/48/64) | `files/icons/mac.rs` |
| Đổi tên không ghi đè | `renamex_np(RENAME_EXCL)`; đổi hoa/thường trên APFS (không phân biệt hoa thường) vẫn được; exFAT ⇒ link rồi gỡ | `files/disk.rs` |
| Mã lỗi file | errno của mac (17 là "đã có", khác Windows) | `files/disk.rs` |
| MOTW | xattr `com.apple.quarantine` = `0081;<hex giây>;UTD;` (file tải xong, file chuyển khác ổ) | `utd-engine/src/safety` |
| Guard | thêm loại nguy hiểm khi chạy trên mac: app, dmg, pkg, mpkg, command, tool, terminal, workflow, action, scpt(d), applescript, webloc, inetloc, fileloc, prefpane, kext, plugin, bundle, dylib, osax, qlgenerator, saver, appex, xpc, mobileconfig; nhận ra Mach-O "universal" (CA FE BA BE, khác file .class Java) | `utd-engine/src/safety` |
| Dò yt-dlp/ffmpeg | thêm `/opt/homebrew/bin`, `/usr/local/bin`, `/opt/local/bin` (app mở từ Finder không có PATH của shell) | `media/tools.rs` |
| App Nap | macOS hạ app bị che/khoá màn hình xuống mức nền (đo: luồng ưu tiên 4). Có download chạy ⇒ giữ `NSProcessInfo.beginActivity(UserInitiatedAllowingIdleSystemSleep)` (đo: ưu tiên 46), hết download thì nhả | `app_nap.rs`, `state.rs` |
| Menu app | menu mặc định của Tauri trên mac: About UTD, Hide, Quit (⌘Q), Edit (Cut/Copy/Paste/Select All — cần để ⌘C/⌘V chạy trong ô nhập), Window | (sẵn có) |
| Phím tắt | ô tìm hiện `⌘ K` trên Mac (`Ctrl K` trên Windows; bắt phím vốn nhận cả hai) | `soniox-toolbar.tsx` |
| `legacy.rs` | chỉ Windows (mac trả None) — có sẵn | — |
| Đóng gói | `tauri.macos.conf.json`: `.app` + `.dmg`, ký ad-hoc (`signingIdentity: "-"`), macOS 11+, bỏ điều khoản giấy phép khỏi .dmg (không phải bấm Agree); `Info.plist`: câu giải thích quyền Downloads/Desktop/Documents/ổ ngoài/ổ mạng; icon `.icns` có đủ 16→1024 | `src-tauri/` |
| Toolchain | `rust-toolchain.toml` vẫn ghim MSVC cho Windows (máy dev mặc định GNU); mac dùng `RUSTUP_TOOLCHAIN=stable` | `rust-toolchain.toml` |
| Thông báo updater | "Bản mới chưa có gói cho macOS." khi `latest.json` thiếu mục mac | `updater.rs` |
| CI | job `test-macos` (macos-latest: test + clippy 3 crate); phát hành: `release-check` → `build-windows` ‖ `build-macos` (universal) → `release` gộp `latest.json` (`windows-x86_64`, `darwin-aarch64`, `darwin-x86_64`), `SHA256SUMS.txt` 7 file, bản nháp | `.github/workflows/ci.yml`, `scripts/release/latest-json.mjs` |
| Tài liệu | `docs/RELEASE.md` viết lại cho 2 nền tảng (mở lần đầu khi chưa notarize, gỡ, dữ liệu ở đâu, ký mã Apple sau này) | `docs/RELEASE.md` |

## Đã kiểm trên Mac thật

- `cargo test` cả 3 crate: utd-engine 141 bài (64 + 77 tích hợp), utd-mp4 14, app 123 — xanh (Windows cũng 141 / 14 / 123: bài chỉ-Windows và chỉ-mac thay chỗ nhau). `cargo clippy --all-targets -D warnings` cả 3 crate — sạch trên cả hai máy.
  Test mới chạy trên mac: Thùng rác thật, icon Finder thật (32/48/64 px), xattr quarantine, socket 0600 + nói chuyện một dòng mỗi chiều, script host chạy được, manifest chỉ vào trình duyệt đã cài, đổi tên hoa/thường, ổ ngoài đã rút.
- `pnpm tauri build` ra `UTD.app` (18 MB) + `UTD_0.1.0_aarch64.dmg` (6 MB), ký ad-hoc (`codesign -dv`: `Signature=adhoc`, hardened runtime), `Info.plist` đủ các khoá.
- Cài như người dùng: .dmg gắn cờ quarantine như tải từ Safari → mount → chép `UTD.app`: cờ quarantine theo sang app, mở thì macOS giữ app lại chờ xác nhận (Gatekeeper) và chạy ở AppTranslocation — đúng như hướng dẫn "Mở lần đầu" trong RELEASE.md.
- App chạy (bản chép từ build, không cờ quarantine) với `UTD_DATA_DIR` riêng: tạo `ipc.json` (0600), `ipc.sock` (0600), journal, khu cách ly.
- Đường trình duyệt → native host → Unix socket → app: gửi tin nhắn native messaging y như Chrome (độ dài 4 byte + JSON) vào script host → app trả `{"ok":true,"taskId":…}` và bắt đầu tải.
- Tải HTTP lớn đa kết nối: 1 GiB (16 kết nối) từ máy chủ cục bộ có Range trong 1,6 s (~624 MiB/s); từ `sin-speed.hetzner.com` 1 GiB trong 164 s (6,3 MiB/s) — curl 16 kết nối song song cùng lúc chỉ được 7 MiB/s, curl 8 kết nối 12 MiB/s ⇒ giới hạn ở máy chủ/mạng khi mở nhiều kết nối, không phải macOS (xem "Việc còn lại").
- File tải xong mang `com.apple.quarantine: 0081;…;UTD;`.
- Guard cách ly thật: `chay.command`, `cai-dat.pkg`, `UTD-fake.dmg` (`guard.dangerous_type`), `/bin/ls` đổi tên `anh-dep.jpg` (`guard.extension_mismatch`, Mach-O universal); `bao-cao.pdf` qua. File cách ly nằm ở `<data>/quarantine/*.quarantined`, chỉ đọc.
- Ghép MP4 bằng Rust: luồng DASH thật của YouTube (`f399` AV1 1080p 400 MB + `f140` AAC 144 MB, video 2,5 giờ, tải bằng yt-dlp 2026.08.19 từ Homebrew) ghép trong ~0,4 s; ffprobe đọc đúng av1 + aac cùng thời lượng; ffmpeg giải mã 30 s đầu và đoạn cuối không lỗi.
- App Nap: luồng của UTD ở ưu tiên 4 khi màn hình khoá; có download thì lên 46; tải xong trở về 4.
- Menu bar đọc qua Accessibility: Apple, UTD (About UTD, Services, Hide UTD, Hide Others, Quit UTD), File, Edit (Undo, Redo, Cut, Copy, Paste, Select All…), View, Window, Help. Cửa sổ chính 1320×861.

### Giao diện, bấm thật trên Mac (màn hình đã mở khoá)

Ảnh ở `ui-demos/macos-*.png` (thu về 1x, cắt riêng cửa sổ, không lộ màn hình riêng của máy).

- Màn chính: bảng có icon thật của Finder (gói `.pkg` hiện icon Installer, `.command` icon script, `.mp4` icon YouTube…), ô tìm hiện `⌘ K` — `macos-01-main.png`.
- Thêm URL → gõ link 1 GB → Bắt đầu: tải 16 kết nối, 3,5 → 15 MB/s khi app đang hiện — `macos-02-downloading.png`.
- Chuột phải → **Tạm dừng** (journal `paused`, 273 MB) → **Tiếp tục** (tải nối từ 25%, không tải lại) → hoàn tất 1 GiB — `macos-03-context-menu.png`.
- **Mở file** (PDF thật tải về) → Preview mở đúng file. **Mở thư mục** → Finder mở thư mục tải và chọn sẵn file — `macos-06-finder-reveal.png`. **Xoá file (vào Thùng rác)** → file rời thư mục, hiện "Đã chuyển file vào Thùng rác.", dòng biến khỏi danh sách.
- Media: gửi link YouTube từ "trình duyệt" (native host) → app tự mở hộp **Tải media**, phân tích xong (ảnh, tên, thời lượng, mức chất lượng) — `macos-04-media-dialog.png`. "Me at the zoo" (19 s) xong ngay; video 2,5 giờ 1080p (544 MB) tải + ghép xong trong **9,4 s** (~58 MB/s), tên file tiếng Việt đúng, ffprobe: AV1 1920×1080 + AAC, có cờ quarantine.
- Cài đặt: thấy yt-dlp 2026.08.19 của Homebrew ("bản có sẵn trên máy"). **Cài extension** → trạng thái "Đã cài", tạo script host đúng (kèm `UTD_DATA_DIR` của bản thử), manifest ghi vào Chrome, Chromium, Edge, Brave, Vivaldi (Firefox chưa cài trên máy nên bỏ qua đúng), Finder mở thư mục extension — `macos-05-browser-integration.png`. Chạy đúng script mà manifest của Chrome trỏ tới (y như Chrome sẽ chạy) → app nhận link và tên file gợi ý. (Sau khi ghép: bản thử chỉ cài tích hợp khi đặt thêm `UTD_TEST_BROWSER_INTEGRATION=1`, vì đăng ký là của chung cả máy và sẽ đè bản đã cài.)
- **Đổi thư mục** → hộp chọn thư mục gốc của macOS ("Chọn thư mục tải") mở đúng thư mục hiện tại; Cancel không đổi gì.
- Menu UTD (`macos-07-app-menu.png`), **About UTD** (icon, phiên bản 0.1.0, bản quyền — `macos-08-about.png`), **⌘Q** thoát app.
- Mở lần đầu khi có cờ quarantine (macOS 27, tiếng Việt), chụp đủ 4 bước: *"UTD" chưa được mở* → Cài đặt hệ thống → Quyền riêng tư & Bảo mật → *"UTD" đã bị chặn…* **Vẫn mở** → hộp *Mở "UTD"?* (nút xanh mặc định là "Chuyển vào Thùng rác"!) → hỏi mật khẩu quản trị — `macos-gatekeeper-1..4`. Tôi dừng ở bước mật khẩu (bấm Huỷ, không nhập). RELEASE.md đã viết theo đúng các bước này.

Sự cố nhỏ lúc kiểm (đã xử lý): lệnh `osascript` tới Finder làm macOS hỏi cấp quyền tự động hoá cho `sshd-keygen-wrapper` → đã bấm **Từ chối**; một cú bấm lệch toạ độ mở cửa sổ "Giới thiệu về Safari" → đã đóng.

## Chưa kiểm được

- Extension thật nạp vào trình duyệt (Load unpacked) rồi bấm chuột phải trên trang web — chưa làm để không sửa hồ sơ trình duyệt của chủ máy; đã thử đúng script/manifest mà trình duyệt sẽ chạy.
- Hộp xin quyền thư mục Downloads: lúc tải vào `~/Downloads/UTD` macOS không hỏi gì (tải được), nên chưa chụp được hộp này.
- Kéo-thả file/link vào cửa sổ.
- Tự cập nhật trên mac từ bản này lên bản sau (cần bản phát hành mới hơn v0.1.0); hiện đã kiểm "Bạn đang dùng bản mới nhất" với `latest.json` thật.
- Chạy bản universal trên Mac Intel (chỉ có Mac chip Apple; `lipo` xác nhận có cả `x86_64`).

## Lỗi nhỏ thấy trên Mac (chưa sửa, ngoài phạm vi hoặc để phiên khác)

- Menu app tiếng Anh ("About UTD", "Hide UTD", "Quit UTD"…) dù macOS đặt tiếng Việt: menu mặc định của Tauri — phiên (1) đa ngôn ngữ có thể tự dựng menu theo ngôn ngữ.
- Cài đặt → yt-dlp ghi "Giấy phép: Unlicense (bản .exe có phần GPLv3+)" cả trên Mac — phiên (2).
- ~~Ô dung lượng đĩa ghi nhãn `/` trên Mac~~ — đã sửa: hiện tên ổ như Finder (`NSURLVolumeLocalizedNameKey`: "Macintosh HD", cả với thư mục người dùng nằm ở ổ Data), không có thì `/Volumes/<tên>`; Windows vẫn `C:` (`commands.rs::disk_label`). Ô dung lượng ở thanh bên tự xuống dòng khi tên dài ("Macintosh HD" trên, "34,5 GB trống" dưới); nhãn ngắn như `C:` vẫn chung một dòng (`soniox.tsx::StorageCard`). Kiểm bằng mắt trên app Mac thật và bản web.

## Đã dọn trên Mac

App thử, máy chủ thử, dữ liệu thử, manifest `app.utd.desktop.json` của 5 trình duyệt, `~/Library/{Caches,WebKit}/app.utd.desktop`, `defaults` của `app.utd.desktop` — đã xoá. Còn lại có chủ ý: `~/utd` (mã + bản build 5,8 GB, để kiểm thanh tiêu đề sau), `~/utd-test` (công cụ chụp/bấm + venv pyobjc, 44 MB), Rust (`~/.rustup`, `~/.cargo`), `yt-dlp` cài bằng Homebrew. **Thùng rác của Mac** còn 2 file thử do UTD chuyển vào (`bao-cao.pdf` 13 KB, `1Gb.dat.*.part` 1 GB) — chủ máy dọn Thùng rác khi tiện.

## Sau khi ghép vào main (e847c4d: thanh tiêu đề + 5 nhánh)

Nhánh này tua nhanh lên `e847c4d`, kiểm lại trên Mac thật:

- `cargo test`: utd-engine 143, utd-mp4 19, app 201 — xanh. `cargo clippy --all-targets -D warnings` 3 crate — sạch sau khi sửa `media/install_tests.rs` (`fetch_update` Rust 1.99 báo deprecated ⇒ vòng `compare_exchange_weak` như `engine/adaptive.rs`). Code chỉ-mac mới sau ghép (socket nhận tin nhắn hàng loạt/kênh, đường dự phòng `sun_path`, `app_nap.rs` mới, nhãn ổ, `titlebar` phần mac) biên dịch và chạy test được. Windows: 3 crate + clippy + lint + build + Playwright 65/65 xanh.
- Tự tải công cụ với `UTD_DATA_DIR` rỗng + `UTD_TOOLS_OWN_ONLY=1` (bỏ qua Homebrew): mở app là tự tải đủ `yt-dlp` (yt-dlp_macos universal, 2026.10.21), `deno` 2.9.8 (aarch64), `ffmpeg`/`ffprobe` 9.0.2 của Martin Riedl (ký Developer ID) vào `<data>/bin`, có quyền chạy, không mang cờ quarantine; chạy `--version` cả bốn đều được.
- **Lỗi tìm ra và đã sửa**: yt-dlp_macos (PyInstaller một file) đẻ một tiến trình con; giết tiến trình ngoài thì tiến trình con vẫn sống và vẫn ghi stdout (đo thật) ⇒ nút Dừng của Tải cả kênh trên Mac sẽ treo như lỗi Windows trước đây. Sửa: `tokio_command` đặt `process_group(0)`, `kill_tree` gửi `killpg(SIGKILL)` cho cả nhóm (`media/mod.rs`). Đo lại với yt-dlp_macos thật: tiến trình con chết theo, stdout đóng ngay. Test `kill_tree_also_stops_children_holding_the_pipe` giờ chạy cả Windows và macOS.
- Socket Unix nhận đúng tin nhắn mới từ extension: lô link ("Đã mở UTD để chọn link") và link kênh TikTok ("Đã mở UTD để tải cả kênh").
- (Phần giao diện lúc đó chờ Mac mở khoá — đã kiểm ở mục dưới.) Danh sách cũ: thanh tiêu đề (đèn giao thông thẳng hàng logo, kéo ở dải đầu, nhấp đúp theo cài đặt hệ thống, toàn màn hình, menu ⋯/chuột phải không lấn dải, hộp thoại vẫn kéo được, ảnh `titlebar-macos-*.png`), tải 1 video YouTube ra MP4 bằng bộ công cụ tự tải, hộp Thêm hàng loạt và Tải cả kênh (bấm Dừng thật trong app).

## Sau khi ghép bd1ef9c (v0.1.0) — giao diện trên Mac thật + bản phát hành

Nhánh tua nhanh lên `origin/main` bd1ef9c, dựng `.app` trên Mac, chạy chế độ `UTD_DATA_DIR` + `UTD_TOOLS_OWN_ONLY=1`.

Thanh tiêu đề (`ui-demos/titlebar-macos-*.png`):
- **Lỗi tìm ra, đã sửa**: đèn giao thông cao hơn logo ~4pt (đo pixel: tâm đèn 16pt, tâm logo và dải 40px 20pt). Tauri dùng `trafficLightPosition.y` sao cho đỉnh đèn ≈ `y − 9`pt (đo `y:18` ⇒ đỉnh 9pt, `y:26` ⇒ 17pt), nên đổi `y:18 → y:22` (`tauri.macos.conf.json`): tâm đèn 19,5pt, đỉnh đèn trùng đỉnh chữ "utd" — ảnh `titlebar-macos-5-truoc-y18-lech` / `-6-sau-y22-thang-hang`. Ghi cả vào `notes/titlebar.md`.
- Kéo ở dải trống của thanh: cửa sổ dời đúng (+100,+50) rồi về; kéo trong nội dung không dời. Đang mở hộp thoại (Thêm hàng loạt) vẫn kéo được, hộp vẫn mở.
- Nhấp đúp (máy để mặc định, `AppleActionOnDoubleClick` chưa đặt): phóng to (0,30 – 1920×983, chừa thanh menu + Dock), nhấp lần nữa khôi phục. Không đổi cài đặt hệ thống để thử "thu nhỏ"/"không làm gì" (có test Rust riêng).
- Toàn màn hình (⌃⌘F): đèn ẩn, logo về lề trái ~23pt, nội dung ngay dưới thanh, không chừa chỗ thừa; thoát về đúng cỡ cũ.
- Cửa sổ không focus: đèn xám, logo mờ — `titlebar-macos-2-khong-focus`.
- Menu chuột phải và menu ⋯ ở dòng trên cùng mở xuống dưới, không lấn lên dải 40px.
- Chế độ tối: giao diện Soniox tự đặt bảng màu sáng (`[data-variant="soniox"]`) nên app không có chế độ tối để kiểm. **Lỗi tìm ra, đã sửa**: theme mặc định là "system" và phím D (ngoài ô nhập) đổi sáng/tối ⇒ máy để Dark Mode, hoặc lỡ gõ D, thì menu chuột phải/⋯ (vẽ ngoài khung Soniox) thành nền đen trên trang sáng — thấy cả ở bản v0.1.0. Sửa: `ThemeProvider` ghim sáng, tắt phím D (prop `hotkey` mới), khoá lưu mới `utd-theme` bỏ giá trị "dark" cũ (`src/main.tsx`, `src/components/theme-provider.tsx`).
- **Lỗi tìm ra, đã sửa**: chuột phải vào chỗ trống hiện menu gốc của WebKit chỉ có "Reload" (bấm là tải lại cả giao diện). Sửa: chặn menu gốc ở mọi chỗ trừ ô nhập chữ (`src/shared/window/native-menu.ts`, gọi ở `main.tsx`); ô tìm vẫn có Cut/Copy/Paste, menu riêng của dòng vẫn chạy. Áp cả Windows (WebView2 có Back/Refresh/Print).

Tính năng sau ghép:
- Hộp **Thêm hàng loạt** mở bằng ⌘B; dán 3 link ⇒ tự đọc tên/kích thước, nhóm theo đuôi; "Bắt đầu ngay" chạy theo hàng đợi (1 tải, 1 chờ) — `macos-10-hang-loat.png`. (Một link bị bộ gõ tiếng Việt của Mac đổi "ER/tests" thành "Ẻ/tets" lúc tôi gõ giả lập — lỗi cách gõ thử, không phải app.)
- **Tải 1 video ra MP4 bằng bộ công cụ tự tải**: Me at the zoo (AV1 + AAC, 19 s), có cờ quarantine.
- **Tải cả kênh** (link @tiktok gửi từ "trình duyệt"): hộp tự mở, đọc kênh dần (602 → 1.227 video), 2 tiến trình yt-dlp (cha + con). Bấm **Dừng**: cả hai tiến trình biến mất trong 1 s (bản sửa `killpg` chạy đúng trong app), danh sách giữ phần đã đọc. Chọn 2 video ⇒ tải xong vào thư mục con `TikTok` (HEVC 1080×1920, 1:45 và 1:47, có cờ quarantine), không sót tiến trình — `macos-09-tai-ca-kenh.png`.
- Tốc độ: có lúc ô Tốc độ ghi "0 MB/giây" khi đang tải — đã truy (dòng gỡ lỗi tạm, không commit): sự kiện tiến độ tới đều, tốc độ tức thời bằng 0 thật vì máy chủ Hetzner nghẽn (curl cùng lúc ~2 KB/s, sau đó trả 429). Không phải lỗi.

Bản phát hành **v0.1.0** (tải từ Releases về Mac):
- `UTD_0.1.0_universal.dmg` 14,5 MB: `shasum -a 256` khớp `SHA256SUMS.txt`; mount được, không hỏi điều khoản, có `UTD.app` + lối tắt `Applications`; `lipo -archs` = `x86_64 arm64`; `codesign -dv` = ad-hoc + hardened runtime, `codesign --verify --deep --strict` qua; `LSMinimumSystemVersion` 11.0. `latest.json` có `darwin-aarch64`, `darwin-x86_64` (cùng trỏ gói universal) và `windows-x86_64`.
- Chép app ra thư mục thử, chạy với dữ liệu trống: lần đầu tự tải đủ yt-dlp/deno/ffmpeg/ffprobe; tải file 14,9 MB từ GitHub trong 1,5 s (16 kết nối, SHA256 khớp); tải video MP4; Cài đặt → Cập nhật → **Kiểm tra** ⇒ "Bạn đang dùng bản mới nhất.". (Một file thử từ Hetzner lỗi `http.429` — máy chủ chặn vì tôi tải nhiều; đã xoá vào Thùng rác qua app.)
- Bản v0.1.0 còn 2 lỗi đã sửa trên nhánh này (đèn giao thông lệch 4pt, menu nền đen sau phím D/Dark Mode) và menu "Reload" — vào bản sau.

Kiểm sau sửa: Mac + Windows (xem commit).

## Việc còn lại / gợi ý

- Engine: với máy chủ giới hạn theo số kết nối (Hetzner speed test), 16 kết nối chậm hơn 8 kết nối gần một nửa — đáng để engine tự giảm số kết nối khi thêm kết nối không tăng tốc. Không riêng mac.
- Chế độ thử `UTD_DATA_DIR` trên mac: dữ liệu WebKit (localStorage) vẫn dùng chung theo bundle id `app.utd.desktop` (Windows có `WEBVIEW2_USER_DATA_FOLDER`, mac chưa có cách tương đương đơn giản).
- Đóng cửa sổ trên mac là thoát app (giống Windows hiện tại); người dùng mac quen app ở lại Dock — cân nhắc khi làm khay hệ thống.
- DMG dựng trên CI không có bố cục cửa sổ đẹp (Tauri bỏ bước AppleScript tới Finder khi `CI=true`); vẫn có app + lối tắt Applications.
- Notarize khi có tài khoản Apple Developer: RELEASE.md mục Ký mã → macOS.
