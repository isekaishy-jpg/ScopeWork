//! Named-pipe transport and the hidden shutdown window: the coordinator's only FFI.
//!
//! The pipe is restricted to its owner (`D:P(A;;GA;;;OW)`), rejects remote clients and is
//! created with `FILE_FLAG_FIRST_PIPE_INSTANCE`, so a process that took the name first is
//! detected. Clients open it as a file and check that the serving process runs as the same
//! user. The hidden top-level window lets Restart Manager, logoff and shutdown ask the
//! coordinator to finish and exit, which a windowless process cannot otherwise receive.
use std::fs::File;
use std::path::Path;

/// The pipe name for a research home: unique per user and per home.
pub fn pipe_name(home: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let canonical = std::fs::canonicalize(home)
        .unwrap_or_else(|_| home.to_path_buf())
        .to_string_lossy()
        .to_lowercase();
    let digest = Sha256::digest(format!("{}\n{canonical}", imp::current_user_sid()?).as_bytes());
    let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!(r"\\.\pipe\scopework-{hex}"))
}

/// Creates the server side. `first` claims the name; it fails if any process already holds it.
pub fn listen(name: &str, first: bool) -> Result<File, String> {
    imp::listen(name, first)
}
/// Waits for a client on a pipe instance created by `listen`.
pub fn accept(instance: &File) -> Result<(), String> {
    imp::accept(instance)
}
/// Opens a client connection, or `None` when no coordinator serves this name. Fails when the
/// serving process belongs to another user.
pub fn connect(name: &str) -> Result<Option<File>, String> {
    imp::connect(name)
}
/// Stops this process's standard handles from being inherited, so a coordinator started from
/// here does not hold the caller's output pipe open (a script reading this command's output
/// would otherwise wait until the coordinator exits).
pub fn keep_standard_handles() {
    imp::keep_standard_handles()
}
/// Starts the hidden window thread. `on_query` runs when Windows asks whether the process may
/// end (stop admitting work); `on_end` runs when it will end (finish and exit).
pub fn shutdown_window(on_query: fn(), on_end: fn()) {
    imp::shutdown_window(on_query, on_end)
}

#[cfg(windows)]
mod imp {
    use std::fs::File;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_PIPE_CONNECTED, GetLastError, HANDLE, HWND, INVALID_HANDLE_VALUE,
        LPARAM, LRESULT, LocalFree, WPARAM,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        EqualSid, GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeServerProcessId, PIPE_READMODE_BYTE,
        PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, RegisterClassW,
        TranslateMessage, WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW, WS_OVERLAPPEDWINDOW,
    };

    const PIPE_ACCESS_DUPLEX: u32 = 3;
    const FILE_FLAG_FIRST_PIPE_INSTANCE: u32 = 0x0008_0000;
    const ERROR_PIPE_BUSY: i32 = 231;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }
    /// The token user of a process, as owned SID bytes.
    fn token_user(process: HANDLE) -> Result<Vec<u8>, String> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: valid process handle and out pointer.
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
            return Err("Cannot open process token".into());
        }
        let mut buf = vec![0u8; 256];
        let mut len = 0u32;
        // SAFETY: buffer and length describe owned memory; the token is closed below.
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buf.as_mut_ptr() as _,
                buf.len() as u32,
                &mut len,
            )
        };
        // SAFETY: the token handle was opened above.
        unsafe { CloseHandle(token) };
        if ok == 0 {
            return Err("Cannot read process user".into());
        }
        buf.truncate(len as usize);
        Ok(buf)
    }
    /// The SID pointer of a TOKEN_USER held in a byte buffer. A Vec<u8> is only byte-aligned,
    /// so the structure is read unaligned; the SID it points to lies inside the same buffer.
    fn sid(token_user: &[u8]) -> *mut core::ffi::c_void {
        debug_assert!(token_user.len() >= std::mem::size_of::<TOKEN_USER>());
        // SAFETY: the buffer holds a TOKEN_USER returned by GetTokenInformation.
        unsafe {
            std::ptr::read_unaligned(token_user.as_ptr() as *const TOKEN_USER)
                .User
                .Sid
        }
    }
    fn same_user(a: &[u8], b: &[u8]) -> bool {
        // SAFETY: both SIDs point into live buffers returned by GetTokenInformation.
        unsafe { EqualSid(sid(a), sid(b)) != 0 }
    }
    pub fn current_user_sid() -> Result<String, String> {
        // SAFETY: the pseudo handle of the current process needs no closing.
        let user = token_user(unsafe { GetCurrentProcess() })?;
        let mut text: *mut u16 = std::ptr::null_mut();
        // SAFETY: user holds a TOKEN_USER; text receives a LocalAlloc'd string freed below.
        let ok = unsafe { ConvertSidToStringSidW(sid(&user), &mut text) };
        if ok == 0 {
            return Err("Cannot format user SID".into());
        }
        let mut len = 0;
        // SAFETY: text is NUL-terminated.
        while unsafe { *text.add(len) } != 0 {
            len += 1;
        }
        // SAFETY: len counts the initialised characters before the terminator.
        let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
        // SAFETY: allocated by ConvertSidToStringSidW.
        unsafe { LocalFree(text as _) };
        Ok(sid)
    }
    pub fn listen(name: &str, first: bool) -> Result<File, String> {
        let name = wide(name);
        let descriptor = wide("D:P(A;;GA;;;OW)");
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 0,
        };
        // SAFETY: descriptor is NUL-terminated; the result is freed after use below.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                descriptor.as_ptr(),
                SDDL_REVISION_1,
                &mut attributes.lpSecurityDescriptor,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err("Cannot build the pipe's security descriptor".into());
        }
        let flags = PIPE_ACCESS_DUPLEX
            | if first {
                FILE_FLAG_FIRST_PIPE_INSTANCE
            } else {
                0
            };
        // SAFETY: name is NUL-terminated and attributes point to a valid descriptor.
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                flags,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                &attributes,
            )
        };
        // SAFETY: GetLastError right after the call; the descriptor is no longer needed.
        let error = unsafe { GetLastError() };
        unsafe { LocalFree(attributes.lpSecurityDescriptor as _) };
        if handle == INVALID_HANDLE_VALUE {
            return Err(if first && error == 5 {
                "pipe_taken: another process already serves this pipe name".into()
            } else {
                format!("Cannot create pipe (error {error})")
            });
        }
        // SAFETY: a valid pipe handle owned from here on by the File.
        Ok(unsafe { File::from_raw_handle(handle as _) })
    }
    pub fn accept(instance: &File) -> Result<(), String> {
        // SAFETY: the handle belongs to a live pipe instance.
        if unsafe { ConnectNamedPipe(instance.as_raw_handle() as HANDLE, std::ptr::null_mut()) }
            == 0
        {
            // SAFETY: read right after the failing call.
            let error = unsafe { GetLastError() };
            if error != ERROR_PIPE_CONNECTED {
                return Err(format!("Cannot accept a client (error {error})"));
            }
        }
        Ok(())
    }
    pub fn connect(name: &str) -> Result<Option<File>, String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let pipe = loop {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(name)
            {
                Ok(pipe) => break pipe,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                // Every instance is busy; the server creates another for each client.
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                    if std::time::Instant::now() >= deadline {
                        return Ok(None);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(e) => return Err(format!("Cannot reach the coordinator: {e}")),
            }
        };
        let mut pid = 0u32;
        // SAFETY: valid pipe handle and out pointer.
        if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle() as HANDLE, &mut pid) } == 0 {
            return Err("Cannot identify the coordinator process".into());
        }
        // SAFETY: plain query access; the handle is closed below.
        let server = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if server.is_null() {
            return Err("Cannot inspect the coordinator process".into());
        }
        let theirs = token_user(server);
        // SAFETY: opened above.
        unsafe { CloseHandle(server) };
        // SAFETY: the pseudo handle of the current process needs no closing.
        let mine = token_user(unsafe { GetCurrentProcess() })?;
        if !same_user(&theirs?, &mine) {
            return Err("The process serving this research home's pipe belongs to another user; refusing it.".into());
        }
        Ok(Some(pipe))
    }

    pub fn keep_standard_handles() {
        use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
        use windows_sys::Win32::System::Console::{
            GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        };
        for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            // SAFETY: GetStdHandle returns this process's handle or null; clearing the inherit
            // flag on a valid handle has no other effect.
            unsafe {
                let handle = GetStdHandle(which);
                if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                    SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
                }
            }
        }
    }
    /// The query-end-session and end-session handlers.
    type Callbacks = (fn(), fn());
    static CALLBACKS: std::sync::OnceLock<Callbacks> = std::sync::OnceLock::new();
    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match (msg, CALLBACKS.get()) {
            (WM_QUERYENDSESSION, Some((on_query, _))) => {
                on_query();
                1
            }
            (WM_ENDSESSION, Some((_, on_end))) => {
                if wparam != 0 {
                    on_end();
                }
                0
            }
            // SAFETY: default handling for every other message.
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }
    pub fn shutdown_window(on_query: fn(), on_end: fn()) {
        if CALLBACKS.set((on_query, on_end)).is_err() {
            return;
        }
        std::thread::spawn(|| {
            let class = wide("ScopeworkCoordinator");
            // SAFETY: standard window-class registration and a never-shown top-level window,
            // with a message loop on this thread for its lifetime.
            unsafe {
                let instance = GetModuleHandleW(std::ptr::null());
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(window_proc),
                    hInstance: instance,
                    lpszClassName: class.as_ptr(),
                    ..std::mem::zeroed()
                };
                if RegisterClassW(&wc) == 0 {
                    return;
                }
                let hwnd = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    class.as_ptr(),
                    WS_OVERLAPPEDWINDOW,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    instance,
                    std::ptr::null(),
                );
                if hwnd.is_null() {
                    return;
                }
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        });
    }
    #[cfg(test)]
    pub fn same_user_bytes(a: &[u8], b: &[u8]) -> bool {
        same_user(a, b)
    }
    #[cfg(test)]
    pub fn own_token() -> Vec<u8> {
        // SAFETY: the pseudo handle of the current process needs no closing.
        token_user(unsafe { GetCurrentProcess() }).unwrap()
    }
}

#[cfg(not(windows))]
mod imp {
    use std::fs::File;
    const UNSUPPORTED: &str = "The coordinator needs Windows named pipes; using direct mode.";
    pub fn current_user_sid() -> Result<String, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn listen(_: &str, _: bool) -> Result<File, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn accept(_: &File) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    pub fn connect(_: &str) -> Result<Option<File>, String> {
        Ok(None)
    }
    pub fn shutdown_window(_: fn(), _: fn()) {}
    pub fn keep_standard_handles() {}
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn a_second_server_for_the_same_name_is_refused() {
        let name = format!(r"\\.\pipe\scopework-test-{}", uuid::Uuid::new_v4().simple());
        let first = listen(&name, true).unwrap();
        let second = listen(&name, true);
        assert!(second.unwrap_err().starts_with("pipe_taken"));
        // The owner may add instances for further clients.
        let extra = listen(&name, false).unwrap();
        drop((first, extra));
    }

    #[test]
    fn a_client_reaches_its_own_users_server_and_absent_names_fall_back() {
        let name = format!(r"\\.\pipe\scopework-test-{}", uuid::Uuid::new_v4().simple());
        assert!(connect(&name).unwrap().is_none(), "no server: direct mode");
        let instance = listen(&name, true).unwrap();
        let server = std::thread::spawn(move || {
            accept(&instance).unwrap();
            instance
        });
        let client = connect(&name).unwrap();
        assert!(client.is_some());
        drop(server.join().unwrap());
    }

    #[test]
    fn a_different_user_is_not_the_same_user() {
        let mine = imp::own_token();
        assert!(imp::same_user_bytes(&mine, &mine));
        // A TOKEN_USER whose SID is LocalSystem (S-1-5-18), standing in for another account.
        let mut other = mine.clone();
        let sid_offset = std::mem::size_of::<windows_sys::Win32::Security::TOKEN_USER>();
        let system: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];
        other.truncate(sid_offset);
        other.extend_from_slice(&system);
        // Point the TOKEN_USER's SID at the substituted bytes.
        let base = other.as_mut_ptr();
        // SAFETY: `other` is large enough for TOKEN_USER plus the 12-byte SID written above;
        // the byte buffer is only byte-aligned, so the structure is accessed unaligned.
        unsafe {
            let user = base as *mut windows_sys::Win32::Security::TOKEN_USER;
            let mut value = std::ptr::read_unaligned(user);
            value.User.Sid = base.add(sid_offset) as _;
            std::ptr::write_unaligned(user, value);
        }
        assert!(!imp::same_user_bytes(&other, &mine));
    }
}
