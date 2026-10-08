//! Widgets of the M5b shell (contract ADR 0109): the navigation rail and the
//! view host (WP161, `ui/src/features/shell/`), the settings page (WP163,
//! `ui/src/features/settings/`) and the message list header (WP162,
//! `ui/src/widgets/`). The options are listed in
//! `userDocs/en/layout-reference.md`.
//!
//! The rail, the views and the settings page take their content from the
//! layout's `rail`, `views` and `settings-page` sections; `nail-config`
//! refuses these widgets when their section is missing.

use super::OptionType::{Bool, Words};
use super::{WidgetSpec, widget};

/// The filter chips a `list-header` can show (`nail_api::views::ListFilter`).
pub(crate) const LIST_FILTERS: &[&str] = &["all", "unread", "flagged", "attachments"];

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // The navigation rail (`rail { … }`). `show-labels` (default #false):
    // labels under the icons; `show-logo` (default #true): the app mark on
    // top.
    widget(
        nail_api::views::NAV_RAIL_WIDGET,
        &[("show-labels", Bool), ("show-logo", Bool)],
        &[],
        false,
    ),
    // The active view's window tree (`views { … }`).
    widget(nail_api::views::VIEW_HOST_WIDGET, &[], &[], false),
    // The settings page (`settings-page { … }`): grouped navigation,
    // search, the selected section. `search` (default #true).
    widget(
        nail_api::views::SETTINGS_PAGE_WIDGET,
        &[("search", Bool)],
        &[],
        false,
    ),
    // Above the message list: search with the Ctrl+K hint (`search`,
    // `hint`), the folder title with counts (`title`, `counts`), filter
    // chips (`chips`, default "all unread flagged attachments").
    widget(
        nail_api::views::LIST_HEADER_WIDGET,
        &[
            ("search", Bool),
            ("hint", Bool),
            ("title", Bool),
            ("counts", Bool),
            ("chips", Words(LIST_FILTERS)),
        ],
        &[],
        false,
    ),
];

#[cfg(test)]
mod tests {
    use nail_api::views::ListFilter;

    #[test]
    fn list_filters_match_the_contract() {
        let wire: Vec<&str> = ListFilter::ALL.iter().map(|f| f.as_str()).collect();
        assert_eq!(wire, super::LIST_FILTERS);
    }
}
