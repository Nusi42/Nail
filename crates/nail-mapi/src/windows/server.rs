//! A stand-in for the app's end of the pipe, for this crate's tests and the
//! harness ("fake Nail.exe"). It creates the pipe the way the shell must
//! (WP113): first instance only, remote clients rejected, a protected DACL
//! granting the current user alone (`D:P(A;;GA;;;<SID>)`).

use std::fs::File;
use std::io;
use std::os::windows::io::{FromRawHandle, OwnedHandle};

use nail_api::mapi::{MapiPipeRequest, MapiPipeResponse, read_frame, write_frame};
use windows_sys::Win32::Foundation::{
    ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_WAIT,
};

use super::client::current_user;
use super::text::to_wide;

/// A created, not yet connected pipe instance.
#[derive(Debug)]
pub struct FakeAppPipe {
    handle: OwnedHandle,
}

/// The SDDL of the pipe's security descriptor for a user SID.
pub fn pipe_sddl(user_sid: &str) -> String {
    format!("D:P(A;;GA;;;{user_sid})")
}

impl FakeAppPipe {
    /// Creates the pipe `name` (fails when it exists already).
    pub fn create(name: &str) -> io::Result<Self> {
        let sid = current_user()
            .and_then(|s| s.to_string_sid())
            .ok_or_else(|| io::Error::other("no user SID"))?;
        let sddl = to_wide(&pipe_sddl(&sid));
        let mut descriptor = std::ptr::null_mut();
        // SAFETY: `sddl` is zero-terminated; Windows allocates `descriptor`.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &raw mut descriptor,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: super::size_u32::<SECURITY_ATTRIBUTES>(),
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let wide = to_wide(name);
        // SAFETY: all pointers are valid for the call.
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                64 * 1024,
                64 * 1024,
                0,
                &raw const attributes,
            )
        };
        let error = io::Error::last_os_error();
        // SAFETY: frees the descriptor Windows allocated above.
        unsafe { LocalFree(descriptor) };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return Err(error);
        }
        // SAFETY: a fresh handle owned from here on.
        Ok(Self {
            handle: unsafe { OwnedHandle::from_raw_handle(handle) },
        })
    }

    /// Waits for a client and returns the connected pipe as a file (tests
    /// that answer by hand).
    pub fn into_file_after_connect(self) -> io::Result<File> {
        use std::os::windows::io::AsRawHandle;
        // SAFETY: blocking connect on the synchronous pipe handle.
        let ok = unsafe { ConnectNamedPipe(self.handle.as_raw_handle(), std::ptr::null_mut()) };
        // SAFETY: reads the thread's last error.
        if ok == 0 && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
            return Err(io::Error::last_os_error());
        }
        Ok(File::from(self.handle))
    }

    /// Waits for one client, reads its request, answers with `answer` and
    /// closes. Returns the request.
    pub fn serve_one(
        self,
        answer: impl FnOnce(&MapiPipeRequest) -> MapiPipeResponse,
    ) -> io::Result<MapiPipeRequest> {
        let mut file = self.into_file_after_connect()?;
        let request: MapiPipeRequest =
            read_frame(&mut file).map_err(|e| io::Error::other(e.to_string()))?;
        let response = answer(&request);
        write_frame(&mut file, &response).map_err(|e| io::Error::other(e.to_string()))?;
        // FlushFileBuffers: returns once the client has read the answer.
        file.sync_all()?;
        Ok(request)
    }
}

/// A pipe name no other test uses.
pub fn unique_pipe_name(tag: &str) -> String {
    format!(
        r"\\.\pipe\nail-mapi-test-{tag}-{}",
        crate::staging::unique_suffix()
    )
}
