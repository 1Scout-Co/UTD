//! Bước cuối của đường tải nhanh: mọi luồng đã xong ⇒ ffmpeg ghép/chuyển ra file cuối (file tạm
//! của task rồi đổi tên không ghi đè), kiểm Guard, gắn MOTW (chung với yt-dlp), dọn luồng tạm.

use std::path::{Path, PathBuf};

use utd_engine::{DownloadTask, ErrorClass, TaskStatus};

use crate::media::fast::Fast;
use crate::media::finish::{self, error};
use crate::media::mux::{self, Hint, MuxRun};
use crate::media::parts;
use crate::media::resolve::Layout;
use crate::media::rollup::Rollup;

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
            let message = if matches!(layout, Layout::Audio(_)) {
                "Không chuyển được âm thanh bằng ffmpeg."
            } else {
                "Không ghép được video và âm thanh bằng ffmpeg."
            };
            return finish::fail(
                fast.sink,
                &fast.journal,
                task,
                error(ErrorClass::Permanent, "media.mux", message),
            );
        }
    };
    finish::finalize(fast.sink, &fast.journal, task, &destination, &output)?;
    parts::discard_all(&fast.journal, &children);
    Ok(())
}

/// Thử lần lượt các cách ffmpeg, ghi vào file tạm của task rồi đổi tên không ghi đè.
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
    if let Some(ffmpeg) = fast.ffmpeg.as_deref() {
        for recipe in mux::recipes(layout, hints) {
            let _ = std::fs::remove_file(&temp);
            let args = mux::ffmpeg_args(recipe, inputs, &temp);
            match mux::run_ffmpeg(ffmpeg, &args, &fast.cancel).await {
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
