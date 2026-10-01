//! Widgets of M3c WP94 (Exchange accounts; contract ADR 0072, UI ADR 0076).
//! Rendered by `ui/src/features/exchange/`.
//!
//! None takes `row` children; the options are listed in
//! `userDocs/en/exchange.md#layout-options`. Shared mailboxes appear in the
//! existing folder tree, the room finder in the event editor and the
//! out-of-office editor as a dialog, so they need no widget of their own.

use super::OptionType::Bool;
use super::{WidgetSpec, widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // Focused / Other tabs over the message list of an Exchange inbox.
    widget(
        "focused-tabs",
        &[("show-all", Bool), ("show-switch", Bool)],
        &[],
        false,
    ),
    // A banner while automatic replies are on, with a way to switch them
    // off or edit them.
    widget(
        "automatic-replies-banner",
        &[("only-active-account", Bool)],
        &[],
        false,
    ),
    // The shared and delegate mailboxes of the selected Exchange account.
    widget("exchange-mailboxes", &[], &[], false),
    // The master category list with colours (category views, editing).
    widget(
        "exchange-categories",
        &[("show-counts", Bool), ("show-empty", Bool)],
        &[],
        false,
    ),
    // Where each Nail rule runs on Exchange accounts and the server's
    // inbox rules (Settings).
    widget("exchange-rules", &[], &[], false),
];

#[cfg(test)]
mod tests {
    #[test]
    fn exchange_widgets_are_registered_and_parse() {
        for spec in super::WIDGETS {
            assert!(crate::kdl_util::is_kebab(spec.kind), "{}", spec.kind);
            assert!(crate::widgets::find(spec.kind).is_some());
        }
        let text = "layout \"x\" extends=\"default\" {
    window \"main\" {
        stack {
            automatic-replies-banner only-active-account=#true
            focused-tabs show-all=#false show-switch=#true
            exchange-mailboxes
            exchange-categories show-counts=#true show-empty=#false
            exchange-rules
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
    }

    #[test]
    fn unknown_options_are_refused() {
        let text = "layout \"x\" extends=\"default\" {
    window \"main\" {
        stack {
            focused-tabs show-counts=#true
        }
    }
}
";
        assert!(crate::parse_layout(text, "layout.kdl").is_err());
    }
}
