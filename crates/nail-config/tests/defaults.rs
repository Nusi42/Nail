//! The shipped defaults resolve to exactly the contract fixtures in
//! `crates/nail-api/fixtures` (apart from source locations, which the
//! fixtures leave out). After changing `defaults/*.kdl`, regenerate them
//! with `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test defaults`
//! (then `npx prettier --write crates/nail-api/fixtures`).

use std::path::Path;

use nail_api::{LayoutNode, ResolvedLayout, ResolvedTheme};
use pretty_assertions::assert_eq;

fn fixture<T: serde::de::DeserializeOwned>(name: &str) -> T {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../nail-api/fixtures")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap()
}

/// Rewrites a fixture when `NAIL_UPDATE_SNAPSHOTS=1`.
fn update<T: serde::Serialize>(name: &str, value: &T) {
    if std::env::var_os("NAIL_UPDATE_SNAPSHOTS").is_some() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../nail-api/fixtures")
            .join(name);
        let mut text = serde_json::to_string_pretty(value).unwrap();
        text.push('\n');
        std::fs::write(&path, text).unwrap();
    }
}

fn strip_theme_sources(mut theme: ResolvedTheme) -> ResolvedTheme {
    for rule in &mut theme.parts {
        rule.source = None;
    }
    theme
}

fn strip_node_sources(node: &mut LayoutNode) {
    match node {
        LayoutNode::Widget { source, .. } => *source = None,
        LayoutNode::Split { children, .. }
        | LayoutNode::Stack { children, .. }
        | LayoutNode::Tabs { children, .. } => children.iter_mut().for_each(strip_node_sources),
    }
}

fn strip_layout_sources(mut layout: ResolvedLayout) -> ResolvedLayout {
    for bp in layout.windows.values_mut().flatten() {
        strip_node_sources(&mut bp.root);
    }
    layout
}

#[test]
fn default_theme_matches_the_fixture() {
    let actual = strip_theme_sources(nail_config::default_theme());
    update("default-theme.json", &actual);
    let expected: ResolvedTheme = fixture("default-theme.json");
    // Compare as JSON first for a readable diff, then structurally.
    assert_eq!(
        serde_json::to_value(&actual).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert_eq!(actual, expected);
}

#[test]
fn default_layout_matches_the_fixture() {
    let actual = strip_layout_sources(nail_config::default_layout());
    update("default-layout.json", &actual);
    let expected: ResolvedLayout = fixture("default-layout.json");
    assert_eq!(
        serde_json::to_value(&actual).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert_eq!(actual, expected);
}

#[test]
fn defaults_carry_source_locations() {
    let theme = nail_config::default_theme();
    let app = theme.parts.iter().find(|r| r.selector == "app").unwrap();
    let source = app.source.as_ref().unwrap();
    assert_eq!(source.file, nail_config::DEFAULT_THEME_SOURCE);
    let line = nail_config::DEFAULT_THEME_KDL
        .lines()
        .nth(source.line as usize - 1)
        .unwrap();
    assert!(line.contains("part \"app\""), "{line}");

    let layout = nail_config::default_layout();
    let mut sources = Vec::new();
    for bp in &layout.windows["main"] {
        bp.root.walk(&mut |n| {
            if let LayoutNode::Widget { kind, source, .. } = n {
                sources.push((kind.clone(), source.clone().unwrap()));
            }
        });
    }
    let (kind, source) = &sources[0];
    let line = nail_config::DEFAULT_LAYOUT_KDL
        .lines()
        .nth(source.line as usize - 1)
        .unwrap();
    assert_eq!(&line[source.column as usize - 1..][..kind.len()], kind);
}

#[test]
fn parsing_the_default_text_equals_the_default() {
    let theme = nail_config::parse_theme(nail_config::DEFAULT_THEME_KDL, "x.kdl").unwrap();
    assert_eq!(
        strip_theme_sources(theme),
        strip_theme_sources(nail_config::default_theme())
    );
    let layout = nail_config::parse_layout(nail_config::DEFAULT_LAYOUT_KDL, "x.kdl").unwrap();
    assert_eq!(
        strip_layout_sources(layout),
        strip_layout_sources(nail_config::default_layout())
    );
}

#[test]
fn empty_extension_equals_the_default() {
    let theme =
        nail_config::parse_theme("theme \"default\" extends=\"default\"", "theme.kdl").unwrap();
    assert_eq!(theme, nail_config::default_theme());
    let layout =
        nail_config::parse_layout("layout \"default\" extends=\"default\"", "layout.kdl").unwrap();
    assert_eq!(layout, nail_config::default_layout());
}

/// The example themes shipped in `defaults/themes/` (e.g. high contrast)
/// parse and resolve.
#[test]
fn example_themes_parse() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../defaults/themes");
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "kdl") {
            let text = std::fs::read_to_string(&path).unwrap();
            if let Err(e) = nail_config::parse_theme(&text, "theme.kdl") {
                panic!("{}: {e}", path.display());
            }
            count += 1;
        }
    }
    assert!(count >= 1, "no example themes in {}", dir.display());
}
