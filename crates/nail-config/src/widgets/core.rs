//! The built-in widgets of M0 / M1a (and the PIM widgets reserved for
//! later milestones).

use super::{OptionType, WidgetSpec, widget};

/// The `folder-tree sections=` words (`nail_api::views::FolderPaneSectionKind`).
pub(crate) const FOLDER_SECTIONS: &[&str] = &["favorites", "accounts", "smart-folders"];
/// The `folder-tree favorites=` words (`nail_api::views::FavoriteFolder`).
pub(crate) const FAVORITE_FOLDERS: &[&str] = &[
    "all-inboxes",
    "unread",
    "flagged",
    "snoozed",
    "drafts",
    "sent",
    "outbox",
];

/// Built-in widgets. Implemented widgets have typed options; future ones
/// accept any options until they exist.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    widget(
        "account-switcher",
        &[("show-status", OptionType::Bool)],
        &[],
        false,
    ),
    widget(
        "folder-tree",
        &[
            ("show-unread", OptionType::Bool),
            ("show-account-badges", OptionType::Bool),
            // M5b (ADR 0109): grouped sections (default all three, in
            // this order), the unified folders under Favorites, collapsible
            // sections and account colours.
            ("sections", OptionType::Words(FOLDER_SECTIONS)),
            ("favorites", OptionType::Words(FAVORITE_FOLDERS)),
            ("collapsible", OptionType::Bool),
            ("account-colors", OptionType::Bool),
            // M5b (WP162): the New message button on top of the grouped
            // pane (default #true).
            ("compose", OptionType::Bool),
        ],
        &[],
        false,
    ),
    widget(
        "message-list",
        &[
            ("page-size", OptionType::UInt),
            ("group-by", OptionType::OneOf(&["none", "date", "sender"])),
            (
                "threads",
                OptionType::OneOf(&["off", "collapsed", "expanded"]),
            ),
            ("follow-search", OptionType::Bool),
            // Column ids in display order, e.g. "from subject date"; written
            // back by the column reorder (`LayoutPatch::ColumnOrder`, M1b
            // WP24). The row view ignores it.
            ("columns", OptionType::Str),
        ],
        &[],
        true,
    ),
    widget(
        "message-view",
        &[
            ("headers", OptionType::Str),
            ("mark-read", OptionType::Bool),
        ],
        &[],
        true,
    ),
    widget(
        "thread-view",
        &[
            (
                "expand",
                OptionType::OneOf(&["latest", "unread", "all", "none"]),
            ),
            ("hide-single", OptionType::Bool),
        ],
        &[],
        false,
    ),
    widget(
        "search-bar",
        &[
            ("scope", OptionType::OneOf(&["all", "account", "folder"])),
            ("hints", OptionType::Bool),
            ("live-check", OptionType::Bool),
        ],
        &[],
        false,
    ),
    widget("composer", &[("send-later", OptionType::Bool)], &[], false),
    widget("outbox", &[], &[], false),
    widget("folder-picker", &[], &[], false),
    widget("account-setup", &[], &[], false),
    widget("account-settings", &[], &[], false),
    // M5a (ADR 0102): theme gallery, Follow Windows, density, overrides.
    widget("appearance-settings", &[], &[], false),
    widget("virtual-account-editor", &[], &[], false),
    widget(
        "toolbar",
        &[("toolbar", OptionType::Str)],
        &["toolbar"],
        false,
    ),
    widget("status-bar", &[("items", OptionType::Str)], &[], false),
    widget("text", &[("text", OptionType::Str)], &[], false),
    widget("spacer", &[], &[], false),
];

#[cfg(test)]
mod m5b_tests {
    use nail_api::views::{FavoriteFolder, FolderPaneSectionKind};

    #[test]
    fn folder_pane_words_match_the_contract() {
        let sections: Vec<&str> = FolderPaneSectionKind::ALL
            .iter()
            .map(|k| k.as_str())
            .collect();
        assert_eq!(sections, super::FOLDER_SECTIONS);
        let favorites: Vec<&str> = FavoriteFolder::ALL.iter().map(|f| f.as_str()).collect();
        assert_eq!(favorites, super::FAVORITE_FOLDERS);
    }

    #[test]
    fn folder_tree_options_are_checked() {
        let layout = |widget: &str| {
            format!(
                "layout \"x\" extends=\"default\" {{
    window \"w\" {{
        {widget}
    }}
}}
"
            )
        };
        crate::parse_layout(
            &layout("folder-tree sections=\"accounts favorites\" favorites=\"all-inboxes drafts\" collapsible=#false account-colors=#true compose=#false"),
            "layout.kdl",
        )
        .unwrap();
        for bad in [
            "folder-tree sections=\"accounts accounts\"",
            "folder-tree sections=\"labels\"",
            "folder-tree favorites=\"starred\"",
            "list-header chips=\"all spam\"",
            "calendar view-switch=\"tabs\"",
            "calendar-list actions=\"links\"",
        ] {
            assert!(
                crate::parse_layout(&layout(bad), "layout.kdl").is_err(),
                "{bad}"
            );
        }
        crate::parse_layout(
            &layout("list-header chips=\"all unread\" search=#true hint=#true title=#true counts=#false"),
            "layout.kdl",
        )
        .unwrap();
    }
}
