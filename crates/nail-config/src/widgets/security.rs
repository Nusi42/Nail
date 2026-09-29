//! Widgets of M2a WP34 (security UI, ADR 0029): key and certificate
//! manager, per-identity crypto settings, composer sign / encrypt toggles
//! with recipient key status, the message security and auth-results
//! badges, reader warnings, encryption-at-rest settings.
//!
//! Only WP34 edits this file during wave M2a. Register every widget kind
//! the feature slot `ui/src/features/security/` renders.

use super::WidgetSpec;
#[allow(unused_imports)]
use super::{OptionType, future_widget, widget};

/// Widgets of this feature (none yet).
pub(crate) const WIDGETS: &[WidgetSpec] = &[];

#[cfg(test)]
mod tests {
    #[test]
    fn security_widgets_are_unique_and_kebab_case() {
        for spec in super::WIDGETS {
            assert!(crate::kdl_util::is_kebab(spec.kind), "{}", spec.kind);
            assert!(crate::widgets::find(spec.kind).is_some());
        }
    }
}
