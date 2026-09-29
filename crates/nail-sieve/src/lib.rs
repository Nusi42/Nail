//! Server-side rules through Sieve and ManageSieve (plan M2c WP51;
//! contract ADR 0043).
//!
//! **Stub (WP50).** WP51 fills this crate; the contract it builds against
//! is `nail_api::sieve` (settings, rule placement, vacation) and
//! `nail_api::rules` (the rule model). Expected shape:
//!
//! * `compile` — rules (and the vacation settings) → one Sieve script
//!   (RFC 5228 + `fileinto`, `imap4flags`, `vacation`, `regex`,
//!   `variables`, `date` where the server has them), plus a
//!   `RuleSieveStatus` per rule (`server` / `client` / `not-expressible`
//!   with localized reasons). Capability-aware: a rule that needs an
//!   extension the server lacks is `client`.
//! * `parse` — a small Sieve parser for the round-trip test harness
//!   (compile → parse → equivalent rule).
//! * `client` — a ManageSieve client (RFC 5804): greeting / `CAPABILITY`,
//!   `STARTTLS`, `AUTHENTICATE PLAIN` / `XOAUTH2`, `LISTSCRIPTS`,
//!   `CHECKSCRIPT`, `PUTSCRIPT`, `SETACTIVE`, `LOGOUT`; string literals
//!   `{n+}`; errors mapped to `nail_api::ErrorCode` with
//!   `LocalizedMessage`s (e.g. the server's `CHECKSCRIPT` complaint).
//! * The mock ManageSieve server lives in `nail-testkit-net`
//!   (`managesieve.rs`, also WP51).
//!
//! The crate does not depend on `nail-core` or `nail-store`: the core
//! (WP55) calls it with the rules, the account's settings and the IMAP
//! login, and records uploads in the store (`Store::record_sieve_upload`).

#![warn(missing_docs)]

/// The ManageSieve port (RFC 5804), re-exported for the client.
pub use nail_api::sieve::MANAGESIEVE_DEFAULT_PORT;

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "nail-sieve");
        assert_eq!(super::MANAGESIEVE_DEFAULT_PORT, 4190);
    }
}
