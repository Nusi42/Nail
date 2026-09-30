//! Widgets of M2c WP54 (server-side rules and vacation replies, ADR 0043): rule
//! placement per rule, the ManageSieve state and the vacation reply editor.
//! Rendered by `ui/src/features/sieve/`.
//!
//! Slot created empty by WP50; filled by WP54 (the kinds have no options).

use super::{WidgetSpec, widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // Settings: ManageSieve per account, script state, where each rule runs.
    widget("sieve-status", &[], &[], false),
    // Settings: the vacation (out-of-office) reply of an account.
    widget("vacation-editor", &[], &[], false),
];

/// Prefixes every widget kind of this feature starts with.
#[cfg(test)]
const PREFIXES: &[&str] = &["sieve-", "vacation-"];

#[cfg(test)]
mod tests {
    #[test]
    fn sieve_widgets_are_unique_and_kebab_case() {
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
    fn sieve_widgets_parse_in_a_layout() {
        let text = "layout \"x\" extends=\"default\" {
    window \"s\" {
        stack {
            sieve-status
            vacation-editor
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
        let bad = text.replace("vacation-editor", "vacation-editor days=3");
        assert!(crate::parse_layout(&bad, "layout.kdl").is_err());
    }
}
