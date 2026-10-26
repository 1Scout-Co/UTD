//! yt-dlp đọc cookie từ named pipe thay vì file: không ghi đĩa (notes/auth-plan.md §5.1, đã thử với
//! yt-dlp.exe 2026.10.21 trong `notes/auth-thu/`).
//!
//! yt-dlp (tiến trình con PyInstaller) mở đường dẫn cookie ba lần: `os.access` (nối rồi đóng), nạp (đọc),
//! và lưu lúc thoát (ghi lại cả hũ). Vì vậy: tạo sẵn nhiều instance; mỗi lần nối ghi sẵn nội dung rồi đua
//! — bên kia đọc hết (FlushFileBuffers xong) thì đóng handle để nó gặp EOF (không `DisconnectNamedPipe`:
//! làm ReadFile phía kia lỗi); nhận được byte thì đó là lần lưu, đọc tới hết rồi bỏ. Chỉ phục vụ tiến trình
//! yt-dlp app vừa sinh hoặc con của nó; DACL chỉ người dùng; tên ngẫu nhiên, không ai chiếm trước được.
//!
//! yt-dlp bỏ qua cookie không báo lỗi nếu bước `os.access` thất bại: người gọi phải kiểm `loaded()` trước
//! khi tin kết quả (§5.1).

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use tokio_util::sync::CancellationToken;
use utd_engine::auth::CookieJar;

/// Số instance tạo sẵn: kiểm quyền, nạp, lưu, và một dự phòng.
#[cfg(windows)]
const INSTANCES: usize = 4;

pub(crate) struct CookiePipe {
    path: String,
    allowed: Arc<OnceLock<u32>>,
    loaded: Arc<AtomicBool>,
    stop: CancellationToken,
}

impl CookiePipe {
    /// Đường dẫn truyền cho `yt-dlp --cookies`.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// PID của yt-dlp vừa sinh: chỉ tiến trình này (hoặc con của nó) được đọc cookie.
    pub fn allow(&self, pid: u32) {
        let _ = self.allowed.set(pid);
    }

    /// yt-dlp đã đọc trọn nội dung cookie ít nhất một lần.
    pub fn loaded(&self) -> bool {
        self.loaded.load(Ordering::Acquire)
    }
}

impl Drop for CookiePipe {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

/// macOS dùng FIFO (giai đoạn 2 của kế hoạch): chưa có thì báo, không bao giờ rơi về file trên đĩa.
#[cfg(not(windows))]
pub(crate) fn open(_jar: &CookieJar) -> io::Result<CookiePipe> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "cookie pipe for yt-dlp is not available on this platform yet",
    ))
}

#[cfg(windows)]
pub(crate) fn open(jar: &CookieJar) -> io::Result<CookiePipe> {
    use tokio::net::windows::named_pipe::ServerOptions;
    use zeroize::Zeroizing;

    let path = format!(r"\\.\pipe\utd-ck-{}", uuid::Uuid::new_v4().simple());
    let acl = crate::ipc::pipe_acl::PipeAcl::current_user()?;
    let payload = Arc::new(Zeroizing::new(jar.netscape().as_bytes().to_vec()));
    let allowed = Arc::new(OnceLock::new());
    let loaded = Arc::new(AtomicBool::new(false));
    let stop = CancellationToken::new();
    for index in 0..INSTANCES {
        let server = acl.create(
            ServerOptions::new()
                .first_pipe_instance(index == 0)
                .reject_remote_clients(true)
                .max_instances(INSTANCES)
                .in_buffer_size(256 * 1024)
                .out_buffer_size(256 * 1024),
            &path,
        )?;
        tokio::spawn(serving::serve(
            server,
            payload.clone(),
            allowed.clone(),
            loaded.clone(),
            stop.clone(),
        ));
    }
    Ok(CookiePipe {
        path,
        allowed,
        loaded,
        stop,
    })
}

#[cfg(windows)]
mod serving {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, OnceLock};
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::windows::named_pipe::NamedPipeServer;
    use tokio_util::sync::CancellationToken;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Storage::FileSystem::FlushFileBuffers;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
    use zeroize::Zeroizing;

    /// Chờ app báo PID của yt-dlp (sinh ngay sau khi tạo pipe) tối đa chừng này.
    const WAIT_FOR_PID: Duration = Duration::from_secs(3);

    pub(super) async fn serve(
        mut server: NamedPipeServer,
        payload: Arc<Zeroizing<Vec<u8>>>,
        allowed: Arc<OnceLock<u32>>,
        loaded: Arc<AtomicBool>,
        stop: CancellationToken,
    ) {
        tokio::select! {
            connected = server.connect() => if connected.is_err() { return },
            _ = stop.cancelled() => return,
        }
        if !client_allowed(&server, &allowed).await {
            log::warn!("cookie pipe: refused a client that is not the yt-dlp we started");
            return;
        }
        // Lần `os.access` nối rồi đóng ngay: ghi lỗi là xong.
        if server.write_all(&payload).await.is_err() {
            return;
        }
        let raw = server.as_raw_handle() as isize;
        // SAFETY: handle thuộc `server`, `server` sống tới khi lời gọi này trả về (chờ `flush` bên dưới
        // trước khi bỏ `server`). Trả về khi phía kia đọc hết hoặc đóng.
        let flush = tokio::task::spawn_blocking(move || unsafe {
            FlushFileBuffers(HANDLE(raw as *mut c_void)).is_ok()
        });
        tokio::pin!(flush);
        let mut sink = Zeroizing::new(vec![0u8; 16 * 1024]);
        let mut wrote_back = false;
        let mut reading = true;
        loop {
            tokio::select! {
                flushed = &mut flush => {
                    // Phía kia đọc hết nội dung (không phải lần lưu): yt-dlp đã nạp cookie.
                    if flushed.unwrap_or(false) && !wrote_back {
                        loaded.store(true, Ordering::Release);
                    }
                    return;
                }
                read = server.read(&mut sink[..]), if reading => match read {
                    Ok(0) | Err(_) => reading = false,
                    Ok(_) => wrote_back = true,
                },
                _ = stop.cancelled() => {
                    // Gỡ FlushFileBuffers đang chờ rồi mới bỏ handle.
                    let _ = server.disconnect();
                    let _ = (&mut flush).await;
                    return;
                }
            }
        }
    }

    async fn client_allowed(server: &NamedPipeServer, allowed: &OnceLock<u32>) -> bool {
        let mut client = 0u32;
        // SAFETY: handle của pipe vừa có client nối.
        let known =
            unsafe { GetNamedPipeClientProcessId(HANDLE(server.as_raw_handle()), &mut client) }
                .is_ok();
        if !known {
            return false;
        }
        let deadline = tokio::time::Instant::now() + WAIT_FOR_PID;
        let parent = loop {
            if let Some(pid) = allowed.get() {
                break *pid;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        client == parent
            || tokio::task::spawn_blocking(move || descends_from(client, parent))
                .await
                .unwrap_or(false)
    }

    /// `pid` là con (tới 4 đời) của `ancestor`. yt-dlp.exe (PyInstaller) chạy lại chính nó thành tiến
    /// trình con và chính tiến trình con mở pipe.
    fn descends_from(pid: u32, ancestor: u32) -> bool {
        let parents = parent_map();
        let mut current = pid;
        for _ in 0..4 {
            match parents.iter().find(|(child, _)| *child == current) {
                Some((_, parent)) if *parent == ancestor => return true,
                Some((_, parent)) if *parent != 0 && *parent != current => current = *parent,
                _ => return false,
            }
        }
        false
    }

    fn parent_map() -> Vec<(u32, u32)> {
        let mut pairs = Vec::new();
        // SAFETY: ảnh chụp danh sách tiến trình, đóng ở cuối; PROCESSENTRY32W khởi tạo đúng dwSize.
        unsafe {
            let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
                return pairs;
            };
            let mut entry = PROCESSENTRY32W {
                dwSize: size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snapshot, &mut entry).is_ok() {
                loop {
                    pairs.push((entry.th32ProcessID, entry.th32ParentProcessID));
                    if Process32NextW(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snapshot);
        }
        pairs
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::process::Stdio;
    use std::sync::Arc;

    use utd_engine::auth::{BrowserCookie, CookieJar};
    use zeroize::Zeroizing;

    fn jar(secret: &str) -> CookieJar {
        CookieJar::new([BrowserCookie {
            name: "sid".into(),
            value: Zeroizing::new(secret.into()),
            domain: "127.0.0.1".into(),
            host_only: true,
            path: "/".into(),
            secure: false,
            expires: None,
        }])
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
    }

    /// Tiến trình con app sinh ra (như yt-dlp) đọc được cookies.txt qua pipe; cờ `loaded` bật.
    #[test]
    fn the_spawned_child_reads_the_cookie_file() {
        runtime().block_on(async {
            let pipe = super::open(&jar("UTDCANARYpipe1")).unwrap();
            let child = tokio::process::Command::new("cmd.exe")
                .args(["/C", "type", pipe.path()])
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            pipe.allow(child.id().unwrap());
            let output = child.wait_with_output().await.unwrap();
            let text = String::from_utf8_lossy(&output.stdout);
            assert!(
                text.contains("127.0.0.1\tFALSE\t/\tFALSE\t0\tsid\tUTDCANARYpipe1"),
                "{text}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            assert!(pipe.loaded());
        });
    }

    /// Tiến trình khác (ở đây chính tiến trình test, không phải con của yt-dlp) không nhận được gì.
    #[test]
    fn a_stranger_gets_nothing() {
        runtime().block_on(async {
            let pipe = Arc::new(super::open(&jar("UTDCANARYpipe2")).unwrap());
            let child = tokio::process::Command::new("cmd.exe")
                .args(["/C", "exit 0"])
                .spawn()
                .unwrap();
            pipe.allow(child.id().unwrap());
            let path = pipe.path().to_string();
            let read = tokio::task::spawn_blocking(move || {
                use std::io::Read;
                let mut text = String::new();
                let _ =
                    std::fs::File::open(&path).and_then(|mut file| file.read_to_string(&mut text));
                text
            })
            .await
            .unwrap();
            assert!(!read.contains("UTDCANARYpipe2"), "{read}");
            assert!(!pipe.loaded());
        });
    }

    /// yt-dlp.exe thật (đường dẫn trong `UTD_TEST_YT_DLP`) đọc cookie qua pipe, gửi đúng cookie tới máy chủ
    /// thử, thoát 0 (lần lưu cookie lúc thoát được nhận và bỏ). Chạy: `cargo test -p app --lib -- --ignored
    /// real_yt_dlp`.
    #[test]
    #[ignore = "cần yt-dlp thật: đặt UTD_TEST_YT_DLP"]
    fn real_yt_dlp_sends_the_piped_cookie() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let yt_dlp = std::env::var("UTD_TEST_YT_DLP").expect("UTD_TEST_YT_DLP");
        runtime().block_on(async {
            let secret = "UTDCANARYpipe3";
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let seen = Arc::new(std::sync::Mutex::new(Vec::<bool>::new()));
            let log = seen.clone();
            tokio::spawn(async move {
                loop {
                    let Ok((mut stream, _)) = listener.accept().await else { return };
                    let log = log.clone();
                    tokio::spawn(async move {
                        let mut request = vec![0u8; 8192];
                        let size = stream.read(&mut request).await.unwrap_or(0);
                        let text = String::from_utf8_lossy(&request[..size]).to_string();
                        let signed_in = text.contains(&format!("sid={secret}"));
                        log.lock().unwrap().push(signed_in);
                        let reply = if signed_in {
                            "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: 4\r\nConnection: close\r\n\r\nabcd"
                        } else {
                            "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        };
                        let _ = stream.write_all(reply.as_bytes()).await;
                    });
                }
            });
            let pipe = super::open(&jar(secret)).unwrap();
            let child = tokio::process::Command::new(&yt_dlp)
                .args(["--ignore-config", "--no-warnings", "-J", "--cookies", pipe.path()])
                .arg(format!("http://127.0.0.1:{port}/file.mp4"))
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            pipe.allow(child.id().unwrap());
            let output = child.wait_with_output().await.unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert!(pipe.loaded());
            let seen = seen.lock().unwrap().clone();
            assert!(!seen.is_empty() && seen.iter().all(|signed_in| *signed_in), "{seen:?}");
        });
    }
}
