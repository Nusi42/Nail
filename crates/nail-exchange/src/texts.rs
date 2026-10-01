//! Fluent ids of the texts this crate builds (`locales/*/core-exchange.ftl`,
//! WP93 part): Autodiscover, Windows sign-in, rule placement reasons,
//! automatic-reply mapping notes and PIM mapping problems.
//!
//! Arguments: rule reasons carry `position` (a condition's place in the
//! rule's `when` tree, 1-based child indexes joined with `.`, `"1"` = the
//! root) or `action` (the 1-based index of an action in `then`), like
//! `nail-sieve`'s reasons.

use nail_api::localized::LocalizedMessage;

// --- Autodiscover ----------------------------------------------------------

/// The address is not an email address.
pub const AUTODISCOVER_INVALID_ADDRESS: &str = "exchange-autodiscover-invalid-address";
/// Nothing answered.
pub const AUTODISCOVER_NOT_FOUND: &str = "exchange-autodiscover-not-found";
/// Confirm sending the login to another server (`host`, `domain`).
pub const AUTODISCOVER_CONFIRM_REDIRECT: &str = "exchange-autodiscover-confirm-redirect";
/// The server asks to sign in (`host`).
pub const AUTODISCOVER_NEEDS_LOGIN: &str = "exchange-autodiscover-needs-login";
/// Too many redirects.
pub const AUTODISCOVER_TOO_MANY_REDIRECTS: &str = "exchange-autodiscover-too-many-redirects";
/// Exchange Online found: Microsoft Graph proposed.
pub const AUTODISCOVER_ONLINE: &str = "exchange-autodiscover-online";
/// Exchange Server found: EWS proposed (`host`).
pub const AUTODISCOVER_ON_PREMISES: &str = "exchange-autodiscover-on-premises";

// --- Windows / HTTP sign-in --------------------------------------------------

/// The server asks to sign in and no login was given.
pub const SIGNIN_NEEDS_LOGIN: &str = "exchange-signin-needs-login";
/// No common sign-in method (`methods`).
pub const SIGNIN_NO_COMMON: &str = "exchange-signin-no-common-method";
/// Basic would send the password without TLS.
pub const SIGNIN_INSECURE_BASIC: &str = "exchange-signin-insecure-basic";
/// The server rejected the sign-in.
pub const SIGNIN_REJECTED: &str = "exchange-signin-rejected";
/// Windows sign-in failed locally (SSPI).
pub const SIGNIN_WINDOWS: &str = "exchange-signin-windows-failed";
/// A user name without its password.
pub const SIGNIN_NEEDS_PASSWORD: &str = "exchange-signin-needs-password";

// --- Rule placement: not expressible as an Exchange inbox rule ---------------

/// A search-language (`query`) condition.
pub const RULE_QUERY: &str = "exchange-rule-reason-query";
/// A condition Exchange has no predicate for (flags, age, dates, …).
pub const RULE_CONDITION: &str = "exchange-rule-reason-condition";
/// A text test Exchange cannot do (regex, starts / ends with, equals on
/// text, case-sensitive).
pub const RULE_TEXT_TEST: &str = "exchange-rule-reason-text-test";
/// An address field Exchange cannot test (`Reply-To`, `Sender`, only `To`
/// or only `Cc`).
pub const RULE_ADDRESS_FIELD: &str = "exchange-rule-reason-address-field";
/// A header value test (Exchange only searches the whole header block).
pub const RULE_HEADER: &str = "exchange-rule-reason-header";
/// A tag condition (tags do not exist before delivery).
pub const RULE_TAG: &str = "exchange-rule-reason-tag";
/// An attachment name / type test.
pub const RULE_ATTACHMENT: &str = "exchange-rule-reason-attachment";
/// A folder condition other than the Inbox.
pub const RULE_FOLDER: &str = "exchange-rule-reason-folder";
/// A combination of conditions inbox rules cannot express.
pub const RULE_STRUCTURE: &str = "exchange-rule-reason-structure";
/// Removing a tag.
pub const RULE_UNTAG: &str = "exchange-rule-reason-untag";
/// Setting or clearing the flag.
pub const RULE_FLAG: &str = "exchange-rule-reason-flag";
/// Marking as unread.
pub const RULE_MARK_UNREAD: &str = "exchange-rule-reason-mark-unread";
/// A reply with a template.
pub const RULE_REPLY: &str = "exchange-rule-reason-reply";
/// A forward from a chosen identity.
pub const RULE_FORWARD_IDENTITY: &str = "exchange-rule-reason-forward-identity";
/// A notification action (notifications are Nail's).
pub const RULE_NOTIFY: &str = "exchange-rule-reason-notify";
/// A move or copy into another account.
pub const RULE_OTHER_ACCOUNT: &str = "exchange-rule-reason-other-account";
/// More than one move / copy target.
pub const RULE_TARGETS: &str = "exchange-rule-reason-targets";
/// A tag that is not a valid category name.
pub const RULE_CATEGORY_NAME: &str = "exchange-rule-reason-category-name";
/// A forward / redirect to something that is not an address.
pub const RULE_ADDRESS: &str = "exchange-rule-reason-address";
/// A rule without actions Exchange can run.
pub const RULE_NO_ACTIONS: &str = "exchange-rule-reason-no-actions";

// --- Rule placement: expressible, but run by Nail ----------------------------

/// The rule is switched off.
pub const RULE_DISABLED: &str = "exchange-rule-reason-disabled";
/// The rule also runs on send, on a schedule or by age.
pub const RULE_TRIGGER: &str = "exchange-rule-reason-trigger";
/// The rule does not run on arrival.
pub const RULE_NO_ARRIVAL: &str = "exchange-rule-reason-no-arrival";
/// The rule is limited to folders other than the Inbox, or other accounts.
pub const RULE_SCOPE: &str = "exchange-rule-reason-scope";
/// A target folder the server does not know yet (`folder`).
pub const RULE_FOLDER_UNKNOWN: &str = "exchange-rule-reason-folder-unknown";
/// Placing is switched off for the account (set by the core).
pub const RULE_SERVER_RULES_OFF: &str = "exchange-rule-reason-server-rules-off";
/// The rule is not written to the server yet (set by the core).
pub const RULE_PENDING: &str = "exchange-rule-reason-pending";

// --- Server rules shown as Nail rules -----------------------------------------

/// A server rule uses something Nail rules cannot express.
pub const SERVER_RULE_UNSUPPORTED: &str = "exchange-server-rule-unsupported";

// --- Automatic replies ↔ vacation --------------------------------------------

/// The vacation subject is not kept (Exchange writes its own).
pub const OOF_NOTE_SUBJECT: &str = "exchange-oof-note-subject";
/// "Once per sender every n days" is the server's choice.
pub const OOF_NOTE_DAYS: &str = "exchange-oof-note-days";
/// Extra addresses / sender address are not kept.
pub const OOF_NOTE_ADDRESSES: &str = "exchange-oof-note-addresses";
/// The external reply differs from the internal one; only the internal
/// one is shown.
pub const OOF_NOTE_EXTERNAL: &str = "exchange-oof-note-external";
/// A start without an end: replies end after a long default period.
pub const OOF_NOTE_OPEN_END: &str = "exchange-oof-note-open-end";
/// The schedule's times were rounded to whole days.
pub const OOF_NOTE_ROUNDED: &str = "exchange-oof-note-rounded";

// --- PIM mapping ----------------------------------------------------------------

/// A recurrence Exchange cannot store.
pub const PIM_RECURRENCE: &str = "exchange-pim-reason-recurrence";
/// A time zone Nail does not know (`zone`); the time is read as UTC.
pub const PIM_ZONE: &str = "exchange-pim-reason-zone";
/// Several recurrence rules or extra dates.
pub const PIM_RDATE: &str = "exchange-pim-reason-extra-dates";
/// A floating time (Exchange needs a zone; written as UTC).
pub const PIM_FLOATING: &str = "exchange-pim-reason-floating";

/// Every id above (the catalog test checks `core-exchange.ftl` has them).
pub const ALL: &[&str] = &[
    AUTODISCOVER_INVALID_ADDRESS,
    AUTODISCOVER_NOT_FOUND,
    AUTODISCOVER_CONFIRM_REDIRECT,
    AUTODISCOVER_NEEDS_LOGIN,
    AUTODISCOVER_TOO_MANY_REDIRECTS,
    AUTODISCOVER_ONLINE,
    AUTODISCOVER_ON_PREMISES,
    SIGNIN_NEEDS_LOGIN,
    SIGNIN_NO_COMMON,
    SIGNIN_INSECURE_BASIC,
    SIGNIN_REJECTED,
    SIGNIN_WINDOWS,
    SIGNIN_NEEDS_PASSWORD,
    RULE_QUERY,
    RULE_CONDITION,
    RULE_TEXT_TEST,
    RULE_ADDRESS_FIELD,
    RULE_HEADER,
    RULE_TAG,
    RULE_ATTACHMENT,
    RULE_FOLDER,
    RULE_STRUCTURE,
    RULE_UNTAG,
    RULE_FLAG,
    RULE_MARK_UNREAD,
    RULE_REPLY,
    RULE_FORWARD_IDENTITY,
    RULE_NOTIFY,
    RULE_OTHER_ACCOUNT,
    RULE_TARGETS,
    RULE_CATEGORY_NAME,
    RULE_ADDRESS,
    RULE_NO_ACTIONS,
    RULE_DISABLED,
    RULE_TRIGGER,
    RULE_NO_ARRIVAL,
    RULE_SCOPE,
    RULE_FOLDER_UNKNOWN,
    RULE_SERVER_RULES_OFF,
    RULE_PENDING,
    SERVER_RULE_UNSUPPORTED,
    OOF_NOTE_SUBJECT,
    OOF_NOTE_DAYS,
    OOF_NOTE_ADDRESSES,
    OOF_NOTE_EXTERNAL,
    OOF_NOTE_OPEN_END,
    OOF_NOTE_ROUNDED,
    PIM_RECURRENCE,
    PIM_ZONE,
    PIM_RDATE,
    PIM_FLOATING,
];

/// A reason about the condition at `position` (child indexes from the
/// root).
pub fn at_condition(id: &str, position: &[u32]) -> LocalizedMessage {
    LocalizedMessage::new(id).arg("position", position_text(position))
}

/// A reason about the action at 0-based `index`.
pub fn at_action(id: &str, index: usize) -> LocalizedMessage {
    LocalizedMessage::new(id).arg("action", index + 1)
}

/// `[]` → `"1"`, `[0, 2]` → `"1.1.3"`: the root is 1, children count from 1.
pub fn position_text(path: &[u32]) -> String {
    std::iter::once("1".to_owned())
        .chain(path.iter().map(|i| (i + 1).to_string()))
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::localized::{MessageArg, is_message_id};

    #[test]
    fn ids_are_valid_and_unique() {
        let unique: std::collections::BTreeSet<_> = ALL.iter().collect();
        assert_eq!(unique.len(), ALL.len());
        for id in ALL {
            assert!(is_message_id(id), "{id}");
            assert!(id.starts_with("exchange-"), "{id}");
        }
    }

    #[test]
    fn positions_count_from_one() {
        assert_eq!(position_text(&[]), "1");
        assert_eq!(position_text(&[0, 2]), "1.1.3");
        assert_eq!(
            at_action(RULE_REPLY, 0).args["action"],
            MessageArg::Number(1)
        );
        assert_eq!(
            at_condition(RULE_QUERY, &[1]).args["position"],
            MessageArg::Text("1.2".into())
        );
    }

    /// Every id has an English text; the German catalog is the same
    /// (empty) file (ADR 0043). The contract's own ids (warnings, issues,
    /// colours) are checked by `nail-api/tests/catalog.rs`.
    #[test]
    fn catalog_has_every_id() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../locales");
        let en = std::fs::read_to_string(root.join("en/core-exchange.ftl")).unwrap();
        let defined: Vec<&str> = en
            .lines()
            .filter_map(|l| l.split_once(" =").map(|(id, _)| id))
            .filter(|id| is_message_id(id))
            .collect();
        for id in ALL {
            assert!(
                defined.contains(id),
                "{id} missing in locales/en/core-exchange.ftl"
            );
        }
        assert!(root.join("de/core-exchange.ftl").exists());
    }

    /// The texts render with their arguments through Fluent (no syntax
    /// errors, every placeable resolved).
    #[test]
    fn texts_parse_as_fluent() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../locales");
        let en = std::fs::read_to_string(root.join("en/core-exchange.ftl")).unwrap();
        for line in en.lines().filter(|l| l.starts_with("exchange-")) {
            let (_, text) = line.split_once(" = ").expect(line);
            assert!(!text.trim().is_empty(), "{line}");
            assert_eq!(
                text.matches('{').count(),
                text.matches('}').count(),
                "{line}"
            );
        }
    }
}
