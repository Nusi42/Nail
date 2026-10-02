//! Caller structures → [`nail_api::mapi::MapiMessage`].
//!
//! The `MapiMessage` / `MapiRecipDesc` / `MapiFileDesc` structures (and
//! their `W` twins) are windows-sys' bindings of `MAPI.h`, so the layout is
//! the system's. Every pointer is checked before it is read
//! (`super::memory`); counts are checked against the contract's limits
//! before any element is touched.

use nail_api::mapi::{
    MAX_MAPI_ATTACHMENTS, MAX_MAPI_RECIPIENTS, MAX_MAPI_TEXT_BYTES, MapiAttachment, MapiMessage,
    MapiRecipient, MapiRecipientClass, MapiResult, MapiUi,
};
use windows_sys::Win32::System::Mapi::{
    MapiFileDesc, MapiFileDescW, MapiMessage as RawMessage, MapiMessageW as RawMessageW,
    MapiRecipDesc, MapiRecipDescW,
};

use super::memory::{StrError, read_struct, read_units};
use super::text::{ansi_to_string, wide_to_string};

/// Longest name, address or path the DLL reads (units; Windows paths end at
/// 32 767 characters).
pub const MAX_FIELD_UNITS: usize = 32 * 1024;

/// The fields of a message structure, whatever its character type.
pub struct MessageFields<U, R, F> {
    subject: *const U,
    note_text: *const U,
    recip_count: u32,
    recips: *const R,
    file_count: u32,
    files: *const F,
}

/// The "A" or "W" flavour of the structures.
pub trait Abi {
    /// `u8` (ANSI) or `u16` (UTF-16).
    type Unit: Copy + Default + PartialEq;
    /// `MapiMessage` / `MapiMessageW`.
    type Message: Copy;
    /// `MapiRecipDesc` / `MapiRecipDescW`.
    type Recip: Copy;
    /// `MapiFileDesc` / `MapiFileDescW`.
    type File: Copy;

    /// The message's fields.
    fn message(m: &Self::Message) -> MessageFields<Self::Unit, Self::Recip, Self::File>;
    /// `(ulRecipClass, lpszName, lpszAddress)`.
    fn recip(r: &Self::Recip) -> (u32, *const Self::Unit, *const Self::Unit);
    /// `(lpszPathName, lpszFileName)`.
    fn file(f: &Self::File) -> (*const Self::Unit, *const Self::Unit);
    /// Units → text.
    fn decode(units: &[Self::Unit]) -> String;
}

/// `MAPISendMail`: ANSI.
pub enum Ansi {}
/// `MAPISendMailW`: UTF-16.
pub enum Wide {}

impl Abi for Ansi {
    type Unit = u8;
    type Message = RawMessage;
    type Recip = MapiRecipDesc;
    type File = MapiFileDesc;

    fn message(m: &RawMessage) -> MessageFields<u8, MapiRecipDesc, MapiFileDesc> {
        MessageFields {
            subject: m.lpszSubject,
            note_text: m.lpszNoteText,
            recip_count: m.nRecipCount,
            recips: m.lpRecips,
            file_count: m.nFileCount,
            files: m.lpFiles,
        }
    }
    fn recip(r: &MapiRecipDesc) -> (u32, *const u8, *const u8) {
        (r.ulRecipClass, r.lpszName, r.lpszAddress)
    }
    fn file(f: &MapiFileDesc) -> (*const u8, *const u8) {
        (f.lpszPathName, f.lpszFileName)
    }
    fn decode(units: &[u8]) -> String {
        ansi_to_string(units)
    }
}

impl Abi for Wide {
    type Unit = u16;
    type Message = RawMessageW;
    type Recip = MapiRecipDescW;
    type File = MapiFileDescW;

    fn message(m: &RawMessageW) -> MessageFields<u16, MapiRecipDescW, MapiFileDescW> {
        MessageFields {
            subject: m.lpszSubject,
            note_text: m.lpszNoteText,
            recip_count: m.nRecipCount,
            recips: m.lpRecips,
            file_count: m.nFileCount,
            files: m.lpFiles,
        }
    }
    fn recip(r: &MapiRecipDescW) -> (u32, *const u16, *const u16) {
        (r.ulRecipClass, r.lpszName, r.lpszAddress)
    }
    fn file(f: &MapiFileDescW) -> (*const u16, *const u16) {
        (f.lpszPathName, f.lpszFileName)
    }
    fn decode(units: &[u16]) -> String {
        wide_to_string(units)
    }
}

/// Reads an optional string; empty strings are `None`.
pub fn text<A: Abi>(
    ptr: *const A::Unit,
    max_units: usize,
    too_long: MapiResult,
    unreadable: MapiResult,
) -> Result<Option<String>, MapiResult> {
    match read_units(ptr, max_units) {
        Ok(Some(units)) if units.is_empty() => Ok(None),
        Ok(Some(units)) => Ok(Some(A::decode(&units))),
        Ok(None) => Ok(None),
        Err(StrError::TooLong) => Err(too_long),
        Err(StrError::Unreadable) => Err(unreadable),
    }
}

/// Decodes the message a program passed to `MAPISendMail(W)` with
/// `flFlags`; `program` is the calling executable's name.
pub fn decode_message<A: Abi>(
    ptr: *const A::Message,
    flags: u32,
    program: Option<String>,
) -> Result<MapiMessage, MapiResult> {
    // MAPI_E_INVALID_MESSAGE is not among the codes Nail returns; a missing
    // or unreadable message is a plain failure.
    let msg = read_struct(ptr).ok_or(MapiResult::Failure)?;
    let fields = A::message(&msg);
    let subject = text::<A>(
        fields.subject,
        MAX_MAPI_TEXT_BYTES,
        MapiResult::TextTooLarge,
        MapiResult::Failure,
    )?;
    let body = text::<A>(
        fields.note_text,
        MAX_MAPI_TEXT_BYTES,
        MapiResult::TextTooLarge,
        MapiResult::Failure,
    )?;
    let recip_count = fields.recip_count as usize;
    if recip_count > MAX_MAPI_RECIPIENTS {
        return Err(MapiResult::TooManyRecipients);
    }
    let file_count = fields.file_count as usize;
    if file_count > MAX_MAPI_ATTACHMENTS {
        return Err(MapiResult::TooManyFiles);
    }
    if recip_count > 0 && fields.recips.is_null() {
        return Err(MapiResult::InvalidRecips);
    }
    if file_count > 0 && fields.files.is_null() {
        return Err(MapiResult::AttachmentNotFound);
    }

    let mut recipients = Vec::with_capacity(recip_count);
    for index in 0..recip_count {
        let msg =
            read_struct(fields.recips.wrapping_add(index)).ok_or(MapiResult::InvalidRecips)?;
        let (class, name, address) = A::recip(&msg);
        // MAPI_ORIG (the sender) is dropped.
        let Some(class) = MapiRecipientClass::from_mapi(class)? else {
            continue;
        };
        let field = |ptr| {
            text::<A>(
                ptr,
                MAX_FIELD_UNITS,
                MapiResult::InvalidRecips,
                MapiResult::InvalidRecips,
            )
        };
        recipients.push(MapiRecipient {
            class,
            name: field(name)?,
            address: field(address)?,
        });
    }

    let mut attachments = Vec::with_capacity(file_count);
    for index in 0..file_count {
        let msg =
            read_struct(fields.files.wrapping_add(index)).ok_or(MapiResult::AttachmentNotFound)?;
        let (path, name) = A::file(&msg);
        let field = |ptr| {
            text::<A>(
                ptr,
                MAX_FIELD_UNITS,
                MapiResult::AttachmentNotFound,
                MapiResult::AttachmentNotFound,
            )
        };
        let path = field(path)?.ok_or(MapiResult::AttachmentNotFound)?;
        attachments.push(MapiAttachment {
            path,
            file_name: field(name)?,
        });
    }

    Ok(MapiMessage {
        subject,
        body,
        recipients,
        attachments,
        ui: MapiUi::from_flags(flags),
        program,
    })
}

/// Splits `MAPISendDocuments`' delimited file lists (`lpszFilePaths`,
/// `lpszFileNames`) into attachments.
pub fn split_documents(paths: &str, names: Option<&str>, delimiter: char) -> Vec<MapiAttachment> {
    let names: Vec<&str> = names
        .map(|n| n.split(delimiter).collect())
        .unwrap_or_default();
    paths
        .split(delimiter)
        .enumerate()
        .filter(|(_, path)| !path.trim().is_empty())
        .map(|(index, path)| MapiAttachment {
            path: path.trim().to_owned(),
            file_name: names
                .get(index)
                .map(|n| n.trim())
                .filter(|n| !n.is_empty())
                .map(str::to_owned),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::memory::tests::GuardedPages;
    use super::super::text::{to_ansi, to_wide};
    use super::*;
    use pretty_assertions::assert_eq;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::System::Mapi::{MAPI_BCC, MAPI_CC, MAPI_DIALOG, MAPI_ORIG, MAPI_TO};

    fn wide_recip(class: u32, name: Option<&[u16]>, address: Option<&[u16]>) -> MapiRecipDescW {
        MapiRecipDescW {
            ulReserved: 0,
            ulRecipClass: class,
            lpszName: name.map_or(null_mut(), |n| n.as_ptr().cast_mut()),
            lpszAddress: address.map_or(null_mut(), |a| a.as_ptr().cast_mut()),
            ulEIDSize: 0,
            lpEntryID: null_mut(),
        }
    }

    fn wide_message(
        subject: Option<&[u16]>,
        body: Option<&[u16]>,
        recips: &mut [MapiRecipDescW],
        files: &mut [MapiFileDescW],
    ) -> RawMessageW {
        RawMessageW {
            ulReserved: 0,
            lpszSubject: subject.map_or(null_mut(), |s| s.as_ptr().cast_mut()),
            lpszNoteText: body.map_or(null_mut(), |s| s.as_ptr().cast_mut()),
            lpszMessageType: null_mut(),
            lpszDateReceived: null_mut(),
            lpszConversationID: null_mut(),
            flFlags: 0,
            lpOriginator: null_mut(),
            nRecipCount: recips.len() as u32,
            lpRecips: if recips.is_empty() {
                null_mut()
            } else {
                recips.as_mut_ptr()
            },
            nFileCount: files.len() as u32,
            lpFiles: if files.is_empty() {
                null_mut()
            } else {
                files.as_mut_ptr()
            },
        }
    }

    #[test]
    fn decodes_a_wide_message() {
        let subject = to_wide("Grüße – Q3");
        let body = to_wide("Line 1\r\nLine 2");
        let ada = to_wide("Ada Lovelace");
        let ada_addr = to_wide("SMTP:ada@x.example");
        let bob = to_wide("bob@x.example");
        let me = to_wide("me@x.example");
        let eve = to_wide("eve@x.example");
        let path = to_wide(r"C:\Temp\report.docx");
        let name = to_wide("Q3 report.docx");
        let path2 = to_wide(r"C:\Temp\b.pdf");
        let mut recips = [
            wide_recip(MAPI_TO, Some(&ada), Some(&ada_addr)),
            wide_recip(MAPI_ORIG, None, Some(&me)),
            wide_recip(MAPI_CC, Some(&bob), None),
            wide_recip(MAPI_BCC, None, Some(&eve)),
        ];
        let mut files = [
            MapiFileDescW {
                ulReserved: 0,
                flFlags: 0,
                nPosition: u32::MAX,
                lpszPathName: path.as_ptr().cast_mut(),
                lpszFileName: name.as_ptr().cast_mut(),
                lpFileType: null_mut(),
            },
            MapiFileDescW {
                ulReserved: 0,
                flFlags: 0,
                nPosition: 0,
                lpszPathName: path2.as_ptr().cast_mut(),
                lpszFileName: null_mut(),
                lpFileType: null_mut(),
            },
        ];
        let msg = wide_message(Some(&subject), Some(&body), &mut recips, &mut files);
        let m = decode_message::<Wide>(&msg, MAPI_DIALOG, Some("WINWORD.EXE".into())).unwrap();
        assert_eq!(m.subject.as_deref(), Some("Grüße – Q3"));
        assert_eq!(m.body.as_deref(), Some("Line 1\r\nLine 2"));
        assert_eq!(m.recipients.len(), 3, "MAPI_ORIG is dropped");
        assert_eq!(m.recipients[0].class, MapiRecipientClass::To);
        assert_eq!(m.recipients[0].name.as_deref(), Some("Ada Lovelace"));
        assert_eq!(m.recipients[0].email().as_deref(), Some("ada@x.example"));
        assert_eq!(m.recipients[1].class, MapiRecipientClass::Cc);
        assert_eq!(m.recipients[1].email().as_deref(), Some("bob@x.example"));
        assert_eq!(m.recipients[2].class, MapiRecipientClass::Bcc);
        assert_eq!(m.attachments[0].path, r"C:\Temp\report.docx");
        assert_eq!(
            m.attachments[0].file_name.as_deref(),
            Some("Q3 report.docx")
        );
        assert_eq!(m.attachments[1].file_name, None);
        assert_eq!(m.ui, MapiUi::Dialog);
        assert_eq!(m.program.as_deref(), Some("WINWORD.EXE"));
        assert_eq!(m.check(), Ok(()));
    }

    #[test]
    fn decodes_an_ansi_message() {
        let subject = to_ansi("Invoice");
        let addr = to_ansi("smtp:<carol@x.example>");
        let path = to_ansi(r"C:\Temp\invoice.pdf");
        let mut recips = [MapiRecipDesc {
            ulReserved: 0,
            ulRecipClass: MAPI_TO,
            lpszName: null_mut(),
            lpszAddress: addr.as_ptr().cast_mut(),
            ulEIDSize: 0,
            lpEntryID: null_mut(),
        }];
        let mut files = [MapiFileDesc {
            ulReserved: 0,
            flFlags: 0,
            nPosition: 0,
            lpszPathName: path.as_ptr().cast_mut(),
            lpszFileName: null_mut(),
            lpFileType: null_mut(),
        }];
        let msg = RawMessage {
            ulReserved: 0,
            lpszSubject: subject.as_ptr().cast_mut(),
            lpszNoteText: null_mut(),
            lpszMessageType: null_mut(),
            lpszDateReceived: null_mut(),
            lpszConversationID: null_mut(),
            flFlags: 0,
            lpOriginator: null_mut(),
            nRecipCount: 1,
            lpRecips: recips.as_mut_ptr(),
            nFileCount: 1,
            lpFiles: files.as_mut_ptr(),
        };
        let m = decode_message::<Ansi>(&msg, 0, None).unwrap();
        assert_eq!(m.subject.as_deref(), Some("Invoice"));
        assert_eq!(m.body, None);
        assert_eq!(m.recipients[0].email().as_deref(), Some("carol@x.example"));
        assert_eq!(m.attachments[0].display_name(), "invoice.pdf");
        assert_eq!(m.ui, MapiUi::None);
    }

    #[test]
    fn empty_and_minimal_messages() {
        let msg = wide_message(None, None, &mut [], &mut []);
        let m = decode_message::<Wide>(&msg, 0xC, None).unwrap();
        assert_eq!(m.subject, None);
        assert!(m.recipients.is_empty() && m.attachments.is_empty());
        assert_eq!(m.ui, MapiUi::Modeless);
        let empty = to_wide("");
        let msg = wide_message(Some(&empty), Some(&empty), &mut [], &mut []);
        assert_eq!(decode_message::<Wide>(&msg, 0, None).unwrap().subject, None);
    }

    #[test]
    fn bad_pointers_and_counts_are_refused() {
        // No message / garbage message pointers.
        assert_eq!(
            decode_message::<Wide>(null(), 0, None),
            Err(MapiResult::Failure)
        );
        assert_eq!(
            decode_message::<Ansi>(0x40 as *const RawMessage, 0, None),
            Err(MapiResult::Failure)
        );
        // Counts beyond the limits are refused before the arrays are read.
        let mut msg = wide_message(None, None, &mut [], &mut []);
        msg.nRecipCount = u32::MAX;
        msg.lpRecips = 0x50 as *mut MapiRecipDescW;
        assert_eq!(
            decode_message::<Wide>(&msg, 0, None),
            Err(MapiResult::TooManyRecipients)
        );
        let mut msg = wide_message(None, None, &mut [], &mut []);
        msg.nFileCount = (MAX_MAPI_ATTACHMENTS + 1) as u32;
        assert_eq!(
            decode_message::<Wide>(&msg, 0, None),
            Err(MapiResult::TooManyFiles)
        );
        // Counts without arrays.
        let mut msg = wide_message(None, None, &mut [], &mut []);
        msg.nRecipCount = 2;
        assert_eq!(
            decode_message::<Wide>(&msg, 0, None),
            Err(MapiResult::InvalidRecips)
        );
        let mut msg = wide_message(None, None, &mut [], &mut []);
        msg.nFileCount = 1;
        assert_eq!(
            decode_message::<Wide>(&msg, 0, None),
            Err(MapiResult::AttachmentNotFound)
        );
        // Arrays at garbage addresses.
        let mut msg = wide_message(None, None, &mut [], &mut []);
        msg.nRecipCount = 1;
        msg.lpRecips = 0x60 as *mut MapiRecipDescW;
        assert_eq!(
            decode_message::<Wide>(&msg, 0, None),
            Err(MapiResult::InvalidRecips)
        );
        let mut msg = wide_message(None, None, &mut [], &mut []);
        msg.nFileCount = 1;
        msg.lpFiles = 0x70 as *mut MapiFileDescW;
        assert_eq!(
            decode_message::<Wide>(&msg, 0, None),
            Err(MapiResult::AttachmentNotFound)
        );
        // Garbage strings.
        let mut msg = wide_message(None, None, &mut [], &mut []);
        msg.lpszSubject = 0x80 as *mut u16;
        assert_eq!(
            decode_message::<Wide>(&msg, 0, None),
            Err(MapiResult::Failure)
        );
        let mut recips = [wide_recip(MAPI_TO, None, None)];
        recips[0].lpszAddress = 0x90 as *mut u16;
        let msg = wide_message(None, None, &mut recips, &mut []);
        assert_eq!(
            decode_message::<Wide>(&msg, 0, None),
            Err(MapiResult::InvalidRecips)
        );
        // Unknown recipient class.
        let addr = to_wide("a@x.example");
        let mut recips = [wide_recip(7, None, Some(&addr))];
        let msg = wide_message(None, None, &mut recips, &mut []);
        assert_eq!(
            decode_message::<Wide>(&msg, 0, None),
            Err(MapiResult::BadRecipType)
        );
        // A file without a path.
        let mut files = [MapiFileDescW::default()];
        let msg = wide_message(None, None, &mut [], &mut files);
        assert_eq!(
            decode_message::<Wide>(&msg, 0, None),
            Err(MapiResult::AttachmentNotFound)
        );
    }

    #[test]
    fn structures_running_off_readable_memory() {
        let pages = GuardedPages::new();
        // A recipient array whose second element would cross into the
        // inaccessible page.
        let size = size_of::<MapiRecipDescW>();
        let first = pages.before_end(size).cast::<MapiRecipDescW>();
        let addr = to_wide("a@x.example");
        // SAFETY: one element fits before the inaccessible page.
        unsafe { first.write_unaligned(wide_recip(MAPI_TO, None, Some(&addr))) };
        let mut msg = wide_message(None, None, &mut [], &mut []);
        msg.lpRecips = first;
        msg.nRecipCount = 1;
        assert_eq!(
            decode_message::<Wide>(&msg, 0, None)
                .unwrap()
                .recipients
                .len(),
            1
        );
        msg.nRecipCount = 2;
        assert_eq!(
            decode_message::<Wide>(&msg, 0, None),
            Err(MapiResult::InvalidRecips)
        );
        // A subject without a terminator before the end of readable memory.
        let tail = pages.before_end(6).cast::<u16>();
        // SAFETY: three units before the inaccessible page.
        unsafe { std::ptr::copy_nonoverlapping([0x41u16, 0x42, 0x43].as_ptr(), tail, 3) };
        let mut msg = wide_message(None, None, &mut [], &mut []);
        msg.lpszSubject = tail;
        assert_eq!(
            decode_message::<Wide>(&msg, 0, None),
            Err(MapiResult::Failure)
        );
        // The message structure itself straddling the boundary.
        let msg = pages.before_end(8).cast::<RawMessageW>();
        assert_eq!(
            decode_message::<Wide>(msg, 0, None),
            Err(MapiResult::Failure)
        );
    }

    #[test]
    fn huge_text_is_too_large() {
        let mut big = vec![0x61u16; MAX_MAPI_TEXT_BYTES + 1];
        big.push(0);
        let msg = wide_message(None, Some(&big), &mut [], &mut []);
        assert_eq!(
            decode_message::<Wide>(&msg, 0, None),
            Err(MapiResult::TextTooLarge)
        );
        let mut long_name = vec![0x61u16; MAX_FIELD_UNITS + 1];
        long_name.push(0);
        let mut recips = [wide_recip(MAPI_TO, Some(&long_name), None)];
        let msg = wide_message(None, None, &mut recips, &mut []);
        assert_eq!(
            decode_message::<Wide>(&msg, 0, None),
            Err(MapiResult::InvalidRecips)
        );
    }

    #[test]
    fn document_lists() {
        let files = split_documents(r"C:\a.txt;C:\b.pdf;", Some("A.txt;;"), ';');
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, r"C:\a.txt");
        assert_eq!(files[0].file_name.as_deref(), Some("A.txt"));
        assert_eq!(files[1].file_name, None);
        assert!(split_documents("", None, ';').is_empty());
        let files = split_documents(r"C:\a|C:\b", None, '|');
        assert_eq!(files[1].path, r"C:\b");
    }
}
