//! Widgets of M2c WP54 (junk, snooze, notifications, ADR 0043): junk settings and
//! status, the snoozed list, notification settings. Rendered by
//! `ui/src/features/triage/`.
//!
//! Slot created empty by WP50; WP54 adds its widget kinds here.

use super::WidgetSpec;
#[allow(unused_imports)]
use super::{OptionType, future_widget, widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[];

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
}
