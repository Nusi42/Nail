//! M5a (ADR 0102, WP152): the theme list (built-ins and `themes/*.kdl`),
//! `extends` chains, problems per theme file, the selection in `theme.kdl`,
//! Follow Windows, density on every theme, and comment-preserving writes of
//! the selection and the overrides.
#![cfg(test)]

use std::sync::mpsc;
use std::time::{Duration, Instant};

use nail_api::appearance::{
    ColorScheme, Density, SystemAppearance, ThemeKind, ThemeOverrides, ThemePreferences,
    ThemeProblem, ThemeSource,
};
use nail_api::{ErrorCode, Event, ResolvedTheme};
use nail_config::{
    BACKUP_DIR, ConfigKind, ConfigManager, ConfigUpdate, ThemeCatalog, ThemeFile, UserThemeText,
    appearance_settings, apply_theme_preferences, apply_theme_selection, resolve_selected,
};
use pretty_assertions::assert_eq;

fn user(stem: &str, text: &str) -> UserThemeText {
    UserThemeText {
        stem: stem.to_owned(),
        text: Ok(text.to_owned()),
    }
}

fn write(dir: &std::path::Path, name: &str, text: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn builtin(id: &str) -> ResolvedTheme {
    ThemeCatalog::builtin().resolve(id).unwrap()
}

fn token<'t>(theme: &'t ResolvedTheme, category: &str, name: &str) -> Option<&'t str> {
    theme.tokens.get(category, name)
}

fn parse(text: &str) -> ThemeFile {
    ThemeFile::parse(text, "theme.kdl").unwrap()
}

const OCEAN: &str = r##"// A dark blue theme on top of Graphite.
theme "Ocean" extends="graphite" {
    tokens {
        color "accent" "#00aaff"
    }
}
"##;

const DEEP_OCEAN: &str = r##"theme "Deep ocean" extends="ocean" {
    tokens { color "bg" "#001020"; }
}
"##;

const PAPER: &str = r##"theme "Paper" extends="default" mode="light" {
    tokens { color "bg" "#fbf8f0"; }
}
"##;

const HIGH_CONTRAST: SystemAppearance = SystemAppearance {
    scheme: ColorScheme::Dark,
    high_contrast: true,
};

// ---------------------------------------------------------------------------
// The theme list

#[test]
fn user_themes_come_after_the_builtins_and_extend_them() {
    let catalog = ThemeCatalog::with_user_themes(vec![
        user("paper", PAPER),
        user("ocean", OCEAN),
        user("deep-ocean", DEEP_OCEAN),
    ]);
    let themes = catalog.themes();
    let ids: Vec<&str> = themes.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "frost",
            "sage",
            "graphite",
            "nocturne",
            "high-contrast",
            "deep-ocean",
            "ocean",
            "paper"
        ]
    );
    let ocean = catalog.get("ocean").unwrap();
    assert_eq!(ocean.name, "Ocean");
    assert_eq!(ocean.source, ThemeSource::User);
    assert_eq!(ocean.label, None);
    assert_eq!(ocean.extends.as_deref(), Some("graphite"));
    assert_eq!(ocean.file, "themes/ocean.kdl");
    // No `mode` of its own: the kind comes from the chain (Graphite: dark).
    assert_eq!(ocean.kind, ThemeKind::Dark);
    assert!(ocean.usable());
    assert_eq!(catalog.get("deep-ocean").unwrap().kind, ThemeKind::Dark);
    assert_eq!(catalog.get("paper").unwrap().kind, ThemeKind::Light);
    assert!(catalog.file_errors().is_empty());

    // The chain: base ← graphite ← ocean ← deep-ocean.
    let graphite = builtin("graphite");
    let deep = catalog.resolve("deep-ocean").unwrap();
    assert_eq!(token(&deep, "color", "bg"), Some("#001020"));
    assert_eq!(token(&deep, "color", "accent"), Some("#00aaff"));
    assert_eq!(
        token(&deep, "color", "fg"),
        token(&graphite, "color", "fg"),
        "inherited from Graphite"
    );
    assert_eq!(
        token(&deep, "space", "row"),
        token(&nail_config::default_theme(), "space", "row"),
        "inherited from the base"
    );
}

#[test]
fn a_user_theme_with_a_builtin_id_is_listed_but_never_applied() {
    let catalog = ThemeCatalog::with_user_themes(vec![
        user(
            "frost",
            r##"theme "My frost" extends="default" { tokens { color "bg" "#123456"; }; }"##,
        ),
        user(
            "mine",
            r##"theme "Mine" extends="frost" { tokens { color "fg" "#000001"; }; }"##,
        ),
    ]);
    let themes = catalog.themes();
    let shadow = themes
        .iter()
        .find(|t| t.id == "frost" && t.source == ThemeSource::User)
        .unwrap();
    assert_eq!(
        shadow.problem.as_ref().unwrap().id,
        ThemeProblem::ShadowsBuiltin.message_id()
    );
    // Lookups and chains use the built-in.
    assert_eq!(catalog.get("frost").unwrap().source, ThemeSource::Builtin);
    assert_eq!(catalog.resolve("frost").unwrap(), builtin("frost"));
    let mine = catalog.resolve("mine").unwrap();
    assert_eq!(
        token(&mine, "color", "bg"),
        token(&builtin("frost"), "color", "bg")
    );
    let errors = catalog.file_errors();
    assert_eq!(
        errors["themes/frost.kdl"][0].localized.id,
        "config-theme-shadows-builtin"
    );
}

#[test]
fn broken_user_themes_are_listed_with_their_problem() {
    let catalog = ThemeCatalog::with_user_themes(vec![
        user("syntax", "theme \"Syntax\" extends=\"default\" {"),
        user("unknown-base", "theme \"U\" extends=\"nowhere\""),
        user("on-broken", "theme \"B\" extends=\"syntax\""),
        user("loop-a", "theme \"A\" extends=\"loop-b\""),
        user("loop-b", "theme \"B\" extends=\"loop-a\""),
        user(
            "bad-ref",
            "theme \"R\" extends=\"sage\" {\n    tokens { color \"bg\" \"$color.nope\"; }\n}\n",
        ),
        user("ok", "theme \"Ok\" extends=\"on-broken-not\""),
    ]);
    let problem = |id: &str| {
        let info = catalog
            .themes()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap_or_else(|| panic!("{id} not listed"));
        assert!(!info.usable(), "{id}");
        assert!(catalog.resolve(id).is_none(), "{id}");
        info.problem.unwrap()
    };
    let syntax = problem("syntax");
    assert_eq!(syntax.id, ThemeProblem::Invalid.message_id());
    assert!(
        format!("{:?}", syntax.args).contains("themes/syntax.kdl:1:"),
        "{syntax:?}"
    );
    assert_eq!(
        catalog.get("syntax").unwrap().name,
        "syntax",
        "name falls back to the id"
    );
    for id in ["unknown-base", "on-broken", "bad-ref", "ok"] {
        assert_eq!(problem(id).id, ThemeProblem::Invalid.message_id(), "{id}");
    }
    for id in ["loop-a", "loop-b"] {
        assert_eq!(
            problem(id).id,
            ThemeProblem::ExtendsCycle.message_id(),
            "{id}"
        );
    }

    let errors = catalog.file_errors();
    let first = |file: &str| errors[file][0].localized.id.clone();
    assert_eq!(
        first("themes/unknown-base.kdl"),
        "config-theme-extends-unknown"
    );
    assert_eq!(first("themes/on-broken.kdl"), "config-theme-extends-broken");
    assert_eq!(first("themes/loop-a.kdl"), "config-theme-extends-cycle");
    assert_eq!(first("themes/bad-ref.kdl"), "config-unknown-token");
    assert_eq!(errors["themes/bad-ref.kdl"][0].line, 2);
    assert_eq!(errors.len(), 7);
}

// ---------------------------------------------------------------------------
// The selection

#[test]
fn the_selection_is_read_from_theme_kdl() {
    let defaults = appearance_settings(None);
    assert!(defaults.follow_system);
    assert_eq!(
        (defaults.light.as_str(), defaults.dark.as_str()),
        ("frost", "graphite")
    );

    // `extends="default"` without an `appearance` node: the new default look.
    let s = appearance_settings(Some(&parse("theme \"x\" extends=\"default\"")));
    assert_eq!(s, defaults);

    // A theme id: a fixed theme.
    let s = appearance_settings(Some(&parse("theme \"x\" extends=\"sage\"")));
    assert!(!s.follow_system);
    assert_eq!(s.theme, "sage");

    let s = appearance_settings(Some(&parse(
        r##"theme "x" extends="nocturne" {
    appearance follow-system=#true light="sage" dark="nocturne" density="compact"
    tokens {
        color "accent" "#3b5bdb"
        font-size "body" 15 compact=14
        radius "md" 8
    }
}"##,
    )));
    assert!(s.follow_system);
    assert_eq!(s.theme, "nocturne");
    assert_eq!((s.light.as_str(), s.dark.as_str()), ("sage", "nocturne"));
    assert_eq!(s.density, Density::Compact);
    assert_eq!(
        s.overrides,
        ThemeOverrides {
            font_size: Some(15),
            radius: Some(8),
            accent: Some("#3b5bdb".into()),
        }
    );
    // Values the settings cannot show read as no override.
    let s = appearance_settings(Some(&parse(
        "theme \"x\" extends=\"default\" { tokens { font-size \"body\" \"0.9rem\"; }; }",
    )));
    assert_eq!(s.overrides.font_size, None);
}

#[test]
fn appearance_node_errors_point_at_the_value() {
    for (text, id) in [
        (
            "theme \"x\" extends=\"default\" { appearance density=\"tight\"; }",
            "config-appearance-density-unknown",
        ),
        (
            "theme \"x\" extends=\"default\" { appearance light=\"Frost\"; }",
            "config-appearance-theme-invalid",
        ),
        (
            "theme \"x\" extends=\"default\" { appearance follow-system=\"yes\"; }",
            "config-expected-bool",
        ),
        (
            "theme \"x\" extends=\"default\" { appearance accent=\"#fff\"; }",
            "config-unknown-property-hint",
        ),
    ] {
        let err = nail_config::parse_theme(text, "theme.kdl").unwrap_err();
        assert_eq!(err.localized.id, id, "{text}");
        assert_eq!(err.line, 1, "{text}");
    }
}

#[test]
fn follow_windows_switches_between_the_pair() {
    let catalog = ThemeCatalog::builtin();
    let file = parse(
        "theme \"Mine\" extends=\"default\" {\n    appearance light=\"sage\" dark=\"nocturne\"\n}\n",
    );
    let light = resolve_selected(&catalog, Some(&file), Some(SystemAppearance::LIGHT)).unwrap();
    assert_eq!(light.active, "sage");
    assert_eq!(light.theme.name, "Mine");
    assert_eq!(
        token(&light.theme, "color", "bg"),
        token(&builtin("sage"), "color", "bg")
    );
    let dark = resolve_selected(&catalog, Some(&file), Some(SystemAppearance::DARK)).unwrap();
    assert_eq!(dark.active, "nocturne");
    assert_eq!(dark.theme.mode, builtin("nocturne").mode);
    assert_eq!(
        token(&dark.theme, "color", "bg"),
        token(&builtin("nocturne"), "color", "bg")
    );
    let hc = resolve_selected(&catalog, Some(&file), Some(HIGH_CONTRAST)).unwrap();
    assert_eq!(hc.active, "high-contrast");
    // Unknown OS state: the light theme.
    assert_eq!(
        resolve_selected(&catalog, Some(&file), None)
            .unwrap()
            .active,
        "sage"
    );
    // No file at all: Frost / Graphite.
    assert_eq!(
        resolve_selected(&catalog, None, Some(SystemAppearance::DARK))
            .unwrap()
            .active,
        "graphite"
    );
}

#[test]
fn the_users_overrides_apply_on_top_of_every_theme() {
    let catalog = ThemeCatalog::with_user_themes(vec![user("ocean", OCEAN)]);
    let file = parse(
        r##"theme "Mine" extends="ocean" {
    tokens {
        color "accent" "#3b5bdb"
        radius "md" 8
    }
    part "message-list.row" { padding "$space.row"; }
}"##,
    );
    let selected = resolve_selected(&catalog, Some(&file), Some(SystemAppearance::LIGHT)).unwrap();
    assert_eq!(selected.active, "ocean", "a fixed theme ignores the OS");
    let theme = &selected.theme;
    assert_eq!(token(theme, "color", "accent"), Some("#3b5bdb"));
    assert_eq!(token(theme, "radius", "md"), Some("8px"));
    // The accent override is one value for every mode.
    for (mode, tokens) in &theme.modes {
        assert_eq!(tokens.get("color", "accent"), None, "{mode}");
    }
    assert_eq!(
        token(theme, "color", "bg"),
        token(&catalog.resolve("ocean").unwrap(), "color", "bg")
    );
    assert!(theme.parts.iter().any(|p| p.selector == "message-list.row"));
}

/// A user theme on top of High contrast (its kind is high contrast).
const INK: &str = r##"theme "Ink" extends="high-contrast" {
    tokens { color "bg" "#000011"; }
}
"##;

/// The user's `theme.kdl` with every override, on `extends`.
fn with_overrides(extends: &str, follow: bool) -> ThemeFile {
    parse(&format!(
        r##"theme "Mine" extends="{extends}" {{
    appearance follow-system={}
    tokens {{
        color "accent" "#ff0066"
        font-size "body" 15
        radius "md" 8
    }}
}}"##,
        if follow { "#true" } else { "#false" }
    ))
}

#[test]
fn the_accent_override_is_ignored_while_a_high_contrast_theme_is_active() {
    // ADR 0161.
    let catalog = ThemeCatalog::with_user_themes(vec![user("ink", INK)]);
    assert_eq!(catalog.get("ink").unwrap().kind, ThemeKind::HighContrast);
    let hc = builtin("high-contrast");
    let hc_accent = token(&hc, "color", "accent");
    assert_eq!(hc_accent, Some("#ffff00"));
    let check_ignored = |file: &ThemeFile, system, active: &str| {
        let selected = resolve_selected(&catalog, Some(file), system).unwrap();
        assert_eq!(selected.active, active);
        let theme = &selected.theme;
        assert_eq!(token(theme, "color", "accent"), hc_accent, "{active}");
        // The theme's own per-mode values (the print view) stay too.
        assert_eq!(
            theme.modes["print"].get("color", "accent"),
            hc.modes["print"].get("color", "accent"),
            "{active}"
        );
        // Text size and radius still apply.
        assert_eq!(token(theme, "font-size", "body"), Some("15px"));
        assert_eq!(token(theme, "radius", "md"), Some("8px"));
        // The stored value is kept.
        assert_eq!(
            selected.settings.overrides.accent.as_deref(),
            Some("#ff0066")
        );
        selected
    };
    // High contrast picked as the fixed theme.
    check_ignored(
        &with_overrides("high-contrast", false),
        Some(SystemAppearance::LIGHT),
        "high-contrast",
    );
    // Follow Windows with a Windows contrast theme on.
    let following = with_overrides("default", true);
    check_ignored(&following, Some(HIGH_CONTRAST), "high-contrast");
    // A user theme extending High contrast.
    let ink = check_ignored(
        &with_overrides("ink", false),
        Some(SystemAppearance::DARK),
        "ink",
    );
    assert_eq!(token(&ink.theme, "color", "bg"), Some("#000011"));

    // Any other theme: the override applies again.
    let light =
        resolve_selected(&catalog, Some(&following), Some(SystemAppearance::LIGHT)).unwrap();
    assert_eq!(light.active, "frost");
    assert_eq!(token(&light.theme, "color", "accent"), Some("#ff0066"));
    let fixed = resolve_selected(
        &catalog,
        Some(&with_overrides("graphite", false)),
        Some(HIGH_CONTRAST),
    )
    .unwrap();
    assert_eq!(fixed.active, "graphite", "a fixed theme ignores the OS");
    assert_eq!(token(&fixed.theme, "color", "accent"), Some("#ff0066"));
}

#[test]
fn a_broken_or_missing_fixed_theme_falls_back_by_kind() {
    let catalog = ThemeCatalog::with_user_themes(vec![
        user(
            "broken-dark",
            "theme \"B\" extends=\"graphite\" mode=\"dark\" {",
        ),
        user(
            "dark-loop",
            "theme \"L\" extends=\"dark-loop\" mode=\"dark\"",
        ),
    ]);
    let file = parse("theme \"Mine\" extends=\"broken-dark\"");
    let selected = resolve_selected(&catalog, Some(&file), None).unwrap();
    // A theme that does not parse has no known kind: light.
    assert_eq!(selected.active, "frost");
    let file = parse("theme \"Mine\" extends=\"dark-loop\"");
    assert_eq!(
        resolve_selected(&catalog, Some(&file), None)
            .unwrap()
            .active,
        "graphite",
        "an unusable dark theme falls back to the default dark theme"
    );
    let file = parse("theme \"Mine\" extends=\"gone\"");
    let selected = resolve_selected(&catalog, Some(&file), None).unwrap();
    assert_eq!(selected.active, "frost");
    assert_eq!(selected.settings.theme, "gone", "the selection is kept");
}

#[test]
fn density_applies_to_every_theme() {
    let catalog = ThemeCatalog::with_user_themes(vec![user("ocean", OCEAN), user("paper", PAPER)]);
    let compact_file = parse(
        "theme \"Mine\" extends=\"default\" {\n    appearance follow-system=#false density=\"compact\"\n}\n",
    );
    for info in catalog.themes() {
        let file = ThemeFile::parse(
            &format!(
                "theme \"Mine\" extends=\"{}\" {{\n    appearance density=\"compact\"\n}}\n",
                info.id
            ),
            "theme.kdl",
        )
        .unwrap();
        let selected = resolve_selected(&catalog, Some(&file), None).unwrap();
        assert_eq!(selected.active, info.id);
        assert_eq!(selected.theme.density, Density::Compact, "{}", info.id);
        let compact = selected
            .theme
            .modes
            .get(nail_api::appearance::COMPACT_VARIANT)
            .unwrap_or_else(|| panic!("{}: no compact variant", info.id));
        for spec in nail_api::appearance::token_schema().filter(|s| s.density) {
            assert!(
                compact.get(spec.category, spec.name).is_some(),
                "{}: {}.{} has no compact value",
                info.id,
                spec.category,
                spec.name
            );
        }
        // Comfortable keeps the same compact variant available, only the
        // density flag differs.
        let comfortable = resolve_selected(
            &catalog,
            Some(
                &ThemeFile::parse(
                    &format!("theme \"Mine\" extends=\"{}\"", info.id),
                    "theme.kdl",
                )
                .unwrap(),
            ),
            None,
        )
        .unwrap();
        assert_eq!(comfortable.theme.density, Density::Comfortable);
    }
    let selected = resolve_selected(&catalog, Some(&compact_file), None).unwrap();
    assert_eq!(selected.active, "frost");
    assert_eq!(selected.theme.density, Density::Compact);
}

#[test]
fn a_complete_theme_kdl_is_used_as_it_is() {
    // A complete file (no `extends`), as before M5: only it applies.
    let text = nail_config::DEFAULT_THEME_KDL.replace(
        "theme \"default\" mode=\"system\" {",
        "theme \"complete\" mode=\"system\" {\n    appearance density=\"compact\"",
    );
    let file = parse(&text);
    let selected = resolve_selected(
        &ThemeCatalog::builtin(),
        Some(&file),
        Some(SystemAppearance::DARK),
    )
    .unwrap();
    let mut expected = nail_config::parse_theme(&text, "theme.kdl").unwrap();
    expected.density = Density::Compact;
    assert_eq!(selected.theme, expected);
}

// ---------------------------------------------------------------------------
// Comment-preserving writes

const COMMENTED: &str = r##"// My theme settings.
theme "Mine" extends="default" {
    // Picked in Settings.
    appearance follow-system=#true light="frost" dark="graphite" // the pair

    tokens {
        // Brand colour.
        color "accent" "#ff0000" dark="#ff8080" // red
        color "bg" "#ffffff"
        radius "md" 4
        color "accent" "#00ff00"
    }

    /* keep me */
    part "app" { color "red"; }
}
"##;

#[test]
fn preference_writes_keep_comments_and_formatting() {
    let preferences = ThemePreferences {
        follow_system: false,
        light: "sage".into(),
        dark: "nocturne".into(),
        density: Density::Compact,
        overrides: ThemeOverrides {
            font_size: Some(16),
            radius: None,
            accent: Some("#3b5bdb".into()),
        },
    };
    let out = apply_theme_preferences(Some(COMMENTED), &preferences, "theme.kdl").unwrap();
    assert_eq!(
        out,
        r##"// My theme settings.
theme "Mine" extends="default" {
    // Picked in Settings.
    appearance follow-system=#false light="sage" dark="nocturne" density="compact" // the pair

    tokens {
        // Brand colour.
        color "accent" "#3b5bdb" // red
        color "bg" "#ffffff"
        font-size "body" 16 compact=15
    }

    /* keep me */
    part "app" { color "red"; }
}
"##
    );
    let file = parse(&out);
    let settings = appearance_settings(Some(&file));
    assert_eq!(ThemePreferences::from(&settings), preferences);

    // Writing the same again changes nothing.
    assert_eq!(
        apply_theme_preferences(Some(&out), &preferences, "theme.kdl").unwrap(),
        out
    );

    // Clearing every override removes the lines, the comments above the
    // removed lines go with them, the others stay.
    let cleared = ThemePreferences {
        overrides: ThemeOverrides::default(),
        ..preferences
    };
    let out = apply_theme_preferences(Some(&out), &cleared, "theme.kdl").unwrap();
    assert!(
        out.contains("    tokens {\n        color \"bg\" \"#ffffff\"\n    }\n"),
        "{out}"
    );
    assert!(out.contains("// Picked in Settings."), "{out}");
    assert!(out.contains("/* keep me */"), "{out}");
    assert_eq!(
        appearance_settings(Some(&parse(&out))).overrides,
        ThemeOverrides::default()
    );
}

#[test]
fn overrides_go_into_a_new_tokens_block_when_there_is_none() {
    let text =
        "// Mine\ntheme \"Mine\" extends=\"frost\" {\n    part \"app\" { color \"red\"; }\n}\n";
    let preferences = ThemePreferences {
        follow_system: false,
        light: "frost".into(),
        dark: "graphite".into(),
        density: Density::Comfortable,
        overrides: ThemeOverrides {
            font_size: None,
            radius: Some(12),
            accent: None,
        },
    };
    let out = apply_theme_preferences(Some(text), &preferences, "theme.kdl").unwrap();
    assert_eq!(
        out,
        "// Mine\ntheme \"Mine\" extends=\"frost\" {\n    appearance follow-system=#false light=\"frost\" dark=\"graphite\" density=\"comfortable\"\n    part \"app\" { color \"red\"; }\n    tokens {\n        radius \"md\" 12\n    }\n}\n"
    );
    let s = appearance_settings(Some(&parse(&out)));
    assert_eq!(s.theme, "frost", "the fixed theme is untouched");
    assert_eq!(s.overrides.radius, Some(12));
}

#[test]
fn selection_writes_keep_comments_and_switch_follow_windows_off() {
    let out = apply_theme_selection(Some(COMMENTED), "nocturne", "theme.kdl").unwrap();
    assert_eq!(
        out,
        COMMENTED
            .replace("extends=\"default\"", "extends=\"nocturne\"")
            .replace("follow-system=#true", "follow-system=#false")
    );
    let s = appearance_settings(Some(&parse(&out)));
    assert_eq!(s.theme, "nocturne");
    assert!(!s.follow_system);
    assert_eq!(
        (s.light.as_str(), s.dark.as_str()),
        ("frost", "graphite"),
        "the pair is kept"
    );

    // Without an `appearance` node the theme id alone means "fixed".
    let out = apply_theme_selection(
        Some("theme \"x\" extends=\"default\" { // mine\n}\n"),
        "sage",
        "theme.kdl",
    )
    .unwrap();
    assert_eq!(out, "theme \"x\" extends=\"sage\" { // mine\n}\n");
}

// ---------------------------------------------------------------------------
// ConfigManager

fn prefs(follow: bool, light: &str, dark: &str) -> ThemePreferences {
    ThemePreferences {
        follow_system: follow,
        light: light.into(),
        dark: dark.into(),
        density: Density::Comfortable,
        overrides: ThemeOverrides::default(),
    }
}

#[test]
fn the_manager_lists_user_themes_and_follows_the_os() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "themes/ocean.kdl", OCEAN);
    write(dir.path(), "themes/broken.kdl", "theme \"Broken\" {");
    write(
        dir.path(),
        "theme.kdl",
        "theme \"Mine\" extends=\"default\" {\n    appearance light=\"frost\" dark=\"ocean\"\n}\n",
    );
    let manager = ConfigManager::load(dir.path());
    let appearance = manager.appearance();
    assert_eq!(appearance.themes.len(), 7);
    assert_eq!(appearance.active, "frost");
    assert_eq!(appearance.system, None);
    assert!(
        manager.errors().is_empty(),
        "a broken user theme is not a theme.kdl error"
    );
    let file_errors = manager.theme_file_errors();
    assert_eq!(
        file_errors.keys().collect::<Vec<_>>(),
        ["themes/broken.kdl"]
    );
    assert!(manager.error_events().iter().any(|e| matches!(
        e,
        Event::ConfigErrors { file, errors } if file == "themes/broken.kdl" && !errors.is_empty()
    )));

    // Windows switches to dark: the pair's dark theme (a user theme).
    let updates = manager.set_system_appearance(Some(SystemAppearance::DARK));
    let [
        ConfigUpdate::ThemeChanged { appearance },
        ConfigUpdate::Changed { theme, .. },
    ] = updates.as_slice()
    else {
        panic!("{updates:?}")
    };
    assert_eq!(appearance.active, "ocean");
    assert_eq!(appearance.system, Some(SystemAppearance::DARK));
    assert_eq!(token(theme, "color", "accent"), Some("#00aaff"));
    assert_eq!(manager.theme(), *theme);
    assert!(matches!(
        updates[0].to_event(),
        Event::ThemeChanged { ref appearance } if appearance.active == "ocean"
    ));
    // The same state again: nothing.
    assert!(
        manager
            .set_system_appearance(Some(SystemAppearance::DARK))
            .is_empty()
    );
    // High contrast wins while following Windows.
    manager.set_system_appearance(Some(HIGH_CONTRAST));
    assert_eq!(manager.appearance().active, "high-contrast");
    // Unknown again: the light theme.
    manager.set_system_appearance(None);
    assert_eq!(manager.appearance().active, "frost");
}

#[test]
fn set_theme_writes_extends_with_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "theme.kdl", COMMENTED);
    write(dir.path(), "themes/broken.kdl", "theme \"Broken\" {");
    let manager = ConfigManager::load(dir.path());
    manager.set_system_appearance(Some(SystemAppearance::LIGHT));

    let (appearance, updates) = manager.set_theme("graphite").unwrap();
    assert_eq!(appearance.active, "graphite");
    assert!(!appearance.settings.follow_system);
    assert_eq!(appearance, manager.appearance());
    assert!(
        matches!(
            updates.as_slice(),
            [
                ConfigUpdate::ThemeChanged { .. },
                ConfigUpdate::Changed { .. }
            ]
        ),
        "{updates:?}"
    );
    let text = std::fs::read_to_string(dir.path().join("theme.kdl")).unwrap();
    assert!(
        text.contains("theme \"Mine\" extends=\"graphite\" {"),
        "{text}"
    );
    assert!(text.contains("// Brand colour."), "{text}");
    assert_eq!(
        manager
            .list_backups(nail_api::ConfigFile::Theme)
            .unwrap()
            .len(),
        1
    );
    assert!(dir.path().join(BACKUP_DIR).is_dir());
    assert_eq!(
        token(&manager.theme(), "color", "bg"),
        Some("#ffffff"),
        "the user's own tokens stay on top"
    );

    // Selecting the same theme again writes nothing.
    let (_, updates) = manager.set_theme("graphite").unwrap();
    assert!(updates.is_empty(), "{updates:?}");
    assert_eq!(
        manager
            .list_backups(nail_api::ConfigFile::Theme)
            .unwrap()
            .len(),
        1
    );

    let err = manager.set_theme("nope").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    assert_eq!(
        err.localized.unwrap().id,
        ThemeProblem::Unknown.message_id()
    );
    let err = manager.set_theme("broken").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    assert_eq!(
        err.localized.unwrap().id,
        ThemeProblem::Invalid.message_id()
    );

    // An invalid theme.kdl refuses the edit.
    write(dir.path(), "theme.kdl", "theme \"x\" { bogus; }");
    let err = manager.set_theme("sage").unwrap_err();
    assert_eq!(err.code, ErrorCode::Config);
}

#[test]
fn set_theme_finds_a_theme_file_added_a_moment_ago() {
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::load(dir.path());
    write(dir.path(), "themes/ocean.kdl", OCEAN);
    let (appearance, updates) = manager.set_theme("ocean").unwrap();
    assert_eq!(appearance.active, "ocean");
    assert!(appearance.themes.iter().any(|t| t.id == "ocean"));
    assert!(
        matches!(updates.first(), Some(ConfigUpdate::ThemeChanged { .. })),
        "{updates:?}"
    );
    assert_eq!(token(&manager.theme(), "color", "accent"), Some("#00aaff"));
}

#[test]
fn set_theme_preferences_writes_the_appearance_node_and_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::load(dir.path());
    manager.set_system_appearance(Some(SystemAppearance::DARK));
    let preferences = ThemePreferences {
        density: Density::Compact,
        overrides: ThemeOverrides {
            font_size: Some(16),
            radius: Some(4),
            accent: Some("#3b5bdb".into()),
        },
        ..prefs(true, "sage", "nocturne")
    };
    let (appearance, updates) = manager.set_theme_preferences(&preferences).unwrap();
    assert_eq!(appearance.active, "nocturne");
    assert_eq!(ThemePreferences::from(&appearance.settings), preferences);
    assert!(
        matches!(
            updates.as_slice(),
            [
                ConfigUpdate::ThemeChanged { .. },
                ConfigUpdate::Changed { .. }
            ]
        ),
        "{updates:?}"
    );
    let theme = manager.theme();
    assert_eq!(theme.density, Density::Compact);
    assert_eq!(token(&theme, "font-size", "body"), Some("16px"));
    assert_eq!(
        theme.modes[nail_api::appearance::COMPACT_VARIANT].get("font-size", "body"),
        Some("15px"),
        "Compact still shrinks the text"
    );
    assert_eq!(token(&theme, "radius", "md"), Some("4px"));
    assert_eq!(token(&theme, "color", "accent"), Some("#3b5bdb"));
    assert_eq!(
        token(&theme, "color", "bg"),
        token(&builtin("nocturne"), "color", "bg")
    );
    let text = std::fs::read_to_string(dir.path().join("theme.kdl")).unwrap();
    assert!(text.starts_with("// Changes made in Nail."), "{text}");
    // A missing file is recorded as absent in the backup.
    assert_eq!(
        manager
            .list_backups(nail_api::ConfigFile::Theme)
            .unwrap()
            .len(),
        1
    );

    // Invalid overrides and unknown themes are refused before writing.
    let bad = ThemePreferences {
        overrides: ThemeOverrides {
            font_size: Some(40),
            ..ThemeOverrides::default()
        },
        ..prefs(true, "frost", "graphite")
    };
    let err = manager.set_theme_preferences(&bad).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    let localized = err.localized.unwrap();
    assert_eq!(localized.id, "theme-overrides-issue-font-size-range");
    assert!(
        err.message.contains("11") && err.message.contains("20"),
        "{}",
        err.message
    );
    let err = manager
        .set_theme_preferences(&prefs(true, "frost", "nope"))
        .unwrap_err();
    assert_eq!(
        err.localized.unwrap().id,
        ThemeProblem::Unknown.message_id()
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("theme.kdl")).unwrap(),
        text,
        "nothing written"
    );
}

#[test]
fn the_manager_ignores_the_accent_in_high_contrast_and_keeps_it_stored() {
    // ADR 0161: the host following Windows, and a remote session's own OS state.
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::load(dir.path());
    manager.set_system_appearance(Some(SystemAppearance::LIGHT));
    let preferences = ThemePreferences {
        overrides: ThemeOverrides {
            font_size: Some(16),
            radius: None,
            accent: Some("#ff0066".into()),
        },
        ..prefs(true, "frost", "graphite")
    };
    manager.set_theme_preferences(&preferences).unwrap();
    assert_eq!(token(&manager.theme(), "color", "accent"), Some("#ff0066"));
    let stored = std::fs::read_to_string(dir.path().join("theme.kdl")).unwrap();
    assert!(stored.contains(r##"color "accent" "#ff0066""##), "{stored}");

    // A remote session in a contrast theme: its own theme without the accent.
    let (appearance, theme) = manager.appearance_for(Some(HIGH_CONTRAST));
    assert_eq!(appearance.active, "high-contrast");
    assert_eq!(token(&theme, "color", "accent"), Some("#ffff00"));
    assert_eq!(token(&theme, "font-size", "body"), Some("16px"));
    assert_eq!(
        token(&manager.theme(), "color", "accent"),
        Some("#ff0066"),
        "the host is unchanged"
    );

    // Windows switches to a contrast theme.
    let updates = manager.set_system_appearance(Some(HIGH_CONTRAST));
    let [
        ConfigUpdate::ThemeChanged { appearance },
        ConfigUpdate::Changed { theme, .. },
    ] = updates.as_slice()
    else {
        panic!("{updates:?}")
    };
    assert_eq!(appearance.active, "high-contrast");
    assert_eq!(
        appearance.settings.overrides.accent.as_deref(),
        Some("#ff0066")
    );
    assert!(nail_api::appearance::accent_ignored(
        &appearance.active,
        &appearance.themes
    ));
    assert_eq!(token(theme, "color", "accent"), Some("#ffff00"));
    assert_eq!(token(theme, "font-size", "body"), Some("16px"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("theme.kdl")).unwrap(),
        stored,
        "nothing written"
    );

    // Back to light: the accent applies again.
    manager.set_system_appearance(Some(SystemAppearance::LIGHT));
    assert_eq!(token(&manager.theme(), "color", "accent"), Some("#ff0066"));
}

#[test]
fn theme_kdl_edits_report_theme_changed_only_when_the_selection_changes() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "theme.kdl",
        "theme \"Mine\" extends=\"default\"\n",
    );
    let manager = ConfigManager::load(dir.path());
    // A part rule: the theme changes, the selection does not.
    write(
        dir.path(),
        "theme.kdl",
        "theme \"Mine\" extends=\"default\" { part \"app\" { color \"red\"; }; }\n",
    );
    let updates = manager.reload_updates(ConfigKind::Theme);
    assert!(
        matches!(updates.as_slice(), [ConfigUpdate::Changed { .. }]),
        "{updates:?}"
    );
    // The density: both.
    write(
        dir.path(),
        "theme.kdl",
        "theme \"Mine\" extends=\"default\" { appearance density=\"compact\"; part \"app\" { color \"red\"; }; }\n",
    );
    let updates = manager.reload_updates(ConfigKind::Theme);
    assert!(
        matches!(updates.as_slice(), [ConfigUpdate::ThemeChanged { appearance }, ConfigUpdate::Changed { theme, .. }]
            if appearance.settings.density == Density::Compact && theme.density == Density::Compact),
        "{updates:?}"
    );
}

#[test]
fn reloading_the_themes_directory() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "theme.kdl",
        "theme \"Mine\" extends=\"ocean\"\n",
    );
    let manager = ConfigManager::load(dir.path());
    assert_eq!(
        manager.appearance().active,
        "frost",
        "ocean does not exist yet"
    );
    assert!(manager.reload_themes().is_empty(), "nothing changed");

    // The theme appears: the list and the active theme change.
    write(dir.path(), "themes/ocean.kdl", OCEAN);
    let updates = manager.reload_themes();
    assert!(
        matches!(updates.as_slice(), [ConfigUpdate::ThemeChanged { appearance }, ConfigUpdate::Changed { .. }]
            if appearance.active == "ocean"),
        "{updates:?}"
    );
    // Editing the active theme re-resolves it; the list changes too, since
    // its gallery preview shows the theme's own colours (M5b).
    write(
        dir.path(),
        "themes/ocean.kdl",
        &OCEAN.replace("#00aaff", "#0088cc"),
    );
    let updates = manager.reload_themes();
    assert!(
        matches!(updates.as_slice(), [ConfigUpdate::ThemeChanged { appearance }, ConfigUpdate::Changed { .. }]
            if appearance.themes.iter().any(|t| t.id == "ocean"
                && t.preview.as_ref().and_then(|p| p.get("accent")).map(String::as_str) == Some("#0088cc"))),
        "{updates:?}"
    );
    assert_eq!(token(&manager.theme(), "color", "accent"), Some("#0088cc"));

    // Breaking it: its errors, the list (with the problem) and the
    // fallback (a file that does not parse has no known kind: Frost).
    write(
        dir.path(),
        "themes/ocean.kdl",
        "theme \"Ocean\" extends=\"graphite\" {",
    );
    let updates = manager.reload_themes();
    let [
        ConfigUpdate::ThemeFileErrors { file, errors },
        ConfigUpdate::ThemeChanged { appearance },
        ConfigUpdate::Changed { .. },
    ] = updates.as_slice()
    else {
        panic!("{updates:?}")
    };
    assert_eq!(file, "themes/ocean.kdl");
    assert!(!errors.is_empty());
    assert_eq!(appearance.active, "frost");
    assert!(matches!(
        updates[0].to_event(),
        Event::ConfigErrors { ref file, .. } if file == "themes/ocean.kdl"
    ));

    // Removing it: the error list is cleared.
    std::fs::remove_file(dir.path().join("themes/ocean.kdl")).unwrap();
    let updates = manager.reload_themes();
    assert!(
        matches!(&updates[0], ConfigUpdate::ThemeFileErrors { errors, .. } if errors.is_empty()),
        "{updates:?}"
    );
    assert!(manager.theme_file_errors().is_empty());
}

/// Waits for an update matching `pred`, skipping others.
fn wait_for(
    rx: &mpsc::Receiver<ConfigUpdate>,
    pred: impl Fn(&ConfigUpdate) -> bool,
) -> ConfigUpdate {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(update) if pred(&update) => return update,
            Ok(_) => {}
            Err(e) => panic!("no matching config update: {e}"),
        }
    }
}

#[test]
fn the_watcher_hot_reloads_user_themes() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("config");
    write(
        &config_dir,
        "theme.kdl",
        "theme \"Mine\" extends=\"paper\"\n",
    );
    let manager = ConfigManager::load(&config_dir);
    let (tx, rx) = mpsc::channel();
    let _watcher = manager
        .watch(Duration::from_millis(50), move |update| {
            tx.send(update).ok();
        })
        .unwrap();
    assert!(config_dir.join("themes").is_dir(), "watch creates themes/");

    write(&config_dir, "themes/paper.kdl", PAPER);
    let update = wait_for(
        &rx,
        |u| matches!(u, ConfigUpdate::ThemeChanged { appearance } if appearance.active == "paper"),
    );
    let ConfigUpdate::ThemeChanged { appearance } = update else {
        unreachable!()
    };
    assert!(appearance.themes.iter().any(|t| t.id == "paper"));
    wait_for(
        &rx,
        |u| matches!(u, ConfigUpdate::Changed { theme, .. } if theme.tokens.get("color", "bg") == Some("#fbf8f0")),
    );

    write(&config_dir, "themes/paper.kdl", "theme \"Paper\" {");
    wait_for(
        &rx,
        |u| matches!(u, ConfigUpdate::ThemeFileErrors { file, errors } if file == "themes/paper.kdl" && !errors.is_empty()),
    );
    assert_eq!(manager.appearance().active, "frost");
}
