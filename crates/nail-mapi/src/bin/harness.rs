//! Simple MAPI test harness (WP111 slot): will load `nail_mapi.dll` and
//! call `MAPISendMailW` like Explorer's "Send to → Mail recipient", for the
//! DLL's tests and the M4b end-to-end scenario. Exits with the MAPI code.

fn main() -> std::process::ExitCode {
    // Stub: MAPI_E_NOT_SUPPORTED until WP111.
    std::process::ExitCode::from(nail_api::mapi::MapiResult::NotSupported.code() as u8)
}
