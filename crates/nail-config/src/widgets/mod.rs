//! The widget registry of `layout.kdl`: every widget kind with its typed
//! options.
//!
//! The table is split by feature so the M1b packages never edit the same
//! file (ADR 0022): [`core`] holds the built-in widgets, [`composer`],
//! [`rules`], [`mcp`] and [`customization`] the widgets of WP21–WP24,
//! [`security`] those of M2a WP34 (ADR 0029), [`remote`] those of M2b
//! WP43 (ADR 0036). The
//! React widget registry (`ui/src/features/*/index.ts`) must register a
//! component for every kind listed here.

mod composer;
mod core;
mod customization;
mod mcp;
mod remote;
mod rules;
mod security;

/// Type of a widget option.
#[derive(Debug, Clone, Copy)]
pub(crate) enum OptionType {
    /// `#true` / `#false`.
    Bool,
    /// A non-negative integer.
    UInt,
    /// Any string.
    Str,
    /// One of the listed strings.
    OneOf(&'static [&'static str]),
}

/// A widget kind known to the layout.
pub(crate) struct WidgetSpec {
    pub(crate) kind: &'static str,
    /// `None`: the widget is not implemented yet; any options are accepted.
    pub(crate) options: Option<&'static [(&'static str, OptionType)]>,
    /// Required options.
    pub(crate) required: &'static [&'static str],
    /// Whether the widget takes `row "<template>"` children.
    pub(crate) rows: bool,
}

/// A widget with typed options.
pub(crate) const fn widget(
    kind: &'static str,
    options: &'static [(&'static str, OptionType)],
    required: &'static [&'static str],
    rows: bool,
) -> WidgetSpec {
    WidgetSpec {
        kind,
        options: Some(options),
        required,
        rows,
    }
}

/// A widget that is not implemented yet (any options are accepted).
pub(crate) const fn future_widget(kind: &'static str, rows: bool) -> WidgetSpec {
    WidgetSpec {
        kind,
        options: None,
        required: &[],
        rows,
    }
}

/// All tables, in the order their kinds are listed in error messages.
const TABLES: &[&[WidgetSpec]] = &[
    core::WIDGETS,
    composer::WIDGETS,
    rules::WIDGETS,
    mcp::WIDGETS,
    customization::WIDGETS,
    security::WIDGETS,
    remote::WIDGETS,
];

fn all() -> impl Iterator<Item = &'static WidgetSpec> {
    TABLES.iter().flat_map(|table| table.iter())
}

/// The spec of a widget kind.
pub(crate) fn find(kind: &str) -> Option<&'static WidgetSpec> {
    all().find(|w| w.kind == kind)
}

/// Every widget kind.
pub(crate) fn kinds() -> impl Iterator<Item = &'static str> {
    all().map(|w| w.kind)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn kinds_are_unique_kebab_case_and_found() {
        let mut seen = HashSet::new();
        for kind in kinds() {
            assert!(seen.insert(kind), "widget `{kind}` is registered twice");
            assert!(
                crate::kdl_util::is_kebab(kind),
                "`{kind}` is not kebab-case"
            );
            assert!(find(kind).is_some());
        }
        assert!(find("message-list").is_some_and(|w| w.rows));
        assert!(find("toolbar").is_some_and(|w| w.required == ["toolbar"]));
        assert!(find("calendar").is_some_and(|w| w.options.is_none()));
        assert!(find("nope").is_none());
    }
}
