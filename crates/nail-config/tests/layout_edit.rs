//! M5d write-back (WP184, ADR 0131): the layout editor's and toolbar
//! customizer's patches land in `layout.kdl` with comments and formatting
//! preserved. Each case compares against a golden file in
//! `tests/golden/layout_edit/`; run with `NAIL_UPDATE_GOLDEN=1` to rewrite
//! them after an intended change (and review the diff).
#![cfg(test)]

use std::path::PathBuf;

use nail_api::editing::{
    DockSide, EditKind, HistoryScope, LayoutPlacement, NewWidget, ToolbarArea, ToolbarSlot,
    config_revision,
};
use nail_api::{ErrorCode, LayoutNode, LayoutNodeRef, LayoutPatch, ResolvedLayout, ToolbarItem};
use nail_config::{
    ConfigManager, DEFAULT_LAYOUT_KDL, EditError, apply_layout_patches, default_layout,
    parse_layout_all,
};
use pretty_assertions::assert_eq;
use serde_json::json;

fn golden(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/layout_edit")
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

fn at(window: &str, breakpoint: u32, path: &[u32]) -> LayoutNodeRef {
    LayoutNodeRef {
        window: window.into(),
        breakpoint,
        path: path.to_vec(),
    }
}

fn mail(path: &[u32]) -> LayoutNodeRef {
    at("mail", 0, path)
}

fn into(parent: LayoutNodeRef, index: u32) -> LayoutPlacement {
    LayoutPlacement::Into { parent, index }
}

fn beside(target: LayoutNodeRef, side: DockSide) -> LayoutPlacement {
    LayoutPlacement::Beside { target, side }
}

fn apply(text: Option<&str>, patches: &[LayoutPatch]) -> String {
    let out = apply_layout_patches(text, patches, "layout.kdl")
        .unwrap_or_else(|e| panic!("patch failed: {e}"));
    parse_layout_all(&out, "layout.kdl").unwrap_or_else(|e| panic!("invalid result: {e:?}"));
    out
}

fn resolved(text: &str) -> ResolvedLayout {
    parse_layout_all(text, "layout.kdl").unwrap()
}

/// The ids of a tree, depth first, with `!` for hidden nodes and the
/// children in brackets: `panes[sidebar[folders] list]`.
fn shape(node: &LayoutNode) -> String {
    let mut out = node
        .id()
        .map_or_else(|| kind_of(node).to_owned(), str::to_owned);
    if node.is_hidden() {
        out.push('!');
    }
    let children = node.children();
    if !children.is_empty() {
        let inner: Vec<String> = children.iter().map(shape).collect();
        out.push('[');
        out.push_str(&inner.join(" "));
        out.push(']');
    }
    out
}

fn kind_of(node: &LayoutNode) -> &str {
    match node {
        LayoutNode::Split { .. } => "split",
        LayoutNode::Stack { .. } => "stack",
        LayoutNode::Tabs { .. } => "tabs",
        LayoutNode::Widget { kind, .. } => kind,
    }
}

fn mail_shape(text: &str) -> String {
    shape(&resolved(text).windows["mail"][0].root)
}

fn sizes_of(node: &LayoutNode) -> Vec<String> {
    match node {
        LayoutNode::Split { sizes, .. } | LayoutNode::Stack { sizes, .. } => sizes.clone(),
        _ => Vec::new(),
    }
}

fn error_id(err: EditError) -> (ErrorCode, String) {
    let api: nail_api::ApiError = err.into();
    let id = api
        .localized
        .as_deref()
        .map(|l| l.id.clone())
        .unwrap_or_default();
    (api.code, id)
}

fn refused(text: Option<&str>, patch: LayoutPatch) -> (ErrorCode, String) {
    error_id(apply_layout_patches(text, &[patch], "layout.kdl").unwrap_err())
}

const MAIL: &str = r#"// My mail window, tuned by hand.
layout "Mine" extends="default" {
    window "mail" {
        // Folders | list | reader.
        split "horizontal" id="panes" \
            sizes="240px 1fr 1.4fr" /* tuned */ \
            min-sizes="200px 300px 320px" {
            // The folder pane.
            stack id="sidebar" sizes="1fr auto" {
                folder-tree id="folders" show-unread=#true // unread counts
                label-list id="gmail-labels"
            }

            // Messages.
            stack id="list-pane" sizes="auto 1fr" {
                list-header id="list-header"
                message-list id="list" page-size=50 {
                    row "{from.name}  {date | relative}"
                    row "{subject}"
                }
            }
            // The reader with its toolbar.
            stack id="reader" sizes="auto 1fr" {
                toolbar id="reader-toolbar" toolbar="reader"
                message-view id="message" headers="from to date"
            }
        }
    }

    // Reader buttons.
    toolbar "reader" {
        item "mail.reply" // most used
        item "mail.forward" rank=1

        // Rarely.
        item "mail.delete"
        more {
            item "mail.archive" label="Archive it"
        }
    }
}
"#;

// ---------------------------------------------------------------------------
// Panes

#[test]
fn swapping_two_panes_keeps_their_comments_and_their_own_sizes() {
    let out = apply(
        Some(MAIL),
        &[LayoutPatch::SwapPanes {
            a: mail(&[0]),
            b: mail(&[2]),
        }],
    );
    golden("swap_panes", &out);
    assert_eq!(
        mail_shape(&out),
        "panes[reader[reader-toolbar message] list-pane[list-header list] sidebar[folders gmail-labels]]"
    );
    // Each pane takes its own size and limits along (ADR 0138).
    let root = &resolved(&out).windows["mail"][0].root;
    assert_eq!(sizes_of(root), ["1.4fr", "1fr", "240px"]);
    // Swapping back restores the file byte for byte.
    let back = apply(
        Some(&out),
        &[LayoutPatch::SwapPanes {
            a: mail(&[2]),
            b: mail(&[0]),
        }],
    );
    assert_eq!(back, MAIL);
}

#[test]
fn swapping_nested_panes_of_different_depths_reindents_them() {
    let out = apply(
        Some(MAIL),
        &[LayoutPatch::SwapPanes {
            a: mail(&[1, 1]),
            b: mail(&[2]),
        }],
    );
    golden("swap_nested", &out);
    assert_eq!(
        mail_shape(&out),
        "panes[sidebar[folders gmail-labels] list-pane[list-header reader[reader-toolbar message]] list]"
    );
}

#[test]
fn moving_a_widget_into_another_stack_updates_both_size_lists() {
    let out = apply(
        Some(MAIL),
        &[LayoutPatch::MovePane {
            node: mail(&[0, 1]),
            to: into(mail(&[2]), 1),
        }],
    );
    golden("move_into_stack", &out);
    let root = &resolved(&out).windows["mail"][0].root;
    assert_eq!(
        shape(root),
        "panes[sidebar[folders] list-pane[list-header list] reader[reader-toolbar gmail-labels message]]"
    );
    assert_eq!(sizes_of(root.at_path(&[0]).unwrap()), ["1fr"]);
    // Stack to stack: the label list keeps its `auto` height.
    assert_eq!(
        sizes_of(root.at_path(&[2]).unwrap()),
        ["auto", "auto", "1fr"]
    );
}

#[test]
fn moving_within_a_container_keeps_the_pane_sizes() {
    let out = apply(
        Some(MAIL),
        &[LayoutPatch::MovePane {
            node: mail(&[2]),
            to: into(mail(&[]), 0),
        }],
    );
    golden("move_within_split", &out);
    let root = &resolved(&out).windows["mail"][0].root;
    assert_eq!(
        shape(root),
        "panes[reader[reader-toolbar message] sidebar[folders gmail-labels] list-pane[list-header list]]"
    );
    assert_eq!(sizes_of(root), ["1.4fr", "240px", "1fr"]);
    // The index is the one the node gets, also when it moves to the end.
    let out = apply(
        Some(MAIL),
        &[LayoutPatch::MovePane {
            node: mail(&[0]),
            to: into(mail(&[]), 99),
        }],
    );
    assert_eq!(
        mail_shape(&out),
        "panes[list-pane[list-header list] reader[reader-toolbar message] sidebar[folders gmail-labels]]"
    );
}

#[test]
fn docking_across_the_split_wraps_the_target_in_a_new_split() {
    let out = apply(
        Some(MAIL),
        &[LayoutPatch::MovePane {
            node: mail(&[1, 1]),
            to: beside(mail(&[2]), DockSide::Top),
        }],
    );
    golden("dock_wraps_target", &out);
    let root = &resolved(&out).windows["mail"][0].root;
    assert_eq!(
        shape(root),
        "panes[sidebar[folders gmail-labels] list-pane[list-header] split[list reader[reader-toolbar message]]]"
    );
    let LayoutNode::Split {
        orientation, sizes, ..
    } = root.at_path(&[2]).unwrap()
    else {
        panic!("expected the new split");
    };
    assert_eq!(*orientation, nail_api::Orientation::Vertical);
    assert_eq!(sizes, &["1fr", "1fr"]);
    // The list pane lost the list's size.
    assert_eq!(sizes_of(root.at_path(&[1]).unwrap()), ["auto"]);
}

#[test]
fn docking_along_the_split_goes_into_the_parent_split() {
    let out = apply(
        Some(MAIL),
        &[LayoutPatch::MovePane {
            node: mail(&[0]),
            to: beside(mail(&[2]), DockSide::Right),
        }],
    );
    golden("dock_into_parent", &out);
    let root = &resolved(&out).windows["mail"][0].root;
    assert_eq!(
        shape(root),
        "panes[list-pane[list-header list] reader[reader-toolbar message] sidebar[folders gmail-labels]]"
    );
    // Within its own split the folder pane keeps its width and minimum.
    assert_eq!(sizes_of(root), ["1fr", "1.4fr", "240px"]);
    let LayoutNode::Split { min_sizes, .. } = root else {
        panic!("expected the split");
    };
    assert_eq!(
        min_sizes,
        &[
            Some("300px".to_owned()),
            Some("320px".to_owned()),
            Some("200px".to_owned())
        ]
    );
}

/// The split's per-child lists as `(size, min, max, collapsible)` per child.
fn limits(node: &LayoutNode) -> Vec<(String, Option<String>, Option<String>, bool)> {
    let LayoutNode::Split {
        sizes,
        min_sizes,
        max_sizes,
        collapsible,
        ..
    } = node
    else {
        panic!("expected a split: {node:?}");
    };
    (0..sizes.len())
        .map(|i| {
            (
                sizes[i].clone(),
                min_sizes.get(i).cloned().flatten(),
                max_sizes.get(i).cloned().flatten(),
                collapsible.get(i).copied().unwrap_or(false),
            )
        })
        .collect()
}

fn pane(
    size: &str,
    min: Option<&str>,
    max: Option<&str>,
    collapsible: bool,
) -> (String, Option<String>, Option<String>, bool) {
    (
        size.into(),
        min.map(Into::into),
        max.map(Into::into),
        collapsible,
    )
}

/// Review finding (ADR 0138): swapping Folders with Message in the default
/// mail view must not give the reader the folder pane's 260–480 px slot.
#[test]
fn swapping_default_panes_carries_their_widths_and_limits() {
    let folders = pane("260px", Some("200px"), Some("480px"), true);
    let list = pane("1fr", Some("300px"), None, false);
    let reader = pane("1.4fr", Some("320px"), None, true);
    let root = |text: &str| resolved(text).windows["mail"][0].root.clone();
    assert_eq!(
        limits(&root(DEFAULT_LAYOUT_KDL)),
        [folders.clone(), list.clone(), reader.clone()]
    );

    let swapped = apply(
        None,
        &[LayoutPatch::SwapPanes {
            a: mail(&[0]),
            b: mail(&[2]),
        }],
    );
    let tree = root(&swapped);
    assert_eq!(shape(&tree).split('[').nth(1), Some("reader"));
    assert_eq!(
        limits(&tree),
        [reader.clone(), list.clone(), folders.clone()]
    );

    // Docking Folders right of Message (same split) keeps them too.
    let docked = apply(
        None,
        &[LayoutPatch::MovePane {
            node: mail(&[0]),
            to: beside(mail(&[2]), DockSide::Right),
        }],
    );
    assert_eq!(
        limits(&root(&docked)),
        [list.clone(), reader.clone(), folders.clone()]
    );

    // Docking Folders left of Messages from the right end brings it back.
    let back = apply(
        Some(&docked),
        &[LayoutPatch::MovePane {
            node: mail(&[2]),
            to: beside(mail(&[0]), DockSide::Left),
        }],
    );
    assert_eq!(limits(&root(&back)), [folders, list, reader]);
}

/// Swapping nodes of containers of different kinds leaves the entries with
/// the slots (a stack's height is no split width).
#[test]
fn swapping_across_container_kinds_keeps_the_slot_entries() {
    let out = apply(
        Some(MAIL),
        &[LayoutPatch::SwapPanes {
            a: mail(&[0, 0]),
            b: mail(&[2]),
        }],
    );
    let root = &resolved(&out).windows["mail"][0].root;
    assert_eq!(sizes_of(root), ["240px", "1fr", "1.4fr"]);
    assert_eq!(sizes_of(root.at_path(&[0]).unwrap()), ["1fr", "auto"]);
}

/// Presets keep each pane's limits (review finding, ADR 0138), also after a
/// preset that put the pane into a split of the other orientation.
#[test]
fn presets_keep_the_pane_limits() {
    let below = apply(
        None,
        &[LayoutPatch::ApplyPreset {
            preset: "reader-below".into(),
            target: mail(&[]),
        }],
    );
    let root = &resolved(&below).windows["mail"][0].root;
    assert_eq!(
        limits(root),
        [
            pane("260px", Some("200px"), Some("480px"), true),
            pane("1fr", None, None, false)
        ]
    );
    // The vertical split gets no horizontal limits.
    assert_eq!(
        limits(root.at_path(&[1]).unwrap()),
        [
            pane("1fr", None, None, false),
            pane("1.2fr", None, None, false)
        ]
    );
    let three = apply(
        Some(&below),
        &[LayoutPatch::ApplyPreset {
            preset: "three-panes".into(),
            target: mail(&[]),
        }],
    );
    assert_eq!(
        limits(&resolved(&three).windows["mail"][0].root),
        limits(&default_layout().windows["mail"][0].root)
    );
}

#[test]
fn docking_beside_the_root_wraps_the_root() {
    let out = apply(
        Some(MAIL),
        &[LayoutPatch::MovePane {
            node: mail(&[2]),
            to: beside(mail(&[]), DockSide::Bottom),
        }],
    );
    golden("dock_beside_root", &out);
    assert_eq!(
        mail_shape(&out),
        "split[panes[sidebar[folders gmail-labels] list-pane[list-header list]] reader[reader-toolbar message]]"
    );
}

#[test]
fn a_split_left_with_one_child_is_replaced_by_it() {
    let file = r#"layout "Two" extends="default" {
    window "mail" {
        // Only two panes.
        split "horizontal" id="panes" sizes="1fr 2fr" {
            message-list id="list"
            // The reader.
            message-view id="message"
        }
    }
}
"#;
    let out = apply(Some(file), &[LayoutPatch::RemoveNode { node: mail(&[0]) }]);
    golden("remove_collapses_split", &out);
    assert_eq!(mail_shape(&out), "message");
    // The root itself is never removed.
    assert_eq!(
        refused(Some(&out), LayoutPatch::RemoveNode { node: mail(&[]) }),
        (
            ErrorCode::InvalidRequest,
            "edit-issue-invalid-placement".into()
        )
    );
}

#[test]
fn removing_nodes_tidies_empty_containers() {
    let out = apply(
        Some(MAIL),
        &[
            LayoutPatch::RemoveNode {
                node: mail(&[1, 0]),
            },
            LayoutPatch::RemoveNode {
                node: mail(&[1, 0]),
            },
        ],
    );
    golden("remove_empties_stack", &out);
    let root = &resolved(&out).windows["mail"][0].root;
    assert_eq!(
        shape(root),
        "panes[sidebar[folders gmail-labels] reader[reader-toolbar message]]"
    );
    assert_eq!(sizes_of(root), ["240px", "1.4fr"]);
}

#[test]
fn widgets_from_the_palette_get_ids_and_size_entries() {
    let widget = |kind: &str, options: serde_json::Value| NewWidget {
        kind: kind.into(),
        id: None,
        label: None,
        options: options.as_object().cloned().unwrap_or_default(),
    };
    let out = apply(
        Some(MAIL),
        &[
            LayoutPatch::AddWidget {
                widget: widget("mini-month", json!({})),
                to: into(mail(&[0]), 1),
            },
            LayoutPatch::AddWidget {
                widget: widget("toolbar", json!({"toolbar": "reader"})),
                to: into(mail(&[1]), 0),
            },
            LayoutPatch::AddWidget {
                widget: NewWidget {
                    label: Some("Due".into()),
                    ..widget("plugin-panel", json!({"panel": "follow-up.due"}))
                },
                to: beside(mail(&[2, 1]), DockSide::Bottom),
            },
            LayoutPatch::AddWidget {
                widget: widget("mini-month", json!({})),
                to: into(mail(&[0]), 99),
            },
        ],
    );
    golden("add_widgets", &out);
    let root = &resolved(&out).windows["mail"][0].root;
    assert_eq!(
        shape(root),
        "panes[sidebar[folders mini-month gmail-labels mini-month-2] \
         list-pane[toolbar list-header list] reader[reader-toolbar split[message plugin-panel]]]"
    );
    assert_eq!(
        sizes_of(root.at_path(&[0]).unwrap()),
        ["1fr", "1fr", "auto", "1fr"]
    );
    assert_eq!(
        sizes_of(root.at_path(&[1]).unwrap()),
        ["auto", "auto", "1fr"]
    );
    // Docked below a node of a stack: wrapped in a new vertical split.
    let LayoutNode::Widget { options, label, .. } = root.at_path(&[2, 1, 1]).unwrap() else {
        panic!("expected the panel");
    };
    assert_eq!(options["panel"], json!("follow-up.due"));
    assert_eq!(label.as_deref(), Some("Due"));
}

#[test]
fn palette_widgets_are_checked() {
    let (code, id) = refused(
        Some(MAIL),
        LayoutPatch::AddWidget {
            widget: NewWidget {
                kind: "no-such-widget".into(),
                id: None,
                label: None,
                options: serde_json::Map::new(),
            },
            to: into(mail(&[0]), 0),
        },
    );
    assert_eq!(
        (code, id.as_str()),
        (ErrorCode::InvalidRequest, "edit-issue-unknown-widget")
    );
    // Options are checked like the file's: the config's own error.
    let (code, id) = refused(
        Some(MAIL),
        LayoutPatch::AddWidget {
            widget: NewWidget {
                kind: "folder-tree".into(),
                id: None,
                label: None,
                options: json!({"no-such-option": true})
                    .as_object()
                    .cloned()
                    .unwrap(),
            },
            to: into(mail(&[0]), 0),
        },
    );
    assert_eq!(code, ErrorCode::InvalidRequest);
    assert!(id.starts_with("config-option-unknown"), "{id}");
    // Into a widget.
    let (_, id) = refused(
        Some(MAIL),
        LayoutPatch::AddWidget {
            widget: NewWidget {
                kind: "mini-month".into(),
                id: None,
                label: None,
                options: serde_json::Map::new(),
            },
            to: into(mail(&[0, 0]), 0),
        },
    );
    assert_eq!(id, "edit-issue-invalid-placement");
}

#[test]
fn hiding_and_showing_writes_only_the_property() {
    let hidden = apply(
        Some(MAIL),
        &[LayoutPatch::SetHidden {
            node: mail(&[0]),
            hidden: true,
        }],
    );
    golden("set_hidden", &hidden);
    assert_eq!(
        hidden,
        MAIL.replace(
            "stack id=\"sidebar\" sizes=\"1fr auto\" {",
            "stack id=\"sidebar\" sizes=\"1fr auto\" hidden=#true {"
        )
    );
    assert!(
        resolved(&hidden).windows["mail"][0]
            .root
            .at_path(&[0])
            .unwrap()
            .is_hidden()
    );
    let shown = apply(
        Some(&hidden),
        &[LayoutPatch::SetHidden {
            node: mail(&[0]),
            hidden: false,
        }],
    );
    assert_eq!(shown, MAIL);
}

#[test]
fn bad_references_are_refused() {
    let cases = [
        (
            LayoutPatch::MovePane {
                node: mail(&[]),
                to: into(mail(&[0]), 0),
            },
            "edit-issue-invalid-placement",
        ),
        (
            LayoutPatch::MovePane {
                node: mail(&[0]),
                to: into(mail(&[0]), 0),
            },
            "edit-issue-invalid-placement",
        ),
        (
            LayoutPatch::MovePane {
                node: mail(&[1]),
                to: beside(mail(&[1, 1]), DockSide::Left),
            },
            "edit-issue-invalid-placement",
        ),
        (
            LayoutPatch::MovePane {
                node: mail(&[0]),
                to: into(at("mail", 1, &[]), 0),
            },
            "edit-issue-invalid-placement",
        ),
        (
            LayoutPatch::SwapPanes {
                a: mail(&[1]),
                b: mail(&[1, 0]),
            },
            "edit-issue-invalid-placement",
        ),
        (
            LayoutPatch::SetHidden {
                node: mail(&[7]),
                hidden: true,
            },
            "edit-issue-node-not-found",
        ),
        (
            // A widget's `row` is not a layout node.
            LayoutPatch::RemoveNode {
                node: mail(&[1, 1, 0]),
            },
            "edit-issue-node-not-found",
        ),
        (
            LayoutPatch::SetHidden {
                node: at("mail", 5, &[]),
                hidden: true,
            },
            "edit-issue-node-not-found",
        ),
        (
            LayoutPatch::SetHidden {
                node: at("no-such-window", 0, &[]),
                hidden: true,
            },
            "edit-issue-node-not-found",
        ),
    ];
    for (patch, expected) in cases {
        let (code, id) = refused(Some(MAIL), patch.clone());
        assert_eq!(
            (code, id.as_str()),
            (ErrorCode::InvalidRequest, expected),
            "{patch:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Presets and reset

#[test]
fn presets_rearrange_the_panes_of_a_default_window() {
    let below = apply(
        None,
        &[LayoutPatch::ApplyPreset {
            preset: "reader-below".into(),
            target: mail(&[]),
        }],
    );
    golden("preset_reader_below", &below);
    let root = &resolved(&below).windows["mail"][0].root;
    let panes = |n: &LayoutNode| -> Vec<String> {
        n.children()
            .iter()
            .map(|c| c.id().unwrap_or("?").to_owned())
            .collect()
    };
    assert_eq!(root.id(), Some("panes"));
    assert_eq!(panes(root), ["sidebar", "?"]);
    assert_eq!(panes(root.at_path(&[1]).unwrap()), ["list-pane", "reader"]);
    assert_eq!(sizes_of(root), ["260px", "1fr"]);
    // The narrow breakpoint is untouched.
    let narrow = |l: &ResolvedLayout| {
        let mut v = serde_json::to_value(&l.windows["mail"][1]).unwrap();
        strip_sources(&mut v);
        v
    };
    assert_eq!(narrow(&resolved(&below)), narrow(&default_layout()));

    let focus = apply(
        Some(&below),
        &[LayoutPatch::ApplyPreset {
            preset: "focus".into(),
            target: mail(&[]),
        }],
    );
    golden("preset_focus", &focus);
    let shape_of = |text: &str| {
        let root = &resolved(text).windows["mail"][0].root;
        let mut s = shape(root);
        s.truncate(s.find('[').unwrap_or(s.len()));
        (
            s,
            root.children()
                .iter()
                .map(|c| c.id().unwrap_or("?").to_owned() + if c.is_hidden() { "!" } else { "" })
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(
        shape_of(&focus),
        (
            "panes".to_owned(),
            vec![
                "reader".to_owned(),
                "sidebar!".to_owned(),
                "list-pane!".to_owned()
            ]
        )
    );
    // Applying it again changes nothing.
    let again = apply(
        Some(&focus),
        &[LayoutPatch::ApplyPreset {
            preset: "focus".into(),
            target: mail(&[]),
        }],
    );
    assert_eq!(again, focus);
    // Back to three panes: every pane is shown again.
    let three = apply(
        Some(&focus),
        &[LayoutPatch::ApplyPreset {
            preset: "three-panes".into(),
            target: mail(&[]),
        }],
    );
    golden("preset_three_panes_after_focus", &three);
    let root = &resolved(&three).windows["mail"][0].root;
    assert_eq!(panes(root), ["sidebar", "list-pane", "reader"]);
    root.walk(&mut |n| assert!(!n.is_hidden(), "{n:?}"));
    assert_eq!(sizes_of(root), ["260px", "1fr", "1.4fr"]);
}

#[test]
fn presets_are_checked_against_the_window() {
    let (_, id) = refused(
        None,
        LayoutPatch::ApplyPreset {
            preset: "nope".into(),
            target: mail(&[]),
        },
    );
    assert_eq!(id, "edit-issue-unknown-preset");
    // A preset of another window.
    let (_, id) = refused(
        None,
        LayoutPatch::ApplyPreset {
            preset: "focus".into(),
            target: at("calendar", 0, &[]),
        },
    );
    assert_eq!(id, "edit-issue-unknown-preset");
    // A replaced mail window without the preset's panes.
    let file = "layout \"x\" extends=\"default\" {\n    window \"mail\" {\n        message-list id=\"list\"\n    }\n}\n";
    let (code, id) = refused(
        Some(file),
        LayoutPatch::ApplyPreset {
            preset: "three-panes".into(),
            target: mail(&[]),
        },
    );
    assert_eq!(
        (code, id.as_str()),
        (ErrorCode::InvalidRequest, "edit-issue-preset-pane-missing")
    );
}

#[test]
fn resetting_a_window_removes_it_from_a_sparse_file() {
    let out = apply(
        Some(MAIL),
        &[LayoutPatch::ResetWindow {
            window: "mail".into(),
        }],
    );
    golden("reset_window_sparse", &out);
    assert_eq!(
        resolved(&out).windows["mail"],
        default_layout().windows["mail"]
    );
    // Nothing to reset: unchanged.
    let same = apply(
        Some(&out),
        &[LayoutPatch::ResetWindow {
            window: "mail".into(),
        }],
    );
    assert_eq!(same, out);
}

#[test]
fn resetting_a_window_of_a_full_file_writes_the_default_back_in_place() {
    let file =
        DEFAULT_LAYOUT_KDL.replacen("sizes=\"260px 1fr 1.4fr\"", "sizes=\"300px 1fr 1fr\"", 1);
    assert_ne!(file, DEFAULT_LAYOUT_KDL);
    let out = apply(
        Some(&file),
        &[LayoutPatch::ResetWindow {
            window: "mail".into(),
        }],
    );
    let ours = resolved(&out);
    let defaults = default_layout();
    let strip = |l: &ResolvedLayout| {
        let mut v = serde_json::to_value(&l.windows).unwrap();
        strip_sources(&mut v);
        v
    };
    assert_eq!(strip(&ours), strip(&defaults));
    assert_eq!(
        ours.windows.keys().collect::<Vec<_>>(),
        defaults.windows.keys().collect::<Vec<_>>(),
        "the windows keep their order"
    );
}

fn strip_sources(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.remove("source");
            map.values_mut().for_each(strip_sources);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(strip_sources),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Toolbars

fn slot(area: ToolbarArea, index: u32) -> ToolbarSlot {
    ToolbarSlot { area, index }
}

#[test]
fn toolbar_items_move_between_the_bar_and_more() {
    let out = apply(
        Some(MAIL),
        &[
            // The ranked forward button into "More" (loses its rank).
            LayoutPatch::MoveToolbarItem {
                toolbar: "reader".into(),
                from: slot(ToolbarArea::Bar, 1),
                to: slot(ToolbarArea::More, 0),
            },
            // Archive (with its label) onto the bar, first.
            LayoutPatch::MoveToolbarItem {
                toolbar: "reader".into(),
                from: slot(ToolbarArea::More, 1),
                to: slot(ToolbarArea::Bar, 0),
            },
            // Delete to the front of the bar.
            LayoutPatch::MoveToolbarItem {
                toolbar: "reader".into(),
                from: slot(ToolbarArea::Bar, 2),
                to: slot(ToolbarArea::Bar, 0),
            },
        ],
    );
    golden("toolbar_moves", &out);
    let layout = resolved(&out);
    let items = &layout.toolbars["reader"];
    let names: Vec<String> = items.iter().map(item_name).collect();
    assert_eq!(names, ["mail.delete", "mail.archive", "mail.reply", "more"]);
    let ToolbarItem::More { items: menu, .. } = &items[3] else {
        panic!("expected More");
    };
    assert_eq!(
        menu,
        &[nail_api::MenuItem::Action {
            action: "mail.forward".into(),
            args: vec![],
            label: None,
            icon: None,
        }]
    );
    let ToolbarItem::Action { label, .. } = &items[1] else {
        panic!("expected archive");
    };
    assert_eq!(label.as_deref(), Some("Archive it"));
}

fn item_name(item: &ToolbarItem) -> String {
    match item {
        ToolbarItem::Action { action, .. } => action.clone(),
        ToolbarItem::More { .. } => "more".into(),
        ToolbarItem::Spacer => "spacer".into(),
        ToolbarItem::Separator => "separator".into(),
    }
}

#[test]
fn toolbar_items_are_added_and_removed() {
    let out = apply(
        Some(MAIL),
        &[
            LayoutPatch::AddToolbarItem {
                toolbar: "reader".into(),
                item: ToolbarItem::Separator,
                to: slot(ToolbarArea::Bar, 1),
            },
            LayoutPatch::AddToolbarItem {
                toolbar: "reader".into(),
                item: ToolbarItem::Action {
                    action: "mail.move".into(),
                    args: vec![],
                    label: None,
                    icon: Some("folder-input".into()),
                    rank: Some(2),
                    display: None,
                },
                to: slot(ToolbarArea::More, 99),
            },
            LayoutPatch::RemoveToolbarItem {
                toolbar: "reader".into(),
                from: slot(ToolbarArea::Bar, 0),
            },
        ],
    );
    golden("toolbar_add_remove", &out);
    let layout = resolved(&out);
    let names: Vec<String> = layout.toolbars["reader"].iter().map(item_name).collect();
    assert_eq!(names, ["separator", "mail.forward", "mail.delete", "more"]);
    // A toolbar without "More" gets one at the end of the bar.
    let file = "layout \"x\" extends=\"default\" {\n    toolbar \"reader\" {\n        item \"mail.reply\"\n    }\n}\n";
    let out = apply(
        Some(file),
        &[LayoutPatch::MoveToolbarItem {
            toolbar: "reader".into(),
            from: slot(ToolbarArea::Bar, 0),
            to: slot(ToolbarArea::More, 0),
        }],
    );
    assert_eq!(
        out,
        "layout \"x\" extends=\"default\" {\n    toolbar \"reader\" {\n        more {\n            item \"mail.reply\"\n        }\n    }\n}\n"
    );
}

#[test]
fn a_default_toolbar_is_copied_into_a_sparse_file() {
    let out = apply(
        None,
        &[LayoutPatch::RemoveToolbarItem {
            toolbar: "reader".into(),
            from: slot(ToolbarArea::Bar, 0),
        }],
    );
    golden("toolbar_copied_from_defaults", &out);
    let layout = resolved(&out);
    let defaults = default_layout();
    assert_eq!(
        layout.toolbars["reader"],
        defaults.toolbars["reader"][1..].to_vec()
    );
    assert_eq!(
        layout.toolbar_settings["reader"], defaults.toolbar_settings["reader"],
        "hide-disabled stays"
    );
}

#[test]
fn bad_toolbar_slots_are_refused() {
    let cases = [
        LayoutPatch::MoveToolbarItem {
            toolbar: "reader".into(),
            from: slot(ToolbarArea::Bar, 9),
            to: slot(ToolbarArea::Bar, 0),
        },
        LayoutPatch::RemoveToolbarItem {
            toolbar: "reader".into(),
            from: slot(ToolbarArea::More, 3),
        },
        LayoutPatch::AddToolbarItem {
            toolbar: "reader".into(),
            item: ToolbarItem::Spacer,
            to: slot(ToolbarArea::More, 0),
        },
        LayoutPatch::AddToolbarItem {
            toolbar: "reader".into(),
            item: ToolbarItem::More {
                label: None,
                icon: None,
                items: vec![],
            },
            to: slot(ToolbarArea::Bar, 0),
        },
        LayoutPatch::RemoveToolbarItem {
            toolbar: "no-such-toolbar".into(),
            from: slot(ToolbarArea::Bar, 0),
        },
        LayoutPatch::RemoveToolbarItem {
            toolbar: "Not Kebab".into(),
            from: slot(ToolbarArea::Bar, 0),
        },
    ];
    for patch in cases {
        let (code, id) = refused(Some(MAIL), patch.clone());
        assert_eq!(
            (code, id.as_str()),
            (ErrorCode::InvalidRequest, "edit-issue-toolbar-slot-invalid"),
            "{patch:?}"
        );
    }
    // A spacer cannot move into "More" either.
    let file =
        "layout \"x\" extends=\"default\" {\n    toolbar \"reader\" {\n        spacer\n    }\n}\n";
    let (_, id) = refused(
        Some(file),
        LayoutPatch::MoveToolbarItem {
            toolbar: "reader".into(),
            from: slot(ToolbarArea::Bar, 0),
            to: slot(ToolbarArea::More, 0),
        },
    );
    assert_eq!(id, "edit-issue-toolbar-slot-invalid");
}

// ---------------------------------------------------------------------------
// Line endings and indentation

#[test]
fn crlf_and_tab_files_keep_their_style() {
    let file = MAIL.replace("    ", "\t").replace('\n', "\r\n");
    let out = apply(
        Some(&file),
        &[
            LayoutPatch::MovePane {
                node: mail(&[1, 1]),
                to: beside(mail(&[2]), DockSide::Top),
            },
            LayoutPatch::AddWidget {
                widget: NewWidget {
                    kind: "mini-month".into(),
                    id: Some("month".into()),
                    label: None,
                    options: serde_json::Map::new(),
                },
                to: into(mail(&[0]), 0),
            },
            LayoutPatch::MoveToolbarItem {
                toolbar: "reader".into(),
                from: slot(ToolbarArea::Bar, 0),
                to: slot(ToolbarArea::More, 0),
            },
        ],
    );
    assert!(
        !out.replace("\r\n", "").contains('\n'),
        "only CRLF: {out:?}"
    );
    assert!(!out.contains("    "), "only tabs: {out:?}");
    golden("crlf_tabs", &out.replace("\r\n", "\n"));
}

// ---------------------------------------------------------------------------
// Through the manager: undo history and changes on disk

#[test]
fn editor_patches_are_undoable_and_check_the_base_revision() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("layout.kdl"), MAIL).unwrap();
    let manager = ConfigManager::load(dir.path());
    let revision = config_revision(Some(MAIL));
    assert_eq!(
        manager.edit_history(HistoryScope::Layout).revision,
        revision
    );

    let swap = LayoutPatch::SwapPanes {
        a: mail(&[0]),
        b: mail(&[2]),
    };
    manager
        .update_layout_checked(std::slice::from_ref(&swap), Some(&revision))
        .unwrap();
    let written = std::fs::read_to_string(dir.path().join("layout.kdl")).unwrap();
    assert_ne!(written, MAIL);
    let history = manager.edit_history(HistoryScope::Layout);
    assert_eq!(history.undo.len(), 1);
    assert_eq!(history.undo[0].kind, EditKind::SwapPanes);
    assert_eq!(history.revision, config_revision(Some(&written)));

    // The editor still holds the old revision: conflict, nothing written.
    let err = manager
        .update_layout_checked(&[swap], Some(&revision))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("layout.kdl")).unwrap(),
        written
    );

    // Several kinds at once are one "layout edits" entry.
    manager
        .update_layout(&[
            LayoutPatch::SetHidden {
                node: mail(&[1]),
                hidden: true,
            },
            LayoutPatch::RemoveToolbarItem {
                toolbar: "reader".into(),
                from: slot(ToolbarArea::Bar, 0),
            },
        ])
        .unwrap();
    let history = manager.edit_history(HistoryScope::Layout);
    assert_eq!(history.undo.last().unwrap().kind, EditKind::LayoutEdits);

    // Undo twice: the original file, byte for byte.
    manager.undo_edit(HistoryScope::Layout, false).unwrap();
    manager.undo_edit(HistoryScope::Layout, false).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("layout.kdl")).unwrap(),
        MAIL
    );
    // A hand edit since: redo is a conflict, `force` backs it up first.
    let hand = MAIL.replace("// Messages.", "// Messages (edited by hand).");
    std::fs::write(dir.path().join("layout.kdl"), &hand).unwrap();
    let err = manager.redo_edit(HistoryScope::Layout, false).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    manager.redo_edit(HistoryScope::Layout, true).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("layout.kdl")).unwrap(),
        written
    );
    let backups = manager.list_backups(nail_api::ConfigFile::Layout).unwrap();
    assert!(!backups.is_empty());
}

#[test]
fn a_failed_patch_writes_nothing_and_records_nothing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("layout.kdl"), MAIL).unwrap();
    let manager = ConfigManager::load(dir.path());
    let err = manager
        .update_layout(&[
            LayoutPatch::SetHidden {
                node: mail(&[0]),
                hidden: true,
            },
            LayoutPatch::RemoveNode { node: mail(&[]) },
        ])
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("layout.kdl")).unwrap(),
        MAIL
    );
    assert!(manager.edit_history(HistoryScope::Layout).undo.is_empty());
}
