//! macOS: icon như Finder qua `NSWorkspace` (file có thật: icon của chính nó; còn lại theo đuôi),
//! vẽ vào bitmap RGBA vuông đúng cỡ rồi bỏ nhân alpha cho PNG.

use objc2::AnyThread;
use objc2_app_kit::{
    NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace, NSGraphicsContext, NSWorkspace,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use super::Source;

/// Gọi trong autorelease pool (`shell::with_com`). None khi cỡ lạ hoặc AppKit không vẽ được.
pub(in crate::files) fn extract(source: &Source, size: u32) -> Option<(u32, u32, Vec<u8>)> {
    if !(16..=256).contains(&size) {
        return None;
    }
    let workspace = NSWorkspace::sharedWorkspace();
    let image = match source {
        Source::File(path) => workspace.iconForFile(&NSString::from_str(&path.to_string_lossy())),
        // `iconForContentType` cần UniformTypeIdentifiers; bản theo đuôi này vẫn chạy tới macOS mới nhất.
        #[allow(deprecated)]
        Source::Extension(extension) => workspace.iconForFileType(&NSString::from_str(extension)),
    };
    let side = size as isize;
    // SAFETY: `planes` null ⇒ AppKit tự cấp bộ nhớ; tên không gian màu là hằng của AppKit.
    let bitmap = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            side,
            side,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            side * 4,
            32,
        )
    }?;
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    let frame = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(size as f64, size as f64),
    );
    image.drawInRect_fromRect_operation_fraction(
        frame,
        NSRect::ZERO,
        NSCompositingOperation::Copy,
        1.0,
    );
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();
    read_pixels(&bitmap, size)
}

/// Bitmap của AppKit là RGBA đã nhân alpha, mỗi dòng `bytesPerRow` byte (có thể dư).
fn read_pixels(bitmap: &NSBitmapImageRep, size: u32) -> Option<(u32, u32, Vec<u8>)> {
    let data = bitmap.bitmapData();
    let row = usize::try_from(bitmap.bytesPerRow()).ok()?;
    let width = size as usize * 4;
    if data.is_null() || row < width {
        return None;
    }
    let mut rgba = Vec::with_capacity(width * size as usize);
    for line in 0..size as usize {
        // SAFETY: bitmap có `size` dòng, mỗi dòng `row` ≥ `width` byte, sống tới hết hàm.
        rgba.extend_from_slice(unsafe { std::slice::from_raw_parts(data.add(line * row), width) });
    }
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        if alpha != 0 && alpha != 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    rgba.iter()
        .any(|&byte| byte != 0)
        .then_some((size, size, rgba))
}
