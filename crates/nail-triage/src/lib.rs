//! Junk classifier, snooze scheduler and notification policy (plan M2c
//! WP52; contract ADR 0043). **Pure logic**: no I/O, no store, no clock of
//! its own (callers pass "now"), so everything is unit-testable.
//!
//! **Stub (WP50).** WP52 fills this crate against `nail_api::triage`.
//! Expected shape:
//!
//! * `classifier` — a tokenizer over headers (`From`, `Subject`,
//!   `List-Id`, …, prefixed by header name) and body text, and a Bayesian
//!   (Robinson / Fisher) classifier over token counts `(junk, ham)` per
//!   token. The counts come from the store (`Store::junk_token_counts`;
//!   `Store::train_junk` hashes the tokens); the classifier returns a
//!   score 0–100 and a verdict against `JunkSettings::threshold`, and
//!   nothing before `JunkSettings::min_training` messages of each class.
//! * `snooze` — the scheduler: next wake time from the snooze records,
//!   which records are due, the wake folder fallback (the folder is gone →
//!   Inbox), rebinding by `Message-ID` after a resync.
//! * `notify` — the notification policy: given `NotificationSettings`,
//!   the arriving messages (account, folder, role), the rules' `notify`
//!   decisions and the local time, which `MailNotification`s to request
//!   (grouped per folder, preview only when allowed, quiet hours across
//!   midnight and per weekday).

#![warn(missing_docs)]

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "nail-triage");
    }
}
