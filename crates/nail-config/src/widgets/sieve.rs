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
mod tests {
    use crate::widgets::testing::{assert_prefixed, parses};

    #[test]
    fn sieve_widgets_parse() {
        assert_prefixed(super::WIDGETS, &["sieve-", "vacation-"]);
        assert!(parses(&["sieve-status", "vacation-editor"]));
        assert!(!parses(&["vacation-editor days=3"]));
    }
}
