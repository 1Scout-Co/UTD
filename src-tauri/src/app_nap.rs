//! macOS: App Nap hạ app bị che, thu nhỏ hay lúc khoá màn hình xuống mức nền (CPU và mạng ưu tiên
//! thấp): đo trên Mac thật, tải 1 GB qua 16 kết nối chỉ ngang một kết nối curl. Có download đang chạy thì
//! báo hệ thống đây là việc người dùng đang chờ, hết download thì trả lại. Máy vẫn được ngủ khi rảnh.

use std::sync::Arc;
use std::time::Duration;

use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};

use crate::state::RunningTasks;

const CHECK_EVERY: Duration = Duration::from_secs(2);

/// Luồng nền nhỏ nhìn `RunningTasks` mỗi 2 giây; giữ "activity" khi có download, nhả khi không còn.
pub fn watch(running: Arc<RunningTasks>) {
    let spawned = std::thread::Builder::new()
        .name("utd-app-nap".into())
        .spawn(move || {
            let process = NSProcessInfo::processInfo();
            let reason = NSString::from_str("UTD đang tải file");
            let mut activity = None;
            loop {
                objc2::rc::autoreleasepool(|_| {
                    let busy = !running.is_empty();
                    if busy && activity.is_none() {
                        activity = Some(process.beginActivityWithOptions_reason(
                            NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
                            &reason,
                        ));
                    } else if !busy && let Some(token) = activity.take() {
                        // SAFETY: `token` là đúng đối tượng `beginActivity` trả về, mới nhả một lần.
                        unsafe { process.endActivity(&token) };
                    }
                });
                std::thread::sleep(CHECK_EVERY);
            }
        });
    if let Err(error) = spawned {
        log::warn!("App Nap watcher failed to start: {error}");
    }
}
