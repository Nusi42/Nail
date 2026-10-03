//! The merge dialog engine: previews by section with conflicts, applying
//! choices while keeping the user's file intact, "take all" / "keep all".
//! Results are compared with golden files in `tests/golden/merge/`
//! (`NAIL_UPDATE_GOLDEN=1` rewrites them).

use std::path::PathBuf;

use nail_api::customize::{
    ConfigMergePreview, MergeChange, MergeChoice, MergeConflictKind, MergeSection, MergeTake,
};
use nail_api::{ConfigFile, ErrorCode};
use nail_config::{EditError, preview_merge, resolve_merge};
use pretty_assertions::assert_eq;

fn golden(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/merge")
        .join(format!("{name}.kdl"));
    if std::env::var_os("NAIL_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (run with NAIL_UPDATE_GOLDEN=1)", path.display()))
        .replace("\r\n", "\n");
    assert_eq!(expected, actual, "golden file {}", path.display());
}

const MY_THEME: &str = r##"// My colours.
theme "Mine" extends="default" {
    tokens {
        // The accent everybody asks about.
        color "accent" "#ff0066" dark="#ff4d94"
        space "row" 6 // roomy
    }

    part "message-list.row:unread .subject" {
        font-weight 700
    }
}
"##;

const THEIR_THEME: &str = r##"theme "Nord" extends="default" {
    tokens {
        color "accent" "#88c0d0"
        color "frost" "#8fbcbb"
        space "row" 6
    }
    part "message-list.row[tag=\"work\"]" {
        border-left "3px solid $color.frost"
    }
    icon "archive" svg="M3 3h18v4H3z M5 9h14v12H5z"
    sounds {
        mail.new "chime"
    }
}
"##;

fn item<'p>(
    preview: &'p ConfigMergePreview,
    section: MergeSection,
    key: &str,
) -> &'p nail_api::customize::MergeItem {
    preview
        .sections
        .iter()
        .find(|s| s.section == section)
        .and_then(|s| s.items.iter().find(|i| i.key == key))
        .unwrap_or_else(|| panic!("no item {key} in {section:?}: {preview:#?}"))
}

fn take(id: &str, take: MergeTake) -> MergeChoice {
    MergeChoice {
        item_id: id.to_owned(),
        take,
    }
}

#[test]
fn a_theme_preview_groups_items_by_section() {
    let preview = preview_merge(ConfigFile::Theme, Some(MY_THEME), THEIR_THEME).unwrap();
    assert!(preview.errors.is_empty());
    let sections: Vec<MergeSection> = preview.sections.iter().map(|s| s.section).collect();
    assert_eq!(
        sections,
        [
            MergeSection::Header,
            MergeSection::Tokens,
            MergeSection::Parts,
            MergeSection::Icons,
            MergeSection::Message
        ]
    );

    let name = item(&preview, MergeSection::Header, "name");
    assert_eq!(name.change, MergeChange::Changed);
    assert_eq!(name.theirs.as_deref(), Some("theme \"Nord\""));

    let accent = item(&preview, MergeSection::Tokens, "color.accent");
    assert_eq!(accent.change, MergeChange::Changed);
    assert_eq!(accent.id, "tokens:color.accent");
    assert_eq!(
        accent.mine.as_deref(),
        Some("color \"accent\" \"#ff0066\" dark=\"#ff4d94\"\n")
    );
    assert_eq!(
        accent.theirs.as_deref(),
        Some("color \"accent\" \"#88c0d0\"\n")
    );
    assert!(accent.conflicts.is_empty());

    // Named explicitly by the incoming file and equal: listed as unchanged.
    let row = item(&preview, MergeSection::Tokens, "space.row");
    assert_eq!(row.change, MergeChange::Unchanged);

    assert_eq!(
        item(&preview, MergeSection::Tokens, "color.frost").change,
        MergeChange::Added
    );
    // The new part needs the new token.
    let work = item(
        &preview,
        MergeSection::Parts,
        "message-list.row[tag=\"work\"]",
    );
    assert_eq!(work.change, MergeChange::Added);
    assert_eq!(work.conflicts.len(), 1);
    assert_eq!(work.conflicts[0].kind, MergeConflictKind::Invalid);
    assert!(work.conflicts[0].message.contains("$color.frost"));

    // The base theme has no icons since M5a (Lucide in the UI): added.
    assert_eq!(
        item(&preview, MergeSection::Icons, "archive").change,
        MergeChange::Added
    );
    assert_eq!(
        item(&preview, MergeSection::Message, "sounds.mail.new").change,
        MergeChange::Added
    );
    // Only in mine: the unread part rule. The base has no such rule since
    // M5a (the UI skin styles unread rows), so the incoming file drops it.
    let unread = item(
        &preview,
        MergeSection::Parts,
        "message-list.row:unread .subject",
    );
    assert_eq!(unread.change, MergeChange::Removed);
}

#[test]
fn taking_single_theme_items_keeps_the_rest_of_my_file() {
    let outcome = resolve_merge(
        ConfigFile::Theme,
        Some(MY_THEME),
        THEIR_THEME,
        &[
            take("tokens:color.accent", MergeTake::Theirs),
            take("tokens:color.frost", MergeTake::Theirs),
            take("parts:message-list.row[tag=\"work\"]", MergeTake::Theirs),
            take("message:sound:sounds.mail.new", MergeTake::Theirs),
        ],
        MergeTake::Mine,
    )
    .unwrap();
    golden("theme_some_items", &outcome.text);
    let theme = outcome.theme.unwrap();
    assert_eq!(theme.name, "Mine");
    assert_eq!(theme.tokens.get("color", "accent"), Some("#88c0d0"));
    assert_eq!(theme.tokens.get("color", "frost"), Some("#8fbcbb"));
    assert_eq!(
        theme.sounds.get("mail.new").map(String::as_str),
        Some("chime")
    );
    assert!(outcome.text.contains("space \"row\" 6 // roomy"));
    assert!(outcome.text.contains("font-weight 700"));
    assert!(outcome.layout.is_none());
}

#[test]
fn take_all_and_keep_all() {
    let all = resolve_merge(
        ConfigFile::Theme,
        Some(MY_THEME),
        THEIR_THEME,
        &[],
        MergeTake::Theirs,
    )
    .unwrap();
    assert_eq!(
        all.text, THEIR_THEME,
        "take all writes the incoming file as it is"
    );
    let none = resolve_merge(
        ConfigFile::Theme,
        Some(MY_THEME),
        THEIR_THEME,
        &[],
        MergeTake::Mine,
    )
    .unwrap();
    assert_eq!(none.text, MY_THEME);
    // "Keep all" with one exception.
    let one = resolve_merge(
        ConfigFile::Theme,
        Some(MY_THEME),
        THEIR_THEME,
        &[take("header:name", MergeTake::Theirs)],
        MergeTake::Mine,
    )
    .unwrap();
    assert_eq!(
        one.text,
        MY_THEME.replace("theme \"Mine\"", "theme \"Nord\"")
    );
}

#[test]
fn without_a_file_mine_is_the_default() {
    let preview = preview_merge(ConfigFile::Theme, None, THEIR_THEME).unwrap();
    assert_eq!(
        item(&preview, MergeSection::Tokens, "color.accent").change,
        MergeChange::Changed
    );
    let outcome = resolve_merge(
        ConfigFile::Theme,
        None,
        THEIR_THEME,
        &[take("tokens:color.accent", MergeTake::Theirs)],
        MergeTake::Mine,
    )
    .unwrap();
    golden("theme_from_nothing", &outcome.text);
    assert_eq!(
        outcome.theme.unwrap().tokens.get("color", "accent"),
        Some("#88c0d0")
    );
}

#[test]
fn a_sparse_file_cannot_remove_default_items() {
    // A complete incoming theme without the default `danger` colour.
    let complete = nail_config::DEFAULT_THEME_KDL.replace("color \"danger\"", "color \"alarm\"");
    assert_ne!(complete, nail_config::DEFAULT_THEME_KDL);
    let fixed = complete.replace("$color.danger", "$color.alarm");
    let preview = preview_merge(ConfigFile::Theme, Some(MY_THEME), &fixed).unwrap();
    assert!(preview.errors.is_empty(), "{:?}", preview.errors);
    let danger = item(&preview, MergeSection::Tokens, "color.danger");
    assert_eq!(danger.change, MergeChange::Removed);
    assert_eq!(danger.conflicts[0].kind, MergeConflictKind::Invalid);
}

const MY_LAYOUT: &str = r#"layout "Mine" extends="default" {
    keys context="list" {
        bind "e" "mail.archive" // muscle memory
    }
    toolbar "reader" {
        item "mail.reply"
        item "plugin.todo.add"
    }
}
"#;

const THEIR_LAYOUT: &str = r#"layout "Theirs" extends="default" {
    window "outbox" {
        stack {
            outbox id="outbox"
            plugin-panel "stats.chart"
        }
    }
    keys context="list" {
        bind "e" "mail.delete"
        bind "q" "plugin.quick.act"
    }
    keys {
        unbind "c"
    }
    toolbar "reader" {
        item "mail.reply"
        item "mail.forward"
    }
}
"#;

#[test]
fn a_layout_preview_flags_conflicts() {
    let preview = preview_merge(ConfigFile::Layout, Some(MY_LAYOUT), THEIR_LAYOUT).unwrap();
    let kinds = |section, key| {
        item(&preview, section, key)
            .conflicts
            .iter()
            .map(|c| c.kind)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        kinds(MergeSection::Keys, "list: e"),
        [MergeConflictKind::DuplicateKey]
    );
    assert_eq!(
        kinds(MergeSection::Keys, "list: q"),
        [MergeConflictKind::UnknownAction]
    );
    assert_eq!(
        item(&preview, MergeSection::Keys, "global: c").change,
        MergeChange::Removed
    );
    assert_eq!(
        kinds(MergeSection::Windows, "outbox"),
        [MergeConflictKind::UnknownWidget]
    );
    assert_eq!(
        kinds(MergeSection::Toolbars, "reader"),
        [MergeConflictKind::DropsPluginItems]
    );
}

#[test]
fn installed_plugin_items_are_not_flagged() {
    let mut installed = nail_config::InstalledPlugins::default();
    installed.actions.insert("plugin.quick.act".to_owned());
    installed.panels.insert("stats.chart".to_owned());
    let preview = nail_config::preview_merge_with(
        ConfigFile::Layout,
        Some(MY_LAYOUT),
        THEIR_LAYOUT,
        &installed,
    )
    .unwrap();
    assert!(
        item(&preview, MergeSection::Keys, "list: q")
            .conflicts
            .is_empty()
    );
    assert!(
        item(&preview, MergeSection::Windows, "outbox")
            .conflicts
            .is_empty()
    );
}

#[test]
fn taking_layout_items_edits_my_file() {
    let outcome = resolve_merge(
        ConfigFile::Layout,
        Some(MY_LAYOUT),
        THEIR_LAYOUT,
        &[
            take("keys:list: e", MergeTake::Theirs),
            take("keys:global: c", MergeTake::Theirs),
            take("toolbars:reader", MergeTake::Theirs),
        ],
        MergeTake::Mine,
    )
    .unwrap();
    golden("layout_some_items", &outcome.text);
    let layout = outcome.layout.unwrap();
    let action = |ctx: &str, key: &str| {
        layout
            .keys
            .iter()
            .find(|k| k.context == ctx && k.key == key)
            .map(|k| k.action.clone())
    };
    assert_eq!(action("list", "e").as_deref(), Some("mail.delete"));
    assert_eq!(action("global", "c"), None);
    assert_eq!(action("list", "q"), None, "kept mine: not bound");
    assert_eq!(layout.toolbars["reader"].len(), 2);
    assert_eq!(layout.name, "Mine");
}

#[test]
fn errors_are_reported() {
    // The incoming file is invalid: the preview lists every error.
    let broken = "layout \"x\" extends=\"default\" {\n    bogus\n    keys { bind \"ctrl+\" \"mail.archive\" }\n}\n";
    let preview = preview_merge(ConfigFile::Layout, Some(MY_LAYOUT), broken).unwrap();
    assert!(preview.sections.is_empty());
    assert_eq!(preview.errors.len(), 2, "{:?}", preview.errors);
    assert_eq!(preview.errors[0].line, 2);
    assert_eq!(preview.errors[1].line, 3);
    let err = resolve_merge(
        ConfigFile::Layout,
        Some(MY_LAYOUT),
        broken,
        &[],
        MergeTake::Theirs,
    )
    .unwrap_err();
    assert!(matches!(err, EditError::Invalid(_)));

    // My file is invalid.
    let err = preview_merge(ConfigFile::Layout, Some("layout {"), THEIR_LAYOUT).unwrap_err();
    assert!(matches!(err, EditError::Config(_)));
    assert_eq!(nail_api::ApiError::from(err).code, ErrorCode::Config);

    // Unknown choice ids and files that cannot be merged.
    let err = resolve_merge(
        ConfigFile::Layout,
        Some(MY_LAYOUT),
        THEIR_LAYOUT,
        &[take("keys:nope", MergeTake::Theirs)],
        MergeTake::Mine,
    )
    .unwrap_err();
    assert!(
        matches!(err, EditError::Invalid(m) if nail_config::l10n::english(&m).contains("keys:nope"))
    );
    let err = preview_merge(ConfigFile::Accounts, None, "").unwrap_err();
    assert_eq!(
        nail_api::ApiError::from(err).code,
        ErrorCode::InvalidRequest
    );
}
