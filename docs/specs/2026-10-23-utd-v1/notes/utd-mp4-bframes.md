# utd-mp4: B-frame (H.264/HEVC), edit list của init, file > 4 GiB

Việc (4), nhánh `claude/mp4-bframes-db1f71`, 2026-10-24. Đối chiếu với ffmpeg 8.1.2 (bản Gyan
trên máy) và mã nguồn tag `n8.1.2` (`libavformat/movenc.c`, `mov.c`, `demux.c`, `isom.c`,
`fftools/ffmpeg_demux.c`).

## Đã làm

- **B-frame**: đọc cts từng mẫu (trun v0 không dấu, v1 có dấu; ctts của file thường), ghi `ctts`
  version 0 gộp mẫu liền nhau như ffmpeg. Không ghi `cslg` (ffmpeg cũng không ghi ở chế độ mp4).
- **Mốc thời gian như ffmpeg** (`crates/utd-mp4/src/timing.rs`): mô phỏng đúng chuỗi
  `ffmpeg -i hình -i tiếng -map 0:v:0 -map 1:a:0 -c copy -f mp4` (cũng là lệnh app/yt-dlp dùng):
  1. mov.c (fMP4 tự tắt advanced_editlist): DTS = tfdt − media_time của elst init, PTS = DTS + cts;
     AAC có elst thì media_time còn là `start_pad` (priming).
  2. demux.c: start_time = PTS gói đầu (+ start_pad của AAC theo tần số mẫu).
  3. ffmpeg_demux.c: `ts_offset = −start_time` (micro giây, làm tròn hai lần) cộng vào mọi gói của
     đầu vào đó — mỗi đầu vào tự về 0.
  4. movenc.c: `start_cts` = cts gói đầu, hạ xuống cts nhỏ hơn của các gói có DTS ≤ 0;
     `end_pts` = max(PTS + duration); tkhd/mvhd = ⌈end_pts⌉ (ms), mdhd = end_pts − (start_dts +
     start_cts), elst = (⌈mdhd⌉ + ⌊start_dts + start_cts⌋ ms, media_time = −start_dts).
  Hệ quả đáng nhớ: với x264 thường `start_cts` < cts đầu ⇒ segment elst nhỏ hơn tkhd vài ms
  (đúng y ffmpeg, ví dụ 266 vs 267). Media_time của elst init (bất kỳ, một mục) không đổi file ra.
- **Ngưỡng version 1** của mvhd/tkhd/mdhd/elst theo ffmpeg: ≥ INT32_MAX (trước là > u32::MAX).
- **Init có edts**: nhận một mục, media_time ≥ 0, tốc độ 1 (segment bị mov.c bỏ qua ở fMP4).
  AAC có priming (media_time > 0) chỉ nhận khi AudioSpecificConfig là AAC-LC, tần số theo bảng =
  timescale, không SBR (2 byte, có thể kèm byte 0 hoặc khối `56 E5` báo vắng SBR mà ffmpeg tự ghi).
- **Vẫn trả Unsupported (đường lui ffmpeg giữ nguyên)**: cts âm (trun v1 — mov.c dịch DTS theo mức
  âm nhất *đã gặp*, đổi dần giữa fragment) hoặc cts v0 > 2³¹; ctts trong file thường (mov.c dùng
  mov_fix_index, có bỏ gói); edit list nhiều mục/đoạn trống/đổi tốc độ; `iTunSMPB` (mov.c lấy làm
  priming); `sdtp` trong traf, hoặc sdtp có mẫu disposable (ffmpeg ghi lại sdtp); mẫu duration 0
  (fftools sửa DTS không tăng); mẫu hình đầu không phải keyframe (fftools bỏ gói trước keyframe);
  tiếng có cts; PTS đầu > 0 sau khi dịch (ffmpeg sẽ thêm đoạn trống — không gặp với đầu vào đã nhận).
- App: `native_brand` không cần đổi (đã nhận avc/hev/hvc); thêm ca hvc1/hev1 vào test.
- Test: 5 test đơn vị mới (`tests/bframes.rs`; số mong đợi tính tay theo movenc.c, bản tiếng đã
  cho ffmpeg ghép lại và ra đúng số). Bộ `#[ignore]` mới tự so với ffmpeg
  (`tests/ffmpeg.rs`): các box mvhd/tkhd/elst/mdhd/stts/stss/ctts/stsz/sgpd/sbgp trùng từng byte,
  gói ffprobe từng stream (pts, dts, duration, size, flags), framemd5, thời lượng ffprobe, giải mã
  `-xerror` 1 giây ở đầu/giữa/cuối. `examples/merge.rs` để ghép tay/đo.

## Cách chạy bộ #[ignore]

```
UTD_FFMPEG=<thư mục bin ffmpeg>
UTD_MP4_GEN=<thư mục>       # tự sinh fixture thiếu bằng ffmpeg (x264 -bf 0/3, x265 bframes 0/3,
                            # frag_keyframe/default_base_moof/omit_tfhd_offset/frag_every_frame,
                            # delay_moov = init có edts, negative_cts_offsets, AAC priming)
UTD_MP4_FIXTURES=<thư mục>  # f140.m4a + f399.mp4 / f299.mp4 / f137.mp4 / f298.mp4 (--fixup never)
UTD_MP4_VIDEO=… UTD_MP4_AUDIO=…  # một cặp bất kỳ (file lớn)
cargo test --release -p utd-mp4 -- --ignored --nocapture
```

## Kết quả (2026-10-24)

Máy dùng chung với 5 phiên khác. Ổ D: là SSD SATA QLC (Patriot P210) đang bị các phiên khác
dùng gần hết (Disk Time > 500 %, ~60 MB/s) nên số đo file lớn trên D: không dùng được; số dưới đây
đo bằng `examples/merge` và `ffmpeg -c copy -f mp4` chạy xen kẽ, file ra ở C: (NVMe), "nóng" =
đầu vào đã nằm trong cache.

| Đầu vào | Trước (ffmpeg) | Sau (Rust) | So với ffmpeg |
|---|---|---|---|
| f299 H.264 1080p60 có B-frame, 2 giờ 28 phút, 1,09 GB + f140 (534 470 + 383 631 mẫu) | 7,8–9,4 s (trước đây utd-mp4 đọc ~0,2 s rồi trả Unsupported) | 0,73–0,97 s nóng; 4,8 s lượt đầu | trùng hết; Explorer 02:28:27 cả hai |
| f399 AV1 (hồi quy) + f140 | 7,7–8,4 s | 0,52–0,67 s nóng | trùng hết; Explorer 02:28:27 |
| f401 AV1 4K 1 giờ, 5,49 GB + f140 → 5,55 GB | 7,2–9,7 s (một lượt 53 s) | 4,2–5,7 s | trùng hết; co64 + mdat largesize, mvhd/tkhd/mdhd v0 như ffmpeg; Explorer 01:00:13 |
| Tự sinh: x264 1080p `-bf 3` lặp 15 × 10 s, 4,75 GB + AAC có priming → 4,75 GB | 4,0–5,6 s | 3,2–5,7 s | trùng hết; co64 + largesize; Explorer 00:02:30 |
| Tự sinh: 1 giờ, timescale 600 kHz (mdhd 2,16·10⁹) + AAC 8 kHz priming | 0,15–0,22 s | 0,02 s | mdhd hình version 1 như ffmpeg (bản cũ ghi v0) |

File lớn ít gói thì cả hai bị chặn ở tốc độ ghi đĩa nên chênh ít; file nhiều gói (video dài) thì
ffmpeg chậm vì xử lý từng gói. Phép so trên D: lúc đĩa nghẽn: f401 Rust 199 s, ffmpeg 433 s.

"Trùng hết" = mvhd/tkhd/elst/mdhd/stts/stss/ctts/stsz/sgpd/sbgp trùng từng byte, gói ffprobe từng
stream (pts, dts, duration, size, flags) và framemd5 trùng, thời lượng ffprobe bằng nhau, giải mã
`-xerror` đầu/giữa/cuối không lỗi. Fixture tự sinh (`UTD_MP4_GEN`): 12 cặp + 1 m4a trùng ffmpeg
(H.264/HEVC không và có B-frame, init có edts, priming AAC, 4 kiểu cờ fragment, file thường); 3 file
trả Unsupported đúng (cts âm, B-frame trong file thường, elst dịch AAC file thường).

Lệnh đã chạy xanh: `cargo test -p utd-mp4` (19), `cargo test --release -p utd-mp4 -- --ignored`
(3: fixture tự sinh, f399 + f299 + f140 thật, cặp > 4 GiB có B-frame), `cargo test -p utd-engine`,
`cargo test -p app --lib` (121), `cargo clippy -p app/-p utd-mp4/-p utd-engine --all-targets
-- -D warnings`, `pnpm lint`, `pnpm build`. Không đụng UI nên không chạy Playwright. File media
thử đã xoá.

## Chưa làm / việc sau

- Chưa chạy app thật end-to-end với link H.264 (API utd-mp4 không đổi; app chọn sẵn 399+140 nên
  H.264 chỉ gặp khi người dùng chọn chất lượng có avc1 hoặc video không có AV1).
- Vẫn để ffmpeg: B-frame trong MP4 thường (không phân mảnh), cts âm (`negative_cts_offsets`),
  edit list nhiều mục / đoạn trống, AAC HE/SBR có priming, file có sdtp disposable.
- App: khi thiếu ffmpeg, `media/output.rs::mux_and_finish` báo `tool.ffmpeg_missing` ngay cả với
  cặp mp4 + m4a mà utd-mp4 ghép được — có thể thử Rust trước (đụng luồng của việc (2) tự tải
  công cụ nên để phiên điều phối quyết).
