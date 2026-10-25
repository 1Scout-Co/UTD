//! Windows: named pipe chỉ người dùng đang chạy app mở được. Không truyền security attributes thì Windows
//! dùng DACL mặc định của named pipe, vẫn cho Everyone và tài khoản ẩn danh quyền đọc
//! (learn.microsoft.com/windows/win32/ipc/named-pipe-security-and-access-rights). Dùng cho pipe IPC (có
//! token) và sau này cho pipe cookie của yt-dlp (notes/auth-plan.md §5.1).

use std::ffi::c_void;
use std::io;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{HSTRING, PWSTR};

/// Security descriptor `D:P(A;;GA;;;<SID người dùng>)`: một ACE duy nhất, không nhận quyền thừa kế.
pub(crate) struct PipeAcl {
    descriptor: PSECURITY_DESCRIPTOR,
}

// SAFETY: descriptor chỉ được đọc sau khi tạo (CreateNamedPipeW chép nó vào pipe) và giải phóng đúng một
// lần ở `Drop`, nên dùng chung giữa các luồng được.
unsafe impl Send for PipeAcl {}
unsafe impl Sync for PipeAcl {}

impl PipeAcl {
    pub(crate) fn current_user() -> io::Result<Self> {
        let sddl = HSTRING::from(format!("D:P(A;;GA;;;{})", current_user_sid()?));
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: chuỗi SDDL kết thúc NUL; Windows cấp bộ nhớ bằng LocalAlloc, `Drop` giải phóng.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &sddl,
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(io::Error::other)?;
        Ok(Self { descriptor })
    }

    /// Một instance của pipe `name` mang DACL này, theo các tuỳ chọn của `options`.
    pub(crate) fn create(&self, options: &ServerOptions, name: &str) -> io::Result<NamedPipeServer> {
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor.0,
            bInheritHandle: false.into(),
        };
        // SAFETY: `attributes` sống hết lời gọi; descriptor hợp lệ tới khi `self` bị drop.
        unsafe {
            options.create_with_security_attributes_raw(name, (&raw mut attributes).cast::<c_void>())
        }
    }
}

impl Drop for PipeAcl {
    fn drop(&mut self) {
        // SAFETY: bộ nhớ do ConvertStringSecurityDescriptorToSecurityDescriptorW cấp, chưa ai giải phóng.
        unsafe { LocalFree(Some(HLOCAL(self.descriptor.0))) };
    }
}

/// SID của người dùng đang chạy app, dạng chữ (`S-1-5-21-…`).
pub(crate) fn current_user_sid() -> io::Result<String> {
    let mut token = HANDLE::default();
    // SAFETY: handle giả của tiến trình hiện tại; token đóng ở cuối hàm.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(io::Error::other)?;
    let sid = token_user_sid(token);
    // SAFETY: token vừa mở ở trên, đóng đúng một lần.
    let _ = unsafe { CloseHandle(token) };
    sid
}

fn token_user_sid(token: HANDLE) -> io::Result<String> {
    let mut needed = 0u32;
    // Lần đầu chỉ hỏi cỡ bộ đệm: lỗi ERROR_INSUFFICIENT_BUFFER là đúng ý.
    // SAFETY: không truyền bộ đệm, Windows chỉ ghi `needed`.
    let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut needed) };
    // u64 để TOKEN_USER (chứa con trỏ) được căn đúng.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(size_of::<u64>())];
    // SAFETY: bộ đệm đủ `needed` byte, căn 8 byte.
    unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )
    }
    .map_err(io::Error::other)?;
    // SAFETY: Windows vừa ghi một TOKEN_USER hợp lệ vào đầu bộ đệm; SID nằm trong cùng bộ đệm.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = PWSTR::null();
    // SAFETY: SID hợp lệ; Windows cấp chuỗi bằng LocalAlloc, giải phóng ngay dưới.
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) }.map_err(io::Error::other)?;
    // SAFETY: chuỗi kết thúc NUL do Windows vừa cấp.
    let sid = unsafe { text.to_string() }.map_err(io::Error::other);
    // SAFETY: giải phóng đúng một lần chuỗi vừa cấp.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    sid
}

#[cfg(test)]
mod tests {
    use std::os::windows::io::AsRawHandle;

    use uuid::Uuid;
    use windows::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertStringSidToSidW, GetSecurityInfo,
        SE_KERNEL_OBJECT,
    };
    use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSID};

    use super::*;

    /// SID dạng `S-1-…` của người được cấp quyền trong ACE cuối của DACL. SDDL viết tắt vài SID quen
    /// (vd `LA` = tài khoản Administrator dựng sẵn — máy CI của GitHub chạy bằng tài khoản này), nên đổi
    /// về dạng đầy đủ rồi mới so với SID của người dùng hiện tại.
    fn trustee_sid(dacl: &str) -> String {
        let trustee = dacl.trim_end_matches(')').rsplit(";;;").next().unwrap();
        let mut sid = PSID::default();
        // SAFETY: chuỗi SID hợp lệ; Windows cấp SID bằng LocalAlloc, giải phóng ngay dưới.
        unsafe { ConvertStringSidToSidW(&HSTRING::from(trustee), &mut sid) }.unwrap();
        let mut text = PWSTR::null();
        // SAFETY: SID vừa được cấp ở trên.
        unsafe { ConvertSidToStringSidW(sid, &mut text) }.unwrap();
        // SAFETY: chuỗi kết thúc NUL do Windows vừa cấp; giải phóng chuỗi và SID đúng một lần.
        let value = unsafe { text.to_string() }.unwrap();
        unsafe {
            LocalFree(Some(HLOCAL(text.0.cast())));
            LocalFree(Some(HLOCAL(sid.0)));
        }
        value
    }

    /// DACL đọc lại từ pipe, dạng SDDL.
    fn dacl_of(server: &NamedPipeServer) -> String {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: handle của pipe còn sống; Windows cấp descriptor, giải phóng ở cuối.
        let status = unsafe {
            GetSecurityInfo(
                HANDLE(server.as_raw_handle()),
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                Some(&mut descriptor),
            )
        };
        assert!(status.is_ok(), "GetSecurityInfo: {status:?}");
        let mut text = PWSTR::null();
        // SAFETY: descriptor vừa đọc ở trên.
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                None,
            )
        }
        .unwrap();
        // SAFETY: chuỗi do Windows cấp; giải phóng chuỗi và descriptor đúng một lần.
        let sddl = unsafe { text.to_string() }.unwrap();
        unsafe {
            LocalFree(Some(HLOCAL(text.0.cast())));
            LocalFree(Some(HLOCAL(descriptor.0)));
        }
        sddl
    }

    #[test]
    fn only_the_current_user_is_allowed_and_the_name_cannot_be_taken_first() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let acl = PipeAcl::current_user().unwrap();
            let sid = current_user_sid().unwrap();
            assert!(sid.starts_with("S-1-"), "{sid}");
            let name = format!(r"\\.\pipe\utd-acl-test-{}", Uuid::new_v4().simple());
            let server = acl
                .create(ServerOptions::new().first_pipe_instance(true), &name)
                .unwrap();

            // Một ACE cho đúng người dùng; không Everyone (WD), ẩn danh (AN), Administrators (BA), SYSTEM (SY).
            let dacl = dacl_of(&server);
            assert!(dacl.starts_with("D:P(A;;"), "{dacl}");
            assert_eq!(trustee_sid(&dacl), sid, "{dacl}");
            assert!(trustee_sid("D:P(A;;FA;;;LA)").ends_with("-500"));
            assert_eq!(dacl.matches('(').count(), 1, "{dacl}");

            // Native host (cùng người dùng) mở được bằng đường dẫn như file.
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&name)
                .unwrap();
            // Tên đã có instance ⇒ "first instance" bị từ chối: kẻ chen trước không giữ được tên.
            assert!(
                acl.create(ServerOptions::new().first_pipe_instance(true), &name)
                    .is_err()
            );
            // Instance kế tiếp (không đòi "first") của chính app vẫn tạo được.
            acl.create(&ServerOptions::new(), &name).unwrap();
            drop(server);
        });
    }
}
