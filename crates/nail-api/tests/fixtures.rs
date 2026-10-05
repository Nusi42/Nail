//! Checks the hand-written default theme/layout fixtures.
//!
//! These fixtures are the target output of `nail-config`'s shipped defaults
//! (WP1) and the input of the renderer's tests (WP4), so beyond deserializing
//! they must be internally consistent: every token, action, icon, toolbar and
//! message id they reference must exist.
#![cfg(test)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use nail_api::config::layout::label_message_id;
use nail_api::config::theme::css_var_name;
use nail_api::{
    ErrorCode, LayoutNode, MenuItem, RailItem, ResolvedLayout, ResolvedTheme, ToolbarItem,
};
use pretty_assertions::assert_eq;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn theme() -> ResolvedTheme {
    serde_json::from_str(&read_fixture("default-theme.json")).expect("default-theme.json")
}

fn layout() -> ResolvedLayout {
    serde_json::from_str(&read_fixture("default-layout.json")).expect("default-layout.json")
}

/// Extracts every `var(--nail-…)` name from a CSS value.
fn var_refs(value: &str) -> Vec<String> {
    value
        .match_indices("var(")
        .map(|(i, _)| {
            let rest = &value[i + 4..];
            let end = rest.find([')', ',']).unwrap_or(rest.len());
            rest[..end].trim().to_owned()
        })
        .collect()
}

/// Message ids defined in all `.ftl` files of one locale directory.
fn fluent_ids(locale: &str) -> BTreeSet<String> {
    let dir = repo_root().join("locales").join(locale);
    let mut ids = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "ftl") {
            for line in std::fs::read_to_string(&path).unwrap().lines() {
                let Some((id, _)) = line.split_once('=') else {
                    continue;
                };
                let id = id.trim_end();
                let valid = id.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                    && id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
                if valid && !line.starts_with(char::is_whitespace) {
                    ids.insert(id.to_owned());
                }
            }
        }
    }
    ids
}

fn ftl_file_names(locale: &str) -> BTreeSet<String> {
    std::fs::read_dir(repo_root().join("locales").join(locale))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| Path::new(n).extension().is_some_and(|e| e == "ftl"))
        .collect()
}

#[test]
fn fixtures_round_trip() {
    let theme = theme();
    let json = serde_json::to_string(&theme).unwrap();
    assert_eq!(serde_json::from_str::<ResolvedTheme>(&json).unwrap(), theme);

    let layout = layout();
    let json = serde_json::to_string(&layout).unwrap();
    assert_eq!(
        serde_json::from_str::<ResolvedLayout>(&json).unwrap(),
        layout
    );
}

#[test]
fn theme_token_references_resolve() {
    let theme = theme();
    let defined: BTreeSet<String> = theme
        .tokens
        .iter()
        .map(|(cat, name, _)| css_var_name(cat, name))
        .collect();

    let mut values: Vec<(String, &str)> = Vec::new();
    for (cat, name, value) in theme.tokens.iter() {
        values.push((format!("token {cat}.{name}"), value));
    }
    for (mode, set) in &theme.modes {
        for (cat, name, value) in set.iter() {
            assert!(
                defined.contains(&css_var_name(cat, name)),
                "mode {mode} overrides unknown token {cat}.{name}"
            );
            values.push((format!("mode {mode} {cat}.{name}"), value));
        }
    }
    for rule in &theme.parts {
        for (prop, value) in &rule.declarations {
            values.push((format!("part {} {prop}", rule.selector), value));
        }
    }
    let m = &theme.message;
    for value in [
        &m.font_family,
        &m.text_font_family,
        &m.font_size,
        &m.line_height,
        &m.max_width,
    ]
    .into_iter()
    .chain(&m.quote_colors)
    {
        values.push(("message".to_owned(), value));
    }

    for (origin, value) in values {
        assert!(
            !value.contains('$'),
            "{origin}: unresolved token syntax in {value:?}"
        );
        for var in var_refs(value) {
            assert!(defined.contains(&var), "{origin}: unknown token {var}");
        }
    }
}

#[test]
fn theme_parts_are_well_formed() {
    let theme = theme();
    let is_part = |s: &str| {
        !s.is_empty()
            && s.split('.').all(|seg| {
                !seg.is_empty()
                    && seg
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            })
    };
    for rule in &theme.parts {
        assert!(is_part(&rule.selector), "bad part name {:?}", rule.selector);
        assert!(
            !rule.declarations.is_empty(),
            "{} has no declarations",
            rule.selector
        );
        for ancestor in &rule.ancestors {
            assert!(
                is_part(&ancestor.selector),
                "bad ancestor {:?}",
                ancestor.selector
            );
        }
    }
    // The part rules the default UI relies on (list rows, folders, the
    // reader and toolbars are styled by the UI skin since M5a; windows by
    // the window manager's skin and their size by the window spec since M5c;
    // the reader's conversation strip by the UI skin since P2, ADR 0148).
    for part in ["app", "split.divider", "composer", "status-bar.button"] {
        assert!(
            theme.parts.iter().any(|r| r.selector == part),
            "missing part {part}"
        );
    }
}

fn widget_kinds(node: &LayoutNode) -> Vec<String> {
    let mut kinds = Vec::new();
    node.walk(&mut |n| {
        if let LayoutNode::Widget { kind, .. } = n {
            kinds.push(kind.clone());
        }
    });
    kinds
}

#[test]
fn layout_describes_three_panes_and_a_narrow_fallback() {
    let layout = layout();

    // M5b: the main window is the navigation rail beside the view host,
    // for every width, with the status bar below.
    let main = &layout.windows["main"];
    assert_eq!(main.len(), 1);
    let main_kinds = widget_kinds(&main[0].root);
    for kind in ["nav-rail", "view-host", "status-bar", "config-errors"] {
        assert!(main_kinds.contains(&kind.to_owned()), "main has {kind}");
    }
    assert_eq!(layout.views["mail"].window, "mail");

    // The mail view's window: three panes, tabs when narrow.
    let mail_window = &layout.windows["mail"];
    for width in [0, 320, 899, 900, 1280, 3840] {
        assert_eq!(
            mail_window.iter().filter(|bp| bp.matches(width)).count(),
            1,
            "exactly one breakpoint must match width {width}"
        );
    }

    let wide = mail_window.iter().find(|bp| bp.matches(1280)).unwrap();
    let LayoutNode::Split {
        sizes, children, ..
    } = &wide.root
    else {
        panic!("the wide mail window is a split: {:?}", wide.root);
    };
    assert_eq!(sizes.len(), children.len());
    let pane_kinds: Vec<Vec<String>> = children.iter().map(widget_kinds).collect();
    assert_eq!(pane_kinds[0][0], "folder-tree");
    // The list pane: the list header (M5b), the Exchange automatic-replies
    // banner, the Gmail category tabs (M3b) and the Exchange Focused tabs
    // (M3c) above the message list.
    assert_eq!(
        pane_kinds[1],
        [
            "list-header",
            "automatic-replies-banner",
            "category-tabs",
            "focused-tabs",
            "message-list"
        ]
    );
    // The reader: its bar and the conversation as cards (P2 WP203; each
    // open card shows the message-view).
    assert!(pane_kinds[2].contains(&"thread-view".to_owned()));
    assert!(pane_kinds[2].contains(&"toolbar".to_owned()));

    let narrow = mail_window.iter().find(|bp| bp.matches(600)).unwrap();
    assert!(matches!(narrow.root, LayoutNode::Tabs { .. }));

    for bp in main.iter().chain(mail_window) {
        bp.root.walk(&mut |n| match n {
            LayoutNode::Split {
                sizes, children, ..
            }
            | LayoutNode::Stack {
                sizes, children, ..
            } => {
                assert!(sizes.is_empty() || sizes.len() == children.len());
            }
            _ => {}
        });
    }
}

#[test]
fn layout_references_resolve() {
    fn menu_actions<'a>(items: &'a [MenuItem], out: &mut Vec<&'a str>) {
        for item in items {
            match item {
                MenuItem::Action { action, .. } => out.push(action),
                MenuItem::Submenu { items, .. } => menu_actions(items, out),
                MenuItem::Separator | MenuItem::Header { .. } => {}
            }
        }
    }
    let layout = layout();

    // j/k navigate the list.
    for (key, action) in [("j", "mail.next"), ("k", "mail.previous")] {
        assert!(
            layout
                .keys
                .iter()
                .any(|b| b.context == "list" && b.key == key && b.action == action),
            "missing list binding {key} -> {action}"
        );
    }

    let mut used_actions: Vec<&str> = layout.keys.iter().map(|k| k.action.as_str()).collect();
    for items in layout.toolbars.values() {
        for item in items {
            match item {
                ToolbarItem::Action { action, .. } => used_actions.push(action),
                ToolbarItem::More { items, .. } => menu_actions(items, &mut used_actions),
                ToolbarItem::Spacer | ToolbarItem::Separator => {}
            }
        }
    }
    for items in layout.menus.values() {
        menu_actions(items, &mut used_actions);
    }
    for item in &layout.rail {
        if let RailItem::Action { action, .. } = item {
            used_actions.push(action);
        }
    }
    for action in used_actions {
        assert!(
            layout.actions.contains_key(action),
            "action {action} has no entry in `actions`"
        );
    }

    // M5a: the default layout names Lucide icons only (no legacy names).
    for (id, def) in &layout.actions {
        if let Some(icon) = &def.icon {
            assert!(
                nail_api::icons::BUNDLED_ICONS.contains(&icon.as_str()),
                "action {id} uses {icon}, which is not a bundled Lucide icon"
            );
        }
    }

    // M5b: views show existing windows, with bundled icons; the rail names
    // known views; every toolbar has its settings.
    for (id, view) in &layout.views {
        assert!(
            layout.windows.contains_key(&view.window),
            "view {id} shows the unknown window {}",
            view.window
        );
        let icon = view
            .icon
            .as_deref()
            .expect("every default view has an icon");
        assert!(
            nail_api::icons::BUNDLED_ICONS.contains(&icon),
            "view {id}: {icon}"
        );
    }
    for item in &layout.rail {
        if let RailItem::View { view } = item {
            assert!(
                layout.views.contains_key(view),
                "rail names unknown view {view}"
            );
        }
    }
    for name in layout.toolbars.keys() {
        assert!(layout.toolbar_settings.contains_key(name), "toolbar {name}");
    }
    for group in &layout.settings {
        for section in &group.sections {
            if let Some(icon) = &section.icon {
                assert!(
                    nail_api::icons::BUNDLED_ICONS.contains(&icon.as_str()),
                    "{icon}"
                );
            }
        }
    }

    for bp in layout.windows.values().flatten() {
        bp.root.walk(&mut |n| {
            if let LayoutNode::Widget { kind, options, .. } = n
                && kind == "toolbar"
            {
                let name = options["toolbar"].as_str().expect("toolbar option");
                assert!(layout.toolbars.contains_key(name), "unknown toolbar {name}");
            }
        });
    }
}

#[test]
fn labels_and_error_codes_exist_in_the_english_catalog() {
    let en = fluent_ids("en");
    let layout = layout();

    let mut labels: Vec<String> = layout
        .actions
        .values()
        .filter_map(|a| a.label.clone())
        .collect();
    for bp in layout.windows.values().flatten() {
        bp.root.walk(&mut |n| {
            let label = match n {
                LayoutNode::Split { label, .. }
                | LayoutNode::Stack { label, .. }
                | LayoutNode::Tabs { label, .. }
                | LayoutNode::Widget { label, .. } => label,
            };
            labels.extend(label.clone());
        });
    }
    for label in labels {
        let id = label_message_id(&label)
            .unwrap_or_else(|| panic!("literal label {label:?} in default layout"));
        assert!(
            en.contains(&id),
            "locales/en is missing {id} (label {label})"
        );
    }
    for code in ErrorCode::ALL {
        assert!(
            en.contains(&code.message_id()),
            "locales/en is missing {}",
            code.message_id()
        );
    }
    let contract_ids = nail_api::AccountState::ALL
        .iter()
        .map(|s| s.message_id())
        .chain(
            nail_api::SearchErrorKind::ALL
                .iter()
                .map(|k| k.message_id()),
        );
    for id in contract_ids {
        assert!(en.contains(&id), "locales/en is missing {id}");
    }
}

#[test]
fn every_locale_has_the_same_catalog_files() {
    let en = ftl_file_names("en");
    assert!(!en.is_empty());
    assert_eq!(ftl_file_names("de"), en);
}

/// `fixtures/icons.json` mirrors `nail_api::icons` for the UI's icon
/// registry test. Regenerate with
/// `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-api --test fixtures`
/// (then `npx prettier --write crates/nail-api/fixtures`).
#[test]
fn icons_fixture_is_current() {
    let expected = nail_api::icons::icons_fixture();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("icons.json");
    if std::env::var_os("NAIL_UPDATE_SNAPSHOTS").is_some() {
        let mut text = serde_json::to_string_pretty(&expected).unwrap();
        text.push('\n');
        std::fs::write(&path, text).unwrap();
    }
    let actual: serde_json::Value =
        serde_json::from_str(&read_fixture("icons.json")).expect("icons.json");
    assert_eq!(actual, expected, "run with NAIL_UPDATE_SNAPSHOTS=1");
}
