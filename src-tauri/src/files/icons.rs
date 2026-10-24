//! Icon thật của file như Explorer: file đã xong lấy icon của chính nó (game .exe có icon riêng),
//! file dở dang lấy icon theo đuôi. Ra PNG vuông dạng `data:image/png;base64,…`, có bộ nhớ đệm giới hạn.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use base64::Engine as _;
use utd_engine::{DownloadTask, TaskStatus};

/// Loại file mà mỗi file có icon riêng (đọc từ chính file): nhớ theo đường dẫn, còn lại theo đuôi.
/// Không có `.lnk`/`.url`: icon của lối tắt có thể trỏ tới máy lạ (`\\may\chia-se\x.ico`),
/// vẽ icon là Windows tự gửi thông tin đăng nhập tới đó.
const PER_FILE: [&str; 4] = ["exe", "ico", "msi", "scr"];
const MAX_ENTRIES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum IconKey {
    Extension {
        extension: String,
        size: u32,
    },
    File {
        path: String,
        length: u64,
        modified: u64,
        size: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Source {
    Extension(String),
    File(PathBuf),
}

/// Cỡ ảnh (px thật) để vẽ sắc nét ở 32 px CSS trên màn hình có tỉ lệ `scale`.
pub(super) fn icon_px(scale: f64) -> u32 {
    if scale >= 1.75 {
        64
    } else if scale >= 1.25 {
        48
    } else {
        32
    }
}

/// Chọn nguồn icon cho task (đọc đĩa: chỉ gọi trên luồng nền).
pub(super) fn job_for(task: &DownloadTask, size: u32) -> (IconKey, Source) {
    let extension = safe_extension(&task.file_name);
    if task.status == TaskStatus::Completed
        && PER_FILE.contains(&extension.as_str())
        && let Ok(Some(path)) = super::paths::final_file(task)
        && let Ok(metadata) = std::fs::metadata(&path)
    {
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |elapsed| elapsed.as_millis() as u64);
        let key = IconKey::File {
            path: path.to_string_lossy().to_lowercase(),
            length: metadata.len(),
            modified,
            size,
        };
        return (key, Source::File(path));
    }
    let key = IconKey::Extension {
        extension: extension.clone(),
        size,
    };
    (key, Source::Extension(extension))
}

/// Đuôi dùng được làm tên file giả cho shell; lạ quá thì coi như không có đuôi.
fn safe_extension(name: &str) -> String {
    let extension = super::paths::extension_of(name);
    let plain = extension.chars().count() <= 16
        && extension
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '+' | '~'));
    if plain { extension } else { String::new() }
}

/// Bộ nhớ đệm vào trước ra trước, tối đa `limit` mục; nhớ cả lần lấy hỏng (None) để khỏi thử lại liên tục.
pub(super) struct IconCache {
    entries: HashMap<IconKey, Option<String>>,
    order: VecDeque<IconKey>,
    limit: usize,
}

impl IconCache {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            limit: limit.max(1),
        }
    }

    pub(super) fn get(&self, key: &IconKey) -> Option<Option<String>> {
        self.entries.get(key).cloned()
    }

    pub(super) fn put(&mut self, key: IconKey, value: Option<String>) {
        if self.entries.insert(key.clone(), value).is_none() {
            self.order.push_back(key);
        }
        while self.entries.len() > self.limit {
            match self.order.pop_front() {
                Some(oldest) => {
                    self.entries.remove(&oldest);
                }
                None => break,
            }
        }
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
}

fn cache() -> &'static Mutex<IconCache> {
    static CACHE: OnceLock<Mutex<IconCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(IconCache::new(MAX_ENTRIES)))
}

fn cached(key: &IconKey) -> Option<Option<String>> {
    cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(key)
}

fn remember(key: IconKey, value: Option<String>) {
    cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .put(key, value);
}

/// Icon cho từng task (chạy trên luồng nền). Chỉ trả task lấy được icon.
pub(super) fn icons_for(tasks: &[DownloadTask], size: u32) -> HashMap<String, String> {
    let jobs: Vec<(&str, IconKey, Source)> = tasks
        .iter()
        .map(|task| {
            let (key, source) = job_for(task, size);
            (task.id.as_str(), key, source)
        })
        .collect();
    let mut found: HashMap<IconKey, Option<String>> = HashMap::new();
    let missing: Vec<(&IconKey, &Source)> = jobs
        .iter()
        .filter_map(|(_, key, source)| match cached(key) {
            Some(value) => {
                found.insert(key.clone(), value);
                None
            }
            None => Some((key, source)),
        })
        .collect();
    if !missing.is_empty() {
        super::shell::with_com(|| {
            for (key, source) in missing {
                if found.contains_key(key) {
                    continue;
                }
                let url = extract(source, size)
                    .and_then(|(width, height, rgba)| data_url(width, height, &rgba));
                found.insert(key.clone(), url.clone());
                remember(key.clone(), url);
            }
        });
    }
    jobs.into_iter()
        .filter_map(|(id, key, _)| {
            let url = found.get(&key).cloned().flatten()?;
            Some((id.to_string(), url))
        })
        .collect()
}

/// Pixel 32-bit BGRA của Windows → RGBA thẳng (chưa nhân alpha) cho PNG.
/// Ảnh nhân sẵn alpha (premultiplied) thì chia lại; ảnh không có alpha thì coi là đục.
#[cfg(any(windows, test))]
pub(super) fn straight_rgba(mut pixels: Vec<u8>) -> Vec<u8> {
    let has_alpha = pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0);
    let premultiplied = has_alpha
        && pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[0] <= pixel[3] && pixel[1] <= pixel[3] && pixel[2] <= pixel[3]);
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
        if !has_alpha {
            pixel[3] = 255;
        } else if premultiplied && pixel[3] != 0 && pixel[3] != 255 {
            let alpha = u32::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    pixels
}

pub(super) fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || rgba.len() != width as usize * height as usize * 4 {
        return None;
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Balanced);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(rgba).ok()?;
    writer.finish().ok()?;
    Some(out)
}

pub(super) fn data_url(width: u32, height: u32, rgba: &[u8]) -> Option<String> {
    let png = encode_png(width, height, rgba)?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac::extract;

#[cfg(not(any(windows, target_os = "macos")))]
fn extract(_: &Source, _: u32) -> Option<(u32, u32, Vec<u8>)> {
    None
}

/// Hỏi shell một ảnh icon vuông `size` px (`SIIGBF_ICONONLY`: không lấy ảnh xem trước).
#[cfg(windows)]
pub(super) fn extract(source: &Source, size: u32) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::DeleteObject;
    use windows::Win32::UI::Shell::{
        ILFree, IShellItemImageFactory, SHCreateItemFromIDList, SHCreateItemFromParsingName,
        SHSimpleIDListFromPath, SIIGBF_ICONONLY,
    };
    use windows::core::PCWSTR;

    let side = i32::try_from(size)
        .ok()
        .filter(|side| (16..=256).contains(side))?;
    let factory: IShellItemImageFactory = match source {
        Source::File(path) => {
            let path = wide(&super::paths::shell_path(path));
            unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None) }.ok()?
        }
        // File giả (không có trên đĩa) chỉ để shell tra icon theo đuôi.
        Source::Extension(extension) => {
            let name = if extension.is_empty() {
                "utd-icon-6b1f".to_string()
            } else {
                format!("utd-icon-6b1f.{extension}")
            };
            let path = wide(&std::env::temp_dir().join(name));
            unsafe {
                let item = SHSimpleIDListFromPath(PCWSTR(path.as_ptr()));
                if item.is_null() {
                    return None;
                }
                let factory = SHCreateItemFromIDList(item);
                ILFree(Some(item));
                factory.ok()?
            }
        }
    };
    unsafe {
        let bitmap = factory
            .GetImage(SIZE { cx: side, cy: side }, SIIGBF_ICONONLY)
            .ok()?;
        let pixels = read_bitmap(bitmap);
        let _ = DeleteObject(bitmap.into());
        pixels
    }
}

#[cfg(windows)]
fn wide(path: &std::path::Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Đọc HBITMAP thành RGBA từ trên xuống, bất kể bitmap gốc lưu ngược hay xuôi.
#[cfg(windows)]
unsafe fn read_bitmap(
    bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
        GetDIBits, GetObjectW,
    };

    unsafe {
        let mut info = BITMAP::default();
        let size = std::mem::size_of::<BITMAP>() as i32;
        if GetObjectW(bitmap.into(), size, Some((&raw mut info).cast())) == 0 {
            return None;
        }
        let (width, height) = (info.bmWidth, info.bmHeight.abs());
        if !(1..=256).contains(&width) || !(1..=256).contains(&height) {
            return None;
        }
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        let context = CreateCompatibleDC(None);
        if context.is_invalid() {
            return None;
        }
        let lines = GetDIBits(
            context,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut header,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(context);
        (lines == height).then(|| (width as u32, height as u32, straight_rgba(pixels)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    #[test]
    fn premultiplied_pixels_become_straight_rgba() {
        // BGRA nhân sẵn: đỏ 50% trong suốt.
        let pixels = straight_rgba(vec![0, 0, 128, 128, 10, 20, 30, 255, 0, 0, 0, 0]);
        assert_eq!(&pixels[..4], &[255, 0, 0, 128]);
        assert_eq!(&pixels[4..8], &[30, 20, 10, 255]);
        assert_eq!(&pixels[8..], &[0, 0, 0, 0]);
        // Kênh màu lớn hơn alpha ⇒ ảnh chưa nhân alpha: giữ nguyên màu.
        let straight = straight_rgba(vec![200, 100, 50, 100]);
        assert_eq!(straight, vec![50, 100, 200, 100]);
        // Không có alpha nào ⇒ ảnh đục.
        assert_eq!(straight_rgba(vec![1, 2, 3, 0]), vec![3, 2, 1, 255]);
    }

    #[test]
    fn png_round_trips() {
        let rgba: Vec<u8> = (0..4 * 4 * 4).map(|value| value as u8).collect();
        let png = encode_png(4, 4, &rgba).unwrap();
        assert_eq!(png[..8], PNG_SIGNATURE);
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut buffer).unwrap();
        assert_eq!((frame.width, frame.height), (4, 4));
        assert_eq!(&buffer[..frame.buffer_size()], &rgba[..]);
        assert!(encode_png(4, 4, &rgba[..10]).is_none());
        assert!(
            data_url(1, 1, &[1, 2, 3, 4])
                .unwrap()
                .starts_with("data:image/png;base64,iVBORw0KGgo")
        );
    }

    /// CSP của bản đóng gói (dev và e2e không áp CSP): thiếu `data:` thì mọi icon thật thành ô trống.
    #[test]
    fn shipped_csp_allows_data_icons() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).expect("tauri.conf.json");
        let images = config["app"]["security"]["csp"]["img-src"]
            .as_str()
            .expect("img-src");
        assert!(
            images.split_whitespace().any(|source| source == "data:"),
            "img-src cần data: cho icon file ({images})"
        );
    }

    fn task(name: &str, status: TaskStatus, dir: &str) -> DownloadTask {
        let mut task = DownloadTask::new("https://example.com/x", name, dir, 4).unwrap();
        task.status = status;
        task
    }

    #[test]
    fn cache_keys_follow_file_or_extension() {
        let root = std::env::temp_dir().join(format!("utd-icons-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let dir = root.to_string_lossy().into_owned();
        std::fs::write(root.join("Game.exe"), b"MZ").unwrap();
        let mut done = task("Game.exe", TaskStatus::Paused, &dir);
        let (key, source) = job_for(&done, 32);
        assert_eq!(
            source,
            Source::Extension("exe".into()),
            "chưa xong ⇒ theo đuôi"
        );
        assert_eq!(
            key,
            IconKey::Extension {
                extension: "exe".into(),
                size: 32
            }
        );
        done.status = TaskStatus::Completed;
        assert!(matches!(
            job_for(&done, 64),
            (IconKey::File { size: 64, .. }, Source::File(_))
        ));
        let movie = task("Phim.MP4", TaskStatus::Completed, &dir);
        assert_eq!(job_for(&movie, 32).1, Source::Extension("mp4".into()));
        let gone = task("Mat.exe", TaskStatus::Completed, &dir);
        assert_eq!(
            job_for(&gone, 32).1,
            Source::Extension("exe".into()),
            "file mất ⇒ theo đuôi"
        );
        let odd = task("x.a b", TaskStatus::Queued, &dir);
        assert_eq!(job_for(&odd, 32).1, Source::Extension(String::new()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cache_is_bounded() {
        let mut cache = IconCache::new(2);
        let key = |name: &str| IconKey::Extension {
            extension: name.into(),
            size: 32,
        };
        cache.put(key("a"), Some("A".into()));
        cache.put(key("b"), None);
        cache.put(key("a"), Some("A2".into()));
        assert_eq!(cache.len(), 2);
        cache.put(key("c"), Some("C".into()));
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(&key("a")), None, "mục cũ nhất bị bỏ");
        assert_eq!(cache.get(&key("b")), Some(None));
        assert_eq!(cache.get(&key("c")), Some(Some("C".into())));
    }

    #[test]
    fn icon_size_follows_screen_scale() {
        assert_eq!(icon_px(1.0), 32);
        assert_eq!(icon_px(1.5), 48);
        assert_eq!(icon_px(2.0), 64);
    }

    #[cfg(windows)]
    #[test]
    fn extracts_real_shell_icons() {
        let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let explorer = PathBuf::from(windows).join("explorer.exe");
        super::super::shell::with_com(|| {
            for (source, size) in [
                (Source::File(explorer.clone()), 32),
                (Source::Extension("zip".into()), 32),
                (Source::Extension("exe".into()), 64),
            ] {
                let (width, height, rgba) = extract(&source, size).expect("icon");
                assert_eq!((width, height), (size, size), "{source:?}");
                assert!(
                    rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0),
                    "có hình"
                );
                let png = encode_png(width, height, &rgba).unwrap();
                assert_eq!(png[..8], PNG_SIGNATURE);
            }
        });
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn extracts_real_finder_icons() {
        super::super::shell::with_com(|| {
            for (source, size) in [
                (Source::File(PathBuf::from("/bin/ls")), 32),
                (Source::Extension("zip".into()), 32),
                (Source::Extension("pdf".into()), 64),
                (Source::Extension(String::new()), 48),
            ] {
                let (width, height, rgba) = extract(&source, size).expect("icon");
                assert_eq!((width, height), (size, size), "{source:?}");
                assert!(
                    rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0),
                    "có hình"
                );
                let png = encode_png(width, height, &rgba).unwrap();
                assert_eq!(png[..8], PNG_SIGNATURE);
            }
        });
    }
}
