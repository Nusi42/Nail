//! POP3 client (RFC 1939) for Nail.
//!
//! **Stub (M1a WP10).** Work package WP12 fills this crate in with an
//! in-house async client: `STLS`, `USER`/`PASS`, `APOP`, SASL `PLAIN` /
//! `XOAUTH2` (RFC 5034), `CAPA`, `UIDL`, `TOP`, `RETR`, `DELE`, `QUIT`;
//! implicit TLS (995) and STLS (110) over `tokio-rustls`; and the
//! leave-on-server policies of [`nail_api::accounts::Pop3Options`].
//!
//! Tests run against the mock POP3 server of `nail-testkit-net`.
//! Expected shape: `docs/adr/0012-m1a-contract.md`.

#![warn(missing_docs)]

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use base64::Engine as _;

    #[test]
    fn sasl_plain_encoding_is_available() {
        // SASL PLAIN: authzid NUL authcid NUL password, base64-encoded.
        let encoded = base64::engine::general_purpose::STANDARD.encode(b"\0ada\0secret");
        assert_eq!(encoded, "AGFkYQBzZWNyZXQ=");
        assert_eq!(super::CRATE, "nail-proto-pop3");
    }
}
