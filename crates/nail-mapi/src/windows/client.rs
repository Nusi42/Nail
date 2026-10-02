//! The DLL's end of the named pipe (ADR 0087, ADR 0088).
//!
//! * Pipe: `\\.\pipe\nail-mapi-<user SID>` ([`mapi_pipe_name`]), or
//!   `NAIL_MAPI_PIPE` in tests. Opened with `SECURITY_IDENTIFICATION` so
//!   the server can identify, not impersonate, the caller.
//! * The server must run as the same user (`GetNamedPipeServerProcessId`
//!   → its token's user SID = ours); otherwise `MAPI_E_ACCESS_DENIED`
//!   (someone squatting the name).
//! * Nothing listening: start `Nail.exe` ([`app_exe_from_dll`], else the
//!   installation directory from the registry) and retry for
//!   [`MAPI_CONNECT_TIMEOUT_MS`]; then wait up to
//!   [`MAPI_RESPONSE_TIMEOUT_MS`] for the answer (overlapped I/O, so a hung
//!   app cannot hang the calling program forever).

use std::ffi::c_void;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nail_api::desktop::registry as install_registry;
use nail_api::mapi::{
    APP_EXE_FILE, MAPI_CONNECT_TIMEOUT_MS, MAPI_PIPE_ENV, MAPI_RESPONSE_TIMEOUT_MS,
    MapiPipeRequest, MapiPipeResponse, MapiResult, app_exe_from_dll, decode_frame, encode_frame,
    frame_len, mapi_pipe_name,
};
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_MORE_DATA, ERROR_PIPE_BUSY,
    GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
    WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSidToSidW};
use windows_sys::Win32::Security::{
    EqualSid, GetLengthSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, OPEN_EXISTING, ReadFile, SECURITY_IDENTIFICATION,
    SECURITY_SQOS_PRESENT, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleFileNameW, GetModuleHandleExW,
};
use windows_sys::Win32::System::Pipes::{GetNamedPipeServerProcessId, WaitNamedPipeW};
use windows_sys::Win32::System::Threading::{
    CREATE_NEW_PROCESS_GROUP, CreateEventW, CreateProcessW, GetCurrentProcess, OpenProcess,
    OpenProcessToken, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOW,
    WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

use super::text::to_wide;
use crate::forward::Transport;

/// Environment variable naming the program to start when nothing serves
/// the pipe. Only read together with `NAIL_MAPI_PIPE` (tests: the harness
/// plays `Nail.exe`); a real installation never needs it.
pub const MAPI_APP_ENV: &str = "NAIL_MAPI_APP";

fn owned(handle: HANDLE) -> Option<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        None
    } else {
        // SAFETY: a fresh handle this module opened and nobody else owns.
        Some(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}

fn last_error() -> u32 {
    // SAFETY: reads the thread's last-error value.
    unsafe { GetLastError() }
}

/// A SID copied out of a token (DWORD-aligned storage).
pub struct Sid(Vec<u32>);

impl Sid {
    fn as_psid(&self) -> *mut c_void {
        self.0.as_ptr().cast_mut().cast()
    }

    /// The `S-1-5-21-…` form.
    pub fn to_string_sid(&self) -> Option<String> {
        let mut out: *mut u16 = std::ptr::null_mut();
        // SAFETY: `self` holds a valid SID; Windows allocates `out`.
        if unsafe { ConvertSidToStringSidW(self.as_psid(), &mut out) } == 0 || out.is_null() {
            return None;
        }
        let mut len = 0;
        // SAFETY: `out` is a zero-terminated string from Windows.
        while unsafe { *out.add(len) } != 0 {
            len += 1;
        }
        // SAFETY: `len` units were just scanned.
        let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(out, len) });
        // SAFETY: frees the LocalAlloc'ed string.
        unsafe { LocalFree(out.cast()) };
        Some(text)
    }

    /// Parses the `S-1-…` form.
    pub fn from_string_sid(text: &str) -> Option<Self> {
        let wide = to_wide(text);
        let mut sid = std::ptr::null_mut();
        // SAFETY: zero-terminated input; Windows allocates `sid`.
        if unsafe { ConvertStringSidToSidW(wide.as_ptr(), &mut sid) } == 0 || sid.is_null() {
            return None;
        }
        // SAFETY: `sid` is a valid SID from Windows.
        let len = unsafe { GetLengthSid(sid) } as usize;
        let mut copy = vec![0u32; len.div_ceil(4)];
        // SAFETY: copies `len` bytes into storage of at least `len`, then
        // frees Windows' allocation.
        unsafe {
            std::ptr::copy_nonoverlapping(sid.cast::<u8>(), copy.as_mut_ptr().cast(), len);
            LocalFree(sid);
        }
        Some(Self(copy))
    }

    fn equals(&self, other: &Sid) -> bool {
        // SAFETY: both hold valid SIDs.
        unsafe { EqualSid(self.as_psid(), other.as_psid()) != 0 }
    }
}

/// The user SID of a process token.
fn token_user(token: &OwnedHandle) -> Option<Sid> {
    let mut needed = 0u32;
    // SAFETY: measuring call.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    if needed == 0 {
        return None;
    }
    // u64 storage keeps TOKEN_USER's pointer aligned.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: `buffer` holds `needed` bytes.
    let ok = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    if ok == 0 {
        return None;
    }
    // SAFETY: Windows wrote a TOKEN_USER at the start of the buffer.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let sid = user.User.Sid;
    // SAFETY: `sid` points into `buffer` and is valid.
    let len = unsafe { GetLengthSid(sid) } as usize;
    let mut copy = vec![0u32; len.div_ceil(4)];
    // SAFETY: copies `len` bytes of the SID into storage of at least `len`.
    unsafe { std::ptr::copy_nonoverlapping(sid.cast::<u8>(), copy.as_mut_ptr().cast(), len) };
    Some(Sid(copy))
}

fn process_user(process: HANDLE) -> Option<Sid> {
    let mut token = std::ptr::null_mut();
    // SAFETY: `process` is a process handle with query rights.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }
    token_user(&owned(token)?)
}

/// The user this process runs as.
pub fn current_user() -> Option<Sid> {
    // SAFETY: the pseudo handle of the current process needs no closing.
    process_user(unsafe { GetCurrentProcess() })
}

/// The pipe of the current user: `NAIL_MAPI_PIPE` when set, else
/// [`mapi_pipe_name`] with the user's SID.
pub fn pipe_name() -> Option<String> {
    match std::env::var(MAPI_PIPE_ENV) {
        Ok(name) if !name.trim().is_empty() => Some(name),
        _ => Some(mapi_pipe_name(&current_user()?.to_string_sid()?)),
    }
}

/// The path of the module (DLL or test executable) containing this code.
pub fn own_module_path() -> Option<PathBuf> {
    let mut module = std::ptr::null_mut();
    let address = own_module_path as *const () as *const u16;
    // SAFETY: looks up the module containing a function of this module
    // without changing its reference count.
    let ok = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            address,
            &mut module,
        )
    };
    if ok == 0 {
        return None;
    }
    let mut buffer = vec![0u16; 32 * 1024];
    // SAFETY: `buffer` has room for the given number of units.
    let len = unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32) };
    if len == 0 || len as usize >= buffer.len() {
        return None;
    }
    Some(PathBuf::from(String::from_utf16_lossy(
        &buffer[..len as usize],
    )))
}

/// The installation directory the installers recorded
/// (`HKCU`, then `HKLM` 64-bit view, `Software\Nail\Install\InstallDir`).
fn registry_install_dir() -> Option<PathBuf> {
    const KEY_WOW64_64KEY: u32 = 0x0100;
    for root in [
        windows_registry::CURRENT_USER,
        windows_registry::LOCAL_MACHINE,
    ] {
        let dir = root
            .options()
            .read()
            .access(KEY_WOW64_64KEY)
            .open(install_registry::INSTALL_KEY)
            .and_then(|key| key.get_string(install_registry::INSTALL_DIR_VALUE));
        if let Ok(dir) = dir
            && !dir.trim().is_empty()
        {
            return Some(PathBuf::from(dir));
        }
    }
    None
}

/// The program to start when the app is not running.
pub fn app_to_start(pipe_overridden: bool) -> Option<PathBuf> {
    if pipe_overridden {
        // Test setups never start an installed Nail.
        return std::env::var_os(MAPI_APP_ENV)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
    }
    own_module_path()
        .and_then(|dll| app_exe_from_dll(&dll))
        .filter(|exe| exe.is_file())
        .or_else(|| {
            registry_install_dir()
                .map(|dir| dir.join(APP_EXE_FILE))
                .filter(|exe| exe.is_file())
        })
}

/// Starts `exe` without inheriting any of the calling program's handles.
fn launch(exe: &Path) -> bool {
    let application = to_wide(&exe.to_string_lossy());
    let mut command_line = to_wide(&format!("\"{}\"", exe.to_string_lossy()));
    let directory = exe.parent().map(|d| to_wide(&d.to_string_lossy()));
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: all strings are zero-terminated and outlive the call; the
    // command line buffer is writable as CreateProcessW requires.
    let ok = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NEW_PROCESS_GROUP,
            std::ptr::null(),
            directory.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()),
            &startup,
            &mut info,
        )
    };
    if ok == 0 {
        return false;
    }
    drop(owned(info.hThread));
    drop(owned(info.hProcess));
    true
}

/// The named-pipe transport.
pub struct PipeTransport {
    /// The pipe's name.
    pub pipe: String,
    /// What to start when nothing listens (`None`: fail at once).
    pub app: Option<PathBuf>,
    /// How long to wait for the pipe after starting the app (and for a
    /// busy pipe).
    pub connect_timeout: Duration,
    /// How long to wait for the answer.
    pub response_timeout: Duration,
}

impl PipeTransport {
    /// The transport for the current user and installation.
    pub fn from_environment() -> Option<Self> {
        let overridden = std::env::var(MAPI_PIPE_ENV).is_ok_and(|v| !v.trim().is_empty());
        Some(Self {
            pipe: pipe_name()?,
            app: app_to_start(overridden),
            connect_timeout: Duration::from_millis(MAPI_CONNECT_TIMEOUT_MS),
            response_timeout: Duration::from_millis(MAPI_RESPONSE_TIMEOUT_MS),
        })
    }

    fn open(&self, name: &[u16]) -> Result<OwnedHandle, u32> {
        // SAFETY: `name` is zero-terminated; no security attributes.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                std::ptr::null_mut(),
            )
        };
        owned(handle).ok_or_else(last_error)
    }

    /// Connects, starting the app once when nothing listens.
    pub fn connect(&self) -> Result<OwnedHandle, MapiResult> {
        let name = to_wide(&self.pipe);
        let mut launched = false;
        let mut deadline = Instant::now() + self.connect_timeout;
        loop {
            let error = match self.open(&name) {
                Ok(handle) => return Ok(handle),
                Err(error) => error,
            };
            let now = Instant::now();
            match error {
                ERROR_PIPE_BUSY => {
                    if now >= deadline {
                        return Err(MapiResult::Failure);
                    }
                    let wait = (deadline - now).as_millis().clamp(1, 1000) as u32;
                    // SAFETY: `name` is zero-terminated.
                    unsafe { WaitNamedPipeW(name.as_ptr(), wait) };
                }
                ERROR_FILE_NOT_FOUND => {
                    if !launched {
                        let app = self.app.as_deref().ok_or(MapiResult::Failure)?;
                        if !launch(app) {
                            return Err(MapiResult::Failure);
                        }
                        launched = true;
                        deadline = Instant::now() + self.connect_timeout;
                    } else if now >= deadline {
                        return Err(MapiResult::Failure);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                ERROR_ACCESS_DENIED => return Err(MapiResult::AccessDenied),
                _ => return Err(MapiResult::Failure),
            }
        }
    }
}

/// Checks that the pipe's server runs as `user`; returns its process id.
pub fn verify_server(pipe: &OwnedHandle, user: &Sid) -> Result<u32, MapiResult> {
    let mut pid = 0u32;
    // SAFETY: `pipe` is an open client end.
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) } == 0 {
        return Err(MapiResult::Failure);
    }
    // SAFETY: plain query; the handle is owned below.
    let process = owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) })
        .ok_or(MapiResult::AccessDenied)?;
    let server = process_user(process.as_raw_handle()).ok_or(MapiResult::AccessDenied)?;
    if server.equals(user) {
        Ok(pid)
    } else {
        Err(MapiResult::AccessDenied)
    }
}

/// One overlapped read or write with a deadline.
fn transfer(
    pipe: &OwnedHandle,
    buffer: &mut [u8],
    write: bool,
    deadline: Instant,
) -> Option<usize> {
    let handle = pipe.as_raw_handle();
    // SAFETY: an unnamed manual-reset event, owned below.
    let event = owned(unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) })?;
    let mut overlapped = OVERLAPPED {
        hEvent: event.as_raw_handle(),
        ..Default::default()
    };
    let len = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
    // SAFETY: `buffer` and `overlapped` stay alive and unmoved until the
    // operation completed or was cancelled and waited for (below).
    let ok = unsafe {
        if write {
            WriteFile(
                handle,
                buffer.as_ptr(),
                len,
                std::ptr::null_mut(),
                &mut overlapped,
            )
        } else {
            ReadFile(
                handle,
                buffer.as_mut_ptr(),
                len,
                std::ptr::null_mut(),
                &mut overlapped,
            )
        }
    };
    if ok == 0 {
        let error = last_error();
        if error != ERROR_IO_PENDING && error != ERROR_MORE_DATA {
            return None;
        }
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    let wait = remaining.as_millis().min(u128::from(u32::MAX - 1)) as u32;
    // SAFETY: waits on the event owned above.
    let signalled = unsafe { WaitForSingleObject(event.as_raw_handle(), wait) } == WAIT_OBJECT_0;
    let mut done = 0u32;
    if !signalled {
        // SAFETY: cancels this operation and waits until Windows no longer
        // touches `buffer` / `overlapped`.
        unsafe {
            CancelIoEx(handle, &overlapped);
            GetOverlappedResult(handle, &overlapped, &mut done, 1);
        }
        return None;
    }
    // SAFETY: the operation completed (event signalled).
    if unsafe { GetOverlappedResult(handle, &overlapped, &mut done, 0) } == 0
        && last_error() != ERROR_MORE_DATA
    {
        return None;
    }
    Some(done as usize)
}

fn write_all(pipe: &OwnedHandle, mut data: &[u8], deadline: Instant) -> Option<()> {
    while !data.is_empty() {
        // `transfer` takes `&mut` for both directions; copy per chunk.
        let mut chunk = data[..data.len().min(64 * 1024)].to_vec();
        let n = transfer(pipe, &mut chunk, true, deadline)?;
        if n == 0 {
            return None;
        }
        data = &data[n..];
    }
    Some(())
}

fn read_exact(pipe: &OwnedHandle, buffer: &mut [u8], deadline: Instant) -> Option<()> {
    let mut filled = 0;
    while filled < buffer.len() {
        let n = transfer(pipe, &mut buffer[filled..], false, deadline)?;
        if n == 0 {
            return None;
        }
        filled += n;
    }
    Some(())
}

/// Writes the request frame and reads the answer frame.
pub fn exchange(
    pipe: &OwnedHandle,
    request: &MapiPipeRequest,
    timeout: Duration,
) -> Result<MapiPipeResponse, MapiResult> {
    let deadline = Instant::now() + timeout;
    let frame = encode_frame(request).map_err(|_| MapiResult::TextTooLarge)?;
    write_all(pipe, &frame, deadline).ok_or(MapiResult::Failure)?;
    let mut header = [0u8; 4];
    read_exact(pipe, &mut header, deadline).ok_or(MapiResult::Failure)?;
    let len = frame_len(header).map_err(|_| MapiResult::Failure)?;
    let mut body = vec![0u8; len];
    read_exact(pipe, &mut body, deadline).ok_or(MapiResult::Failure)?;
    decode_frame(&body).map_err(|_| MapiResult::Failure)
}

impl Transport for PipeTransport {
    fn send(&self, request: &MapiPipeRequest) -> Result<MapiPipeResponse, MapiResult> {
        let user = current_user().ok_or(MapiResult::Failure)?;
        let pipe = self.connect()?;
        let server = verify_server(&pipe, &user)?;
        // The calling program usually owns the foreground; let Nail bring
        // its composer to the front.
        // SAFETY: plain call with a process id.
        unsafe { AllowSetForegroundWindow(server) };
        exchange(&pipe, request, self.response_timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::super::server::{FakeAppPipe, pipe_sddl, unique_pipe_name};
    use super::*;
    use nail_api::mapi::{MAPI_PROTOCOL_VERSION, MapiMessage, MapiUi};
    use pretty_assertions::assert_eq;

    fn request(subject: &str) -> MapiPipeRequest {
        MapiPipeRequest {
            version: MAPI_PROTOCOL_VERSION,
            message: MapiMessage {
                subject: Some(subject.to_owned()),
                body: None,
                recipients: Vec::new(),
                attachments: Vec::new(),
                ui: MapiUi::Dialog,
                program: None,
            },
        }
    }

    fn transport(pipe: &str, app: Option<PathBuf>, response_ms: u64) -> PipeTransport {
        PipeTransport {
            pipe: pipe.to_owned(),
            app,
            connect_timeout: Duration::from_secs(5),
            response_timeout: Duration::from_millis(response_ms),
        }
    }

    #[test]
    fn user_sid_and_pipe_name() {
        let user = current_user().unwrap();
        let sid = user.to_string_sid().unwrap();
        assert!(sid.starts_with("S-1-5-"), "{sid}");
        let parsed = Sid::from_string_sid(&sid).unwrap();
        assert!(parsed.equals(&user));
        assert!(!Sid::from_string_sid("S-1-5-18").unwrap().equals(&user));
        assert!(Sid::from_string_sid("not a sid").is_none());
        assert_eq!(pipe_sddl("S-1-5-21-1"), "D:P(A;;GA;;;S-1-5-21-1)");
        if std::env::var(MAPI_PIPE_ENV).is_err() {
            assert_eq!(pipe_name().unwrap(), mapi_pipe_name(&sid));
        }
        assert!(own_module_path().unwrap().is_file());
    }

    #[test]
    fn round_trip_over_the_pipe() {
        let name = unique_pipe_name("client");
        let server = FakeAppPipe::create(&name).unwrap();
        let thread = std::thread::spawn(move || {
            server.serve_one(|req| MapiPipeResponse {
                result: if req.message.subject.as_deref() == Some("hello") {
                    MapiResult::Success
                } else {
                    MapiResult::Failure
                },
            })
        });
        let response = transport(&name, None, 5000)
            .send(&request("hello"))
            .unwrap();
        assert_eq!(response.result, MapiResult::Success);
        let seen = thread.join().unwrap().unwrap();
        assert_eq!(seen.message.subject.as_deref(), Some("hello"));
        // First instance only: a second server is refused while one exists.
        let first = FakeAppPipe::create(&name).unwrap();
        assert!(FakeAppPipe::create(&name).is_err());
        drop(first);
    }

    #[test]
    fn servers_of_other_users_are_refused() {
        let name = unique_pipe_name("squat");
        let server = FakeAppPipe::create(&name).unwrap();
        let thread = std::thread::spawn(move || {
            server.serve_one(|_| MapiPipeResponse {
                result: MapiResult::Success,
            })
        });
        let t = transport(&name, None, 2000);
        let pipe = t.connect().unwrap();
        let me = current_user().unwrap();
        assert!(verify_server(&pipe, &me).is_ok());
        let system = Sid::from_string_sid("S-1-5-18").unwrap();
        assert_eq!(verify_server(&pipe, &system), Err(MapiResult::AccessDenied));
        drop(pipe);
        // The server sees a client that went away without a request.
        assert!(thread.join().unwrap().is_err());
    }

    #[test]
    fn nothing_listening_without_an_app_fails_at_once() {
        let started = Instant::now();
        let result = transport(&unique_pipe_name("none"), None, 1000).send(&request("x"));
        assert_eq!(result.unwrap_err(), MapiResult::Failure);
        assert!(started.elapsed() < Duration::from_secs(2));
        // An app that cannot be started.
        let missing = PathBuf::from(r"C:\definitely\not\here\Nail.exe");
        let result = transport(&unique_pipe_name("none"), Some(missing), 1000).send(&request("x"));
        assert_eq!(result.unwrap_err(), MapiResult::Failure);
    }

    #[test]
    fn a_hung_app_times_out() {
        let name = unique_pipe_name("hung");
        let server = FakeAppPipe::create(&name).unwrap();
        let thread = std::thread::spawn(move || {
            server.serve_one(|_| {
                std::thread::sleep(Duration::from_millis(1500));
                MapiPipeResponse {
                    result: MapiResult::Success,
                }
            })
        });
        let started = Instant::now();
        let result = transport(&name, None, 300).send(&request("x"));
        assert_eq!(result.unwrap_err(), MapiResult::Failure);
        assert!(started.elapsed() < Duration::from_millis(1400));
        let _ = thread.join();
    }

    #[test]
    fn a_garbage_answer_is_a_failure() {
        use std::io::{Read, Write};
        let name = unique_pipe_name("garbage");
        let server = FakeAppPipe::create(&name).unwrap();
        let thread = std::thread::spawn(move || {
            let mut file = server.into_file_after_connect()?;
            let mut header = [0u8; 4];
            file.read_exact(&mut header)?;
            let mut body = vec![0u8; u32::from_le_bytes(header) as usize];
            file.read_exact(&mut body)?;
            file.write_all(&[3, 0, 0, 0, b'n', b'o', b'!'])?;
            file.sync_all()
        });
        let pipe = transport(&name, None, 2000).connect().unwrap();
        assert_eq!(
            exchange(&pipe, &request("x"), Duration::from_secs(2)),
            Err(MapiResult::Failure)
        );
        thread.join().unwrap().unwrap();
    }
}
