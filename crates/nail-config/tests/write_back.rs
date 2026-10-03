//! Layout write-back: UI edits land in `layout.kdl` with comments and
//! formatting preserved. Each case compares against a golden file in
//! `tests/golden/write_back/`; run with `NAIL_UPDATE_GOLDEN=1` to rewrite
//! them after an intended change (and review the diff).

use std::path::PathBuf;

use nail_api::{LayoutNodeRef, LayoutPatch, ToolbarItem};
use nail_config::{EditError, apply_layout_patches, default_layout, parse_layout_all};
use pretty_assertions::assert_eq;
use serde_json::json;

fn golden(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/write_back")
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

fn apply(text: Option<&str>, patches: &[LayoutPatch]) -> String {
    let out = apply_layout_patches(text, patches, "layout.kdl")
        .unwrap_or_else(|e| panic!("patch failed: {e}"));
    parse_layout_all(&out, "layout.kdl").unwrap_or_else(|e| panic!("invalid result: {e:?}"));
    out
}

const COMMENTED: &str = r#"// My layout: only what differs from the default.
layout "Mine" extends="default" {
    // Two panes are enough for me.
    window "main" {
        split "horizontal" id="panes" \
            sizes="300px 1fr" /* tuned by hand */ \
            min-sizes="200px none" {
            folder-tree id="folders" show-unread=#true // unread counts
            message-list id="list" page-size=50 {
                row "{from.name}  {date | relative}"
            }
        }
    }

    keys context="list" {
        // Archive like in my old client.
        bind "E" "mail.archive"   // capital on purpose
        bind "d" "mail.delete"
    }

    // Reader buttons.
    toolbar "reader" {
        item "mail.reply" // most used
        item "mail.forward"

        // Destructive ones at the end.
        item "mail.delete"
    }
}
"#;

#[test]
fn dragging_a_divider_changes_only_the_sizes() {
    let out = apply(
        Some(COMMENTED),
        &[LayoutPatch::SplitSizes {
            node: at("main", 0, &[]),
            sizes: vec!["320px".into(), "2fr".into()],
        }],
    );
    golden("split_sizes", &out);
    assert_eq!(
        out,
        COMMENTED.replace("sizes=\"300px 1fr\"", "sizes=\"320px 2fr\""),
        "nothing else changes"
    );
}

#[test]
fn a_window_from_the_defaults_is_copied_before_it_changes() {
    let out = apply(
        Some("layout \"Mine\" extends=\"default\" {\n    // nothing yet\n}\n"),
        &[LayoutPatch::SplitSizes {
            node: at("mail", 0, &[]),
            sizes: vec!["200px".into(), "1fr".into(), "1fr".into()],
        }],
    );
    golden("copy_default_window", &out);
    let layout = parse_layout_all(&out, "layout.kdl").unwrap();
    let defaults = default_layout();
    assert_eq!(layout.windows["mail"].len(), defaults.windows["mail"].len());
    // Only the addressed split (the mail window's panes, M5b) changed.
    let json = |l: &nail_api::ResolvedLayout| serde_json::to_value(&l.windows["mail"]).unwrap();
    let (mut ours, theirs) = (json(&layout), json(&defaults));
    assert_eq!(ours[0]["root"]["sizes"], json!(["200px", "1fr", "1fr"]));
    ours[0]["root"]["sizes"] = theirs[0]["root"]["sizes"].clone();
    strip_sources(&mut ours);
    let mut theirs = theirs;
    strip_sources(&mut theirs);
    assert_eq!(ours, theirs);
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

#[test]
fn the_narrow_breakpoint_is_addressed_by_index() {
    let out = apply(
        None,
        &[LayoutPatch::SplitSizes {
            // The list pane inside the narrow mail window's tabs.
            node: at("mail", 1, &[1]),
            sizes: vec![
                "auto".into(),
                "auto".into(),
                "auto".into(),
                "auto".into(),
                "2fr".into(),
            ],
        }],
    );
    let layout = parse_layout_all(&out, "layout.kdl").unwrap();
    let root = serde_json::to_value(&layout.windows["mail"][1].root).unwrap();
    assert_eq!(
        root["children"][1]["sizes"],
        json!(["auto", "auto", "auto", "auto", "2fr"])
    );
    golden("new_file_narrow_stack", &out);
}

#[test]
fn column_order_is_written_to_the_list_widget() {
    let out = apply(
        Some(COMMENTED),
        &[LayoutPatch::ColumnOrder {
            node: at("main", 0, &[1]),
            columns: vec!["subject".into(), "from".into(), "date".into()],
        }],
    );
    assert!(
        out.contains("message-list id=\"list\" page-size=50 columns=\"subject from date\" {"),
        "{out}"
    );
    let err = apply_layout_patches(
        Some(COMMENTED),
        &[LayoutPatch::ColumnOrder {
            node: at("main", 0, &[]),
            columns: vec!["a".into()],
        }],
        "layout.kdl",
    )
    .unwrap_err();
    assert!(
        matches!(err, EditError::Invalid(m) if nail_config::l10n::english(&m).contains("container"))
    );
}

#[test]
fn toolbar_order_keeps_item_comments() {
    let item = |a: &str| ToolbarItem::Action {
        action: a.into(),
        args: vec![],
        label: None,
        icon: None,
        rank: None,
        display: None,
    };
    let out = apply(
        Some(COMMENTED),
        &[LayoutPatch::ToolbarOrder {
            toolbar: "reader".into(),
            items: vec![
                item("mail.forward"),
                item("mail.reply"),
                ToolbarItem::Spacer,
                item("mail.archive"),
                item("mail.delete"),
            ],
        }],
    );
    golden("toolbar_order", &out);
    let layout = parse_layout_all(&out, "layout.kdl").unwrap();
    assert_eq!(layout.toolbars["reader"].len(), 5);
    assert!(out.contains("item \"mail.reply\" // most used"));
    assert!(out.contains("// Destructive ones at the end.\n        item \"mail.delete\""));
}

#[test]
fn a_toolbar_from_the_defaults_is_added() {
    let out = apply(
        Some(COMMENTED),
        &[LayoutPatch::ToolbarOrder {
            toolbar: "main".into(),
            items: vec![ToolbarItem::Action {
                action: "mail.compose".into(),
                args: vec![],
                label: Some("@mail.action.compose".into()),
                icon: Some("compose".into()),
                rank: None,
                display: None,
            }],
        }],
    );
    assert!(out.ends_with(
        "    toolbar \"main\" {\n        item \"mail.compose\" label=\"@mail.action.compose\" icon=\"compose\"\n    }\n}\n"
    ), "{out}");
}

#[test]
fn key_bindings_are_replaced_added_and_removed() {
    let out = apply(
        Some(COMMENTED),
        &[
            // Replaces `bind "E"` (keys are compared normalized) in place.
            LayoutPatch::AddKeyBinding {
                context: "list".into(),
                key: "e".into(),
                action: "mail.move".into(),
                args: vec![],
            },
            // New binding in an existing block.
            LayoutPatch::AddKeyBinding {
                context: "list".into(),
                key: "ctrl+shift+m".into(),
                action: "view.toggle".into(),
                args: vec![json!("sidebar")],
            },
            // New context → new block.
            LayoutPatch::AddKeyBinding {
                context: "reader".into(),
                key: "x".into(),
                action: "mail.archive".into(),
                args: vec![],
            },
            // A user binding: removed.
            LayoutPatch::RemoveKeyBinding {
                context: "list".into(),
                key: "d".into(),
            },
            // A default binding: `unbind`.
            LayoutPatch::RemoveKeyBinding {
                context: "global".into(),
                key: "c".into(),
            },
        ],
    );
    golden("key_bindings", &out);
    let layout = parse_layout_all(&out, "layout.kdl").unwrap();
    let find = |ctx: &str, key: &str| {
        layout
            .keys
            .iter()
            .find(|k| k.context == ctx && k.key == key)
            .map(|k| (k.action.clone(), k.args.clone()))
    };
    assert_eq!(find("list", "e"), Some(("mail.move".into(), vec![])));
    assert_eq!(
        find("list", "ctrl+shift+m"),
        Some(("view.toggle".into(), vec![json!("sidebar")]))
    );
    assert_eq!(find("reader", "x"), Some(("mail.archive".into(), vec![])));
    assert_eq!(find("list", "d"), None);
    assert_eq!(find("global", "c"), None);
    assert!(
        out.contains("bind \"e\" \"mail.move\"   // capital on purpose"),
        "{out}"
    );
}

#[test]
fn removing_the_last_binding_of_a_block_removes_the_block() {
    let text = "layout \"x\" extends=\"default\" {\n    keys context=\"list\" {\n        bind \"q\" \"mail.archive\"\n    }\n}\n";
    let out = apply(
        Some(text),
        &[LayoutPatch::RemoveKeyBinding {
            context: "list".into(),
            key: "q".into(),
        }],
    );
    assert_eq!(out, "layout \"x\" extends=\"default\" {\n}\n");
    // Removing something that is not bound anywhere changes nothing.
    let same = apply(
        Some(text),
        &[LayoutPatch::RemoveKeyBinding {
            context: "list".into(),
            key: "z".into(),
        }],
    );
    assert_eq!(same, text);
}

#[test]
fn crlf_files_stay_crlf() {
    let text = COMMENTED.replace('\n', "\r\n");
    let out = apply(
        Some(&text),
        &[LayoutPatch::AddKeyBinding {
            context: "composer".into(),
            key: "ctrl+k".into(),
            action: "composer.send".into(),
            args: vec![],
        }],
    );
    assert!(!out.replace("\r\n", "").contains('\n'), "{out:?}");
    assert!(out.contains(
        "keys context=\"composer\" {\r\n        bind \"ctrl+k\" \"composer.send\"\r\n    }\r\n"
    ));
}

#[test]
fn bad_patches_are_refused_without_writing() {
    let cases = [
        LayoutPatch::SplitSizes {
            node: at("main", 0, &[]),
            sizes: vec!["1fr".into()],
        },
        LayoutPatch::SplitSizes {
            node: at("main", 5, &[]),
            sizes: vec!["1fr".into(), "1fr".into()],
        },
        LayoutPatch::SplitSizes {
            node: at("main", 0, &[0]),
            sizes: vec!["1fr".into()],
        },
        LayoutPatch::SplitSizes {
            node: at("main", 0, &[7]),
            sizes: vec!["1fr".into()],
        },
        LayoutPatch::SplitSizes {
            node: at("main", 0, &[]),
            sizes: vec!["1 fr".into(), "x".into()],
        },
        LayoutPatch::AddKeyBinding {
            context: "list".into(),
            key: "ctrl+".into(),
            action: "mail.archive".into(),
            args: vec![],
        },
        LayoutPatch::AddKeyBinding {
            context: "List".into(),
            key: "x".into(),
            action: "mail.archive".into(),
            args: vec![],
        },
        LayoutPatch::AddKeyBinding {
            context: "list".into(),
            key: "x".into(),
            action: "NotAnAction".into(),
            args: vec![],
        },
        LayoutPatch::ToolbarOrder {
            toolbar: "Bad Name".into(),
            items: vec![],
        },
    ];
    for patch in cases {
        let err = apply_layout_patches(Some(COMMENTED), std::slice::from_ref(&patch), "layout.kdl")
            .unwrap_err();
        assert!(matches!(err, EditError::Invalid(_)), "{patch:?}: {err:?}");
    }
    // A complete file (no `extends`) cannot borrow windows from the defaults.
    let complete = nail_config::DEFAULT_LAYOUT_KDL.replace("window \"outbox\"", "window \"box\"");
    let err = apply_layout_patches(
        Some(&complete),
        &[LayoutPatch::SplitSizes {
            node: at("outbox", 0, &[]),
            sizes: vec![],
        }],
        "layout.kdl",
    )
    .unwrap_err();
    assert!(
        matches!(err, EditError::Invalid(m) if nail_config::l10n::english(&m).contains("no window `outbox`"))
    );
}
