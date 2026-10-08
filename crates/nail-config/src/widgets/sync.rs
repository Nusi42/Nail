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
mod tests {
    use crate::widgets::testing::{assert_prefixed, parses};

    #[test]
    fn sync_widgets_parse() {
        assert_prefixed(super::WIDGETS, &["sync-"]);
        assert!(parses(&["sync-folder-policies", "sync-message-body"]));
    }
}
