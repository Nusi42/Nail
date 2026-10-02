//! Nail's Simple MAPI provider (plan M4b WP111; contract ADR 0087,
//! `nail_api::mapi`).
//!
//! Built as `nail_mapi.dll` (installed as `mapi\<arch>\NailMapi.dll` for
//! x64, x86 and ARM64). It exports `MAPISendMail` / `MAPISendMailW`, which
//! decode the caller's message into a [`nail_api::mapi::MapiMessage`] and
//! forward it to the running app over the named pipe
//! [`nail_api::mapi::mapi_pipe_name`] (starting `Nail.exe` when needed),
//! plus the rest of Simple MAPI as safe no-ops or `MAPI_E_NOT_SUPPORTED`.
//!
//! Slot of WP111: this crate is a stub until then.

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
}
