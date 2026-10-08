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

/// The `thread-view card=` words: the reader's per-message widgets a card
/// of the feed can show (P2 WP203; `ui/src/widgets/ThreadViewModel.ts`
/// `CARD_WIDGETS`, same order).
pub(crate) const CARD_WIDGETS: &[&str] = &[
    "security-message",
    "security-warnings",
    "remote-content",
    "junk-verdict",
    "sync-message-body",
    "message-view",
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
            // ADR 0183: Check for new mail beside it (default #true).
            ("check-mail", OptionType::Bool),
        ],
        &[],
        false,
    ),
    widget(
        "message-list",
        &[
            ("page-size", OptionType::UInt),
            ("group-by", OptionType::OneOf(&["none", "date", "sender"])),
            // P2 (ADR 0148): one row per conversation (`collapsed`, the
            // default layout's), or rows that open into their messages
            // (`expandable`, Design B). `expanded` keeps its pre-P2 meaning,
            // one row per message (like `off`), still accepted.
            (
                "threads",
                OptionType::OneOf(&["off", "collapsed", "expandable", "expanded"]),
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
            // P2 (WP203): `cards` = the reader as a card feed (the default
            // layout's), `strip` (the default) = the strip above a separate
            // `message-view`, as in layouts written before P2.
            ("mode", OptionType::OneOf(&["strip", "cards"])),
            // Cards: the per-message widgets an open card shows, in order.
            ("card", OptionType::Words(CARD_WIDGETS)),
            // Cards: the header fields of a card's `message-view` (the
            // card's own header shows sender, recipients and date).
            ("headers", OptionType::Str),
            ("mark-read", OptionType::Bool),
            // Strip only.
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
        let layout = |widget: &str| crate::widgets::testing::layout_with(&[widget]);
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

#[cfg(test)]
mod p2_tests {
    fn layout(widget: &str) -> String {
        crate::widgets::testing::layout_with(&[widget])
    }

    /// P2 (ADR 0148): the conversation modes of the message list.
    #[test]
    fn message_list_thread_modes_are_checked() {
        for mode in ["off", "collapsed", "expandable", "expanded"] {
            crate::parse_layout(
                &layout(&format!("message-list threads=\"{mode}\"")),
                "layout.kdl",
            )
            .unwrap();
        }
        assert!(
            crate::parse_layout(&layout("message-list threads=\"tree\""), "layout.kdl").is_err()
        );
    }
}

#[cfg(test)]
mod p2_reader_tests {
    fn layout(widget: &str) -> String {
        crate::widgets::testing::layout_with(&[widget])
    }

    /// P2 WP203: the reader's card feed and its card widgets.
    #[test]
    fn thread_view_card_options_are_checked() {
        for ok in [
            "thread-view mode=\"cards\"",
            "thread-view mode=\"strip\" expand=\"none\"",
            "thread-view mode=\"cards\" card=\"message-view\" headers=\"cc\" mark-read=#false",
            "thread-view mode=\"cards\" card=\"remote-content security-message message-view\"",
        ] {
            crate::parse_layout(&layout(ok), "layout.kdl").unwrap();
        }
        for bad in [
            "thread-view mode=\"feed\"",
            "thread-view card=\"message-list\"",
            "thread-view card=\"message-view message-view\"",
        ] {
            assert!(
                crate::parse_layout(&layout(bad), "layout.kdl").is_err(),
                "{bad}"
            );
        }
    }
}
