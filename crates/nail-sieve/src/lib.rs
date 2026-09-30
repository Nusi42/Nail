//! Server-side rules through Sieve and ManageSieve (plan M2c WP51; contract
//! ADR 0043, design ADR 0044 (`docs/adr/0044-m2c-sieve.md`)).
//!
//! * [`compile`] — the rules of `rules.kdl` and the vacation settings of
//!   one account → one Sieve script (RFC 5228 + `fileinto`, `imap4flags`,
//!   `copy`, `variables`, `body`, `regex`, `relational`, `date`, `index`,
//!   `vacation`, `include`, as the server offers them), plus a
//!   [`nail_api::sieve::RuleSieveStatus`] per rule:
//!   `server`, `client` or `not-expressible`, with localized reasons
//!   ([`reasons`], `locales/*/core-sieve.ftl`).
//! * [`parse()`] — a small Sieve parser; [`decompile()`] reads a generated
//!   script back into rules for the round-trip harness
//!   (`compile → parse → decompile ≡ normalize_rule`).
//! * [`SieveSession`] — the ManageSieve client (RFC 5804): `STARTTLS`,
//!   `AUTHENTICATE PLAIN` / `XOAUTH2` / `OAUTHBEARER`, `LISTSCRIPTS`,
//!   `PUTSCRIPT`, `CHECKSCRIPT`, `SETACTIVE`, `GETSCRIPT`, `DELETESCRIPT`,
//!   `HAVESPACE`; [`SieveAuth`] resolves the account's IMAP login.
//! * [`upload`] / [`remove`] — one managed script per account that never
//!   overwrites, deletes or silently disables the user's own scripts.
//!
//! The crate does not depend on `nail-core` or `nail-store`: the core
//! (WP55) calls it with the rules, the account's settings and login, and
//! records the upload in the store (`Store::record_sieve_upload`; the hash
//! is [`script_hash`]). The mock server for tests is
//! `nail_testkit_net::MockSieveServer`.

#![warn(missing_docs)]

mod auth;
mod client;
mod compile;
pub mod decompile;
mod ere;
mod error;
pub mod parse;
pub mod reasons;
mod sync;
mod transport;
mod writer;

pub use auth::SieveAuth;
pub use client::{Capabilities, ScriptInfo, SieveConfig, SieveCredentials, SieveSession, TlsTrust};
pub use compile::{
    COMMON_EXTENSIONS, CompileOptions, CompiledScript, FolderMap, FolderResolver, MANAGED_MARKER,
    PREVIOUS_MARKER, VacationOutcome, VacationState, compile,
};
pub use decompile::{Decompiled, DecompiledRule, decompile, normalize_rule};
pub use ere::to_posix_ere;
pub use error::{Result, SieveError};
pub use parse::{ParseError, Script, parse};
pub use sync::{UploadReport, is_managed, recorded_previous, remove, script_hash, upload};
pub use writer::to_crlf;

/// The ManageSieve port (RFC 5804), re-exported for the client.
pub use nail_api::sieve::MANAGESIEVE_DEFAULT_PORT;

use nail_api::localized::LocalizedMessage;
use nail_api::sieve::{RulePlacement, RuleSieveStatus};

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

/// Turns `server` placements into `client` ones with `reason` (e.g.
/// [`reasons::PENDING`] while the script is not uploaded yet,
/// [`reasons::UNAVAILABLE`] when the server has no ManageSieve), for the
/// core's `GetSieveStatus`: a rule only counts as `server` once the server
/// runs it.
pub fn demote_server_rules(rules: &mut [RuleSieveStatus], reason: &str) {
    for rule in rules {
        if rule.placement == RulePlacement::Server {
            rule.placement = RulePlacement::Client;
            rule.reasons = vec![LocalizedMessage::new(reason)];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name() {
        assert_eq!(CRATE, "nail-sieve");
        assert_eq!(MANAGESIEVE_DEFAULT_PORT, 4190);
    }

    #[test]
    fn demotes_only_server_rules() {
        let mut rules = vec![
            RuleSieveStatus {
                rule_id: "a".into(),
                placement: RulePlacement::Server,
                reasons: vec![],
            },
            RuleSieveStatus {
                rule_id: "b".into(),
                placement: RulePlacement::NotExpressible,
                reasons: vec![LocalizedMessage::new(reasons::QUERY)],
            },
        ];
        demote_server_rules(&mut rules, reasons::PENDING);
        assert_eq!(rules[0].placement, RulePlacement::Client);
        assert_eq!(rules[0].reasons[0].id, reasons::PENDING);
        assert_eq!(rules[1].placement, RulePlacement::NotExpressible);
        assert_eq!(rules[1].reasons[0].id, reasons::QUERY);
    }
}
