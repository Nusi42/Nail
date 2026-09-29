//! SMTP submission (RFC 6409) for Nail.
//!
//! **Stub (M1a WP10).** Work package WP12 fills this crate in:
//!
//! * an [`nail_api::engine::OutgoingTransport`] implementation on top of
//!   [`lettre`]'s async SMTP transport (tokio + rustls with the `ring`
//!   provider and the platform verifier): implicit TLS on 465, STARTTLS on
//!   587, plaintext only for local test servers;
//! * authentication PLAIN / LOGIN / XOAUTH2 (the access token comes from
//!   [`nail_api::engine::AccessTokenProvider`] via the core);
//! * `SIZE` check before sending (error code `too_large`), optional DSN;
//! * errors mapped to [`nail_api::engine::EngineError`] (4xx retryable, 5xx
//!   `rejected`).
//!
//! Tests run against the mock SMTP server of `nail-testkit-net`.
//! Expected shape: `docs/adr/0012-m1a-contract.md`.

#![warn(missing_docs)]

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn lettre_builds_an_envelope() {
        let from: lettre::Address = "ada@example.org".parse().unwrap();
        let to: lettre::Address = "bob@example.org".parse().unwrap();
        let envelope = lettre::address::Envelope::new(Some(from), vec![to]).unwrap();
        assert_eq!(envelope.to().len(), 1);
        assert_eq!(super::CRATE, "nail-proto-smtp");
    }
}
