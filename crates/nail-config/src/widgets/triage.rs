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
const PREFIXES: &[&str] = &["junk-", "snooze-", "notification-"];

#[cfg(test)]
mod tests {
    #[test]
    fn triage_widgets_are_unique_and_kebab_case() {
        for spec in super::WIDGETS {
            assert!(crate::kdl_util::is_kebab(spec.kind), "{}", spec.kind);
            assert!(
                super::PREFIXES.iter().any(|p| spec.kind.starts_with(p)),
                "{}",
                spec.kind
            );
            assert!(crate::widgets::find(spec.kind).is_some());
        }
    }

    #[test]
    fn triage_widgets_parse_in_a_layout() {
        let text = "layout \"x\" extends=\"default\" {
    window \"t\" {
        stack {
            junk-settings
            junk-verdict
            snooze-list
            notification-settings
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
    }
}
