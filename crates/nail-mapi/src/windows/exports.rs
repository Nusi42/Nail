//! The Simple MAPI functions NailMapi.dll exports.
//!
//! Signatures are those of `MAPI.h`; the compile-time checks at the end
//! coerce every export to windows-sys' `LPMAPI*` function-pointer type, so a
//! mismatch does not build. Every export runs inside [`guard`]: a panic
//! never crosses into the calling program (it becomes `MAPI_E_FAILURE`).
//!
//! | Function | Behaviour |
//! | --- | --- |
//! | `MAPISendMail`, `MAPISendMailW` | decode, forward to Nail, return its answer |
//! | `MAPISendDocuments` | the files as a message without recipients (UI) |
//! | `MAPILogon` / `MAPILogoff` | succeed (no profiles, no passwords); session handle 1 |
//! | `MAPIResolveName` | an address resolves to itself, anything else is `MAPI_E_UNKNOWN_RECIPIENT` |
//! | `MAPIFreeBuffer` | frees what `MAPIResolveName` returned; refuses other pointers |
//! | `MAPIFindNext`, `MAPIReadMail`, `MAPISaveMail`, `MAPIDeleteMail`, `MAPIAddress`, `MAPIDetails` | `MAPI_E_NOT_SUPPORTED` |

#![allow(non_snake_case)]

use std::collections::HashMap;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;

use nail_api::mapi::{MapiMessage, MapiRecipient, MapiRecipientClass, MapiResult, MapiUi};
use windows_sys::Win32::System::Mapi::{
    MAPI_E_FAILURE, MAPI_E_NOT_SUPPORTED, MAPI_E_UNKNOWN_RECIPIENT, MAPI_TO,
    MapiMessage as RawMessage, MapiMessageW as RawMessageW, MapiRecipDesc, SUCCESS_SUCCESS,
};

use super::client::PipeTransport;
use super::decode::{Abi, Ansi, MAX_FIELD_UNITS, Wide, decode_message, split_documents, text};
use super::memory::{read_units, write_struct};
use super::text::{ansi_to_string, to_ansi};
use crate::forward::{Transport, forward};
use crate::staging::default_staging_root;

/// The session handle `MAPILogon` hands out (any non-zero value works;
/// Nail keeps no session state).
pub const SESSION_HANDLE: usize = 1;

/// Runs an export body; a panic becomes `MAPI_E_FAILURE`.
pub fn guard(body: impl FnOnce() -> u32) -> u32 {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(MAPI_E_FAILURE)
}

/// The calling program's executable name (`WINWORD.EXE`).
fn program() -> Option<String> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
}

/// Forwards over the pipe of the current user.
pub fn send(message: MapiMessage) -> u32 {
    let Some(transport) = PipeTransport::from_environment() else {
        return MapiResult::Failure.code();
    };
    send_with(message, &transport)
}

/// Forwards over `transport` (tests).
pub fn send_with(message: MapiMessage, transport: &dyn Transport) -> u32 {
    forward(message, transport, &default_staging_root()).code()
}

fn send_mail<A: Abi>(message: *const A::Message, flags: u32) -> u32 {
    match decode_message::<A>(message, flags, program()) {
        Ok(message) => send(message),
        Err(result) => result.code(),
    }
}

/// `MAPISendMail`: sends (opens in Nail's composer) an ANSI message.
///
/// # Safety
/// Callable with any arguments: `lpmessage` is validated before use.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MAPISendMail(
    _lhsession: usize,
    _uluiparam: usize,
    lpmessage: *mut RawMessage,
    flflags: u32,
    _ulreserved: u32,
) -> u32 {
    guard(|| send_mail::<Ansi>(lpmessage.cast_const(), flflags))
}

/// `MAPISendMailW`: the UTF-16 variant (Windows 8 and later prefer it).
///
/// # Safety
/// Callable with any arguments: `lpmessage` is validated before use.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MAPISendMailW(
    _lhsession: usize,
    _uluiparam: usize,
    lpmessage: *const RawMessageW,
    flflags: u32,
    _ulreserved: u32,
) -> u32 {
    guard(|| send_mail::<Wide>(lpmessage, flflags))
}

/// `MAPISendDocuments`: attaches the `lpszdelimchar`-separated files of
/// `lpszfilepaths` (named by `lpszfilenames`) to a new message with UI.
///
/// # Safety
/// Callable with any arguments: every pointer is validated before use.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MAPISendDocuments(
    _uluiparam: usize,
    lpszdelimchar: *const u8,
    lpszfilepaths: *const u8,
    lpszfilenames: *const u8,
    _ulreserved: u32,
) -> u32 {
    guard(|| {
        let read = |ptr| {
            text::<Ansi>(
                ptr,
                MAX_FIELD_UNITS * 4,
                MapiResult::AttachmentNotFound,
                MapiResult::AttachmentNotFound,
            )
        };
        let delimiter = match read(lpszdelimchar) {
            Ok(Some(d)) => d.chars().next().unwrap_or(';'),
            Ok(None) => ';',
            Err(result) => return result.code(),
        };
        let paths = match read(lpszfilepaths) {
            Ok(Some(paths)) => paths,
            Ok(None) => return MapiResult::AttachmentNotFound.code(),
            Err(result) => return result.code(),
        };
        let names = match read(lpszfilenames) {
            Ok(names) => names,
            Err(result) => return result.code(),
        };
        let attachments = split_documents(&paths, names.as_deref(), delimiter);
        if attachments.is_empty() {
            return MapiResult::AttachmentNotFound.code();
        }
        send(MapiMessage {
            subject: None,
            body: None,
            recipients: Vec::new(),
            attachments,
            ui: MapiUi::Dialog,
            program: program(),
        })
    })
}

/// `MAPILogon`: always succeeds with [`SESSION_HANDLE`].
///
/// # Safety
/// Callable with any arguments: `lplhsession` is written only when it is
/// writable memory.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MAPILogon(
    _uluiparam: usize,
    _lpszprofilename: *const u8,
    _lpszpassword: *const u8,
    _flflags: u32,
    _ulreserved: u32,
    lplhsession: *mut usize,
) -> u32 {
    guard(|| {
        if !lplhsession.is_null() && !write_struct(lplhsession, SESSION_HANDLE) {
            return MAPI_E_FAILURE;
        }
        SUCCESS_SUCCESS
    })
}

/// `MAPILogoff`: always succeeds.
///
/// # Safety
/// Takes no pointers.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MAPILogoff(
    _lhsession: usize,
    _uluiparam: usize,
    _flflags: u32,
    _ulreserved: u32,
) -> u32 {
    SUCCESS_SUCCESS
}

/// A `MapiRecipDesc` handed out by `MAPIResolveName` with the strings it
/// points to; freed by `MAPIFreeBuffer`.
#[repr(C)]
struct ResolvedRecipient {
    // First field: the pointer handed out is the structure's address.
    desc: MapiRecipDesc,
    _name: Vec<u8>,
    _address: Vec<u8>,
}

// SAFETY: the raw pointers in `desc` point into the structure's own
// vectors (heap buffers that move with it); nothing is shared.
unsafe impl Send for ResolvedRecipient {}

/// Addresses of the buffers this DLL handed out (as integers: raw
/// pointers are not `Send`).
static ALLOCATIONS: Mutex<Option<HashMap<usize, Box<ResolvedRecipient>>>> = Mutex::new(None);

fn allocations<R>(f: impl FnOnce(&mut HashMap<usize, Box<ResolvedRecipient>>) -> R) -> R {
    let mut guard = ALLOCATIONS.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

/// Resolves a name the way Nail can without an address book: an e-mail
/// address (optionally `SMTP:`-prefixed or in `<>`) resolves to itself.
pub fn resolve(name: &str) -> Option<(String, String)> {
    let recipient = MapiRecipient {
        class: MapiRecipientClass::To,
        name: Some(name.to_owned()),
        address: None,
    };
    let email = recipient.email()?;
    Some((name.trim().to_owned(), format!("SMTP:{email}")))
}

/// `MAPIResolveName`: see [`resolve`].
///
/// # Safety
/// Callable with any arguments: every pointer is validated before use.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MAPIResolveName(
    _lhsession: usize,
    _uluiparam: usize,
    lpszname: *const u8,
    _flflags: u32,
    _ulreserved: u32,
    lpprecip: *mut *mut MapiRecipDesc,
) -> u32 {
    guard(|| {
        let Ok(Some(units)) = read_units(lpszname, MAX_FIELD_UNITS) else {
            return MAPI_E_UNKNOWN_RECIPIENT;
        };
        let Some((name, address)) = resolve(&ansi_to_string(&units)) else {
            return MAPI_E_UNKNOWN_RECIPIENT;
        };
        let name = to_ansi(&name);
        let address = to_ansi(&address);
        let mut boxed = Box::new(ResolvedRecipient {
            desc: MapiRecipDesc {
                ulRecipClass: MAPI_TO,
                lpszName: name.as_ptr().cast_mut(),
                lpszAddress: address.as_ptr().cast_mut(),
                ..Default::default()
            },
            _name: name,
            _address: address,
        });
        let pointer: *mut MapiRecipDesc = &mut boxed.desc;
        if lpprecip.is_null() || !write_struct(lpprecip, pointer) {
            return MAPI_E_FAILURE;
        }
        allocations(|map| map.insert(pointer as usize, boxed));
        SUCCESS_SUCCESS
    })
}

/// `MAPIFreeBuffer`: frees a buffer from `MAPIResolveName`; null is fine,
/// any other pointer is refused (`MAPI_E_FAILURE`) and left alone.
///
/// # Safety
/// Callable with any pointer: it is only compared, never dereferenced.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MAPIFreeBuffer(pv: *mut c_void) -> u32 {
    guard(|| {
        if pv.is_null() {
            return SUCCESS_SUCCESS;
        }
        match allocations(|map| map.remove(&(pv as usize))) {
            Some(_) => SUCCESS_SUCCESS,
            None => MAPI_E_FAILURE,
        }
    })
}

/// `MAPIFindNext`: Nail does not expose its mail store over Simple MAPI.
///
/// # Safety
/// Touches none of its arguments.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MAPIFindNext(
    _lhsession: usize,
    _uluiparam: usize,
    _lpszmessagetype: *const u8,
    _lpszseedmessageid: *const u8,
    _flflags: u32,
    _ulreserved: u32,
    _lpszmessageid: *const u8,
) -> u32 {
    MAPI_E_NOT_SUPPORTED
}

/// `MAPIReadMail`: not supported.
///
/// # Safety
/// Touches none of its arguments.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MAPIReadMail(
    _lhsession: usize,
    _uluiparam: usize,
    _lpszmessageid: *const u8,
    _flflags: u32,
    _ulreserved: u32,
    _lppmessage: *mut *mut RawMessage,
) -> u32 {
    MAPI_E_NOT_SUPPORTED
}

/// `MAPISaveMail`: not supported.
///
/// # Safety
/// Touches none of its arguments.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MAPISaveMail(
    _lhsession: usize,
    _uluiparam: usize,
    _lpmessage: *mut RawMessage,
    _flflags: u32,
    _ulreserved: u32,
    _lpszmessageid: *const u8,
) -> u32 {
    MAPI_E_NOT_SUPPORTED
}

/// `MAPIDeleteMail`: not supported.
///
/// # Safety
/// Touches none of its arguments.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MAPIDeleteMail(
    _lhsession: usize,
    _uluiparam: usize,
    _lpszmessageid: *const u8,
    _flflags: u32,
    _ulreserved: u32,
) -> u32 {
    MAPI_E_NOT_SUPPORTED
}

/// `MAPIAddress`: no address-book dialog (Nail's composer has its own).
///
/// # Safety
/// Touches none of its arguments.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MAPIAddress(
    _lhsession: usize,
    _uluiparam: usize,
    _lpszcaption: *const u8,
    _neditfields: u32,
    _lpszlabels: *const u8,
    _nrecips: u32,
    _lprecips: *mut MapiRecipDesc,
    _flflags: u32,
    _ulreserved: u32,
    _lpnnewrecips: *mut u32,
    _lppnewrecips: *mut *mut MapiRecipDesc,
) -> u32 {
    MAPI_E_NOT_SUPPORTED
}

/// `MAPIDetails`: not supported.
///
/// # Safety
/// Touches none of its arguments.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MAPIDetails(
    _lhsession: usize,
    _uluiparam: usize,
    _lprecip: *mut MapiRecipDesc,
    _flflags: u32,
    _ulreserved: u32,
) -> u32 {
    MAPI_E_NOT_SUPPORTED
}

// The exact `MAPI.h` signatures: these coercions fail to build on any
// difference in parameter types, count or calling convention.
mod signatures {
    use super::*;
    use windows_sys::Win32::System::Mapi::{
        LPMAPIADDRESS, LPMAPIDELETEMAIL, LPMAPIDETAILS, LPMAPIFINDNEXT, LPMAPIFREEBUFFER,
        LPMAPILOGOFF, LPMAPILOGON, LPMAPIREADMAIL, LPMAPIRESOLVENAME, LPMAPISAVEMAIL,
        LPMAPISENDDOCUMENTS, LPMAPISENDMAIL, LPMAPISENDMAILW,
    };
    const _: LPMAPISENDMAIL = Some(MAPISendMail);
    const _: LPMAPISENDMAILW = Some(MAPISendMailW);
    const _: LPMAPISENDDOCUMENTS = Some(MAPISendDocuments);
    const _: LPMAPILOGON = Some(MAPILogon);
    const _: LPMAPILOGOFF = Some(MAPILogoff);
    const _: LPMAPIFREEBUFFER = Some(MAPIFreeBuffer);
    const _: LPMAPIRESOLVENAME = Some(MAPIResolveName);
    const _: LPMAPIFINDNEXT = Some(MAPIFindNext);
    const _: LPMAPIREADMAIL = Some(MAPIReadMail);
    const _: LPMAPISAVEMAIL = Some(MAPISaveMail);
    const _: LPMAPIDELETEMAIL = Some(MAPIDeleteMail);
    const _: LPMAPIADDRESS = Some(MAPIAddress);
    const _: LPMAPIDETAILS = Some(MAPIDetails);
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::mapi::{MapiPipeRequest, MapiPipeResponse};
    use pretty_assertions::assert_eq;
    use std::ptr::{null, null_mut};

    #[test]
    fn panics_do_not_cross_the_boundary() {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let code = guard(|| panic!("boom"));
        std::panic::set_hook(previous);
        assert_eq!(code, MAPI_E_FAILURE);
        assert_eq!(guard(|| 7), 7);
    }

    #[test]
    fn logon_logoff() {
        let mut session = 0usize;
        // SAFETY: valid arguments.
        assert_eq!(
            unsafe { MAPILogon(0, null(), null(), 0, 0, &mut session) },
            0
        );
        assert_eq!(session, SESSION_HANDLE);
        // SAFETY: null session pointer is allowed.
        assert_eq!(unsafe { MAPILogon(0, null(), null(), 0, 0, null_mut()) }, 0);
        // SAFETY: a garbage pointer is refused, not written.
        assert_eq!(
            unsafe { MAPILogon(0, null(), null(), 0, 0, 0x10 as *mut usize) },
            MAPI_E_FAILURE
        );
        // SAFETY: no pointers.
        assert_eq!(unsafe { MAPILogoff(SESSION_HANDLE, 0, 0, 0) }, 0);
    }

    #[test]
    fn resolve_name_and_free_buffer() {
        assert_eq!(
            resolve(" SMTP:ada@x.example "),
            Some(("SMTP:ada@x.example".into(), "SMTP:ada@x.example".into()))
        );
        assert_eq!(resolve("Ada Lovelace"), None);
        let name = to_ansi("ada@x.example");
        let mut out: *mut MapiRecipDesc = null_mut();
        // SAFETY: valid arguments.
        let code = unsafe { MAPIResolveName(0, 0, name.as_ptr(), 0, 0, &mut out) };
        assert_eq!(code, SUCCESS_SUCCESS);
        assert!(!out.is_null());
        // SAFETY: the DLL's buffer.
        let desc = unsafe { *out };
        assert_eq!(desc.ulRecipClass, MAPI_TO);
        let address = read_units(desc.lpszAddress.cast_const(), 100)
            .unwrap()
            .unwrap();
        assert_eq!(address, b"SMTP:ada@x.example");
        // SAFETY: frees once; a second free is refused.
        assert_eq!(unsafe { MAPIFreeBuffer(out.cast()) }, SUCCESS_SUCCESS);
        assert_eq!(unsafe { MAPIFreeBuffer(out.cast()) }, MAPI_E_FAILURE);
        // SAFETY: null and foreign pointers.
        assert_eq!(unsafe { MAPIFreeBuffer(null_mut()) }, SUCCESS_SUCCESS);
        let mut foreign = 0u32;
        assert_eq!(
            unsafe { MAPIFreeBuffer((&raw mut foreign).cast()) },
            MAPI_E_FAILURE
        );
        let unknown = to_ansi("Nobody");
        // SAFETY: valid arguments; bad name / out pointers.
        unsafe {
            assert_eq!(
                MAPIResolveName(0, 0, unknown.as_ptr(), 0, 0, &mut out),
                MAPI_E_UNKNOWN_RECIPIENT
            );
            assert_eq!(
                MAPIResolveName(0, 0, 0x10 as *const u8, 0, 0, &mut out),
                MAPI_E_UNKNOWN_RECIPIENT
            );
            assert_eq!(
                MAPIResolveName(0, 0, name.as_ptr(), 0, 0, null_mut()),
                MAPI_E_FAILURE
            );
            assert_eq!(
                MAPIResolveName(0, 0, name.as_ptr(), 0, 0, 0x10 as *mut *mut MapiRecipDesc),
                MAPI_E_FAILURE
            );
        }
        assert!(allocations(|map| map.is_empty()));
    }

    #[test]
    fn unsupported_functions() {
        // SAFETY: none of these touch their arguments.
        unsafe {
            assert_eq!(
                MAPIFindNext(0, 0, null(), null(), 0, 0, null()),
                MAPI_E_NOT_SUPPORTED
            );
            assert_eq!(
                MAPIReadMail(0, 0, null(), 0, 0, null_mut()),
                MAPI_E_NOT_SUPPORTED
            );
            assert_eq!(
                MAPISaveMail(0, 0, null_mut(), 0, 0, null()),
                MAPI_E_NOT_SUPPORTED
            );
            assert_eq!(MAPIDeleteMail(0, 0, null(), 0, 0), MAPI_E_NOT_SUPPORTED);
            assert_eq!(
                MAPIAddress(
                    0,
                    0,
                    null(),
                    0,
                    null(),
                    0,
                    null_mut(),
                    0,
                    0,
                    null_mut(),
                    null_mut()
                ),
                MAPI_E_NOT_SUPPORTED
            );
            assert_eq!(MAPIDetails(0, 0, null_mut(), 0, 0), MAPI_E_NOT_SUPPORTED);
        }
    }

    #[test]
    fn bad_messages_never_reach_the_app() {
        // SAFETY: the export validates its pointer.
        unsafe {
            assert_eq!(
                MAPISendMailW(0, 0, null(), 0, 0),
                MapiResult::Failure.code()
            );
            assert_eq!(
                MAPISendMailW(0, 0, 0x100 as *const RawMessageW, 0, 0),
                MapiResult::Failure.code()
            );
            assert_eq!(
                MAPISendMail(0, 0, 0x100 as *mut RawMessage, 0, 0),
                MapiResult::Failure.code()
            );
            assert_eq!(
                MAPISendDocuments(0, null(), null(), null(), 0),
                MapiResult::AttachmentNotFound.code()
            );
            let empty = to_ansi(";;");
            assert_eq!(
                MAPISendDocuments(0, null(), empty.as_ptr(), null(), 0),
                MapiResult::AttachmentNotFound.code()
            );
        }
    }

    struct Answer(MapiResult);
    impl Transport for Answer {
        fn send(&self, _: &MapiPipeRequest) -> Result<MapiPipeResponse, MapiResult> {
            Ok(MapiPipeResponse { result: self.0 })
        }
    }

    #[test]
    fn send_with_returns_the_apps_code() {
        let message = MapiMessage {
            subject: None,
            body: None,
            recipients: Vec::new(),
            attachments: Vec::new(),
            ui: MapiUi::None,
            program: program(),
        };
        assert_eq!(send_with(message.clone(), &Answer(MapiResult::Success)), 0);
        assert_eq!(send_with(message, &Answer(MapiResult::UserAbort)), 1);
        assert!(program().is_some_and(|p| p.to_ascii_lowercase().ends_with(".exe")));
    }
}
