# Việc 2 — Tự tải yt-dlp / deno / ffmpeg (nhánh `claude/auto-tools-5e44c5`)

Mục tiêu: người không rành công nghệ cài UTD xong là tải được video, không phải tự đi tải công cụ.

## Đã làm

- **Tự tải lần đầu, không hỏi** (`src-tauri/src/media/auto.rs`): mở app mà thiếu yt-dlp / deno / ffmpeg thì tải nền
  theo thứ tự yt-dlp (~17 MB) → deno (~43 MB) → ffmpeg (~200 MB). Mỗi lượt thử **mọi** công cụ còn thiếu (một cái lỗi
  không chặn cái sau); lượt có lỗi thì chờ 5 → 15 → 30 → 60 → 120 → 300 → 600 s (giữ 600 s) rồi thử lại, suốt đời app.
  Có việc cần công cụ (bấm Phân tích / Tải, nút "Thử lại") thì bỏ thời gian chờ, thử ngay.
- **Kiểm SHA-256 như cũ**: lấy file checksum trước, tải vào `.part`, băm trong lúc ghi, sai hash ⇒ xoá; giải nén, chạy
  thử (`--version`/`-version`), rồi mới thay bản cũ (bản cũ của UTD giữ thành `.bak`). Một lần cài mỗi lúc trong cả
  app (nút trong Cài đặt và tự tải dùng chung khoá).
- **Giao diện** (ít chữ, không chặn): thẻ nhỏ ở thanh bên trên thẻ ổ đĩa — "Đang tải ffmpeg" + 45% + thanh tiến độ; lỗi
  thì "Chưa tải được deno" / "↻ Thử lại hoặc chờ 25 s" (rê chuột xem câu lỗi). Hộp Tải media khi đang phân tích mà
  yt-dlp/deno chưa xong: một dòng "Lần đầu dùng: đang tải yt-dlp 45%…". Xong thì thẻ tự ẩn. Code: `ui/tools-auto.tsx`.
- **Task chờ công cụ** (`runner.rs`): task media tạo lúc công cụ đang tải vẫn ở "Đang chờ lượt", chờ xong rồi chạy;
  bấm Tạm dừng thì dừng. Chờ yt-dlp (và ffmpeg nếu tách âm thanh) qua tối đa 6 lượt tự tải (~9 phút); deno / ffmpeg
  (khi tải video) chỉ chờ một lượt — lỗi thì chạy tiếp không có nó. Vẫn thiếu ⇒ lỗi rõ ràng: "Chưa tải được yt-dlp:
  <lý do> UTD sẽ tự thử lại." (Tiếp tục để chạy lại). Người dùng bấm Huỷ lần tự tải ⇒ task thôi chờ.
- **Phân tích chờ công cụ** (`media_probe`): chờ yt-dlp + deno một lượt; `ffmpegAvailable` = có ffmpeg hoặc ffmpeg
  đang được tự tải (task tải sau đó sẽ chờ ffmpeg).
- **Tự cập nhật** bản UTD quản lý: kiểm yt-dlp tối đa mỗi ngày một lần, deno mỗi 30 ngày (lần đầu 60 s sau khi mở app,
  rồi mỗi giờ xem tới hạn chưa; giờ kiểm lưu trong bảng settings: `media_tools_checked`, `media_deno_checked`). Chỉ
  tải khi tag mới hơn bản đang có. ffmpeg không tự cập nhật (lớn, ổn định).
- **Lỗi kiểu extractor/403 ⇒ cập nhật rồi chạy lại một lần** (Phân tích và task tải): nhận diện ở `auto::outdated`
  ("unable to extract", nsig/signature/challenge, "requested format is not available", "no video formats found",
  "HTTP Error 403", "precondition check failed", "please report this issue"/"confirm you are on the latest version";
  loại trừ đăng nhập/cookie/riêng tư/DRM/unsupported URL). Kiểm bản mới tối đa mỗi 30 phút; nhiều task lỗi cùng lúc
  chỉ kiểm một lần, task khác thấy bản đã đổi thì chạy lại luôn. Chỉ chạy lại khi thật sự có bản mới.
- **Công tắc** Cài đặt → Công cụ media → "Tự tải và cập nhật" (mặc định bật, key `media_tools_auto` = "off" khi tắt).
  Tắt giữa lúc đang tự tải ⇒ dừng lần tải đó. Tắt thì app như trước: nút Tải/Cập nhật tay, thiếu thì báo lỗi.
- **deno**: thêm vào dò công cụ, Cài đặt (nút "Tải deno", nguồn denoland/deno, MIT) và truyền
  `--js-runtimes deno:<đường dẫn>` cho mọi lệnh yt-dlp (Phân tích, lấy link, tải) khi yt-dlp ≥ 2025.11.12.
- **Giới thiệu** (Cài đặt, cuối): giấy phép UTD + nguồn/giấy phép yt-dlp, deno, ffmpeg (giấu sau "Công cụ media").

## Kiểm deno với video thật (2026-10-24, yt-dlp 2026.10.21 chính chủ, video `09OGSRptb0M`)

| | Không runtime JS (`--no-js-runtimes`) | Có deno 2.7.12 |
|---|---|---|
| Cảnh báo | "No supported JavaScript runtime… YouTube extraction without a JS runtime has been deprecated, and some formats may be missing" | không |
| Số định dạng | 47 (thiếu vd. 18 = 360p gộp) | 50 |
| Link luồng | không có tham số `n` (client không cần giải) | có `n` đã giải |
| Tải thử 9,5 MB ở giữa file | 4,6 rồi 21 MB/s | 20 / 27 MB/s |

Kết luận: hôm nay không có deno vẫn tải được nhưng yt-dlp đã tuyên bố bỏ đường này và thiếu định dạng ⇒ **tự tải deno**.
Lưu ý khi đo: YouTube bóp yêu cầu Range > ~10 MB xuống ~88 KB/s ở cả hai trường hợp (đúng như `http_chunk_size` của
engine đã xử lý), nên so tốc độ phải dùng khúc < 10 MB.

## Nguồn tải và checksum (`src-tauri/src/media/sources.rs`)

| Công cụ | Windows x64 / arm64 | macOS arm64 / x64 | Checksum |
|---|---|---|---|
| yt-dlp | `yt-dlp.exe` / `yt-dlp_arm64.exe` | `yt-dlp_macos` (universal2) | `SHA2-256SUMS` của bản `latest`; tải link `releases/download/<tag>/…` với tag đọc từ chuyển hướng của file SUMS (không lệch khi `latest` vừa đổi) |
| deno | `deno-x86_64-pc-windows-msvc.zip` / `deno-aarch64-pc-windows-msvc.zip` | `deno-aarch64-apple-darwin.zip` / `deno-x86_64-apple-darwin.zip` | file `<zip>.sha256sum` riêng; Windows là kết quả `Get-FileHash` của PowerShell (`Hash : …`, `Path : …\<zip>`), macOS dạng sha256sum — đọc cả hai; cũng ghim tag |
| ffmpeg | `yt-dlp/FFmpeg-Builds` tag `latest`: `ffmpeg-master-latest-win64-gpl.zip` / `…-winarm64-gpl.zip` | `ffmpeg.martin-riedl.de/redirect/latest/macos/{arm64,amd64}/release/{ffmpeg,ffprobe}.zip` (hai gói) | Windows: `checksums.sha256`; lệch hash/404 chờ 20 s thử lại một lần (bản dựng lại hằng ngày). macOS: link `redirect/latest` chuyển (307) tới `/download/macos/<arch>/<id>_<bản>/ffmpeg.zip`, SHA-256 ở `<link đích>.sha256` |

- Host cho phép: GitHub (`github.com`, `release-assets.githubusercontent.com`, `objects.githubusercontent.com`); macOS
  ffmpeg chỉ `ffmpeg.martin-riedl.de`. HTTPS bắt buộc, kiểm cả từng bước chuyển hướng.
- Engine thêm `fetch::resolve_redirect` (đọc đích chuyển hướng, không tải nội dung) cho kiểu martin-riedl.
- Vì sao martin-riedl cho macOS: yt-dlp/FFmpeg-Builds không có macOS; evermeet.cx (trang ffmpeg.org dẫn) chỉ có Intel.
  Martin Riedl có arm64 + amd64, kênh release (9.0.2 lúc kiểm), SHA-256 từng file, zip có ký (theo trang của họ),
  build `--enable-gpl --enable-version3` (GPLv3). Không có trong danh sách của ffmpeg.org — phiên macOS cân nhắc.
- Giải nén bằng tar của hệ điều hành (`System32\tar.exe` / `/usr/bin/tar`, bsdtar từ chối `..`), chỉ nhận file nằm
  hẳn trong thư mục giải nén; macOS đặt quyền 755.

## Chính sách chọn bản (không đè bản người dùng tự cài)

- **Dò** (`tools.rs`): `<data>\bin` → PATH của tiến trình → PATH đã lưu (registry) → `~/.deno/bin` (+ Homebrew/MacPorts
  trên macOS) → WinGet/Scoop/Chocolatey. yt-dlp: bản trong `<data>\bin` chạy được thì luôn thắng, không thì bản
  `--version` mới nhất. deno: bản của UTD trước, không thì bản đầu tiên chạy được và ≥ 2.0 (deno 1.x coi như không có).
  ffmpeg: như cũ (ưu tiên thư mục có cả ffprobe).
- **Tự tải lần đầu** chỉ tải công cụ *hoàn toàn không có*. Máy đã có yt-dlp ở PATH ⇒ dùng bản đó, không tải.
- **Tự cập nhật hằng ngày** chỉ cập nhật bản trong `<data>\bin`. Bản ở PATH/WinGet… người dùng tự lo.
- **Lỗi kiểu extractor** với bản người dùng tự cài cũ hơn bản mới nhất ⇒ UTD tải bản riêng vào `<data>\bin` (không sửa
  file của họ); từ đó UTD dùng bản của mình vì `<data>\bin` luôn thắng. Không muốn vậy thì tắt công tắc.
- `--js-runtimes` chỉ truyền khi yt-dlp ≥ 2025.11.12 (bản đầu có EJS) và dò được deno; yt-dlp cũ hơn sẽ báo
  "no such option" nếu truyền.

## Chế độ thử

`UTD_DATA_DIR=<thư mục tạm tuyệt đối>` + `UTD_TOOLS_OWN_ONLY=1` ⇒ chỉ dùng công cụ trong `<data>\bin`, bỏ qua bản có sẵn
trên máy (biến thứ hai chỉ có tác dụng khi có `UTD_DATA_DIR`). Dùng để thử luồng tự tải lần đầu trên máy đã cài công cụ.

## Test

- `media::sources` (mọi nền tảng có đủ công cụ, link qua luật host), `media::verify` (SUMS, sidecar PowerShell/sha256sum,
  tag, so phiên bản deno theo số), `media::unpack` (giải nén zip thật bằng tar hệ thống, đường dẫn lạ).
- `media::install_tests` — server giả 127.0.0.1, "file chạy được" là script in phiên bản: ghim tag + bỏ qua khi đã mới
  nhất + giữ `.bak`; SHA-256 sai (thử lại đúng 1 lần) và bản không chạy được ⇒ giữ bản cũ, không sót thư mục tạm;
  deno zip + sidecar PowerShell; hai gói checksum cạnh đích chuyển hướng (kiểu martin-riedl); vòng tự tải gặp 3 yêu
  cầu bị cắt (mất mạng) ⇒ Phân tích báo lỗi ngay, task chờ tới khi tải được.
- `media::auto` — nhận diện lỗi cần cập nhật; cập nhật vì lỗi tối đa mỗi 30 phút, task khác thấy bản mới thì chạy lại;
  không cập nhật bản người dùng tự cài; công cụ hỏng không chặn công cụ sau, chờ có giới hạn; Huỷ/tắt ⇒ thôi chờ.
- `media::tests` — `--js-runtimes` chỉ khi yt-dlp đủ mới và có deno.
- e2e `e2e/tools-auto.spec.ts` (giả app desktop bằng `e2e/tauri-mock.ts`): thẻ tiến độ, %, lỗi + đếm lùi + Thử lại, tự
  ẩn khi xong, a11y; công tắc trong Cài đặt bật sẵn, tắt được, đang tự tải thì có tiến độ + Huỷ. `settings.spec.ts`
  cập nhật cho deno + công tắc (preview khoá).

## Thử thật trên Windows x64 (2026-10-24, bản debug, `UTD_DATA_DIR` rỗng + `UTD_TOOLS_OWN_ONLY=1`)

1. **Lần đầu mở app**: tự tải yt-dlp 2026.10.21 (3 s), deno 2.9.8 (3 s), ffmpeg N-127989 (~45 s, gói 200 MB). Thẻ thanh
   bên hiện "Đang tải ffmpeg 31%" rồi "Giải nén ffmpeg". 60 s sau khi mở, vòng kiểm bản mới chạy: yt-dlp/deno "Current".
2. **Bấm tải lúc ffmpeg còn đang giải nén**: Phân tích "Me at the zoo" (`jNQXAC9IVRw`) mất 6,2 s; bấm Tải về ⇒ task
   "Đang chờ" ~3 s, ffmpeg xong thì chạy đường nhanh + ghép Rust ⇒ `Me at the zoo.mp4` 530 842 byte, hoàn tất.
3. **deno được dùng thật**: Phân tích `09OGSRptb0M` (2 giờ 28 phút) — tiến trình `yt-dlp.exe --js-runtimes
   deno:<data>\bin\deno.exe …` và `deno.exe run --ext=js …` từ `<data>\bin`; ra đủ 1080p…360p + M4A/MP3.
4. **yt-dlp cũ lỗi ⇒ tự cập nhật ⇒ chạy lại**: đặt yt-dlp 2025.01.15 (bản chính chủ cũ) vào `<data>\bin`; Phân tích lỗi
   `ERROR: [youtube] jNQXAC9IVRw: The page needs to be reloaded.` ⇒ app cài 2026.10.21 (bản cũ thành `.bak`) ⇒ phân
   tích lại thành công, tổng 15 s. Câu lỗi này không có trong danh sách ban đầu ⇒ đã nới `outdated()` thành "mọi lỗi
   extractor `ERROR: [site] …` trừ lỗi mạng/đăng nhập/riêng tư/DRM".
5. **Mất mạng rồi có lại**: chạy app với `HTTPS_PROXY` trỏ cổng đóng ⇒ cả ba công cụ lỗi "Không kết nối được nguồn
   tải…", chờ 5 → 15 → 30 s; thẻ hiện "Chưa tải được yt-dlp / ↻ Thử lại hoặc chờ N s"; Phân tích trả "Chưa tải được
   yt-dlp: Không kết nối được nguồn tải. Kiểm tra mạng rồi thử lại. UTD sẽ tự thử lại." Bật proxy CONNECT (mạng có lại)
   + bấm Thử lại ⇒ yt-dlp, deno, ffmpeg tải xong trong ~55 s.

Ghi chú môi trường: cổng debug 9412 lúc thử bị một server node của phiên i18n chiếm ở `127.0.0.1` (WebView2 của app
thử nghe ở `[::1]:9412`) — nối CDP qua `[::1]` được, không đụng tiến trình của phiên kia.

## Đề xuất (chưa làm): tuỳ chọn tải sẵn trong bộ cài NSIS

Mặc định nên giữ như hiện tại — app tự tải lần đầu: bộ cài nhẹ (~10 MB), UTD không phân phối lại bản GPL của ffmpeg
(khỏi nghĩa vụ kèm mã nguồn/giấy phép trong gói), công cụ luôn là bản mới nhất lúc dùng. Nếu muốn có lựa chọn trong
bộ cài: thêm một trang/checkbox trong `src-tauri/windows/hooks.nsh` (`NSIS_HOOK_POSTINSTALL`) chạy
`UTD.exe --prefetch-tools` (cờ mới: gọi `install_job` cho từng công cụ rồi thoát, không mở cửa sổ) — vẫn đi đúng đường
SHA-256 của app, không nhúng URL/hash vào script NSIS. Không nên dùng `NSISdl`/`inetc` tải trực tiếp trong bộ cài vì
không kiểm được SHA-256 theo luật host như app.

## Cho phiên macOS

- Bảng nguồn macOS có sẵn (`Platform::MacArm64`/`MacX64`), biên dịch theo `cfg`; tên file không đuôi `.exe`
  (`yt-dlp`, `deno`, `ffmpeg`, `ffprobe`); giải nén bằng `/usr/bin/tar`; `chmod 755` sau khi giải nén/tải.
- Cần kiểm thật: `yt-dlp_macos` chạy được khi tải bằng reqwest (không có `com.apple.quarantine` nên Gatekeeper không
  hỏi — chưa xác nhận), bố cục zip của martin-riedl (giả định file nằm ở gốc zip; `unpack::locate` cũng tìm trong
  `<gói>/bin`), deno macOS chạy từ `<data>/bin`.
- Mục Cài đặt → Công cụ media đang ghi nguồn ffmpeg "yt-dlp/FFmpeg-Builds" cố định (`settings-media-tools-parts.tsx`);
  macOS nên đổi thành martin-riedl. Mục Giới thiệu đã ghi cả hai.
- `tools.rs` thêm `/opt/homebrew/bin`, `/usr/local/bin`, `/opt/local/bin` vào chỗ dò trên macOS (app mở từ Finder chỉ
  có PATH tối thiểu).

## Chưa kiểm / còn lại

- macOS, Windows arm64: chỉ biên dịch + unit test, chưa chạy thật.
- Trong app thật chưa bấm: công tắc tắt/bật, Huỷ lần tự tải trong Cài đặt, nút "Tải deno" bằng tay (đã có e2e giả lập
  và unit test cho logic). Vòng kiểm hằng ngày qua nhiều ngày: unit test (`due`/`mark`), app thật mới thấy lần đầu.
- `sha2` thêm làm dev-dependency của `app` (đã có sẵn trong `Cargo.lock` vì engine dùng); cổng `deps-check` của kit
  chỉ nhận npm/go nên không vet được crate Rust.
- e2e mới dùng `e2e/tauri-mock.ts` (giả `__TAURI_INTERNALS__`); `media-routing.spec.ts` có bản giả riêng từ trước —
  có thể gộp về helper này sau.
- `--js-runtimes` với yt-dlp cũ hơn 2025.11.12: dựa theo changelog yt-dlp, chưa thử với bản cũ thật.
- Chuỗi giao diện mới viết tiếng Việt cứng (phiên đa ngôn ngữ ghép sau): `tools-auto.tsx`, `settings-about.tsx`, công
  tắc trong `settings-media-tools-parts.tsx`, câu lỗi trong `auto.rs`/`install.rs`.
- Deno lớn (~43 MB zip, ~110 MB giải nén) — có thể cân nhắc chỉ tải deno khi gặp link YouTube đầu tiên nếu muốn lần
  đầu nhẹ hơn.
