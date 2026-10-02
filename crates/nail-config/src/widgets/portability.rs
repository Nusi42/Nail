//! Widgets of M4c WP124 (import, export, backup, diagnostics; contract
//! ADR 0094). Rendered by `ui/src/features/portability/`.
//!
//! Options (WP124 / WP125): `import-export` and `backup-restore` take
//! `show-jobs` (default #true: the list of recent jobs); `diagnostics`
//! takes none. The wizards, the command palette and the print view are
//! dialogs / overlays, not widgets.

use super::OptionType::Bool;
use super::{WidgetSpec, widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // Import from files / Thunderbird, export, running and recent jobs
    // with their reports.
    widget("import-export", &[("show-jobs", Bool)], &[], false),
    // Back up now, verify, restore (dry run first).
    widget("backup-restore", &[("show-jobs", Bool)], &[], false),
    // The diagnostics bundle: choose, preview exactly what goes in, save.
    widget("diagnostics", &[], &[], false),
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
            import-export show-jobs=#false
            backup-restore show-jobs=#true
            diagnostics
            security-recovery-key
            security-smartcards
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
    }

    #[test]
    fn unknown_options_are_refused() {
        for widget in [
            "import-export",
            "backup-restore",
            "diagnostics",
            "security-recovery-key",
            "security-smartcards",
        ] {
            let text = format!(
                "layout \"x\" extends=\"default\" {{
    window \"main\" {{
        stack {{
            {widget} show-everything=#true
        }}
    }}
}}
"
            );
            assert!(
                crate::parse_layout(&text, "layout.kdl").is_err(),
                "{widget}"
            );
        }
    }
}
