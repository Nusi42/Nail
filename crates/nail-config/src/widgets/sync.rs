//! Widgets of M2c WP54 (sync policies, ADR 0043): a folder's sync settings.
//! Rendered by `ui/src/features/sync/`.
//!
//! Slot created empty by WP50; WP54 adds its widget kinds here.

use super::WidgetSpec;
#[allow(unused_imports)]
use super::{OptionType, future_widget, widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[];

/// Prefixes every widget kind of this feature starts with.
#[cfg(test)]
const PREFIXES: &[&str] = &["sync-"];

#[cfg(test)]
mod tests {
    #[test]
    fn sync_widgets_are_unique_and_kebab_case() {
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
