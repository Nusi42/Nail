// Every Simple MAPI function NailMapi.dll exports and its number of
// parameters (each pointer-sized or 32-bit, so 4 bytes on x86). On x86
// `extern "system"` is stdcall and the symbols are decorated
// (`_MAPISendMail@20`); rustc's generated export list maps them to the
// plain names programs look up (`MAPISendMail = _MAPISendMail@20`), so no
// `.def` file is needed. `tests/dll.rs` and `nail-mapi-harness exports`
// check that every name below resolves with `GetProcAddress`.

/// `(export name, parameter count)` of every exported function.
pub const EXPORTS: &[(&str, u32)] = &[
    ("MAPISendMail", 5),
    ("MAPISendMailW", 5),
    ("MAPISendDocuments", 5),
    ("MAPILogon", 6),
    ("MAPILogoff", 4),
    ("MAPIFreeBuffer", 1),
    ("MAPIResolveName", 6),
    ("MAPIFindNext", 7),
    ("MAPIReadMail", 6),
    ("MAPISaveMail", 6),
    ("MAPIDeleteMail", 5),
    ("MAPIAddress", 11),
    ("MAPIDetails", 5),
];
