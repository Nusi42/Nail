//! The built-in widgets of M0 / M1a (and the PIM widgets reserved for
//! later milestones).

use super::{OptionType, WidgetSpec, widget};

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
