//! The calling program's side, for the harness and the integration tests:
//! builds `MapiMessage(W)` structures from Rust values and calls a loaded
//! NailMapi.dll's exports the way Explorer or Office do.

use std::ffi::c_void;
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Mapi::{
    LPMAPIFREEBUFFER, LPMAPILOGOFF, LPMAPILOGON, LPMAPIRESOLVENAME, LPMAPISENDDOCUMENTS,
    LPMAPISENDMAIL, LPMAPISENDMAILW, MapiFileDesc, MapiFileDescW, MapiMessage, MapiMessageW,
    MapiRecipDesc, MapiRecipDescW,
};

use super::text::{to_ansi, to_wide};

/// One recipient: `(ulRecipClass, name, address)`.
pub type CallerRecipient = (u32, Option<String>, Option<String>);

/// A message as a program would pass it.
#[derive(Debug, Clone, Default)]
pub struct CallerMessage {
    /// `lpszSubject`.
    pub subject: Option<String>,
    /// `lpszNoteText`.
    pub body: Option<String>,
    /// `lpRecips`.
    pub recipients: Vec<CallerRecipient>,
    /// `lpFiles`: `(lpszPathName, lpszFileName)`.
    pub files: Vec<(String, Option<String>)>,
}

fn opt<T>(buffer: Option<&Vec<T>>) -> *mut T {
    buffer.map_or(null_mut(), |b| b.as_ptr().cast_mut())
}

impl CallerMessage {
    /// Calls `f` with a `MapiMessageW` pointing at UTF-16 copies.
    pub fn with_wide<R>(&self, f: impl FnOnce(*const MapiMessageW) -> R) -> R {
        let w = |s: &Option<String>| s.as_deref().map(to_wide);
        let subject = w(&self.subject);
        let body = w(&self.body);
        let names: Vec<_> = self.recipients.iter().map(|r| (w(&r.1), w(&r.2))).collect();
        let mut recips: Vec<MapiRecipDescW> = self
            .recipients
            .iter()
            .zip(&names)
            .map(|(r, (name, address))| MapiRecipDescW {
                ulRecipClass: r.0,
                lpszName: opt(name.as_ref()),
                lpszAddress: opt(address.as_ref()),
                ..Default::default()
            })
            .collect();
        let paths: Vec<_> = self.files.iter().map(|(p, n)| (to_wide(p), w(n))).collect();
        let mut files: Vec<MapiFileDescW> = paths
            .iter()
            .map(|(path, name)| MapiFileDescW {
                nPosition: u32::MAX,
                lpszPathName: path.as_ptr().cast_mut(),
                lpszFileName: opt(name.as_ref()),
                ..Default::default()
            })
            .collect();
        let message = MapiMessageW {
            lpszSubject: opt(subject.as_ref()),
            lpszNoteText: opt(body.as_ref()),
            nRecipCount: super::count_u32(recips.len()),
            lpRecips: if recips.is_empty() {
                null_mut()
            } else {
                recips.as_mut_ptr()
            },
            nFileCount: super::count_u32(files.len()),
            lpFiles: if files.is_empty() {
                null_mut()
            } else {
                files.as_mut_ptr()
            },
            ..Default::default()
        };
        f(&raw const message)
    }

    /// Calls `f` with an ANSI `MapiMessage`.
    pub fn with_ansi<R>(&self, f: impl FnOnce(*mut MapiMessage) -> R) -> R {
        let a = |s: &Option<String>| s.as_deref().map(to_ansi);
        let subject = a(&self.subject);
        let body = a(&self.body);
        let names: Vec<_> = self.recipients.iter().map(|r| (a(&r.1), a(&r.2))).collect();
        let mut recips: Vec<MapiRecipDesc> = self
            .recipients
            .iter()
            .zip(&names)
            .map(|(r, (name, address))| MapiRecipDesc {
                ulRecipClass: r.0,
                lpszName: opt(name.as_ref()),
                lpszAddress: opt(address.as_ref()),
                ..Default::default()
            })
            .collect();
        let paths: Vec<_> = self.files.iter().map(|(p, n)| (to_ansi(p), a(n))).collect();
        let mut files: Vec<MapiFileDesc> = paths
            .iter()
            .map(|(path, name)| MapiFileDesc {
                nPosition: u32::MAX,
                lpszPathName: path.as_ptr().cast_mut(),
                lpszFileName: opt(name.as_ref()),
                ..Default::default()
            })
            .collect();
        let mut message = MapiMessage {
            lpszSubject: opt(subject.as_ref()),
            lpszNoteText: opt(body.as_ref()),
            nRecipCount: super::count_u32(recips.len()),
            lpRecips: if recips.is_empty() {
                null_mut()
            } else {
                recips.as_mut_ptr()
            },
            nFileCount: super::count_u32(files.len()),
            lpFiles: if files.is_empty() {
                null_mut()
            } else {
                files.as_mut_ptr()
            },
            ..Default::default()
        };
        f(&raw mut message)
    }
}

/// A loaded NailMapi.dll.
#[derive(Debug)]
pub struct MapiDll {
    module: HMODULE,
}

// SAFETY: a module handle may be used from any thread.
unsafe impl Send for MapiDll {}

impl MapiDll {
    /// `LoadLibraryW(path)`.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let wide = to_wide(&path.to_string_lossy());
        // SAFETY: zero-terminated path.
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self { module })
    }

    /// `GetProcAddress(name)` as an untyped pointer (`None` when missing).
    pub fn symbol(&self, name: &str) -> Option<*const c_void> {
        let mut c = name.as_bytes().to_vec();
        c.push(0);
        // SAFETY: zero-terminated name, loaded module.
        let proc = unsafe { GetProcAddress(self.module, c.as_ptr()) }?;
        Some(proc as *const c_void)
    }

    fn typed<T: Copy>(&self, name: &str) -> Option<T> {
        let ptr = self.symbol(name)?;
        assert_eq!(size_of::<T>(), size_of::<*const c_void>());
        // SAFETY: `T` is the export's `Option<unsafe extern "system" fn>`
        // type from windows-sys (pointer-sized, non-null here).
        Some(unsafe { std::mem::transmute_copy::<*const c_void, T>(&ptr) })
    }

    /// `MAPISendMailW(0, 0, message, flags, 0)`.
    pub fn send_mail_w(&self, message: &CallerMessage, flags: u32) -> Option<u32> {
        let f = self.typed::<LPMAPISENDMAILW>("MAPISendMailW")??;
        // SAFETY: a valid message structure built by `with_wide`.
        Some(message.with_wide(|m| unsafe { f(0, 0, m, flags, 0) }))
    }

    /// `MAPISendMailW` with an arbitrary (possibly bad) message pointer.
    ///
    /// # Safety
    /// None for the caller beyond what the DLL promises: it must refuse bad
    /// pointers instead of crashing. Used by robustness tests.
    pub unsafe fn send_mail_w_raw(&self, message: *const MapiMessageW, flags: u32) -> Option<u32> {
        let f = self.typed::<LPMAPISENDMAILW>("MAPISendMailW")??;
        // SAFETY: see above; the DLL validates every pointer.
        Some(unsafe { f(0, 0, message, flags, 0) })
    }

    /// `MAPISendMail(0, 0, message, flags, 0)` (ANSI).
    pub fn send_mail(&self, message: &CallerMessage, flags: u32) -> Option<u32> {
        let f = self.typed::<LPMAPISENDMAIL>("MAPISendMail")??;
        // SAFETY: a valid message structure built by `with_ansi`.
        Some(message.with_ansi(|m| unsafe { f(0, 0, m, flags, 0) }))
    }

    /// `MAPISendDocuments(0, delimiter, paths, names, 0)`.
    pub fn send_documents(&self, delimiter: &str, paths: &str, names: Option<&str>) -> Option<u32> {
        let f = self.typed::<LPMAPISENDDOCUMENTS>("MAPISendDocuments")??;
        let delimiter = to_ansi(delimiter);
        let paths = to_ansi(paths);
        let names = names.map(to_ansi);
        let names_ptr = names.as_ref().map_or(std::ptr::null(), Vec::as_ptr);
        // SAFETY: zero-terminated strings alive for the call.
        Some(unsafe { f(0, delimiter.as_ptr(), paths.as_ptr(), names_ptr, 0) })
    }

    /// `MAPILogon` → (code, session).
    pub fn logon(&self) -> Option<(u32, usize)> {
        let f = self.typed::<LPMAPILOGON>("MAPILogon")??;
        let mut session = 0usize;
        // SAFETY: valid out pointer.
        let code = unsafe {
            f(
                0,
                std::ptr::null(),
                std::ptr::null(),
                0,
                0,
                &raw mut session,
            )
        };
        Some((code, session))
    }

    /// `MAPILogoff(session)`.
    pub fn logoff(&self, session: usize) -> Option<u32> {
        let f = self.typed::<LPMAPILOGOFF>("MAPILogoff")??;
        // SAFETY: plain values.
        Some(unsafe { f(session, 0, 0, 0) })
    }

    /// `MAPIResolveName(name)` → (code, name, address); frees the result
    /// with `MAPIFreeBuffer` (whose code is the last element).
    pub fn resolve_name(&self, name: &str) -> Option<(u32, Option<String>, Option<String>, u32)> {
        let resolve = self.typed::<LPMAPIRESOLVENAME>("MAPIResolveName")??;
        let free = self.typed::<LPMAPIFREEBUFFER>("MAPIFreeBuffer")??;
        let name = to_ansi(name);
        let mut out: *mut MapiRecipDesc = null_mut();
        // SAFETY: valid strings and out pointer.
        let code = unsafe { resolve(0, 0, name.as_ptr(), 0, 0, &raw mut out) };
        if out.is_null() {
            return Some((code, None, None, 0));
        }
        let read = |p: *const u8| {
            if p.is_null() {
                return None;
            }
            let mut len = 0;
            // SAFETY: zero-terminated strings owned by the DLL's buffer.
            while unsafe { *p.add(len) } != 0 {
                len += 1;
            }
            // SAFETY: `len` bytes were just scanned.
            Some(super::text::ansi_to_string(unsafe {
                std::slice::from_raw_parts(p, len)
            }))
        };
        // SAFETY: the DLL returned a MapiRecipDesc.
        let recip = unsafe { *out };
        let result = (code, read(recip.lpszName), read(recip.lpszAddress));
        // SAFETY: frees the DLL's buffer.
        let freed = unsafe { free(out.cast()) };
        Some((result.0, result.1, result.2, freed))
    }

    /// `MAPIFreeBuffer(pointer)`.
    ///
    /// # Safety
    /// `pointer` must not be a live buffer of this DLL that the caller
    /// still uses; any other value (foreign, dangling, null) is allowed —
    /// the DLL refuses pointers it did not hand out.
    pub unsafe fn free_buffer(&self, pointer: *mut c_void) -> Option<u32> {
        let free = self.typed::<LPMAPIFREEBUFFER>("MAPIFreeBuffer")??;
        // SAFETY: the DLL must refuse pointers it did not allocate.
        Some(unsafe { free(pointer) })
    }
}

impl Drop for MapiDll {
    fn drop(&mut self) {
        // SAFETY: releases the reference taken in `load`.
        unsafe { FreeLibrary(self.module) };
    }
}
