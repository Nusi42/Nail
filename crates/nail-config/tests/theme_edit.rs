//! M5d write-back (WP184, ADR 0131): the theme editor's operations — token
//! and part edits on a user theme or the user's `theme.kdl`, duplicate,
//! rename, delete, theme details — with comments preserved, golden files in
//! `tests/golden/theme_edit/` (`NAIL_UPDATE_GOLDEN=1` rewrites them), undo
//! and conflicts with changes on disk.
#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;

use std::path::Path;

use nail_api::ErrorCode;
use nail_api::appearance::{ThemeKind, ThemeSource};
use nail_api::editing::{
    EditKind, HistoryScope, ThemeEditTarget, ThemePatch, ValueOrigin, config_revision,
};
use nail_config::ConfigManager;
use pretty_assertions::assert_eq;

fn golden(name: &str, actual: &str) {
    common::golden("theme_edit", name, actual);
}

fn error_id(error: &nail_api::ApiError) -> (ErrorCode, &str) {
    (
        error.code,
        error.localized.as_deref().map_or("", |l| l.id.as_str()),
    )
}

const PAPER: &str = r##"// My paper theme.
theme "Paper" extends="frost" {
    tokens {
        // The accent I like.
        color "accent" "#3b5bdb" dark="#91a7ff" // blue
        radius "md" 6
    }

    modes {
        dark {
            color "bg" "#101113"
        }
    }

    // Unread rows stand out.
    part "message-list.row:unread" {
        font-weight "700"
        color "$color.accent" // same as the accent
    }
}
"##;

fn setup(theme_kdl: Option<&str>) -> (tempfile::TempDir, ConfigManager) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("themes")).unwrap();
    std::fs::write(dir.path().join("themes/paper.kdl"), PAPER).unwrap();
    if let Some(text) = theme_kdl {
        std::fs::write(dir.path().join("theme.kdl"), text).unwrap();
    }
    let manager = ConfigManager::load(dir.path());
    (dir, manager)
}

fn read(dir: &Path, file: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(file)).ok()
}

fn paper() -> ThemeEditTarget {
    ThemeEditTarget::Theme {
        theme_id: "paper".into(),
    }
}

fn token(category: &str, name: &str, value: Option<&str>, mode: Option<&str>) -> ThemePatch {
    ThemePatch::SetToken {
        category: category.into(),
        name: name.into(),
        value: value.map(Into::into),
        mode: mode.map(Into::into),
    }
}

fn part(selector: &str, property: &str, value: Option<&str>) -> ThemePatch {
    ThemePatch::SetPartProperty {
        selector: selector.into(),
        property: property.into(),
        value: value.map(Into::into),
    }
}

const UNREAD: &str = "message-list.row:unread";

#[test]
fn token_edits_change_lines_in_place() {
    let (dir, manager) = setup(None);
    manager
        .edit_theme(
            &paper(),
            &[
                // In place: the value and a mode property.
                token("color", "accent", Some("#1c7ed6"), None),
                token("color", "accent", Some("#a5d8ff"), Some("high-contrast")),
                token("radius", "md", Some("10"), None),
                // A new token line and a mode-only value of a base token.
                token("space", "row", Some("12"), None),
                token("color", "bg-subtle", Some("#16171a"), Some("dark")),
                // An existing `modes` line.
                token("color", "bg", Some("#0b0c0e"), Some("dark")),
            ],
            None,
        )
        .unwrap();
    let out = read(dir.path(), "themes/paper.kdl").unwrap();
    golden("tokens_set", &out);
    let details = manager.theme_details("paper").unwrap();
    let find = |category: &str, name: &str| {
        details
            .tokens
            .iter()
            .find(|t| t.category == category && t.name == name)
            .unwrap_or_else(|| panic!("{category}.{name}"))
            .clone()
    };
    let accent = find("color", "accent");
    assert_eq!(accent.value, "#1c7ed6");
    assert_eq!(accent.modes["dark"], "#91a7ff");
    assert_eq!(accent.modes["high-contrast"], "#a5d8ff");
    assert_eq!(accent.origin, ValueOrigin::Own);
    assert_eq!(find("radius", "md").value, "10px");
    assert_eq!(find("space", "row").value, "12px");
    assert_eq!(find("color", "bg").modes["dark"], "#0b0c0e");
    assert_eq!(find("color", "bg-subtle").modes["dark"], "#16171a");
}

#[test]
fn removing_tokens_lets_the_extended_values_apply() {
    let (dir, manager) = setup(None);
    manager
        .edit_theme(
            &paper(),
            &[
                token("color", "accent", None, Some("dark")),
                token("radius", "md", None, None),
                token("color", "bg", None, Some("dark")),
            ],
            None,
        )
        .unwrap();
    let out = read(dir.path(), "themes/paper.kdl").unwrap();
    golden("tokens_removed", &out);
    assert!(
        !out.contains("modes"),
        "the emptied modes block goes: {out}"
    );
    let details = manager.theme_details("paper").unwrap();
    let radius = details
        .tokens
        .iter()
        .find(|t| t.category == "radius" && t.name == "md")
        .unwrap();
    assert_eq!(radius.origin, ValueOrigin::Base);
}

#[test]
fn part_edits_change_one_declaration() {
    let (dir, manager) = setup(None);
    manager
        .edit_theme(
            &paper(),
            &[
                part(UNREAD, "font-weight", Some("600")),
                part(UNREAD, "background-color", Some("$color.accent-soft")),
                part("message-list.row", "padding", Some("4 8")),
                part("folder-tree.item", "border-radius", Some("4")),
            ],
            None,
        )
        .unwrap();
    let out = read(dir.path(), "themes/paper.kdl").unwrap();
    golden("parts_set", &out);
    let details = manager.theme_details("paper").unwrap();
    let own: Vec<_> = details
        .parts
        .iter()
        .filter(|p| p.origin == ValueOrigin::Own)
        .collect();
    assert_eq!(own.len(), 3);
    assert_eq!(own[0].selector, UNREAD);
    assert_eq!(
        own[0].properties.keys().collect::<Vec<_>>(),
        ["font-weight", "color", "background-color"]
    );
    assert_eq!(own[2].properties["border-radius"], "4px");

    // Removing the last declarations removes the rule; reset removes it whole.
    manager
        .edit_theme(
            &paper(),
            &[
                part("message-list.row", "padding", None),
                ThemePatch::ResetPart {
                    selector: UNREAD.into(),
                },
            ],
            None,
        )
        .unwrap();
    let out = read(dir.path(), "themes/paper.kdl").unwrap();
    golden("parts_removed", &out);
    assert!(!out.contains("unread"), "{out}");
    assert!(out.contains("folder-tree.item"), "{out}");
}

#[test]
fn the_users_theme_kdl_gets_overrides_and_parts() {
    let theme = "// My overrides.\ntheme \"custom\" extends=\"sage\" {\n    // nothing yet\n}\n";
    let (dir, manager) = setup(Some(theme));
    let (appearance, updates) = manager
        .edit_theme(
            &ThemeEditTarget::User,
            &[
                token("color", "accent", Some("#c2255c"), None),
                part("app", "font-size", Some("15")),
            ],
            None,
        )
        .unwrap();
    assert!(!updates.is_empty(), "{updates:?}");
    assert_eq!(
        appearance.settings.overrides.accent.as_deref(),
        Some("#c2255c")
    );
    let out = read(dir.path(), "theme.kdl").unwrap();
    golden("user_theme_kdl", &out);
    // The first write is backed up (ADR 0022).
    assert!(
        !manager
            .list_backups(nail_api::ConfigFile::Theme)
            .unwrap()
            .is_empty(),
        "{:?}",
        manager.list_backups(nail_api::ConfigFile::Theme).unwrap()
    );
    // No theme.kdl yet: the sparse skeleton.
    let (dir, manager) = setup(None);
    manager
        .edit_theme(
            &ThemeEditTarget::User,
            &[token("radius", "md", Some("2"), None)],
            None,
        )
        .unwrap();
    assert_eq!(
        read(dir.path(), "theme.kdl").unwrap(),
        "// Changes made in Nail. See the theme reference in the user documentation.\ntheme \"custom\" extends=\"default\" {\n    tokens {\n        radius \"md\" 2\n    }\n}\n"
    );
}

#[test]
fn theme_edits_are_checked() {
    let (dir, manager) = setup(None);
    let builtin = manager
        .edit_theme(
            &ThemeEditTarget::Theme {
                theme_id: "frost".into(),
            },
            &[token("color", "accent", Some("#000000"), None)],
            None,
        )
        .unwrap_err();
    assert_eq!(
        error_id(&builtin),
        (ErrorCode::InvalidRequest, "edit-issue-not-editable")
    );
    let unknown = manager
        .edit_theme(
            &ThemeEditTarget::Theme {
                theme_id: "nope".into(),
            },
            &[],
            None,
        )
        .unwrap_err();
    assert_eq!(
        error_id(&unknown),
        (ErrorCode::NotFound, "edit-issue-theme-not-found")
    );
    let bad = manager
        .edit_theme(&paper(), &[token("colour", "x", Some("#000"), None)], None)
        .unwrap_err();
    assert_eq!(
        error_id(&bad),
        (ErrorCode::InvalidRequest, "edit-issue-invalid-token")
    );
    // A reference to a token no theme has fails when the theme resolves.
    let dangling = manager
        .edit_theme(
            &paper(),
            &[token("color", "accent", Some("$color.no-such-token"), None)],
            None,
        )
        .unwrap_err();
    assert_eq!(
        error_id(&dangling),
        (ErrorCode::InvalidRequest, "edit-issue-invalid-token")
    );
    let bad_part = manager
        .edit_theme(&paper(), &[part(UNREAD, "behavior", Some("x"))], None)
        .unwrap_err();
    assert_eq!(
        error_id(&bad_part),
        (ErrorCode::InvalidRequest, "edit-issue-invalid-part")
    );
    // No patches: nothing is written (not even a skeleton theme.kdl).
    let (_, updates) = manager
        .edit_theme(&ThemeEditTarget::User, &[], None)
        .unwrap();
    assert!(updates.is_empty(), "{updates:?}");
    assert!(read(dir.path(), "theme.kdl").is_none());
    assert_eq!(read(dir.path(), "themes/paper.kdl").unwrap(), PAPER);
    assert!(
        manager
            .edit_history(HistoryScope::Appearance)
            .undo
            .is_empty(),
        "{:?}",
        manager.edit_history(HistoryScope::Appearance).undo
    );
}

#[test]
fn a_stale_revision_is_a_conflict_and_undo_restores_the_text() {
    let (dir, manager) = setup(None);
    let details = manager.theme_details("paper").unwrap();
    assert!(details.editable);
    assert_eq!(details.revision, config_revision(Some(PAPER)));
    manager
        .edit_theme(
            &paper(),
            &[token("color", "accent", Some("#000000"), None)],
            Some(&details.revision),
        )
        .unwrap();
    let edited = read(dir.path(), "themes/paper.kdl").unwrap();
    // The editor still holds the old revision.
    let err = manager
        .edit_theme(
            &paper(),
            &[token("radius", "md", Some("1"), None)],
            Some(&details.revision),
        )
        .unwrap_err();
    assert_eq!(error_id(&err), (ErrorCode::Conflict, "edit-issue-conflict"));
    assert_eq!(read(dir.path(), "themes/paper.kdl").unwrap(), edited);

    let history = manager.edit_history(HistoryScope::Appearance);
    assert_eq!(history.undo.len(), 1);
    assert_eq!(history.undo[0].kind, EditKind::EditTheme);
    assert_eq!(history.undo[0].files, ["themes/paper.kdl"]);
    manager.undo_edit(HistoryScope::Appearance, false).unwrap();
    assert_eq!(read(dir.path(), "themes/paper.kdl").unwrap(), PAPER);
    manager.redo_edit(HistoryScope::Appearance, false).unwrap();
    assert_eq!(read(dir.path(), "themes/paper.kdl").unwrap(), edited);
}

#[test]
fn duplicating_copies_a_theme_under_a_new_id() {
    let (dir, manager) = setup(None);
    let (id, appearance, _) = manager.duplicate_theme("sage", "My Sage").unwrap();
    assert_eq!(id, "my-sage");
    let copy = read(dir.path(), "themes/my-sage.kdl").unwrap();
    golden("duplicate_builtin", &copy);
    let info = appearance
        .themes
        .iter()
        .find(|t| t.id == "my-sage")
        .unwrap();
    assert_eq!(info.name, "My Sage");
    assert_eq!(info.source, ThemeSource::User);
    assert!(info.problem.is_none(), "{info:?}");
    // The same name again gets a free id; a user theme is copied too.
    let (again, _, _) = manager.duplicate_theme("paper", "My Sage").unwrap();
    assert_eq!(again, "my-sage-2");
    let copy = read(dir.path(), "themes/my-sage-2.kdl").unwrap();
    assert_eq!(copy, PAPER.replace("theme \"Paper\"", "theme \"My Sage\""));
    // Undo removes the copy.
    manager.undo_edit(HistoryScope::Appearance, false).unwrap();
    assert!(read(dir.path(), "themes/my-sage-2.kdl").is_none());
    let err = manager.duplicate_theme("sage", "   ").unwrap_err();
    assert_eq!(
        error_id(&err),
        (ErrorCode::InvalidRequest, "edit-issue-invalid-theme-name")
    );
    let err = manager.duplicate_theme("nope", "x").unwrap_err();
    assert_eq!(error_id(&err).0, ErrorCode::NotFound);
}

#[test]
fn renaming_with_a_new_id_moves_the_file_and_the_selection_follows() {
    let theme = "theme \"custom\" extends=\"paper\" {\n    appearance follow-system=#false light=\"paper\" dark=\"graphite\"\n}\n";
    let (dir, manager) = setup(Some(theme));
    std::fs::write(
        dir.path().join("themes/paper-dim.kdl"),
        "theme \"Paper dim\" extends=\"paper\" mode=\"dark\" {\n}\n",
    )
    .unwrap();
    // Only the display name.
    let (id, _, _) = manager.rename_theme("paper", "Paper 2", None).unwrap();
    assert_eq!(id, "paper");
    assert!(
        read(dir.path(), "themes/paper.kdl")
            .unwrap()
            .contains("theme \"Paper 2\" extends=\"frost\" {")
    );
    // A new id: the file moves, theme.kdl and the extending theme follow.
    let (id, appearance, _) = manager
        .rename_theme("paper", "Newsprint", Some("newsprint"))
        .unwrap();
    assert_eq!(id, "newsprint");
    assert!(read(dir.path(), "themes/paper.kdl").is_none());
    golden(
        "rename_new_id",
        &read(dir.path(), "themes/newsprint.kdl").unwrap(),
    );
    assert_eq!(
        read(dir.path(), "theme.kdl").unwrap(),
        theme.replace("\"paper\"", "\"newsprint\"")
    );
    assert!(
        read(dir.path(), "themes/paper-dim.kdl")
            .unwrap()
            .contains("extends=\"newsprint\"")
    );
    assert_eq!(appearance.settings.theme, "newsprint");
    let entry = manager.edit_history(HistoryScope::Appearance);
    let last = entry.undo.last().unwrap();
    assert_eq!(last.kind, EditKind::RenameTheme);
    assert_eq!(last.files.len(), 4);
    // Undo puts all four files back.
    manager.undo_edit(HistoryScope::Appearance, false).unwrap();
    assert_eq!(read(dir.path(), "theme.kdl").unwrap(), theme);
    assert!(read(dir.path(), "themes/newsprint.kdl").is_none());
    assert!(read(dir.path(), "themes/paper.kdl").is_some());

    for (new_id, expected) in [
        ("frost", "edit-issue-theme-exists"),
        ("paper-dim", "edit-issue-theme-exists"),
        ("Not An Id", "edit-issue-invalid-theme-name"),
    ] {
        let err = manager
            .rename_theme("paper", "X", Some(new_id))
            .unwrap_err();
        assert_eq!(error_id(&err), (ErrorCode::InvalidRequest, expected));
    }
    let err = manager.rename_theme("frost", "X", None).unwrap_err();
    assert_eq!(error_id(&err).1, "edit-issue-not-editable");
}

#[test]
fn deleting_a_selected_theme_falls_back_to_the_default_of_its_kind() {
    let theme = "// Mine.\ntheme \"custom\" extends=\"paper\" {\n    appearance follow-system=#true light=\"paper\" dark=\"nocturne\"\n}\n";
    let (dir, manager) = setup(Some(theme));
    let (appearance, _) = manager.delete_theme("paper").unwrap();
    assert!(read(dir.path(), "themes/paper.kdl").is_none());
    assert_eq!(
        read(dir.path(), "theme.kdl").unwrap(),
        theme.replace("\"paper\"", "\"frost\"")
    );
    assert!(appearance.themes.iter().all(|t| t.id != "paper"));
    let last = manager.edit_history(HistoryScope::Appearance);
    assert_eq!(last.undo.last().unwrap().kind, EditKind::DeleteTheme);
    manager.undo_edit(HistoryScope::Appearance, false).unwrap();
    assert_eq!(read(dir.path(), "themes/paper.kdl").unwrap(), PAPER);
    assert_eq!(read(dir.path(), "theme.kdl").unwrap(), theme);
    // A dark theme falls back to Graphite.
    std::fs::write(
        dir.path().join("themes/ink.kdl"),
        "theme \"Ink\" extends=\"nocturne\" {\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("theme.kdl"),
        "theme \"custom\" extends=\"ink\" {\n}\n",
    )
    .unwrap();
    manager.reload_themes();
    assert_eq!(
        manager.theme_catalog().get("ink").unwrap().kind,
        ThemeKind::Dark
    );
    manager.delete_theme("ink").unwrap();
    assert_eq!(
        read(dir.path(), "theme.kdl").unwrap(),
        "theme \"custom\" extends=\"graphite\" {\n}\n"
    );
}

#[test]
fn details_show_where_values_come_from() {
    let (_dir, manager) = setup(None);
    let details = manager.theme_details("paper").unwrap();
    assert_eq!(details.info.id, "paper");
    let origin = |category: &str, name: &str| {
        details
            .tokens
            .iter()
            .find(|t| t.category == category && t.name == name)
            .map(|t| t.origin)
    };
    assert_eq!(origin("color", "accent"), Some(ValueOrigin::Own));
    // Frost sets the surfaces; the base the spacing.
    assert_eq!(origin("color", "fg"), Some(ValueOrigin::Extended));
    assert_eq!(origin("space", "md"), Some(ValueOrigin::Base));
    // A mode value of the theme's own counts as its own.
    assert_eq!(origin("color", "bg"), Some(ValueOrigin::Own));
    // The base's tokens come first, in its order.
    let base = nail_config::default_theme();
    let first = &details.tokens[0];
    assert!(base.tokens.get(&first.category, &first.name).is_some());
    let first_base_part = details.parts.first().unwrap();
    assert_eq!(first_base_part.origin, ValueOrigin::Base);
    assert_eq!(
        details.parts.last().unwrap().origin,
        ValueOrigin::Own,
        "own rules last"
    );
    // A built-in: not editable, revision of the shipped text.
    let frost = manager.theme_details("frost").unwrap();
    assert!(!frost.editable);
    assert_eq!(frost.revision.len(), 16);
    assert!(
        frost
            .tokens
            .iter()
            .any(|t| t.origin == ValueOrigin::Own && t.name == "accent")
    );
}
