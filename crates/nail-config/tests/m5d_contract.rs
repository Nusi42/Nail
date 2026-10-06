//! M5d WP180 (ADR 0131): the contract parts of nail-config — `hidden=` on
//! layout nodes, the `presets` section, the undo history of every write of
//! `layout.kdl` / `theme.kdl` (undo, redo, conflicts with changes on disk,
//! `force`) and the revision check of `UpdateLayout`.
#![cfg(test)]

use nail_api::appearance::{Density, ThemeOverrides, ThemePreferences};
use nail_api::customize::{LayoutNodeRef, LayoutPatch};
use nail_api::editing::{EditKind, HistoryScope, NO_REVISION, config_revision};
use nail_api::{ErrorCode, LayoutNode, PresetNode};
use nail_config::{ConfigManager, default_layout, parse_layout, parse_layout_all};
use pretty_assertions::assert_eq;

fn error_id(error: &nail_api::ApiError) -> Option<&str> {
    error.localized.as_deref().map(|l| l.id.as_str())
}

fn config_ids(text: &str) -> Vec<String> {
    parse_layout_all(text, "layout.kdl")
        .unwrap_err()
        .iter()
        .map(|e| e.localized.id.clone())
        .collect()
}

#[test]
fn nodes_can_be_hidden() {
    let layout = parse_layout(
        r#"layout "x" extends="default" {
    window "mail" {
        split "horizontal" id="panes" hidden=#false {
            stack id="sidebar" hidden=#true {
                folder-tree id="folders" hidden=#true
            }
            message-list id="list"
        }
    }
}
"#,
        "layout.kdl",
    )
    .unwrap();
    let root = &layout.windows["mail"][0].root;
    assert!(!root.is_hidden());
    assert!(root.at_path(&[0]).unwrap().is_hidden());
    assert!(root.at_path(&[0, 0]).unwrap().is_hidden());
    let LayoutNode::Widget { options, .. } = root.at_path(&[0, 0]).unwrap() else {
        panic!("expected the folder tree");
    };
    assert!(
        !options.contains_key("hidden"),
        "`hidden` is not a widget option"
    );
    assert!(!root.at_path(&[1]).unwrap().is_hidden());
    let ids = config_ids(
        "layout \"x\" extends=\"default\" {\n    window \"mail\" {\n        message-list hidden=\"yes\"\n    }\n}\n",
    );
    assert_eq!(ids, ["config-expected-bool"]);
}

#[test]
fn the_default_layout_has_the_four_presets_of_the_mail_view() {
    let layout = default_layout();
    let ids: Vec<&str> = layout.presets.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["three-panes", "reader-below", "wide-reader", "focus"]);
    for preset in &layout.presets {
        assert_eq!(preset.window, "mail");
        assert!(
            preset
                .label
                .as_deref()
                .is_some_and(|l| l.starts_with("@layout.preset."))
        );
        assert!(preset.icon.is_some());
    }
    assert_eq!(
        layout.presets[1].root.pane_ids(),
        ["sidebar", "list-pane", "reader"]
    );
    assert_eq!(
        layout.presets[3].root,
        PresetNode::Pane {
            id: "reader".into()
        }
    );
    // The default layout offers the editors' actions and the toolbar menu.
    assert_eq!(
        layout.actions["layout.customize"].icon.as_deref(),
        Some("layout-dashboard")
    );
    assert!(layout.menus.contains_key("context:toolbar"));
}

#[test]
fn presets_may_name_panes_a_replaced_window_lacks() {
    // A user file that replaced the mail window (with other pane ids) keeps
    // working; applying a default preset is refused per breakpoint instead.
    let layout = parse_layout(
        r#"layout "x" extends="default" {
    window "mail" {
        split "horizontal" id="mine" {
            folder-tree id="folders"
            message-list id="list"
        }
    }
}
"#,
        "layout.kdl",
    )
    .unwrap();
    assert_eq!(layout.presets.len(), 4);
}

#[test]
fn a_user_file_replaces_the_presets_as_a_whole() {
    let layout = parse_layout(
        r#"layout "x" extends="default" {
    presets {
        preset "two" window="mail" label="Two" {
            split "vertical" sizes="1fr 2fr" {
                pane "list-pane"
                tabs {
                    pane "reader"
                    pane "sidebar"
                }
            }
        }
    }
}
"#,
        "layout.kdl",
    )
    .unwrap();
    assert_eq!(layout.presets.len(), 1);
    assert_eq!(layout.presets[0].label.as_deref(), Some("Two"));
    assert_eq!(
        layout.presets[0].root.pane_ids(),
        ["list-pane", "reader", "sidebar"]
    );
}

#[test]
fn bad_presets_are_reported() {
    let wrap = |presets: &str| format!("layout \"x\" extends=\"default\" {{\n{presets}}}\n");
    let cases = [
        (
            "presets {\n preset \"a\" window=\"mail\" { pane \"reader\" }\n preset \"a\" window=\"mail\" { pane \"reader\" }\n}\n",
            "config-preset-duplicate",
        ),
        (
            "presets {\n preset \"a\" { pane \"reader\" }\n}\n",
            "config-preset-window-missing",
        ),
        (
            "presets {\n preset \"a\" window=\"mail\" { pane \"reader\"; pane \"sidebar\" }\n}\n",
            "config-preset-one-root",
        ),
        (
            "presets {\n preset \"a\" window=\"mail\" { split { pane \"reader\"; pane \"reader\" } }\n}\n",
            "config-preset-pane-twice",
        ),
        (
            "presets {\n preset \"a\" window=\"mail\" { folder-tree }\n}\n",
            "config-preset-node-unknown",
        ),
        (
            "presets {\n preset \"a\" window=\"nope\" { pane \"reader\" }\n}\n",
            "config-preset-window-unknown",
        ),
        ("presets {}\npresets {}\n", "config-layout-section-twice"),
    ];
    for (presets, id) in cases {
        let ids = config_ids(&wrap(presets));
        assert!(
            ids.iter().any(|i| i == id),
            "{id} not in {ids:?} for\n{presets}"
        );
    }
}

fn manager() -> (tempfile::TempDir, ConfigManager) {
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::load(dir.path());
    (dir, manager)
}

fn divider(sizes: &str) -> LayoutPatch {
    LayoutPatch::SplitSizes {
        node: LayoutNodeRef {
            window: "mail".into(),
            breakpoint: 0,
            path: vec![],
        },
        sizes: sizes.split(' ').map(str::to_owned).collect(),
    }
}

#[test]
fn layout_writes_are_undone_and_redone() {
    let (dir, manager) = manager();
    let path = dir.path().join("layout.kdl");
    let empty = manager.edit_history(HistoryScope::Layout);
    assert_eq!(empty.revision, NO_REVISION);
    assert!(empty.undo.is_empty() && empty.redo.is_empty());

    manager.update_layout(&[divider("200px 1fr 1fr")]).unwrap();
    let first = std::fs::read_to_string(&path).unwrap();
    manager
        .update_layout_checked(
            &[divider("300px 1fr 1fr"), divider("310px 1fr 1fr")],
            Some(&config_revision(Some(&first))),
        )
        .unwrap();
    let second = std::fs::read_to_string(&path).unwrap();
    let history = manager.edit_history(HistoryScope::Layout);
    assert_eq!(history.undo.len(), 2);
    assert_eq!(history.undo[1].kind, EditKind::SplitSizes);
    assert_eq!(history.undo[1].label.id, "edit-kind-split-sizes");
    assert_eq!(history.undo[1].files, ["layout.kdl"]);
    assert_eq!(history.revision, config_revision(Some(&second)));
    assert!(
        manager
            .edit_history(HistoryScope::Appearance)
            .undo
            .is_empty(),
        "{:?}",
        manager.edit_history(HistoryScope::Appearance).undo
    );

    let (history, updates) = manager.undo_edit(HistoryScope::Layout, false).unwrap();
    assert_eq!((history.undo.len(), history.redo.len()), (1, 1));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
    assert!(!updates.is_empty(), "the restored layout is announced");
    manager.undo_edit(HistoryScope::Layout, false).unwrap();
    assert!(!path.exists(), "the first undo entry created the file");
    manager.redo_edit(HistoryScope::Layout, false).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), first);

    // A new edit clears the redo list.
    manager.update_layout(&[divider("220px 1fr 1fr")]).unwrap();
    let history = manager.edit_history(HistoryScope::Layout);
    assert_eq!((history.undo.len(), history.redo.len()), (2, 0));
}

#[test]
fn changes_on_disk_are_conflicts() {
    let (dir, manager) = manager();
    let path = dir.path().join("layout.kdl");
    manager.update_layout(&[divider("200px 1fr 1fr")]).unwrap();
    let ours = std::fs::read_to_string(&path).unwrap();
    let theirs = "// edited by hand\nlayout \"mine\" extends=\"default\" {\n}\n";
    std::fs::write(&path, theirs).unwrap();

    // An edit based on the old revision is refused and writes nothing.
    let err = manager
        .update_layout_checked(
            &[divider("1px 1fr 1fr")],
            Some(&config_revision(Some(&ours))),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(error_id(&err), Some("edit-issue-conflict"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), theirs);

    // Undo refuses too, and keeps the entry.
    let err = manager.undo_edit(HistoryScope::Layout, false).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(manager.edit_history(HistoryScope::Layout).undo.len(), 1);
    // `force` backs the hand-edited file up and undoes.
    manager.undo_edit(HistoryScope::Layout, true).unwrap();
    assert!(!path.exists());
    let backups = manager.list_backups(nail_api::ConfigFile::Layout).unwrap();
    assert!(!backups.is_empty(), "the hand-edited file was backed up");
}

#[test]
fn appearance_writes_share_one_history() {
    let (dir, manager) = manager();
    let path = dir.path().join("theme.kdl");
    manager.set_theme("sage").unwrap();
    let preferences = ThemePreferences {
        follow_system: false,
        light: "frost".into(),
        dark: "graphite".into(),
        density: Density::Compact,
        overrides: ThemeOverrides {
            font_size: None,
            radius: Some(12),
            accent: Some("#aa3355".into()),
        },
    };
    manager.set_theme_preferences(&preferences).unwrap();
    let history = manager.edit_history(HistoryScope::Appearance);
    let kinds: Vec<EditKind> = history.undo.iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [EditKind::SetTheme, EditKind::SetThemePreferences]);
    assert_eq!(
        history.revision,
        config_revision(Some(&std::fs::read_to_string(&path).unwrap()))
    );

    let (_, updates) = manager.undo_edit(HistoryScope::Appearance, false).unwrap();
    assert!(!updates.is_empty(), "{updates:?}");
    assert_eq!(manager.appearance().settings.density, Density::Comfortable);
    assert_eq!(manager.appearance().active, "sage");
    manager.undo_edit(HistoryScope::Appearance, false).unwrap();
    assert!(!path.exists());
    manager.redo_edit(HistoryScope::Appearance, false).unwrap();
    assert_eq!(manager.appearance().active, "sage");
}
