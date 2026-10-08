//! Icon names (M5a, ADR 0102).
//!
//! Every icon name in the layout model ([`crate::ActionDef::icon`],
//! [`crate::ToolbarItem`] / [`crate::MenuItem`] `icon`), in plug-in
//! manifests and in UI code is a **Lucide** name (<https://lucide.dev>,
//! kebab-case: `reply-all`, `maximize-2`, `square-pen`; canonical names,
//! not Lucide's deprecated aliases). The UI renders it
//! from `lucide-react` (ISC licence) at 16 px with `currentColor`.
//!
//! Resolution, in this order (the UI's `Icon`):
//! 1. the active theme's `icons { icon "<name>" svg=… }` — a theme can
//!    still replace any icon, or add one under a new name;
//! 2. [`lucide_name`]: a name in [`BUNDLED_ICONS`], or a pre-M5 Nail name
//!    from [`LEGACY_ICON_NAMES`] mapped to its Lucide name (user layouts and
//!    plug-ins written before M5 keep their icons);
//! 3. nothing (no icon, no error).
//!
//! The UI bundles only [`BUNDLED_ICONS`] (static imports keep the bundle
//! small); `crates/nail-api/fixtures/icons.json` mirrors both tables for
//! the UI's registry test (`nail-api/tests/fixtures.rs` keeps it current).
//! Adding an icon = add its Lucide name here (sorted) and to the UI's
//! registry.

/// The Lucide icons the UI bundles, sorted.
pub const BUNDLED_ICONS: &[&str] = &[
    "alarm-clock",
    "app-window",
    "archive",
    "archive-restore",
    "archive-x",
    "arrow-left-right",
    "at-sign",
    "bell",
    "bell-off",
    "bold",
    "book-user",
    "bot",
    "calendar",
    "calendar-check",
    "calendar-days",
    "calendar-plus",
    "calendar-range",
    "check",
    "chevron-down",
    "chevron-left",
    "chevron-right",
    "chevron-up",
    "circle-alert",
    "circle-check",
    "circle-question-mark",
    "circle-user-round",
    "clipboard-paste",
    "clock",
    "code",
    "columns-3",
    "command",
    "contact",
    "copy",
    "database",
    "download",
    "ellipsis",
    "ellipsis-vertical",
    "external-link",
    "eye",
    "eye-off",
    "file",
    "file-down",
    "file-pen-line",
    "file-text",
    "flag",
    "flag-off",
    "focus",
    "folder",
    "folder-input",
    "folder-open",
    "folder-plus",
    "folder-sync",
    "forward",
    "funnel",
    "git-merge",
    "globe",
    "grip-vertical",
    "hard-drive-download",
    "hard-drive-upload",
    "heading",
    "image",
    "image-plus",
    "import",
    "inbox",
    "info",
    "italic",
    "key-round",
    "keyboard",
    "languages",
    "layers",
    "layout-dashboard",
    "layout-panel-left",
    "layout-template",
    "link",
    "list",
    "list-checks",
    "list-filter",
    "list-ordered",
    "list-plus",
    "list-todo",
    "lock",
    "lock-open",
    "log-out",
    "mail",
    "mail-check",
    "mail-open",
    "mail-plus",
    "mail-warning",
    "mail-x",
    "maximize-2",
    "minimize-2",
    "minus",
    "monitor",
    "moon",
    "move",
    "package-plus",
    "palette",
    "panel-bottom",
    "panel-left",
    "panels-top-left",
    "paperclip",
    "pen-line",
    "pencil",
    "pin",
    "pin-off",
    "plane",
    "plus",
    "printer",
    "puzzle",
    "quote",
    "redo-2",
    "refresh-cw",
    "reply",
    "reply-all",
    "rotate-ccw",
    "rows-2",
    "save",
    "scan-search",
    "scissors",
    "search",
    "send",
    "send-horizontal",
    "server",
    "settings",
    "settings-2",
    "shield",
    "shield-alert",
    "shield-check",
    "sliders-horizontal",
    "smartphone",
    "sparkles",
    "square",
    "square-check-big",
    "square-pen",
    "star",
    "stethoscope",
    "strikethrough",
    "sun",
    "sun-moon",
    "table",
    "tag",
    "tags",
    "text-quote",
    "text-select",
    "trash",
    "triangle-alert",
    "type",
    "underline",
    "undo-2",
    "upload",
    "user",
    "user-plus",
    "user-round",
    "users",
    "users-round",
    "x",
];

/// Pre-M5 Nail icon names → Lucide names (only names that differ).
pub const LEGACY_ICON_NAMES: &[(&str, &str)] = &[
    ("account-add", "user-plus"),
    ("attachment", "paperclip"),
    ("close", "x"),
    ("compose", "square-pen"),
    ("delete", "trash"),
    ("drafts", "file-pen-line"),
    ("filter", "funnel"),
    ("junk", "shield-alert"),
    ("mark-read", "mail-open"),
    ("outbox", "send-horizontal"),
    ("sent", "send"),
    ("snooze", "alarm-clock"),
    ("sync", "refresh-cw"),
];

/// The Lucide name to render for `name`: itself when bundled, its mapping
/// when it is a legacy Nail name, else `None`.
pub fn lucide_name(name: &str) -> Option<&'static str> {
    if let Ok(i) = BUNDLED_ICONS.binary_search(&name) {
        return Some(BUNDLED_ICONS[i]);
    }
    LEGACY_ICON_NAMES
        .iter()
        .find(|(legacy, _)| *legacy == name)
        .map(|(_, lucide)| *lucide)
}

/// Whether `name` is a well-formed icon name (kebab-case, may contain
/// digits: `maximize-2`). Theme icons may use any such name.
pub fn is_icon_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Both tables as the JSON the UI's registry test reads
/// (`fixtures/icons.json`).
pub fn icons_fixture() -> serde_json::Value {
    serde_json::json!({
        "bundled": BUNDLED_ICONS,
        "legacy": LEGACY_ICON_NAMES
            .iter()
            .map(|(legacy, lucide)| ((*legacy).to_owned(), serde_json::json!(lucide)))
            .collect::<serde_json::Map<_, _>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_icons_are_sorted_unique_and_well_formed() {
        assert!(
            BUNDLED_ICONS.windows(2).all(|w| w[0] < w[1]),
            "sort BUNDLED_ICONS"
        );
        for name in BUNDLED_ICONS {
            assert!(is_icon_name(name), "{name}");
        }
    }

    #[test]
    fn legacy_names_map_to_bundled_icons() {
        for (legacy, lucide) in LEGACY_ICON_NAMES {
            assert!(is_icon_name(legacy), "{legacy}");
            assert!(
                BUNDLED_ICONS.binary_search(lucide).is_ok(),
                "{legacy} → {lucide} is not bundled"
            );
            assert!(
                BUNDLED_ICONS.binary_search(legacy).is_err(),
                "{legacy} is a Lucide name itself"
            );
            assert_eq!(lucide_name(legacy), Some(*lucide));
        }
    }

    #[test]
    fn lookup() {
        assert_eq!(lucide_name("reply-all"), Some("reply-all"));
        assert_eq!(lucide_name("compose"), Some("square-pen"));
        assert_eq!(lucide_name("my-theme-icon"), None);
        assert!(is_icon_name("maximize-2"));
        for bad in ["", "Trash", "2fa", "a-", "a--b", "a_b"] {
            assert!(!is_icon_name(bad), "{bad}");
        }
        let fixture = icons_fixture();
        assert_eq!(fixture["legacy"]["close"], "x");
        assert_eq!(
            fixture["bundled"].as_array().unwrap().len(),
            BUNDLED_ICONS.len()
        );
    }
}
