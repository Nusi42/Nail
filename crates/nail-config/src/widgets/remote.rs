//! Widgets of M2b WP43 (remote UI and privacy UI, ADR 0036): remote-UI
//! settings, pairing, sessions and devices, the security audit view and
//! the reader's "load remote content" bar. Rendered by
//! `ui/src/features/remote/`.
//!
//! Empty after WP40: WP43 adds its kinds here (and the same kinds in
//! `ui/src/features/remote/index.ts`).

use super::WidgetSpec;
#[allow(unused_imports)]
use super::{OptionType, future_widget, widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[];

#[cfg(test)]
mod tests {
    #[test]
    fn remote_widgets_are_unique_and_kebab_case() {
        for spec in super::WIDGETS {
            assert!(crate::kdl_util::is_kebab(spec.kind), "{}", spec.kind);
            assert!(spec.kind.starts_with("remote-"), "{}", spec.kind);
            assert!(crate::widgets::find(spec.kind).is_some());
        }
    }
}
