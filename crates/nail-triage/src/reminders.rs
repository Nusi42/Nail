//! The reminder scheduler (M3a WP73; contract ADR 0058): when calendar and
//! task alarms fire, across time zones and restarts, with snooze and
//! dismiss — on the same injectable clock ([`crate::clock::Clock`]) and
//! timer wheel ([`crate::wheel::TimerWheel`]) as snooze. Pure logic like
//! the rest of the crate: persistence through a trait the core implements
//! on the store (`pim_reminders`), notifications as
//! `nail_api::triage::NotificationKind::Reminder` with texts from
//! `locales/en/core-pim.ftl`.
//!
//! Slot created by WP70; WP73 fills it.

/// The notification kind reminders are shown with.
pub const NOTIFICATION_KIND: nail_api::triage::NotificationKind =
    nail_api::triage::NotificationKind::Reminder;

#[cfg(test)]
mod tests {
    #[test]
    fn reminders_notify_as_reminders() {
        assert_eq!(
            serde_json::to_value(super::NOTIFICATION_KIND).unwrap(),
            serde_json::json!("reminder")
        );
    }
}
