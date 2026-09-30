//! Widgets of M2c WP54 (sync policies, ADR 0043): a folder's sync settings.
//! Rendered by `ui/src/features/sync/`.
//!
//! Slot created empty by WP50; filled by WP54 (the kinds have no options).

use super::{WidgetSpec, widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // Settings: the sync policy (bodies, sync, IDLE) of each folder.
    widget("sync-folder-policies", &[], &[], false),
    // Reader: "Only the headers are stored" with "Download full message".
    widget("sync-message-body", &[], &[], false),
];

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

    #[test]
    fn sync_widgets_parse_in_a_layout() {
        let text = "layout \"x\" extends=\"default\" {
    window \"y\" {
        stack {
            sync-folder-policies
            sync-message-body
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
    }
}
