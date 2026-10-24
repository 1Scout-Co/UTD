//! Bước cuối của đường tải nhanh: mọi luồng đã xong ⇒ ghép/chuyển ra file cuối (file tạm của task
//! rồi đổi tên không ghi đè), kiểm Guard, gắn MOTW (chung với yt-dlp), dọn luồng tạm.
//! fMP4 hình + AAC (hoặc chỉ AAC ra m4a) ghép bằng Rust (utd-mp4) trước; không được thì ffmpeg.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use tokio_util::sync::CancellationToken;
use utd_engine::{DownloadTask, ErrorClass, TaskError, TaskStatus};
use utd_mp4::{Brand, Kind};

use crate::media::fast::Fast;
use crate::media::finish::{self, error};
use crate::media::mux::{self, Hint, MuxRun};
use crate::media::parts;
use crate::media::resolve::{AudioFormat, Layout};
use crate::media::rollup::Rollup;

const MIB: u64 = 1024 * 1024;

enum Produced {
    File(PathBuf),
    Cancelled,
    Failed,
}

pub(super) async fn mux_and_finish(
    fast: &Fast<'_>,
    task: &mut DownloadTask,
    layout: Layout,
    hints: &[Hint],
) -> Result<(), String> {
    let children = fast
        .journal
        .list_children(&task.id)
        .map_err(|problem| problem.to_string())?;
    let inputs: Vec<PathBuf> = children.iter().map(parts::stream_path).collect();
    if inputs.is_empty() || inputs.iter().any(|input| !input.is_file()) {
        return finish::fail(
            fast.sink,
            &fast.journal,
            task,
            error(
                ErrorClass::Transient,
                "media.no_output",
                "Không thấy file luồng đã tải. Bấm Tiếp tục để tải lại.",
            ),
        );
    }
    if fast.ffmpeg.is_none() && layout != Layout::Single {
        // ffmpeg bị gỡ giữa chừng: giữ luồng đã tải, cài lại rồi Tiếp tục là ghép được.
        return finish::fail(
            fast.sink,
            &fast.journal,
            task,
            error(
                ErrorClass::User,
                "tool.ffmpeg_missing",
                "Cần ffmpeg để ghép hoặc chuyển định dạng. Tải ffmpeg ở Cài đặt → Công cụ media rồi thử lại.",
            ),
        );
    }
    if needs_room(layout, hints)
        && let Some(problem) = space_problem(&task.temp_path, &inputs)
    {
        return finish::fail(fast.sink, &fast.journal, task, problem);
    }
    // Đủ byte: báo "verifying" trong lúc ffmpeg ghép/chuyển (file dài có thể mất vài giây),
    // để giao diện hiện "Đang ghép…" thay vì 100% · 0 kết nối như bị treo.
    let rollup = Rollup::new(&children, &[]);
    task.total_bytes = rollup.total();
    task.downloaded_bytes = finish::clamp(rollup.downloaded(), task.total_bytes);
    finish::set_status(fast.sink, &fast.journal, task, TaskStatus::Verifying)?;
    fast.sink.progress(rollup.idle(task));
    let destination = PathBuf::from(&task.dest_dir);
    let output = match produce(fast, task, layout, hints, &inputs, &destination).await {
        Produced::File(path) => path,
        Produced::Cancelled => {
            let downloaded = task.downloaded_bytes;
            return finish::pause(fast.sink, &fast.journal, task, downloaded);
        }
        Produced::Failed => {
            return finish::fail(fast.sink, &fast.journal, task, mux_failed(layout));
        }
    };
    finish::finalize(fast.sink, &fast.journal, task, &destination, &output)?;
    parts::discard_all(&fast.journal, &children);
    Ok(())
}

fn mux_failed(layout: Layout) -> TaskError {
    let message = if matches!(layout, Layout::Audio(_)) {
        "Không chuyển được âm thanh bằng ffmpeg."
    } else {
        "Không ghép được video và âm thanh bằng ffmpeg."
    };
    error(ErrorClass::Permanent, "media.mux", message)
}

/// Chỉ kiểm chỗ trống khi file ra cỡ tổng đầu vào: ghép hình + tiếng, hoặc lấy tiếng từ luồng chỉ
/// có tiếng. Single chỉ sửa vỏ, hỏng thì giữ nguyên file luồng đã nằm sẵn trên đĩa; tiếng tách
/// từ luồng có cả hình nhỏ hơn đầu vào nhiều.
fn needs_room(layout: Layout, hints: &[Hint]) -> bool {
    match layout {
        Layout::Merge => true,
        Layout::Single => false,
        Layout::Audio(_) => hints.first().is_some_and(audio_only),
    }
}

/// Luồng chỉ có tiếng; lượt tiếp tục không biết codec thì đoán theo đuôi file.
fn audio_only(hint: &Hint) -> bool {
    hint.vcodec == "none"
        || (hint.vcodec.is_empty() && matches!(hint.ext.as_str(), "m4a" | "mp3" | "aac"))
}

/// Ổ chứa file tạm phải còn ít nhất tổng kích thước các luồng (file ra gần bằng chừng đó).
/// Không đọc được dung lượng thì cứ ghép như trước.
fn space_problem(temp: &str, inputs: &[PathBuf]) -> Option<TaskError> {
    let temp = Path::new(temp);
    let free = fs2::available_space(temp.parent()?).ok()?;
    let need = missing_room(temp, inputs, free)?;
    let message = format!(
        "Ổ đĩa còn ít chỗ: cần khoảng {} MB trống để ghép. Giải phóng dung lượng rồi thử lại.",
        need.div_ceil(MIB)
    );
    Some(error(ErrorClass::User, "disk.full", &message))
}

/// Số byte cần khi không đủ chỗ. File tạm sót từ lần ghép bị ngắt (app tắt giữa chừng) sẽ bị
/// ghi đè lúc ghép, nên chỗ nó chiếm cũng tính là trống.
fn missing_room(temp: &Path, inputs: &[PathBuf], free: u64) -> Option<u64> {
    let need: u64 = inputs.iter().map(|input| file_len(input)).sum();
    (free.saturating_add(file_len(temp)) < need).then_some(need)
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |metadata| metadata.len())
}

/// Ghép bằng Rust trước, không được thì thử lần lượt các cách ffmpeg; ghi vào file tạm của task
/// rồi đổi tên không ghi đè.
async fn produce(
    fast: &Fast<'_>,
    task: &DownloadTask,
    layout: Layout,
    hints: &[Hint],
    inputs: &[PathBuf],
    destination: &Path,
) -> Produced {
    let base = parts::stem(task);
    let temp = PathBuf::from(&task.temp_path);
    if let Some(brand) = native_brand(layout, hints) {
        match native(&fast.cancel, &task.id, brand, inputs, &temp).await {
            Native::Done => {
                let ext = if brand == Brand::M4a { "m4a" } else { "mp4" };
                if let Some(path) = place(&temp, destination, &base, ext) {
                    return Produced::File(path);
                }
                let _ = std::fs::remove_file(&temp);
            }
            Native::Cancelled => return Produced::Cancelled,
            Native::Skipped => {}
        }
    }
    if let Some(ffmpeg) = fast.ffmpeg.as_deref() {
        for recipe in mux::recipes(layout, hints) {
            let _ = std::fs::remove_file(&temp);
            let args = mux::ffmpeg_args(recipe, inputs, &temp);
            let started = Instant::now();
            let run = mux::run_ffmpeg(ffmpeg, &args, &fast.cancel).await;
            let took = started.elapsed().as_millis();
            log::info!(
                "media.timing task={} mux engine=ffmpeg recipe={recipe:?} took_ms={took}",
                task.id
            );
            match run {
                MuxRun::Done => {
                    if let Some(path) = place(&temp, destination, &base, recipe.output().ext()) {
                        return Produced::File(path);
                    }
                }
                MuxRun::Cancelled => {
                    let _ = std::fs::remove_file(&temp);
                    return Produced::Cancelled;
                }
                MuxRun::Failed(tail) => log::warn!("media: ffmpeg {recipe:?} lỗi: {tail}"),
            }
            let _ = std::fs::remove_file(&temp);
        }
    }
    // Một luồng đã có hình + tiếng: không sửa được vỏ thì giữ nguyên file luồng.
    if layout == Layout::Single {
        let ext = hints.first().map_or("mp4", |hint| hint.ext.as_str());
        if let Some(path) = place(&inputs[0], destination, &base, ext) {
            return Produced::File(path);
        }
    }
    Produced::Failed
}

/// Kết quả ghép bằng Rust.
enum Native {
    Done,
    Cancelled,
    /// Cấu trúc chưa hỗ trợ hoặc lỗi: chạy ffmpeg như cũ.
    Skipped,
}

/// utd-mp4 ghép được không: hình MP4 (AV1/H.264/HEVC) + tiếng AAC, hoặc chỉ AAC ra m4a. utd-mp4 tự
/// kiểm kỹ cấu trúc; hint chỉ để khỏi mở file chắc chắn không hợp (WebM, Opus).
fn native_brand(layout: Layout, hints: &[Hint]) -> Option<Brand> {
    match (layout, hints) {
        (Layout::Merge, [video, audio]) => {
            (iso_video(video) && iso_audio(audio)).then_some(Brand::Mp4)
        }
        (Layout::Audio(AudioFormat::M4a), [audio]) => iso_audio(audio).then_some(Brand::M4a),
        _ => None,
    }
}

fn iso_video(hint: &Hint) -> bool {
    hint.ext == "mp4"
        || ["av01", "avc", "hev", "hvc"]
            .iter()
            .any(|prefix| hint.vcodec.starts_with(prefix))
}

fn iso_audio(hint: &Hint) -> bool {
    matches!(hint.ext.as_str(), "m4a" | "mp4") || hint.acodec.starts_with("mp4a")
}

/// Chạy utd-mp4 trong luồng chặn. Huỷ token ⇒ bật cờ huỷ, chờ luồng dừng rồi xoá file tạm
/// (huỷ được ưu tiên hơn cả khi vừa ghép xong).
async fn native(
    token: &CancellationToken,
    task_id: &str,
    brand: Brand,
    inputs: &[PathBuf],
    temp: &Path,
) -> Native {
    let kinds: &[Kind] = match brand {
        Brand::Mp4 => &[Kind::Video, Kind::Audio],
        Brand::M4a => &[Kind::Audio],
    };
    if inputs.len() != kinds.len() {
        return Native::Skipped;
    }
    let sources: Vec<utd_mp4::Input> = inputs
        .iter()
        .zip(kinds)
        .map(|(path, kind)| utd_mp4::Input {
            path: path.clone(),
            kind: *kind,
        })
        .collect();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    let output = temp.to_path_buf();
    let started = Instant::now();
    let mut job =
        tokio::task::spawn_blocking(move || utd_mp4::merge(&sources, &output, brand, &flag));
    let joined = tokio::select! {
        biased;
        () = token.cancelled() => {
            cancel.store(true, Ordering::Relaxed);
            let _ = job.await;
            let _ = std::fs::remove_file(temp);
            return Native::Cancelled;
        }
        joined = &mut job => joined,
    };
    let took = started.elapsed().as_millis();
    native_result(task_id, brand, joined, took, temp)
}

type Joined = Result<Result<utd_mp4::Report, utd_mp4::Error>, tokio::task::JoinError>;

fn native_result(task_id: &str, brand: Brand, joined: Joined, took: u128, temp: &Path) -> Native {
    match joined {
        Ok(Ok(report)) => {
            log::info!(
                "media.timing task={task_id} mux engine=rust brand={brand:?} took_ms={took} samples={} chunks={} moov_bytes={}",
                report.samples,
                report.chunks,
                report.moov_bytes
            );
            return Native::Done;
        }
        Ok(Err(utd_mp4::Error::Cancelled)) => return Native::Cancelled,
        Ok(Err(utd_mp4::Error::Unsupported(reason))) => {
            log::info!("media: ghép Rust bỏ qua ({reason}), dùng ffmpeg");
        }
        Ok(Err(problem)) => log::warn!("media: ghép Rust lỗi ({problem}), dùng ffmpeg"),
        Err(problem) => log::warn!("media: luồng ghép Rust dừng bất thường ({problem})"),
    }
    let _ = std::fs::remove_file(temp);
    Native::Skipped
}

/// Đổi `from` thành `<base>.<ext>` (trùng thì `<base> (n).<ext>`), không bao giờ ghi đè.
fn place(from: &Path, destination: &Path, base: &str, ext: &str) -> Option<PathBuf> {
    if !std::fs::metadata(from).is_ok_and(|metadata| metadata.len() > 0) {
        return None;
    }
    for _ in 0..3 {
        let target = mux::free_target(destination, base, ext)?;
        match mux::move_no_clobber(from, &target) {
            Ok(()) => return Some(target),
            Err(problem) if problem.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(problem) => {
                log::warn!("media: không đổi tên được file ghép ({problem})");
                return None;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(kind: &[u8; 4], parts: &[&[u8]]) -> Vec<u8> {
        let body = parts.concat();
        [&(body.len() as u32 + 8).to_be_bytes()[..], kind, &body].concat()
    }

    fn words(values: &[u32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_be_bytes())
            .collect()
    }

    /// Loại handler, vmhd/smhd và mục mẫu: hình avc1 hoặc tiếng mp4a (AAC).
    fn media(audio: bool) -> (&'static [u8; 4], Vec<u8>, Vec<u8>) {
        if !audio {
            let header = bx(b"vmhd", &[&words(&[1, 0, 0])]);
            return (b"vide", header, bx(b"avc1", &[&[0; 78]]));
        }
        let esds = bx(
            b"esds",
            &[&[0; 4], &[3, 21, 0, 1, 0, 4, 13, 0x40, 0x15], &[0; 11]],
        );
        let body = [
            &[0, 0, 0, 0, 0, 0, 0, 1][..],
            &[0; 8],
            &[0, 2, 0, 16, 0, 0, 0, 0],
        ];
        let entry = bx(b"mp4a", &[&body.concat(), &words(&[48000 << 16]), &esds]);
        (b"soun", bx(b"smhd", &[&[0; 8]]), entry)
    }

    /// minf: header, dinf, stbl với 3 mẫu 100 B trong một chunk ở `offset`.
    fn minf(header: &[u8], entry: &[u8], offset: u32) -> Vec<u8> {
        let stbl = bx(
            b"stbl",
            &[
                &bx(b"stsd", &[&words(&[0, 1]), entry]),
                &bx(b"stts", &[&words(&[0, 1, 3, 1024])]),
                &bx(b"stsc", &[&words(&[0, 1, 1, 3, 1])]),
                &bx(b"stsz", &[&words(&[0, 0, 3, 100, 100, 100])]),
                &bx(b"stco", &[&words(&[0, 1, offset])]),
            ],
        );
        let url = bx(b"url ", &[&words(&[1])]);
        let dinf = bx(b"dinf", &[&bx(b"dref", &[&words(&[0, 1]), &url])]);
        bx(b"minf", &[header, &dinf, &stbl])
    }

    /// MP4 thường nhỏ nhất utd-mp4 nhận: một trak, 3 mẫu 100 B dữ liệu giả (không giải mã).
    fn tiny_mp4(audio: bool) -> Vec<u8> {
        let ftyp = bx(b"ftyp", &[b"isom", &[0, 0, 2, 0], b"isom"]);
        let matrix = words(&[1 << 16, 0, 0, 0, 1 << 16, 0, 0, 0, 1 << 30]);
        let size: &[u32] = if audio {
            &[0, 0]
        } else {
            &[320 << 16, 240 << 16]
        };
        let tkhd = [
            words(&[3, 0, 0, 1, 0, 3072, 0, 0, 0, 0]),
            matrix.clone(),
            words(size),
        ];
        let (handler, header, entry) = media(audio);
        let hdlr = bx(b"hdlr", &[&[0; 8], handler, &[0; 13]]);
        let mdhd = bx(
            b"mdhd",
            &[&words(&[0, 0, 0, 48000, 3072]), &[0x55, 0xC4, 0, 0]],
        );
        let minf = minf(&header, &entry, ftyp.len() as u32 + 8);
        let mdia = bx(b"mdia", &[&mdhd, &hdlr, &minf]);
        let trak = bx(b"trak", &[&bx(b"tkhd", &[&tkhd.concat()]), &mdia]);
        let mvhd = [
            words(&[0, 0, 0, 1000, 64, 1 << 16]),
            vec![1, 0],
            vec![0; 10],
            matrix,
            vec![0; 24],
            words(&[2]),
        ];
        let moov = bx(b"moov", &[&bx(b"mvhd", &[&mvhd.concat()]), &trak]);
        [ftyp, bx(b"mdat", &[&[7; 300]]), moov].concat()
    }

    struct Folder(PathBuf);

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn streams() -> (Folder, Vec<PathBuf>, PathBuf) {
        let root = std::env::temp_dir().join(format!("utd-output-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let video = root.join("v.f1.mp4");
        let audio = root.join("a.f2.m4a");
        std::fs::write(&video, tiny_mp4(false)).unwrap();
        std::fs::write(&audio, tiny_mp4(true)).unwrap();
        let temp = root.join("task.part");
        (Folder(root), vec![video, audio], temp)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn merges_in_rust_and_honours_cancel() {
        let (_folder, inputs, temp) = streams();
        let token = CancellationToken::new();
        let done = native(&token, "t", Brand::Mp4, &inputs, &temp).await;
        assert!(matches!(done, Native::Done));
        let bytes = std::fs::read(&temp).unwrap();
        assert_eq!(&bytes[4..8], b"ftyp");
        assert_eq!(&bytes[36..40], b"moov", "moov ở đầu file");

        let garbage = [inputs[0].clone(), inputs[0].clone()];
        let skipped = native(&token, "t", Brand::Mp4, &garbage, &temp).await;
        assert!(matches!(skipped, Native::Skipped), "hai hình: để ffmpeg");
        assert!(!temp.exists(), "không để lại file tạm");

        token.cancel();
        let cancelled = native(&token, "t", Brand::Mp4, &inputs, &temp).await;
        assert!(matches!(cancelled, Native::Cancelled));
        assert!(!temp.exists());
    }

    fn hint(vcodec: &str, acodec: &str, ext: &str) -> Hint {
        Hint {
            vcodec: vcodec.into(),
            acodec: acodec.into(),
            ext: ext.into(),
            remux: true,
        }
    }

    #[test]
    fn tries_rust_only_for_mp4_video_with_aac() {
        let av1 = hint("av01.0.09M.08", "none", "mp4");
        let aac = hint("none", "mp4a.40.2", "m4a");
        let opus = hint("none", "opus", "webm");
        let merge = |video: &Hint, audio: &Hint| {
            native_brand(Layout::Merge, &[video.clone(), audio.clone()])
        };
        assert_eq!(merge(&av1, &aac), Some(Brand::Mp4));
        // H.264/HEVC có B-frame utd-mp4 cũng ghép (ctts + edit list như ffmpeg).
        for codec in ["avc1.640028", "hvc1.1.6.L120.90", "hev1.1.6.L93.B0"] {
            assert_eq!(merge(&hint(codec, "none", "mp4"), &aac), Some(Brand::Mp4));
        }
        assert_eq!(merge(&av1, &opus), None, "Opus/WebM: ffmpeg ghép");
        assert_eq!(merge(&hint("vp9", "none", "webm"), &aac), None);
        let resumed = [hint("", "", "mp4"), hint("", "", "m4a")];
        assert_eq!(
            native_brand(Layout::Merge, &resumed),
            Some(Brand::Mp4),
            "lượt tiếp tục chỉ biết đuôi file"
        );
        let m4a = Layout::Audio(AudioFormat::M4a);
        assert_eq!(
            native_brand(m4a, std::slice::from_ref(&aac)),
            Some(Brand::M4a)
        );
        assert_eq!(native_brand(m4a, &[opus]), None);
        assert_eq!(native_brand(Layout::Audio(AudioFormat::Mp3), &[aac]), None);
        assert_eq!(native_brand(Layout::Single, &[av1]), None);
    }

    #[test]
    fn checks_room_only_when_output_is_as_big_as_inputs() {
        let merge = [
            hint("av01", "none", "mp4"),
            hint("none", "mp4a.40.2", "m4a"),
        ];
        assert!(needs_room(Layout::Merge, &merge));
        let resumed = [hint("", "", "mp4")];
        assert!(
            !needs_room(Layout::Single, &resumed),
            "Single hỏng sửa vỏ thì đổi tên luồng, không cần thêm chỗ"
        );
        let mp3 = Layout::Audio(AudioFormat::Mp3);
        let m4a = Layout::Audio(AudioFormat::M4a);
        let muxed = [hint("avc1.64001f", "mp4a.40.2", "mp4")];
        assert!(!needs_room(mp3, &muxed), "tiếng tách từ luồng có hình");
        assert!(!needs_room(m4a, &muxed));
        assert!(needs_room(m4a, &[hint("none", "mp4a.40.2", "m4a")]));
        assert!(
            needs_room(mp3, &[hint("", "", "m4a")]),
            "lượt tiếp tục đoán theo đuôi"
        );
        assert!(!needs_room(mp3, &resumed));
    }

    #[test]
    fn counts_stale_temp_as_free_room() {
        let (_folder, inputs, temp) = streams();
        let need: u64 = inputs.iter().map(|input| file_len(input)).sum();
        assert_eq!(missing_room(&temp, &inputs, need), None);
        assert_eq!(missing_room(&temp, &inputs, need - 1), Some(need));
        std::fs::write(&temp, [0; 500]).unwrap();
        assert_eq!(
            missing_room(&temp, &inputs, need - 500),
            None,
            "file tạm sót từ lần ghép bị ngắt sẽ bị ghi đè"
        );
        assert_eq!(missing_room(&temp, &inputs, need - 501), Some(need));
    }
}
