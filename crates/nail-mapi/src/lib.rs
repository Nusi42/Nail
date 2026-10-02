//! Nail's Simple MAPI provider (plan M4b WP111; ADR 0087 contract, ADR
//! 0088 decisions; `nail_api::mapi`).
//!
//! Built as `nail_mapi.dll`, installed as `mapi\<arch>\NailMapi.dll` for
//! x64, x86 (32-bit Office) and ARM64. Programs reach it through the
//! Windows MAPI stub ("Send to → Mail recipient", Office's "Send as
//! attachment", any `MAPISendMail` caller):
//!
//! 1. `MAPISendMail` / `MAPISendMailW` / `MAPISendDocuments`
//!    ([`windows::exports`]) decode the caller's structures without trusting
//!    a single pointer ([`windows::decode`], `windows::memory`) into a
//!    [`nail_api::mapi::MapiMessage`].
//! 2. [`forward::forward`] runs the contract's checks, copies the
//!    attachments to a private temporary folder ([`staging`]) so the program
//!    may delete its files at once, and sends a
//!    [`nail_api::mapi::MapiPipeRequest`] to the running app over the
//!    user's named pipe ([`windows::client`]), starting `Nail.exe` when
//!    nothing listens. The app saves a draft and opens the composer — it
//!    never sends on the program's behalf.
//! 3. The app's [`nail_api::mapi::MapiResult`] is what the program gets.
//!
//! The rest of Simple MAPI answers as a mail client without a profile or a
//! readable store would: logon / logoff succeed, names that are addresses
//! resolve, reading, saving, deleting and address dialogs are
//! `MAPI_E_NOT_SUPPORTED`. [`registration`] lists the registry values the
//! installers write. `src/bin/harness.rs` calls the DLL like a program and
//! plays a fake `Nail.exe` for tests.

pub mod forward;
pub mod registration;
pub mod staging;
#[cfg(windows)]
pub mod windows;

mod export_table;
pub use export_table::EXPORTS;

/// The pipe protocol version this DLL speaks.
pub fn protocol_version() -> u32 {
    nail_api::mapi::MAPI_PROTOCOL_VERSION
}

#[cfg(test)]
mod tests {
    #[test]
    fn speaks_the_contract_protocol() {
        assert_eq!(super::protocol_version(), 1);
    }

    #[test]
    fn export_table_is_complete_and_sized_for_stdcall() {
        let names: Vec<&str> = super::EXPORTS.iter().map(|(n, _)| *n).collect();
        for required in [
            "MAPISendMail",
            "MAPISendMailW",
            "MAPILogon",
            "MAPILogoff",
            "MAPIFreeBuffer",
            "MAPIResolveName",
            "MAPIFindNext",
            "MAPIReadMail",
            "MAPISaveMail",
            "MAPIDeleteMail",
            "MAPIAddress",
            "MAPIDetails",
            "MAPISendDocuments",
        ] {
            assert!(names.contains(&required), "{required}");
        }
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len());
        // Parameter counts match MAPI.h (x86 decoration = 4 bytes each).
        let count = |name: &str| super::EXPORTS.iter().find(|(n, _)| *n == name).unwrap().1;
        assert_eq!(count("MAPISendMail") * 4, 20);
        assert_eq!(count("MAPIAddress") * 4, 44);
        assert_eq!(count("MAPIFreeBuffer") * 4, 4);
    }
}
