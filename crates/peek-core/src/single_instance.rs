//! Hands a file opened from the shell to the CodePeek window that is already running,
//! so it becomes a tab there instead of a second window.
//!
//! Windows only: a per-user named pipe, local to the machine. Nothing listens on the
//! network and other users cannot write to it. Other platforms always open a new window.

use std::path::{Path, PathBuf};

/// Longest request accepted: a single UTF-8 path.
const MAX_REQUEST_BYTES: usize = 32 * 1024;

/// Sends `path` to a running instance. Returns `true` when one took it over.
pub fn forward_to_running_instance(path: &Path) -> bool {
    let Ok(canonical) = path.canonicalize() else {
        return false;
    };
    if !canonical.is_file() {
        return false;
    }
    platform::forward(&platform::pipe_name(), &canonical)
}

/// Serves open requests from later launches until the process exits.
/// `open` receives a canonical path to an existing file.
pub fn listen(open: impl Fn(PathBuf) + Send + 'static) {
    listen_on(platform::pipe_name(), open);
}

fn listen_on(channel: String, open: impl Fn(PathBuf) + Send + 'static) {
    std::thread::spawn(move || {
        platform::serve(&channel, &|request: &[u8]| {
            if let Some(path) = parse_request(request) {
                open(path);
            }
        });
    });
}

/// A request is only honored when it names an existing regular file by absolute path.
fn parse_request(request: &[u8]) -> Option<PathBuf> {
    if request.is_empty() || request.len() > MAX_REQUEST_BYTES {
        return None;
    }
    let path = PathBuf::from(std::str::from_utf8(request).ok()?);
    (path.is_absolute() && path.is_file()).then_some(path)
}

#[cfg(windows)]
mod platform {
    use super::MAX_REQUEST_BYTES;
    use std::io::Write;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_PIPE_CONNECTED, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        ReadFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId,
        PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow,
        ShowWindow, SW_RESTORE,
    };

    pub(super) fn pipe_name() -> String {
        let user: String = std::env::var("USERNAME")
            .unwrap_or_default()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect();
        format!(r"\\.\pipe\CodePeek.open.{user}")
    }

    pub(super) fn forward(channel: &str, path: &Path) -> bool {
        let Some(text) = path.to_str() else {
            return false;
        };
        let Ok(mut pipe) = std::fs::OpenOptions::new().write(true).open(channel) else {
            return false;
        };
        let mut server_pid = 0u32;
        // SAFETY: the handle belongs to `pipe`, which outlives the call; the out pointer is valid.
        unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle() as HANDLE, &mut server_pid) };
        if pipe.write_all(text.as_bytes()).is_err() {
            return false;
        }
        drop(pipe);
        if server_pid != 0 {
            raise_window_of(server_pid);
        }
        true
    }

    /// The new launch still owns the foreground right the user's click gave it,
    /// so it is the one that can bring the existing window forward.
    fn raise_window_of(pid: u32) {
        unsafe extern "system" fn visit(window: HWND, target: LPARAM) -> i32 {
            let mut owner = 0u32;
            // SAFETY: `window` comes from EnumWindows and the out pointer is valid.
            unsafe { GetWindowThreadProcessId(window, &mut owner) };
            // SAFETY: plain queries and activation on a window handle supplied by the system.
            unsafe {
                if owner as isize == target && IsWindowVisible(window) != 0 {
                    if IsIconic(window) != 0 {
                        ShowWindow(window, SW_RESTORE);
                    }
                    SetForegroundWindow(window);
                    return 0;
                }
            }
            1
        }
        // SAFETY: the callback matches WNDENUMPROC and only reads its arguments.
        unsafe { EnumWindows(Some(visit), pid as isize) };
    }

    pub(super) fn serve(channel: &str, handle: &dyn Fn(&[u8])) {
        let name: Vec<u16> = std::ffi::OsStr::new(channel)
            .encode_wide()
            .chain([0])
            .collect();
        loop {
            // FIRST_PIPE_INSTANCE: only one window per user serves requests, and nobody
            // can squat the name in front of it. REJECT_REMOTE_CLIENTS keeps it local.
            // SAFETY: `name` is a valid NUL-terminated wide string; default security
            // lets only this user (and administrators) write to the pipe.
            let pipe = unsafe {
                CreateNamedPipeW(
                    name.as_ptr(),
                    PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    0,
                    MAX_REQUEST_BYTES as u32,
                    0,
                    std::ptr::null(),
                )
            };
            if pipe == INVALID_HANDLE_VALUE {
                // Another CodePeek window owns the pipe; take over if it closes.
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
            // SAFETY: `pipe` is a valid server handle created above.
            let connected = unsafe {
                ConnectNamedPipe(pipe, std::ptr::null_mut()) != 0
                    || GetLastError() == ERROR_PIPE_CONNECTED
            };
            if connected {
                let mut request = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let mut read = 0u32;
                    // SAFETY: `chunk` is writable for its full length and `read` is valid.
                    let ok = unsafe {
                        ReadFile(
                            pipe,
                            chunk.as_mut_ptr(),
                            chunk.len() as u32,
                            &mut read,
                            std::ptr::null_mut(),
                        )
                    };
                    if ok == 0 || read == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..read as usize]);
                    if request.len() > MAX_REQUEST_BYTES {
                        request.clear();
                        break;
                    }
                }
                handle(&request);
            }
            // SAFETY: `pipe` is still the valid handle created in this iteration.
            unsafe {
                DisconnectNamedPipe(pipe);
                CloseHandle(pipe);
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::Path;

    pub(super) fn pipe_name() -> String {
        String::new()
    }

    pub(super) fn forward(_: &str, _: &Path) -> bool {
        false
    }

    pub(super) fn serve(_: &str, _: &dyn Fn(&[u8])) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_must_name_an_existing_absolute_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("informe año.pdf");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(
            parse_request(file.to_str().unwrap().as_bytes()),
            Some(file.clone())
        );
        assert!(parse_request(b"").is_none());
        assert!(parse_request(b"relativo.txt").is_none());
        assert!(parse_request(dir.path().to_str().unwrap().as_bytes()).is_none());
        assert!(parse_request(&[0xFF, 0xFE, 0x00]).is_none());
        assert!(parse_request(&vec![b'a'; MAX_REQUEST_BYTES + 1]).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn a_second_launch_hands_its_file_to_the_running_window() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("presupuesto año.xlsx");
        let second = dir.path().join("canción.pdf");
        std::fs::write(&first, b"x").unwrap();
        std::fs::write(&second, b"y").unwrap();
        let channel = format!(r"\\.\pipe\CodePeek.test.{}", std::process::id());

        // Nobody is listening yet: the launch must open its own window.
        assert!(!platform::forward(&channel, &first));

        let (tx, rx) = std::sync::mpsc::channel();
        listen_on(channel.clone(), move |path| {
            let _ = tx.send(path);
        });
        for file in [&first, &second] {
            let mut delivered = false;
            for _ in 0..200 {
                if platform::forward(&channel, file) {
                    delivered = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(delivered, "the running instance never accepted {file:?}");
            let received = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
            assert_eq!(&received, file);
        }
    }
}
