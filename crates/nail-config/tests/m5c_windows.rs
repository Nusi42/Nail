//! M5c WP170 (ADR 0116): the window-model properties of `window` nodes —
//! `size=`, `min-size=`, `modal=`, `resizable=`, `pop-out=`, `instances=` —
//! read, merged onto the defaults field by field, resolved into
//! `ResolvedLayout::window_frames`, and kept by the merge dialog and the
//! write-back.

use nail_api::customize::{ConfigFile, LayoutNodeRef, LayoutPatch, MergeChoice, MergeTake};
use nail_api::windows::{
    DEFAULT_WINDOW_SPEC, WindowFrame, WindowInstances, WindowSize, WindowSpec,
};
use nail_config::{
    DEFAULT_LAYOUT_KDL, apply_layout_patches, default_layout, parse_layout, parse_layout_all,
    preview_merge, resolve_merge,
};
use pretty_assertions::assert_eq;

#[test]
fn the_default_layout_sizes_its_windows_and_allows_several_composers() {
    let layout = default_layout();
    let composer = layout.window_frames["composer"];
    assert_eq!(composer.instances, Some(WindowInstances::Multiple));
    assert_eq!(composer.size, Some(WindowSize::new(820, 720)));
    assert_eq!(composer.min_size, Some(WindowSize::new(520, 400)));
    // No default window is modal (only confirmations are, and those are
    // feature dialogs).
    assert!(layout.window_frames.values().all(|f| f.modal != Some(true)));
    // Every secondary window has a size; the main and mail windows have none.
    for name in [
        "calendar",
        "contacts",
        "tasks",
        "settings",
        "composer",
        "outbox",
        "move",
        "account-setup",
        "rules",
        "snoozed",
    ] {
        let frame = layout.window_frames.get(name).expect(name);
        assert!(frame.size.is_some() && frame.min_size.is_some(), "{name}");
        assert_eq!(frame.issue(), None, "{name}");
    }
    assert!(!layout.window_frames.contains_key("main"));
    assert!(!layout.window_frames.contains_key("mail"));
    // The resolved spec: the default spec with the layout's properties.
    let spec = layout.window_spec("composer", DEFAULT_WINDOW_SPEC);
    assert_eq!(
        spec,
        WindowSpec {
            size: WindowSize::new(820, 720),
            min_size: WindowSize::new(520, 400),
            instances: WindowInstances::Multiple,
            pop_out: true,
            ..DEFAULT_WINDOW_SPEC
        }
    );
    assert!(!layout.window_spec("move", DEFAULT_WINDOW_SPEC).pop_out);
    // Popping out is opt-in: only windows that work from core data alone
    // (or hand their state over, the composer) may pop out.
    assert!(
        !layout
            .window_spec("pim-event-editor", DEFAULT_WINDOW_SPEC)
            .pop_out
    );
    for name in ["composer", "settings", "rules", "outbox", "snoozed"] {
        assert!(
            layout.window_spec(name, DEFAULT_WINDOW_SPEC).pop_out,
            "{name}"
        );
    }
    assert!(
        !layout
            .window_spec("account-setup", DEFAULT_WINDOW_SPEC)
            .pop_out
    );
    // A kind the layout does not describe keeps the base.
    assert_eq!(
        layout.window_spec("pim-event-editor", DEFAULT_WINDOW_SPEC),
        DEFAULT_WINDOW_SPEC
    );
}

#[test]
fn a_sparse_file_overrides_window_properties_field_by_field() {
    let layout = parse_layout(
        r#"layout "Mine" extends="default" {
    window "composer" size="900x700" {
        composer id="composer"
    }
    // A replaced window without properties keeps the default's.
    window "rules" {
        rule-manager id="rules"
    }
    window "my-tool" modal=#true resizable=#false pop-out=#false instances="single" {
        outbox id="tool"
    }
}
"#,
        "layout.kdl",
    )
    .unwrap();
    let composer = layout.window_frames["composer"];
    assert_eq!(composer.size, Some(WindowSize::new(900, 700)));
    assert_eq!(composer.min_size, Some(WindowSize::new(520, 400)));
    assert_eq!(composer.instances, Some(WindowInstances::Multiple));
    assert_eq!(
        layout.window_frames["rules"].size,
        Some(WindowSize::new(900, 640))
    );
    assert_eq!(
        layout.window_frames["my-tool"],
        WindowFrame {
            modal: Some(true),
            resizable: Some(false),
            pop_out: Some(false),
            instances: Some(WindowInstances::Single),
            ..WindowFrame::default()
        }
    );
}

#[test]
fn properties_across_breakpoints_merge_and_are_checked() {
    let layout = parse_layout(
        r#"layout "Mine" extends="default" {
    breakpoint max-width=799 {
        window "tool" size="400x300" {
            outbox id="narrow"
        }
    }
    breakpoint min-width=800 {
        window "tool" min-size="300x200" {
            outbox id="wide"
        }
    }
}
"#,
        "layout.kdl",
    )
    .unwrap();
    let frame = layout.window_frames["tool"];
    assert_eq!(frame.size, Some(WindowSize::new(400, 300)));
    assert_eq!(frame.min_size, Some(WindowSize::new(300, 200)));

    let errors = |text: &str| -> Vec<String> {
        parse_layout_all(text, "layout.kdl")
            .unwrap_err()
            .into_iter()
            .map(|e| e.localized.id.clone())
            .collect()
    };
    let wrap = |props: &str| {
        format!(
            "layout \"Mine\" extends=\"default\" {{\n    window \"tool\" {props} {{\n        outbox id=\"x\"\n    }}\n}}\n"
        )
    };
    assert_eq!(
        errors(&wrap(r#"size="big""#)),
        ["config-window-size-invalid"]
    );
    assert_eq!(
        errors(&wrap(r#"size="50x400""#)),
        ["config-window-frame-size-out-of-range"]
    );
    assert_eq!(
        errors(&wrap(r#"size="400x300" min-size="500x200""#)),
        ["config-window-frame-below-minimum"]
    );
    assert_eq!(
        errors(&wrap(r#"instances="many""#)),
        ["config-option-one-of"]
    );
    assert_eq!(errors(&wrap("modal=1")).len(), 1);
    assert_eq!(errors(&wrap("floating=#true")).len(), 1);
}

#[test]
fn the_merge_dialog_shows_and_takes_window_properties() {
    let incoming = r#"layout "Theirs" extends="default" {
    window "composer" size="1000x800" min-size="600x500" modal=#false instances="multiple" {
        composer id="composer"
    }
}
"#;
    let preview = preview_merge(ConfigFile::Layout, None, incoming).unwrap();
    assert!(preview.errors.is_empty(), "{:?}", preview.errors);
    let item = preview
        .sections
        .iter()
        .flat_map(|s| s.items.iter())
        .find(|item| item.id == "windows:composer")
        .expect("the composer window is a merge item");
    let theirs = item.theirs.as_deref().unwrap();
    assert!(
        theirs.starts_with(
            // `pop-out` comes from the default layout it extends.
            r#"window "composer" size="1000x800" min-size="600x500" modal=#false pop-out=#true instances="multiple" {"#
        ),
        "{theirs}"
    );
    // The default side shows the shipped properties.
    assert!(
        item.mine
            .as_deref()
            .unwrap()
            .contains(r#"size="820x720" min-size="520x400" pop-out=#true instances="multiple""#)
    );
    let outcome = resolve_merge(
        ConfigFile::Layout,
        None,
        incoming,
        &[MergeChoice {
            item_id: item.id.clone(),
            take: MergeTake::Theirs,
        }],
        MergeTake::Mine,
    )
    .unwrap();
    let layout = outcome.layout.unwrap();
    assert_eq!(
        layout.window_frames["composer"].size,
        Some(WindowSize::new(1000, 800))
    );
    assert_eq!(
        parse_layout(&outcome.text, "layout.kdl")
            .unwrap()
            .window_frames["composer"],
        layout.window_frames["composer"]
    );
}

#[test]
fn the_write_back_keeps_window_properties() {
    // Dragging the composer's divider copies the default window into the
    // user's sparse file; its properties come along.
    let patched = apply_layout_patches(
        None,
        &[LayoutPatch::SplitSizes {
            node: LayoutNodeRef {
                window: "composer".into(),
                breakpoint: 0,
                path: vec![],
            },
            sizes: vec!["1fr".into(), "200px".into()],
        }],
        "layout.kdl",
    )
    .unwrap();
    assert!(
        patched.contains(
            r#"window "composer" size="820x720" min-size="520x400" pop-out=#true instances="multiple""#
        ),
        "{patched}"
    );
    let layout = parse_layout(&patched, "layout.kdl").unwrap();
    assert_eq!(
        layout.window_frames["composer"].instances,
        Some(WindowInstances::Multiple)
    );
    // The shipped text itself carries the properties.
    assert!(DEFAULT_LAYOUT_KDL.contains(r#"instances="multiple""#));
}
