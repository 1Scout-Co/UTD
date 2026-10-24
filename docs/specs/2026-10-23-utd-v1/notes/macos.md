# Việc (5): UTD trên macOS + phát hành Windows/macOS

Nhánh `claude/macos-ab0b40` (từ `main` @ 9503965). Kiểm trên Mac mini thật: macOS 27.0.1, Apple Silicon (arm64),
Rust 1.99.1 (rustup bản người dùng), Node 24 + pnpm, Xcode Command Line Tools.

## Đã làm

| Phần | macOS làm gì | File |
|---|---|---|
| Thư mục dữ liệu | `~/Library/Application Support/UTD-Data`; chế độ thử `UTD_DATA_DIR` giữ nguyên | `ipc.rs::data_dir` |
| Thư mục tải mặc định | `~/Downloads/UTD` (Tauri `download_dir`, có sẵn); ổ ngoài `/Volumes/<tên>` rút ra ⇒ về mặc định | `commands.rs::volume_root` |
| IPC native host ↔ app | Unix domain socket `<data>/ipc.sock` quyền 0600, socket sót lại thì thay; `ipc.json` tạo quyền 0600; vẫn kiểm token như named pipe | `ipc.rs`, `native_host.rs` |
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

## Chưa kiểm được (và vì sao)

Mac mini **khoá màn hình** suốt lúc kiểm (cần mật khẩu, không nhập hộ) ⇒ không chụp được màn hình, không bấm được giao diện:

- Ảnh chụp giao diện trên Mac cho `ui-demos/` — chưa có.
- Bấm Tạm dừng / Tiếp tục trong giao diện (engine giống hệt Windows; test engine xanh trên mac).
- Hộp Tải media (chọn chất lượng) chạy hết đường trong app; menu chuột phải: Mở file, Mở thư mục, Thùng rác, icon hiện trong bảng (từng hàm Rust đã có test chạy thật trên mac).
- Nút "Cài tích hợp trình duyệt" trong Cài đặt và extension Chrome thật gọi host (đã thử host bằng tin nhắn y hệt Chrome).
- Hộp Gatekeeper và hộp xin quyền thư mục Downloads (nội dung chữ trong RELEASE.md theo hành vi chuẩn của macOS, chưa chụp được trên macOS 27).
- Hộp chọn thư mục (NSOpenPanel), kéo-thả.
- Bản universal (x86_64) và job CI macOS chưa chạy thật (chưa push); bản arm64 dựng và chạy trên Mac thật.
- Tự cập nhật trên mac (cần một bản phát hành thật có `latest.json` mới).

## Việc còn lại / gợi ý

- Phiên (2) (tự tải công cụ): trên mac nút tải yt-dlp/ffmpeg hiện vẫn theo bảng Windows (`yt-dlp.exe`, `System32\tar.exe`) — phiên (2) đang làm bảng nguồn theo nền tảng; xem ghi chú bàn giao trong thư mục ghi chú chung. Người dùng mac tạm thời cài bằng `brew install yt-dlp ffmpeg` (UTD tự thấy).
- Engine: với máy chủ giới hạn theo số kết nối (Hetzner speed test), 16 kết nối chậm hơn 8 kết nối gần một nửa — đáng để engine tự giảm số kết nối khi thêm kết nối không tăng tốc. Không riêng mac.
- Chế độ thử `UTD_DATA_DIR` trên mac: dữ liệu WebKit (localStorage) vẫn dùng chung theo bundle id `app.utd.desktop` (Windows có `WEBVIEW2_USER_DATA_FOLDER`, mac chưa có cách tương đương đơn giản).
- Đóng cửa sổ trên mac là thoát app (giống Windows hiện tại); người dùng mac quen app ở lại Dock — cân nhắc khi làm khay hệ thống.
- DMG dựng trên CI không có bố cục cửa sổ đẹp (Tauri bỏ bước AppleScript tới Finder khi `CI=true`); vẫn có app + lối tắt Applications.
- Notarize khi có tài khoản Apple Developer: RELEASE.md mục Ký mã → macOS.
