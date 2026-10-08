//! Fluent ids of the texts this crate builds (`locales/*/core-sieve.ftl`):
//! why a rule runs on the client ([`RulePlacement::Client`]) or cannot run
//! on the server at all ([`RulePlacement::NotExpressible`]), and ManageSieve
//! errors ([`crate::SieveError::localized`]).
//!
//! Arguments: `position` is a condition's place in the rule's `when` tree
//! (1-based child indexes joined with `.`, `"1"` = the root), `action` the
//! 1-based index of an action in `then`.
//!
//! [`RulePlacement::Client`]: nail_api::sieve::RulePlacement::Client
//! [`RulePlacement::NotExpressible`]: nail_api::sieve::RulePlacement::NotExpressible

use nail_api::localized::LocalizedMessage;

// --- Not expressible in Sieve ------------------------------------------------

/// A search-language (`query`) condition.
pub const QUERY: &str = "sieve-reason-query";
/// An attachment condition.
pub const ATTACHMENT: &str = "sieve-reason-attachment";
/// A message-age condition (relative to "now", not to delivery).
pub const AGE: &str = "sieve-reason-age";
/// A system-flag condition (flags do not exist before delivery).
pub const FLAG: &str = "sieve-reason-flag";
/// A tag condition (keywords do not exist before delivery).
pub const TAG: &str = "sieve-reason-tag";
/// A folder condition other than the Inbox.
pub const FOLDER: &str = "sieve-reason-folder";
/// A regular expression that uses features POSIX regular expressions lack.
pub const REGEX: &str = "sieve-reason-regex";
/// An invalid condition (bad regular expression or date).
pub const INVALID: &str = "sieve-reason-invalid";
/// A header name Sieve cannot test (empty, spaces, colon).
pub const HEADER_NAME: &str = "sieve-reason-header-name";
/// A tag that is not a valid IMAP keyword.
pub const KEYWORD: &str = "sieve-reason-keyword";
/// A "forward as a new message" action.
pub const FORWARD: &str = "sieve-reason-forward";
/// A "reply with a template" action.
pub const REPLY: &str = "sieve-reason-reply";
/// A notification action (notifications are shown by Nail).
pub const NOTIFY: &str = "sieve-reason-notify";
/// A move or copy into another account.
pub const OTHER_ACCOUNT: &str = "sieve-reason-other-account";
/// A redirect to something that is not an address.
pub const ADDRESS: &str = "sieve-reason-address";

// --- Expressible, but run by Nail ---------------------------------------------

/// The rule is switched off.
pub const DISABLED: &str = "sieve-reason-disabled";
/// The rule also runs on send, on a schedule or by age.
pub const TRIGGER: &str = "sieve-reason-trigger";
/// The rule does not run on arrival.
pub const NO_ARRIVAL: &str = "sieve-reason-no-arrival";
/// The rule is limited to folders other than the Inbox.
pub const SCOPE: &str = "sieve-reason-scope";
/// The server lacks a Sieve extension (`extension`).
pub const EXTENSION: &str = "sieve-reason-extension";
/// A target folder Nail cannot name on the server yet (`folder`).
pub const FOLDER_UNKNOWN: &str = "sieve-reason-folder-unknown";
/// More redirects than the server allows (`count`, `max`).
pub const REDIRECTS: &str = "sieve-reason-redirects";
/// ManageSieve is switched off for the account (set by the core).
pub const SIEVE_OFF: &str = "sieve-reason-sieve-off";
/// The server has no ManageSieve service (set by the core).
pub const UNAVAILABLE: &str = "sieve-reason-unavailable";
/// The script with this rule is not uploaded yet (set by the core).
pub const PENDING: &str = "sieve-reason-pending";

// --- ManageSieve errors ---------------------------------------------------------

/// Connecting failed (`host`, `port`).
pub const ERROR_CONNECT: &str = "sieve-error-connect";
/// The TLS handshake failed (`host`).
pub const ERROR_TLS: &str = "sieve-error-tls";
/// The server does not offer STARTTLS.
pub const ERROR_STARTTLS: &str = "sieve-error-starttls";
/// The server requires encryption first.
pub const ERROR_ENCRYPTION: &str = "sieve-error-encryption";
/// The server rejected the login.
pub const ERROR_AUTH: &str = "sieve-error-auth";
/// No common login method (`mechanisms`).
pub const ERROR_MECHANISM: &str = "sieve-error-mechanism";
/// The server is not a ManageSieve server (no `SIEVE` capability).
pub const ERROR_UNAVAILABLE: &str = "sieve-error-unavailable";
/// The server rejected the script (`detail`: the server's complaint).
pub const ERROR_SCRIPT: &str = "sieve-error-script";
/// Over the server's quota (`detail`).
pub const ERROR_QUOTA: &str = "sieve-error-quota";
/// The server refused a command (`command`, `detail`).
pub const ERROR_REFUSED: &str = "sieve-error-refused";
/// The server asks to try again later (`detail`).
pub const ERROR_TRY_LATER: &str = "sieve-error-try-later";
/// A script with Nail's name exists but was not written by Nail
/// (`script`).
pub const ERROR_NOT_MANAGED: &str = "sieve-error-not-managed";
/// The server did not answer in time (`seconds`).
pub const ERROR_TIMEOUT: &str = "sieve-error-timeout";
/// The connection broke or the server sent something unexpected.
pub const ERROR_PROTOCOL: &str = "sieve-error-protocol";

/// Every id above (the catalog test checks `core-sieve.ftl` has them all).
pub const ALL: &[&str] = &[
    QUERY,
    ATTACHMENT,
    AGE,
    FLAG,
    TAG,
    FOLDER,
    REGEX,
    INVALID,
    HEADER_NAME,
    KEYWORD,
    FORWARD,
    REPLY,
    NOTIFY,
    OTHER_ACCOUNT,
    ADDRESS,
    DISABLED,
    TRIGGER,
    NO_ARRIVAL,
    SCOPE,
    EXTENSION,
    FOLDER_UNKNOWN,
    REDIRECTS,
    SIEVE_OFF,
    UNAVAILABLE,
    PENDING,
    ERROR_CONNECT,
    ERROR_TLS,
    ERROR_STARTTLS,
    ERROR_ENCRYPTION,
    ERROR_AUTH,
    ERROR_MECHANISM,
    ERROR_UNAVAILABLE,
    ERROR_SCRIPT,
    ERROR_QUOTA,
    ERROR_REFUSED,
    ERROR_TRY_LATER,
    ERROR_NOT_MANAGED,
    ERROR_TIMEOUT,
    ERROR_PROTOCOL,
];

/// A reason about the condition at `position` / the action at a 0-based
/// index (one definition with the other rule placements).
pub(crate) use nail_api::sieve::{
    reason_at_action as at_action, reason_at_condition as at_condition,
};

/// The server lacks `extension`.
pub(crate) fn extension(extension: &str) -> LocalizedMessage {
    LocalizedMessage::new(EXTENSION).arg("extension", extension)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::localized::MessageArg;

    #[test]
    fn ids_are_valid_and_unique() {
        assert_eq!(
            nail_api::localized::bad_ids(ALL, &["sieve-reason-", "sieve-error-"]),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn extension_reasons_name_the_extension() {
        assert_eq!(
            extension("body").args["extension"],
            MessageArg::from("body")
        );
    }

    /// Every id has an English text, and the German catalog is the same
    /// (empty) file (ADR 0043).
    #[test]
    fn catalog_has_every_id() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../locales");
        let en = std::fs::read_to_string(root.join("en/core-sieve.ftl")).unwrap();
        assert_eq!(
            nail_api::localized::missing_ids(&en, ALL),
            Vec::<&str>::new(),
            "missing in locales/en/core-sieve.ftl"
        );
        let defined = nail_api::localized::catalog_ids(&en);
        for id in &defined {
            assert!(ALL.contains(id), "{id} in core-sieve.ftl is not used");
        }
        assert!(root.join("de/core-sieve.ftl").exists());
    }
}
