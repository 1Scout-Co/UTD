//! Gọi shell của Windows như Explorer: mở bằng ứng dụng mặc định, mở thư mục và chọn file,
//! bỏ vào Thùng rác. Mọi hàm chạy trên luồng nền đã khởi tạo COM (STA), không bao giờ panic.
//! macOS làm y như Finder: `shell/mac.rs`.

#[cfg(windows)]
use std::path::Path;

#[cfg(windows)]
pub(super) use imp::{open_file, open_folder, open_web_page, recycle, select_in_folder, with_com};

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub(super) use mac::{open_file, open_folder, open_web_page, recycle, select_in_folder, with_com};

#[cfg(not(any(windows, target_os = "macos")))]
mod fallback {
    use super::super::text;
    use std::path::Path;

    pub fn with_com<T>(work: impl FnOnce() -> T) -> T {
        work()
    }
    pub fn open_file(_: &Path) -> Result<(), String> {
        Err(text::WINDOWS_ONLY.into())
    }
    pub fn open_folder(_: &Path) -> Result<(), String> {
        Err(text::WINDOWS_ONLY.into())
    }
    pub fn select_in_folder(_: &Path) -> Result<(), String> {
        Err(text::WINDOWS_ONLY.into())
    }
    pub fn recycle(_: &[std::path::PathBuf]) -> Result<(), String> {
        Err(text::WINDOWS_ONLY.into())
    }
    pub fn open_web_page(_: &str) -> Result<(), String> {
        Err(text::WINDOWS_ONLY.into())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub(super) use fallback::{open_file, open_folder, open_web_page, recycle, select_in_folder, with_com};

/// Chuỗi UTF-16 kết thúc bằng 0 cho API Windows (đường dẫn đã bỏ `\\?\`).
#[cfg(windows)]
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    super::paths::shell_path(path)
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
mod imp {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    use windows::Win32::Foundation::{E_ABORT, ERROR_CANCELLED, ERROR_NO_ASSOCIATION};
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
        CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::UI::Shell::{
        FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOCONFIRMMKDIR, FOF_NOERRORUI, FOF_SILENT,
        FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FileOperation, IFileOperation,
        IFileOperationProgressSink, IFileOperationProgressSink_Impl, ILCreateFromPathW, ILFree,
        IShellItem, SEE_MASK_NOASYNC, SHCreateItemFromParsingName, SHELLEXECUTEINFOW,
        SHOpenFolderAndSelectItems, ShellExecuteExW, TSF_DELETE_RECYCLE_IF_POSSIBLE,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HRESULT, PCWSTR, Ref, implement, w};

    use super::super::text;
    use super::wide;

    /// Chạy `work` khi luồng này đã ở STA (shell cần). Luồng đã ở MTA thì vẫn chạy, không gỡ COM của người khác.
    pub fn with_com<T>(work: impl FnOnce() -> T) -> T {
        struct Apartment(bool);
        impl Drop for Apartment {
            fn drop(&mut self) {
                if self.0 {
                    unsafe { CoUninitialize() };
                }
            }
        }
        let started =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        let _apartment = Apartment(started.is_ok());
        work()
    }

    /// `ShellExecuteExW` (không qua `cmd /c start`). `SEE_MASK_NOASYNC`: luồng nền sắp gỡ COM
    /// nên phải đợi shell giao việc xong. Người dùng bấm Huỷ (hộp hỏi quyền) không tính là lỗi.
    fn execute(verb: PCWSTR, target: &Path, folder: Option<&Path>) -> windows::core::Result<()> {
        let target = wide(target);
        let folder = folder.map(wide);
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOASYNC,
            lpVerb: verb,
            lpFile: PCWSTR(target.as_ptr()),
            lpDirectory: folder
                .as_ref()
                .map_or(PCWSTR::null(), |folder| PCWSTR(folder.as_ptr())),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        match unsafe { ShellExecuteExW(&mut info) } {
            Err(error) if error.code() == ERROR_CANCELLED.to_hresult() => Ok(()),
            other => other,
        }
    }

    /// Mở bằng ứng dụng mặc định như nhấp đúp trong Explorer; loại file chưa có ứng dụng ⇒ hộp "Mở bằng".
    pub fn open_file(path: &Path) -> Result<(), String> {
        let folder = path.parent();
        match execute(PCWSTR::null(), path, folder) {
            Err(error) if error.code() == ERROR_NO_ASSOCIATION.to_hresult() => {
                execute(w!("openas"), path, folder)
            }
            other => other,
        }
        .map_err(|_| text::OPEN_FAILED.to_string())
    }

    /// Mở chính thư mục. Động từ "explore" chỉ có ở thư mục: không bao giờ chạy một file.
    pub fn open_folder(folder: &Path) -> Result<(), String> {
        execute(w!("explore"), folder, None).map_err(|_| text::OPEN_FAILED.to_string())
    }

    /// Mở trang web bằng trình duyệt mặc định (`files::open_web_page` đã kiểm chỉ http/https).
    pub fn open_web_page(url: &str) -> Result<(), String> {
        execute(PCWSTR::null(), Path::new(url), None).map_err(|_| text::OPEN_FAILED.to_string())
    }

    /// Mở Explorer ở thư mục chứa file và chọn sẵn file đó.
    pub fn select_in_folder(file: &Path) -> Result<(), String> {
        let path = wide(file);
        let shown = unsafe {
            let item = ILCreateFromPathW(PCWSTR(path.as_ptr()));
            if item.is_null() {
                false
            } else {
                let result = SHOpenFolderAndSelectItems(item, None, 0);
                ILFree(Some(item));
                result.is_ok()
            }
        };
        if shown {
            return Ok(());
        }
        // Dự phòng: explorer.exe với đúng một đối số `/select,"<đường dẫn>"`.
        let target = super::super::paths::shell_path(file);
        let mut command = std::process::Command::new("explorer.exe");
        {
            use std::os::windows::process::CommandExt;
            command.raw_arg(format!("/select,\"{}\"", target.display()));
        }
        command
            .spawn()
            .map(drop)
            .map_err(|_| text::OPEN_FAILED.to_string())
    }

    /// Nghe từng bước xoá: mục nào không vào được Thùng rác (ổ mạng, USB, file quá lớn)
    /// thì huỷ thay vì để Windows xoá hẳn.
    #[implement(IFileOperationProgressSink)]
    struct RecycleOnly {
        vetoed: Arc<AtomicBool>,
        failure: Arc<AtomicI32>,
    }

    impl IFileOperationProgressSink_Impl for RecycleOnly_Impl {
        fn StartOperations(&self) -> windows::core::Result<()> {
            Ok(())
        }
        fn FinishOperations(&self, _: HRESULT) -> windows::core::Result<()> {
            Ok(())
        }
        fn PreRenameItem(
            &self,
            _: u32,
            _: Ref<IShellItem>,
            _: &PCWSTR,
        ) -> windows::core::Result<()> {
            Ok(())
        }
        fn PostRenameItem(
            &self,
            _: u32,
            _: Ref<IShellItem>,
            _: &PCWSTR,
            _: HRESULT,
            _: Ref<IShellItem>,
        ) -> windows::core::Result<()> {
            Ok(())
        }
        fn PreMoveItem(
            &self,
            _: u32,
            _: Ref<IShellItem>,
            _: Ref<IShellItem>,
            _: &PCWSTR,
        ) -> windows::core::Result<()> {
            Ok(())
        }
        fn PostMoveItem(
            &self,
            _: u32,
            _: Ref<IShellItem>,
            _: Ref<IShellItem>,
            _: &PCWSTR,
            _: HRESULT,
            _: Ref<IShellItem>,
        ) -> windows::core::Result<()> {
            Ok(())
        }
        fn PreCopyItem(
            &self,
            _: u32,
            _: Ref<IShellItem>,
            _: Ref<IShellItem>,
            _: &PCWSTR,
        ) -> windows::core::Result<()> {
            Ok(())
        }
        fn PostCopyItem(
            &self,
            _: u32,
            _: Ref<IShellItem>,
            _: Ref<IShellItem>,
            _: &PCWSTR,
            _: HRESULT,
            _: Ref<IShellItem>,
        ) -> windows::core::Result<()> {
            Ok(())
        }
        fn PreDeleteItem(&self, flags: u32, _: Ref<IShellItem>) -> windows::core::Result<()> {
            if flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0 {
                self.vetoed.store(true, Ordering::SeqCst);
                return Err(E_ABORT.into());
            }
            Ok(())
        }
        fn PostDeleteItem(
            &self,
            _: u32,
            _: Ref<IShellItem>,
            result: HRESULT,
            _: Ref<IShellItem>,
        ) -> windows::core::Result<()> {
            if result.is_err() {
                self.failure.store(result.0, Ordering::SeqCst);
            }
            Ok(())
        }
        fn PreNewItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
            Ok(())
        }
        fn PostNewItem(
            &self,
            _: u32,
            _: Ref<IShellItem>,
            _: &PCWSTR,
            _: &PCWSTR,
            _: u32,
            _: HRESULT,
            _: Ref<IShellItem>,
        ) -> windows::core::Result<()> {
            Ok(())
        }
        fn UpdateProgress(&self, _: u32, _: u32) -> windows::core::Result<()> {
            Ok(())
        }
        fn ResetTimer(&self) -> windows::core::Result<()> {
            Ok(())
        }
        fn PauseTimer(&self) -> windows::core::Result<()> {
            Ok(())
        }
        fn ResumeTimer(&self) -> windows::core::Result<()> {
            Ok(())
        }
    }

    /// Chuyển các file vào Thùng rác, không hiện hộp thoại. Không bao giờ xoá hẳn:
    /// mục nào không vào được Thùng rác thì dừng cả lượt và báo lỗi.
    pub fn recycle(paths: &[PathBuf]) -> Result<(), String> {
        let vetoed = Arc::new(AtomicBool::new(false));
        let failure = Arc::new(AtomicI32::new(0));
        let performed = unsafe { recycle_items(paths, &vetoed, &failure) };
        let left = paths.iter().any(|path| super::super::disk::occupied(path));
        if !left && performed.is_ok() {
            return Ok(());
        }
        if vetoed.load(Ordering::SeqCst) {
            return Err(text::NO_RECYCLE_BIN.into());
        }
        let code = failure.load(Ordering::SeqCst) as u32;
        // 0x80070020/21: file đang mở trong ứng dụng khác.
        if matches!(code, 0x8007_0020 | 0x8007_0021) {
            return Err(text::IN_USE.into());
        }
        if left {
            log::warn!("Recycle left files behind (hr {code:#x})");
            return Err(text::RECYCLE_FAILED.into());
        }
        Ok(())
    }

    unsafe fn recycle_items(
        paths: &[PathBuf],
        vetoed: &Arc<AtomicBool>,
        failure: &Arc<AtomicI32>,
    ) -> windows::core::Result<()> {
        unsafe {
            let operation: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
            operation.SetOperationFlags(
                FOF_NOCONFIRMATION
                    | FOF_SILENT
                    | FOF_NOERRORUI
                    | FOF_NOCONFIRMMKDIR
                    | FOF_ALLOWUNDO
                    | FOFX_RECYCLEONDELETE
                    | FOFX_EARLYFAILURE,
            )?;
            let sink: IFileOperationProgressSink = RecycleOnly {
                vetoed: vetoed.clone(),
                failure: failure.clone(),
            }
            .into();
            for path in paths {
                let path = wide(path);
                let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None)?;
                operation.DeleteItem(&item, &sink)?;
            }
            operation.PerformOperations()?;
            if operation.GetAnyOperationsAborted()?.as_bool() {
                return Err(E_ABORT.into());
            }
            Ok(())
        }
    }
}
