//! Widgets of M2c WP54 (junk, snooze, notifications, ADR 0043): junk settings and
//! status, the snoozed list, notification settings. Rendered by
//! `ui/src/features/triage/`.
//!
//! Slot created empty by WP50; filled by WP54 (the kinds have no options).

use super::{WidgetSpec, widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // Settings: the junk classifier of an account, its training status.
    widget("junk-settings", &[], &[], false),
    // Reader: the junk verdict of the shown message, with "Not junk".
    widget("junk-verdict", &[], &[], false),
    // The snoozed messages (window "snoozed").
    widget("snooze-list", &[], &[], false),
    // Settings: new-mail notifications, overrides, quiet hours.
    widget("notification-settings", &[], &[], false),
];

/// Prefixes every widget kind of this feature starts with.
#[cfg(test)]
mod tests {
    use crate::widgets::testing::{assert_prefixed, parses};

    #[test]
    fn triage_widgets_parse() {
        assert_prefixed(super::WIDGETS, &["junk-", "snooze-", "notification-"]);
        assert!(parses(&[
            "junk-settings",
            "junk-verdict",
            "snooze-list",
            "notification-settings",
        ]));
    }
}
