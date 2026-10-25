//! Simple MAPI (M4b, ADR 0087; design.md "Windows installer &
//! distribution" → "Simple MAPI provider").
//!
//! Nail registers as the system's Simple MAPI mail client, so "Send to →
//! Mail recipient" in Explorer, "Send as attachment" in Office and other
//! programs' "email this" open Nail's composer with the files attached.
//! The pieces:
//!
//! * `crates/nail-mapi` (WP111): a DLL (`NailMapi.dll`, x64 / x86 /
//!   ARM64) exporting `MAPISendMail` / `MAPISendMailW` and the rest of
//!   Simple MAPI as safe no-ops or errors. It turns the caller's
//!   `MapiMessage` structure into a [`MapiMessage`] (strings decoded,
//!   recipient classes mapped, `SMTP:` prefixes kept for
//!   [`MapiRecipient::email`]), sends it as a [`MapiPipeRequest`] frame to
//!   the running app over the named pipe [`mapi_pipe_name`] (starting
//!   `Nail.exe` beside it, [`app_exe_from_dll`], when nothing answers) and
//!   returns the [`MapiResult::code`] of the [`MapiPipeResponse`].
//! * The desktop shell (WP113) serves the pipe (ACL: the current user
//!   only) and hands each message to the core
//!   (`nail_core::Service::submit_mapi`).
//! * The core reads the attachments **before answering** (callers often
//!   delete their temporary files as soon as `MAPISendMail` returns),
//!   saves a draft and asks the UI to open it
//!   ([`crate::LaunchRequest::ComposeDraft`]).
//!
//! **Nail never sends silently for another program.** A request without
//! UI (no `MAPI_DIALOG` flag) opens the composer like one with UI; the
//! user presses Send (plan "Scope decisions for M4": outgoing actions need
//! a confirmation the user sees). [`MapiUi`] is recorded for the future.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{ApiError, ErrorCode};

/// Version of the pipe protocol ([`MapiPipeRequest::version`]).
pub const MAPI_PROTOCOL_VERSION: u32 = 1;
/// Most recipients one message may carry
/// ([`MapiResult::TooManyRecipients`] beyond).
pub const MAX_MAPI_RECIPIENTS: usize = 500;
/// Most attachments ([`MapiResult::TooManyFiles`] beyond).
pub const MAX_MAPI_ATTACHMENTS: usize = 100;
/// Most bytes of subject plus body (UTF-8; [`MapiResult::TextTooLarge`]
/// beyond).
pub const MAX_MAPI_TEXT_BYTES: usize = 8 * 1024 * 1024;
/// Largest pipe frame (JSON; attachments travel as paths, not content).
pub const MAX_MAPI_FRAME_BYTES: usize = 16 * 1024 * 1024;
/// How long the DLL waits for the app's pipe after starting `Nail.exe`
/// (milliseconds).
pub const MAPI_CONNECT_TIMEOUT_MS: u64 = 30_000;
/// How long the DLL waits for the answer once connected (milliseconds;
/// reading large attachments takes a while).
pub const MAPI_RESPONSE_TIMEOUT_MS: u64 = 120_000;
/// Environment variable that overrides [`mapi_pipe_name`] in tests (both
/// the DLL and the shell read it).
pub const MAPI_PIPE_ENV: &str = "NAIL_MAPI_PIPE";

/// The named pipe of the current user's running Nail:
/// `\\.\pipe\nail-mapi-<SID>` with the user's SID string (`S-1-5-21-…`).
/// The shell creates it with `FILE_FLAG_FIRST_PIPE_INSTANCE` and an ACL for
/// that user only; the DLL checks the server process runs as the same user.
pub fn mapi_pipe_name(user_sid: &str) -> String {
    format!(r"\\.\pipe\nail-mapi-{user_sid}")
}

/// The DLL's file name in every architecture's folder.
pub const MAPI_DLL_FILE: &str = "NailMapi.dll";

/// Where the installers put the DLL of an architecture, relative to the
/// installation directory: `mapi\<arch>\NailMapi.dll`.
pub fn mapi_dll_relative_path(arch: crate::desktop::WindowsArch) -> String {
    format!(r"mapi\{}\{MAPI_DLL_FILE}", arch.as_str())
}

/// The app beside an installed DLL: `<install dir>\Nail.exe`, two folders
/// above `mapi\<arch>\NailMapi.dll`.
pub fn app_exe_from_dll(dll: &Path) -> Option<PathBuf> {
    let install_dir = dll.parent()?.parent()?.parent()?;
    Some(install_dir.join(APP_EXE_FILE))
}

/// The desktop app's executable name (`mainBinaryName` in
/// `tauri.conf.json`).
pub const APP_EXE_FILE: &str = "Nail.exe";

/// Registry of the Simple MAPI client (WP112 writes it, WP111 documents
/// it, WP113 reads it for [`crate::desktop::MapiClientStatus`]); confirm
/// against current Windows docs.
pub mod registry {
    /// `HKLM` (and, for the default, `HKCU`) `Software\Clients\Mail`; its
    /// default value names the default Simple MAPI client.
    pub const MAIL_CLIENTS_KEY: &str = r"Software\Clients\Mail";
    /// `HKLM\Software\Clients\Mail\Nail` (64-bit view; the 32-bit view
    /// `WOW6432Node` gets the x86 DLL).
    pub const NAIL_MAIL_CLIENT_KEY: &str = r"Software\Clients\Mail\Nail";
    /// The client's name under [`MAIL_CLIENTS_KEY`].
    pub const CLIENT_NAME: &str = "Nail";
    /// `REG_SZ` path of the DLL for the view's architecture.
    pub const DLL_PATH_VALUE: &str = "DLLPath";
    /// `REG_SZ` path of the DLL (preferred by the Windows MAPI stub).
    pub const DLL_PATH_EX_VALUE: &str = "DLLPathEx";
}

/// The kind of a recipient (`MAPI_TO` / `MAPI_CC` / `MAPI_BCC`; the DLL
/// drops `MAPI_ORIG`, the sender).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum MapiRecipientClass {
    /// `MAPI_TO` (1).
    To,
    /// `MAPI_CC` (2).
    Cc,
    /// `MAPI_BCC` (3).
    Bcc,
}

impl MapiRecipientClass {
    /// `ulRecipClass` → class: `None` for `MAPI_ORIG` (0, ignored) and
    /// `Err(BadRecipType)` for unknown values.
    pub fn from_mapi(value: u32) -> Result<Option<Self>, MapiResult> {
        match value {
            0 => Ok(None),
            1 => Ok(Some(Self::To)),
            2 => Ok(Some(Self::Cc)),
            3 => Ok(Some(Self::Bcc)),
            _ => Err(MapiResult::BadRecipType),
        }
    }
}

/// One recipient as the calling program gave it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MapiRecipient {
    /// To / Cc / Bcc.
    pub class: MapiRecipientClass,
    /// `lpszName` (display name; some programs put the address here).
    pub name: Option<String>,
    /// `lpszAddress` (often `SMTP:a@b.example`).
    pub address: Option<String>,
}

impl MapiRecipient {
    /// The e-mail address: `address` without an `SMTP:` prefix (any case),
    /// else `name` when it looks like an address. `None` when neither is
    /// usable ([`MapiResult::InvalidRecips`] / `UnknownRecipient`).
    pub fn email(&self) -> Option<String> {
        let strip = |text: &str| -> Option<String> {
            let text = text.trim();
            let text = match text.get(..5) {
                Some(prefix) if prefix.eq_ignore_ascii_case("smtp:") => &text[5..],
                _ => text,
            };
            let text = text.trim().trim_start_matches('<').trim_end_matches('>');
            let at = text.find('@')?;
            (at > 0 && at + 1 < text.len() && !text.chars().any(char::is_whitespace))
                .then(|| text.to_owned())
        };
        self.address
            .as_deref()
            .and_then(strip)
            .or_else(|| self.name.as_deref().and_then(strip))
    }

    /// The display name, unless it is just the address again.
    pub fn display_name(&self) -> Option<String> {
        let name = self.name.as_deref()?.trim();
        let email = self.email();
        (!name.is_empty() && Some(name) != email.as_deref()).then(|| name.to_owned())
    }
}

/// One file to attach (`lpszPathName`, `lpszFileName`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MapiAttachment {
    /// Absolute path of the file to read.
    pub path: String,
    /// The name to show / send; `null` = the path's file name.
    pub file_name: Option<String>,
}

impl MapiAttachment {
    /// The attachment's name: `file_name` (its last path component, in case
    /// a program passes a path), else the path's.
    pub fn display_name(&self) -> String {
        let last = |text: &str| -> Option<String> {
            let name = text.rsplit(['\\', '/']).next()?.trim();
            (!name.is_empty()).then(|| name.to_owned())
        };
        self.file_name
            .as_deref()
            .and_then(last)
            .or_else(|| last(&self.path))
            .unwrap_or_else(|| "attachment".to_owned())
    }
}

/// Whether the program asked for Nail's UI (`MAPI_DIALOG`,
/// `MAPI_DIALOG_MODELESS`) or none. Nail opens the composer either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum MapiUi {
    /// `MAPI_DIALOG` (modal; the call returns once the draft is open).
    Dialog,
    /// `MAPI_DIALOG_MODELESS` (`MAPISendMailW`).
    Modeless,
    /// Neither: the program wanted to send without UI.
    None,
}

impl MapiUi {
    /// `flFlags` → UI choice (`MAPI_DIALOG` = 0x8,
    /// `MAPI_DIALOG_MODELESS` = 0x4 | 0x8).
    pub fn from_flags(flags: u32) -> Self {
        const MAPI_DIALOG: u32 = 0x8;
        const MAPI_DIALOG_MODELESS: u32 = 0x4;
        if flags & MAPI_DIALOG_MODELESS != 0 {
            Self::Modeless
        } else if flags & MAPI_DIALOG != 0 {
            Self::Dialog
        } else {
            Self::None
        }
    }
}

/// One `MAPISendMail` / `MAPISendMailW` call, decoded by the DLL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MapiMessage {
    /// `lpszSubject`.
    pub subject: Option<String>,
    /// `lpszNoteText` (plain text).
    pub body: Option<String>,
    /// To / Cc / Bcc recipients (may be empty: the user fills them in).
    pub recipients: Vec<MapiRecipient>,
    /// Files to attach.
    pub attachments: Vec<MapiAttachment>,
    /// Whether the program asked for UI.
    pub ui: MapiUi,
    /// The calling program's executable file name (e.g. `WINWORD.EXE`),
    /// for logs.
    pub program: Option<String>,
}

impl MapiMessage {
    /// The checks that need no file system: counts, text size, recipient
    /// addresses, absolute attachment paths. (The core then checks that
    /// each file exists, can be read and fits the attachment limit.)
    pub fn check(&self) -> Result<(), MapiResult> {
        if self.recipients.len() > MAX_MAPI_RECIPIENTS {
            return Err(MapiResult::TooManyRecipients);
        }
        if self.attachments.len() > MAX_MAPI_ATTACHMENTS {
            return Err(MapiResult::TooManyFiles);
        }
        let text =
            self.subject.as_deref().map_or(0, str::len) + self.body.as_deref().map_or(0, str::len);
        if text > MAX_MAPI_TEXT_BYTES {
            return Err(MapiResult::TextTooLarge);
        }
        if self.recipients.iter().any(|r| r.email().is_none()) {
            return Err(MapiResult::InvalidRecips);
        }
        if self
            .attachments
            .iter()
            .any(|a| a.path.trim().is_empty() || !is_absolute_windows_path(&a.path))
        {
            return Err(MapiResult::AttachmentNotFound);
        }
        Ok(())
    }

    /// The recipients' addresses of one class.
    pub fn addresses(&self, class: MapiRecipientClass) -> Vec<String> {
        self.recipients
            .iter()
            .filter(|r| r.class == class)
            .filter_map(MapiRecipient::email)
            .collect()
    }
}

/// `C:\…`, `C:/…` or a UNC path `\\server\share\…`.
fn is_absolute_windows_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/'))
        || path.starts_with(r"\\")
        || Path::new(path).is_absolute()
}

/// What `MAPISendMail` returns (the `MAPI.h` codes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum MapiResult {
    /// `SUCCESS_SUCCESS` (0): the draft is saved and the composer opens.
    Success,
    /// `MAPI_USER_ABORT` (1).
    UserAbort,
    /// `MAPI_E_FAILURE` (2): anything else (Nail could not start, …).
    Failure,
    /// `MAPI_E_LOGIN_FAILURE` (3): no account / identity to write from.
    LoginFailure,
    /// `MAPI_E_INSUFFICIENT_MEMORY` (5).
    InsufficientMemory,
    /// `MAPI_E_ACCESS_DENIED` (6).
    AccessDenied,
    /// `MAPI_E_TOO_MANY_FILES` (9).
    TooManyFiles,
    /// `MAPI_E_TOO_MANY_RECIPIENTS` (10).
    TooManyRecipients,
    /// `MAPI_E_ATTACHMENT_NOT_FOUND` (11).
    AttachmentNotFound,
    /// `MAPI_E_ATTACHMENT_OPEN_FAILURE` (12).
    AttachmentOpenFailure,
    /// `MAPI_E_UNKNOWN_RECIPIENT` (14).
    UnknownRecipient,
    /// `MAPI_E_BAD_RECIPTYPE` (15).
    BadRecipType,
    /// `MAPI_E_TEXT_TOO_LARGE` (18).
    TextTooLarge,
    /// `MAPI_E_INVALID_RECIPS` (25).
    InvalidRecips,
    /// `MAPI_E_NOT_SUPPORTED` (26): a Simple MAPI function Nail does not
    /// offer (reading mail, address book, …).
    NotSupported,
    /// `MAPI_E_ATTACHMENT_TOO_LARGE` (28).
    AttachmentTooLarge,
}

impl MapiResult {
    /// Every result.
    pub const ALL: [MapiResult; 16] = [
        Self::Success,
        Self::UserAbort,
        Self::Failure,
        Self::LoginFailure,
        Self::InsufficientMemory,
        Self::AccessDenied,
        Self::TooManyFiles,
        Self::TooManyRecipients,
        Self::AttachmentNotFound,
        Self::AttachmentOpenFailure,
        Self::UnknownRecipient,
        Self::BadRecipType,
        Self::TextTooLarge,
        Self::InvalidRecips,
        Self::NotSupported,
        Self::AttachmentTooLarge,
    ];

    /// The `ULONG` the DLL returns.
    pub fn code(self) -> u32 {
        match self {
            Self::Success => 0,
            Self::UserAbort => 1,
            Self::Failure => 2,
            Self::LoginFailure => 3,
            Self::InsufficientMemory => 5,
            Self::AccessDenied => 6,
            Self::TooManyFiles => 9,
            Self::TooManyRecipients => 10,
            Self::AttachmentNotFound => 11,
            Self::AttachmentOpenFailure => 12,
            Self::UnknownRecipient => 14,
            Self::BadRecipType => 15,
            Self::TextTooLarge => 18,
            Self::InvalidRecips => 25,
            Self::NotSupported => 26,
            Self::AttachmentTooLarge => 28,
        }
    }

    /// The result of a code (`None` for codes Nail never returns).
    pub fn from_code(code: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.code() == code)
    }

    /// Fluent id for logs and the UI (`mapi-result-*`, core-desktop.ftl).
    pub fn message_id(self) -> String {
        let name = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        format!("mapi-result-{name}")
    }

    /// The result for a core error while preparing the draft: `too_large`
    /// → attachment too large, `not_found` → attachment not found,
    /// `config` → no account to write from (login failure), `forbidden`
    /// → access denied, `cancelled` → user abort, the rest → failure.
    pub fn from_api_error(err: &ApiError) -> Self {
        match err.code {
            ErrorCode::TooLarge => Self::AttachmentTooLarge,
            ErrorCode::NotFound => Self::AttachmentNotFound,
            ErrorCode::Config => Self::LoginFailure,
            ErrorCode::Forbidden => Self::AccessDenied,
            ErrorCode::Cancelled => Self::UserAbort,
            ErrorCode::InvalidRequest => Self::InvalidRecips,
            _ => Self::Failure,
        }
    }
}

/// The DLL → app frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MapiPipeRequest {
    /// [`MAPI_PROTOCOL_VERSION`]; the app answers
    /// [`MapiResult::Failure`] to another one.
    pub version: u32,
    /// The message.
    pub message: MapiMessage,
}

/// The app → DLL frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MapiPipeResponse {
    /// What to return to the program.
    pub result: MapiResult,
}

/// A frame: 4-byte little-endian length, then that many bytes of JSON.
pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, MapiFrameError> {
    let json = serde_json::to_vec(value).map_err(|e| MapiFrameError::Json(e.to_string()))?;
    let len = u32::try_from(json.len())
        .ok()
        .filter(|_| json.len() <= MAX_MAPI_FRAME_BYTES)
        .ok_or(MapiFrameError::TooLarge(json.len()))?;
    let mut frame = Vec::with_capacity(4 + json.len());
    frame.extend_from_slice(&len.to_le_bytes());
    frame.extend_from_slice(&json);
    Ok(frame)
}

/// Writes one frame.
pub fn write_frame<T: Serialize>(out: &mut impl Write, value: &T) -> Result<(), MapiFrameError> {
    let frame = encode_frame(value)?;
    out.write_all(&frame)
        .and_then(|()| out.flush())
        .map_err(|e| MapiFrameError::Io(e.to_string()))
}

/// The body length a frame header announces (checked against
/// [`MAX_MAPI_FRAME_BYTES`]).
pub fn frame_len(header: [u8; 4]) -> Result<usize, MapiFrameError> {
    let len = u32::from_le_bytes(header) as usize;
    if len > MAX_MAPI_FRAME_BYTES {
        return Err(MapiFrameError::TooLarge(len));
    }
    Ok(len)
}

/// Decodes a frame body.
pub fn decode_frame<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, MapiFrameError> {
    serde_json::from_slice(body).map_err(|e| MapiFrameError::Json(e.to_string()))
}

/// Reads one frame.
pub fn read_frame<T: for<'de> Deserialize<'de>>(
    input: &mut impl Read,
) -> Result<T, MapiFrameError> {
    let mut header = [0u8; 4];
    input
        .read_exact(&mut header)
        .map_err(|e| MapiFrameError::Io(e.to_string()))?;
    let len = frame_len(header)?;
    let mut body = vec![0u8; len];
    input
        .read_exact(&mut body)
        .map_err(|e| MapiFrameError::Io(e.to_string()))?;
    decode_frame(&body)
}

/// A pipe frame could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MapiFrameError {
    /// The pipe broke.
    #[error("pipe I/O failed: {0}")]
    Io(String),
    /// Not the expected JSON.
    #[error("invalid frame: {0}")]
    Json(String),
    /// Larger than [`MAX_MAPI_FRAME_BYTES`].
    #[error("frame of {0} bytes is too large")]
    TooLarge(usize),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::WindowsArch;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn recipient(
        class: MapiRecipientClass,
        name: Option<&str>,
        address: Option<&str>,
    ) -> MapiRecipient {
        MapiRecipient {
            class,
            name: name.map(str::to_owned),
            address: address.map(str::to_owned),
        }
    }

    fn message() -> MapiMessage {
        MapiMessage {
            subject: Some("Report".into()),
            body: Some("See attached.".into()),
            recipients: vec![
                recipient(
                    MapiRecipientClass::To,
                    Some("Ada"),
                    Some("SMTP:ada@x.example"),
                ),
                recipient(MapiRecipientClass::Cc, Some("bob@x.example"), None),
                recipient(MapiRecipientClass::Bcc, None, Some("smtp:<eve@x.example>")),
            ],
            attachments: vec![MapiAttachment {
                path: r"C:\Users\a\AppData\Local\Temp\report.docx".into(),
                file_name: Some("Q3 report.docx".into()),
            }],
            ui: MapiUi::Dialog,
            program: Some("WINWORD.EXE".into()),
        }
    }

    #[test]
    fn codes_match_mapi_h() {
        let codes: Vec<u32> = MapiResult::ALL.iter().map(|r| r.code()).collect();
        assert_eq!(
            codes,
            [0, 1, 2, 3, 5, 6, 9, 10, 11, 12, 14, 15, 18, 25, 26, 28]
        );
        for result in MapiResult::ALL {
            assert_eq!(MapiResult::from_code(result.code()), Some(result));
        }
        assert_eq!(MapiResult::from_code(4), None);
        assert_eq!(
            MapiResult::BadRecipType.message_id(),
            "mapi-result-bad-recip-type"
        );
    }

    #[test]
    fn addresses_are_cleaned() {
        let m = message();
        assert_eq!(m.addresses(MapiRecipientClass::To), ["ada@x.example"]);
        assert_eq!(m.addresses(MapiRecipientClass::Cc), ["bob@x.example"]);
        assert_eq!(m.addresses(MapiRecipientClass::Bcc), ["eve@x.example"]);
        assert_eq!(m.recipients[0].display_name().as_deref(), Some("Ada"));
        assert_eq!(m.recipients[1].display_name(), None);
        assert_eq!(
            recipient(MapiRecipientClass::To, Some("Ada"), None).email(),
            None
        );
        assert_eq!(
            recipient(MapiRecipientClass::To, None, Some("SMTP:@x")).email(),
            None
        );
    }

    #[test]
    fn recipient_classes_and_flags() {
        assert_eq!(MapiRecipientClass::from_mapi(0), Ok(None));
        assert_eq!(
            MapiRecipientClass::from_mapi(3),
            Ok(Some(MapiRecipientClass::Bcc))
        );
        assert_eq!(
            MapiRecipientClass::from_mapi(4),
            Err(MapiResult::BadRecipType)
        );
        assert_eq!(MapiUi::from_flags(0x8), MapiUi::Dialog);
        assert_eq!(MapiUi::from_flags(0xC), MapiUi::Modeless);
        assert_eq!(MapiUi::from_flags(0x1), MapiUi::None);
    }

    #[test]
    fn checks_without_the_file_system() {
        assert_eq!(message().check(), Ok(()));
        let mut m = message();
        m.recipients
            .push(recipient(MapiRecipientClass::To, Some("nobody"), None));
        assert_eq!(m.check(), Err(MapiResult::InvalidRecips));
        let mut m = message();
        m.attachments[0].path = "relative.txt".into();
        assert_eq!(m.check(), Err(MapiResult::AttachmentNotFound));
        let mut m = message();
        m.attachments[0].path = r"\\server\share\a.pdf".into();
        assert_eq!(m.check(), Ok(()));
        let mut m = message();
        m.attachments = vec![m.attachments[0].clone(); MAX_MAPI_ATTACHMENTS + 1];
        assert_eq!(m.check(), Err(MapiResult::TooManyFiles));
        let mut m = message();
        m.recipients = vec![m.recipients[0].clone(); MAX_MAPI_RECIPIENTS + 1];
        assert_eq!(m.check(), Err(MapiResult::TooManyRecipients));
        let mut m = message();
        m.body = Some("x".repeat(MAX_MAPI_TEXT_BYTES + 1));
        assert_eq!(m.check(), Err(MapiResult::TextTooLarge));
    }

    #[test]
    fn attachment_names() {
        let a = MapiAttachment {
            path: r"C:\t\x.tmp".into(),
            file_name: None,
        };
        assert_eq!(a.display_name(), "x.tmp");
        let a = MapiAttachment {
            path: r"C:\t\x.tmp".into(),
            file_name: Some(r"C:\docs\Q3.xlsx".into()),
        };
        assert_eq!(a.display_name(), "Q3.xlsx");
    }

    #[test]
    fn frames_round_trip_and_are_bounded() {
        let request = MapiPipeRequest {
            version: MAPI_PROTOCOL_VERSION,
            message: message(),
        };
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &request).unwrap();
        assert_eq!(
            u32::from_le_bytes(buffer[..4].try_into().unwrap()) as usize,
            buffer.len() - 4
        );
        let back: MapiPipeRequest = read_frame(&mut buffer.as_slice()).unwrap();
        assert_eq!(back, request);
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["message"]["ui"], "dialog");
        assert_eq!(value["message"]["recipients"][1]["class"], "cc");
        assert_eq!(
            serde_json::to_value(MapiPipeResponse {
                result: MapiResult::AttachmentTooLarge
            })
            .unwrap(),
            json!({"result": "attachment-too-large"})
        );
        let huge = u32::try_from(MAX_MAPI_FRAME_BYTES + 1)
            .unwrap()
            .to_le_bytes();
        assert!(matches!(frame_len(huge), Err(MapiFrameError::TooLarge(_))));
        let truncated = &buffer[..buffer.len() - 1];
        assert!(matches!(
            read_frame::<MapiPipeRequest>(&mut &truncated[..]),
            Err(MapiFrameError::Io(_))
        ));
    }

    #[test]
    fn names_and_paths() {
        assert_eq!(
            mapi_pipe_name("S-1-5-21-1-2-3-1001"),
            r"\\.\pipe\nail-mapi-S-1-5-21-1-2-3-1001"
        );
        assert_eq!(
            mapi_dll_relative_path(WindowsArch::X86),
            r"mapi\x86\NailMapi.dll"
        );
        // Built with the platform's separator: the DLL path is a Windows
        // fact, the walk up to the install directory is the same everywhere.
        let install_dir = Path::new("Program Files").join("Nail");
        let dll = install_dir.join("mapi").join("arm64").join("NailMapi.dll");
        assert_eq!(app_exe_from_dll(&dll), Some(install_dir.join("Nail.exe")));
    }

    #[test]
    fn api_errors_map_to_mapi_codes() {
        let code = |c: ErrorCode| MapiResult::from_api_error(&ApiError::new(c, "x"));
        assert_eq!(code(ErrorCode::TooLarge), MapiResult::AttachmentTooLarge);
        assert_eq!(code(ErrorCode::NotFound), MapiResult::AttachmentNotFound);
        assert_eq!(code(ErrorCode::Config), MapiResult::LoginFailure);
        assert_eq!(code(ErrorCode::Internal), MapiResult::Failure);
    }
}
