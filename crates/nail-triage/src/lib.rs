//! Junk classifier, snooze scheduler and notification policy (plan M2c
//! WP52; contract ADR 0043, design ADR 0045). **Pure logic**: no I/O,
//! no store and no clock of its own. Persistence goes through traits the
//! core implements on its store ([`classifier::JunkStore`],
//! [`snooze::SnoozeStore`]), time through [`clock::Clock`] (tests use
//! [`clock::ManualClock`]), and every write is returned as a
//! [`ops::TriageOp`] for the core's operation queue.
//!
//! * [`tokenize`] — tokens of selected headers, decoded text and links.
//! * [`classifier`] — per-account Bayesian classifier (Robinson / Fisher),
//!   thresholds with an "unsure" band, training with undo, arrival and
//!   "Mark as junk" plans that never delete.
//! * [`snooze`] — wake times in a [`wheel::TimerWheel`], the `$Snoozed`
//!   keyword and hidden `Snoozed` folder as planned writes, restart
//!   catch-up, rebinding by `Message-ID`.
//! * [`notify`] — the notification policy (settings, overrides, rules,
//!   quiet hours across midnight and time zones) and burst batching.
//! * [`localtime`] — local wall-clock times across DST changes.
//! * [`reminders`] (M3a WP73) — calendar and task alarms: planning across
//!   zones and DST, the reminder scheduler with restart catch-up, snooze
//!   and dismiss as RFC 9074 writes, reminder notifications.

#![warn(missing_docs)]

pub mod classifier;
pub mod clock;
pub mod localtime;
pub mod notify;
pub mod ops;
pub mod reminders;
pub mod snooze;
pub mod tokenize;
pub mod wheel;

pub use classifier::{
    Classification, ClassifierParams, JunkClass, JunkStore, MemoryJunkStore, TrainingReceipt,
};
pub use clock::{Clock, ManualClock, SystemClock};
pub use notify::{Arrival, Decision, NotificationBatcher};
pub use ops::{FolderTarget, TriageOp};
pub use reminders::{
    MemoryReminderStore, Occurrence, OccurrenceTime, PlannedAlarm, ReminderEntry, ReminderItem,
    ReminderOp, ReminderPolicy, ReminderScheduler, ReminderStore,
};
pub use snooze::{MemorySnoozeStore, SnoozeEntry, SnoozeScheduler, SnoozeStore};
pub use tokenize::{TokenSource, Tokenizer};

/// Every Fluent message id this crate produces (all in
/// `locales/en/core-triage.ftl`; checked by `tests/catalog.rs`).
pub const FLUENT_IDS: &[&str] = &[
    "notification-new-mail-title",
    "notification-new-mail-body",
    "notification-snooze-woke-title",
    "notification-test-title",
    "notification-test-body",
    "snooze-error-not-in-future",
    "snooze-error-too-far",
    "snooze-error-store",
];

/// The `keys` that `map` holds, in their order (the schedulers ignore
/// unknown ids).
pub(crate) fn known_keys<K, V>(map: &std::collections::HashMap<K, V>, keys: &[K]) -> Vec<K>
where
    K: Clone + Eq + std::hash::Hash,
{
    keys.iter()
        .filter(|k| map.contains_key(*k))
        .cloned()
        .collect()
}

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "nail-triage");
    }
}
