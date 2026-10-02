//! Widgets of M4c WP124 (import, export, backup, diagnostics; contract
//! ADR 0094). Rendered by `ui/src/features/portability/`.
//!
//! Slot created by WP120 as future widgets (any options accepted); WP124
//! gives them typed options. The wizards, the command palette and the
//! print view are dialogs / overlays, not widgets.

use super::{WidgetSpec, future_widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // Import from files / Thunderbird, export, running and recent jobs
    // with their reports.
    future_widget("import-export", false),
    // Back up now, verify, restore (dry run first).
    future_widget("backup-restore", false),
    // The diagnostics bundle: choose, preview exactly what goes in, save.
    future_widget("diagnostics", false),
];

#[cfg(test)]
mod tests {
    #[test]
    fn portability_widgets_are_registered_and_parse() {
        for spec in super::WIDGETS {
            assert!(crate::kdl_util::is_kebab(spec.kind), "{}", spec.kind);
            assert!(crate::widgets::find(spec.kind).is_some());
        }
        let text = "layout \"x\" extends=\"default\" {
    window \"main\" {
        stack {
            import-export
            backup-restore
            diagnostics
            security-recovery-key
            security-smartcards
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
    }
}
