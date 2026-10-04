//! M5b WP165: layouts written before the navigation rail keep working, and
//! the new default can be taken over through the merge dialog (the UI sends
//! the shipped `defaults/layout.kdl` as the incoming file).
#![cfg(test)]

use nail_api::customize::{ConfigFile, MergeChoice, MergeTake};
use nail_api::{LayoutNode, ResolvedLayout};
use nail_config::{DEFAULT_LAYOUT_KDL, parse_layout, preview_merge, resolve_merge};

/// A user file of the M5a era: its own three-pane main window with the
/// main toolbar, extending the default.
const OLD_LAYOUT: &str = r#"layout "Mine" extends="default" {
    window "main" {
        stack id="root" sizes="auto 1fr" {
            toolbar id="main-toolbar" label="@toolbar.main" toolbar="main"
            split "horizontal" id="panes" sizes="240px 1fr 1.4fr" {
                folder-tree id="folders" show-unread=#true
                message-list id="list"
                message-view id="message"
            }
        }
    }
    keys {
        bind "x" "mail.archive"
    }
}
"#;

fn kinds(layout: &ResolvedLayout, window: &str) -> Vec<String> {
    let mut out = Vec::new();
    for bp in &layout.windows[window] {
        bp.root.walk(&mut |n| {
            if let LayoutNode::Widget { kind, .. } = n {
                out.push(kind.clone());
            }
        });
    }
    out
}

#[test]
fn an_old_layout_keeps_its_main_window_and_inherits_the_views() {
    let layout = parse_layout(OLD_LAYOUT, "layout.kdl").unwrap();
    let main = kinds(&layout, "main");
    assert!(main.contains(&"toolbar".to_owned()));
    assert!(!main.contains(&"view-host".to_owned()));
    // The views, rail and settings page come from the default; the mail
    // view's window exists (the fallback opens views as windows).
    assert_eq!(
        layout.views.keys().map(String::as_str).collect::<Vec<_>>(),
        ["mail", "calendar", "contacts", "tasks", "settings"]
    );
    assert!(layout.windows.contains_key("mail"));
    assert!(!layout.rail.is_empty());
    assert!(!layout.settings.is_empty());
    assert!(layout.toolbars.contains_key("main"));
}

#[test]
fn taking_the_new_main_window_from_the_default_gives_the_rail() {
    let preview = preview_merge(ConfigFile::Layout, Some(OLD_LAYOUT), DEFAULT_LAYOUT_KDL).unwrap();
    assert!(preview.errors.is_empty(), "{:?}", preview.errors);
    let main = preview
        .sections
        .iter()
        .flat_map(|s| s.items.iter())
        .find(|item| item.id == "windows:main")
        .expect("the main window is a merge item");
    assert!(main.theirs.as_deref().unwrap().contains("view-host"));

    let choices = [MergeChoice {
        item_id: main.id.clone(),
        take: MergeTake::Theirs,
    }];
    let outcome = resolve_merge(
        ConfigFile::Layout,
        Some(OLD_LAYOUT),
        DEFAULT_LAYOUT_KDL,
        &choices,
        MergeTake::Mine,
    )
    .unwrap();
    // The merged file is the user's, with the new main window.
    assert!(outcome.text.contains("layout \"Mine\""), "{}", outcome.text);
    assert!(outcome.text.contains("bind \"x\" \"mail.archive\""));
    let layout = outcome.layout.expect("a layout merge resolves a layout");
    let main = kinds(&layout, "main");
    assert!(main.contains(&"nav-rail".to_owned()), "{main:?}");
    assert!(main.contains(&"view-host".to_owned()));
    assert!(kinds(&layout, "mail").contains(&"list-header".to_owned()));
    assert_eq!(layout.views["mail"].window, "mail");
    // And the text parses on its own like the file Nail writes.
    parse_layout(&outcome.text, "layout.kdl").unwrap();
}
