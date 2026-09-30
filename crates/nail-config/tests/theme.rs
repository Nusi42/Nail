//! Theme parsing, merging and error reporting.

use nail_api::{HtmlDarkMode, PartCondition, ResolvedTheme, ThemeMode};
use nail_config::{ConfigError, ConfigErrorKind, parse_theme};
use pretty_assertions::assert_eq;

fn theme(text: &str) -> ResolvedTheme {
    parse_theme(text, "theme.kdl").unwrap_or_else(|e| panic!("{e}"))
}

fn error(text: &str) -> ConfigError {
    parse_theme(text, "theme.kdl").expect_err("expected an error")
}

/// Asserts an error position and a message fragment.
#[track_caller]
fn assert_error(text: &str, line: u32, column: u32, needle: &str) {
    let err = error(text);
    assert_eq!(err.file, "theme.kdl");
    assert!(err.message.contains(needle), "{err}");
    assert_eq!((err.line, err.column), (line, column), "{err}");
}

/// A complete theme without `extends`.
const MINIMAL: &str = r##"
theme "Minimal" mode="dark" {
    tokens {
        color "bg" "#fff" dark="#000"
        color "fg" light="#111" dark="#eee"
        font "body" "Inter, sans-serif"
    }
    message {
        font-family "$font.body"
        text-font-family "monospace"
        font-size 15
        line-height 1.4
        max-width "none"
        quote-colors "$color.fg" "gray"
        html-dark-mode "invert"
    }
}
"##;

#[test]
fn a_complete_theme_without_extends() {
    let t = theme(MINIMAL);
    assert_eq!(t.name, "Minimal");
    assert_eq!(t.mode, ThemeMode::Dark);
    assert_eq!(t.tokens.get("color", "bg"), Some("#fff"));
    // `light=` without a positional value is the base value.
    assert_eq!(t.tokens.get("color", "fg"), Some("#111"));
    assert_eq!(t.modes["dark"].get("color", "fg"), Some("#eee"));
    assert!(!t.modes.contains_key("light"));
    assert!(t.parts.is_empty() && t.icons.is_empty());
    assert_eq!(t.message.font_family, "var(--nail-font-body)");
    assert_eq!(t.message.font_size, "15px");
    assert_eq!(t.message.line_height, "1.4");
    assert_eq!(t.message.quote_colors, ["var(--nail-color-fg)", "gray"]);
    assert_eq!(t.message.html_dark_mode, HtmlDarkMode::Invert);
}

#[test]
fn design_doc_example_nord_dense() {
    // The theme example from docs/design.md ("UI customization").
    let t = theme(
        r##"
theme "Nord Dense" extends="default" {
    tokens {
        color "bg"      light="#eceff4" dark="#2e3440"
        color "accent"  "#88c0d0"
        font  "ui"      "Inter, system-ui"
        space "row"     4
    }
    part "message-list.row" {
        padding "$space.row" "8px"
        border-bottom "1px solid $color.border"
    }
    part "message-list.row:unread .subject" { font-weight 700 }
    part "message-list.row[tag=\"work\"]"   { border-left "3px solid $color.accent" }
    icon "archive" svg="M3 3h18v4H3z M5 9h14v12H5z"
}
"##,
    );
    assert_eq!(t.name, "Nord Dense");
    assert_eq!(t.tokens.get("color", "bg"), Some("#eceff4"));
    assert_eq!(t.modes["dark"].get("color", "bg"), Some("#2e3440"));
    // A redefined token replaces its default mode variants, too.
    assert_eq!(t.tokens.get("color", "accent"), Some("#88c0d0"));
    assert_eq!(t.modes["dark"].get("color", "accent"), None);
    assert_eq!(t.modes["high-contrast"].get("color", "accent"), None);
    // Untouched tokens keep their defaults and variants.
    assert_eq!(t.modes["dark"].get("color", "fg"), Some("#e3e6eb"));
    assert_eq!(t.tokens.get("font", "ui"), Some("Inter, system-ui"));
    assert_eq!(t.tokens.get("space", "row"), Some("4px"));

    // Same selector as a default rule: declarations merge into that rule.
    let rows: Vec<_> = t
        .parts
        .iter()
        .filter(|r| {
            r.selector == "message-list.row" && r.states.is_empty() && r.conditions.is_empty()
        })
        .collect();
    assert_eq!(rows.len(), 1);
    let row = rows[0];
    assert_eq!(row.declarations["padding"], "var(--nail-space-row) 8px");
    assert_eq!(
        row.declarations["border-bottom"],
        "1px solid var(--nail-color-border)"
    );
    assert_eq!(
        row.declarations["display"], "flex",
        "default declarations stay"
    );
    assert_eq!(row.source.as_ref().unwrap().file, "theme.kdl");

    let subject = t
        .parts
        .iter()
        .find(|r| r.selector == "message-list.row.subject")
        .unwrap();
    assert_eq!(subject.declarations["font-weight"], "700");
    assert_eq!(subject.ancestors[0].states, ["unread"]);

    let work = t
        .parts
        .iter()
        .find(|r| !r.conditions.is_empty() && r.selector == "message-list.row")
        .unwrap();
    assert_eq!(
        work.conditions,
        [PartCondition {
            key: "tag".into(),
            value: Some("work".into())
        }]
    );
    assert_eq!(
        work.declarations["border-left"],
        "3px solid var(--nail-color-accent)"
    );
    let src = work.source.as_ref().unwrap();
    assert_eq!((src.line, src.column), (14, 10));

    assert_eq!(t.icons["archive"].path, "M3 3h18v4H3z M5 9h14v12H5z");
    assert_eq!(t.icons["archive"].view_box, "0 0 24 24");
    assert!(t.icons.contains_key("reply"), "default icons stay");
}

#[test]
fn every_section_parses() {
    let t = theme(
        r##"
theme "All" extends="default" mode="high-contrast" {
    tokens {
        z-index "popup" 100
        radius "pill" 999
        opacity "ghost" 0.3
        motion "slow" "400ms"
        border "strong" "2px solid $color.fg"
        color "link" "$color.accent" high-contrast="#00ffff"
    }
    modes {
        dark { color "bg" "#000000"; }
        high-contrast { color "link" "#ff00ff"; }
    }
    parts {
        part "toolbar.button:hover:focus-visible" { outline-width 2; }
    }
    part "message-list.row[encrypted]" { opacity "$opacity.ghost"; }
    icons {
        icon "star" svg="M12 2l3 7h7l-5.5 4 2 7-6.5-4.5L5.5 20l2-7L2 9h7z" view-box="0 0 24 24"
    }
    icon "dot" svg="M0 0h2v2H0z" view-box="0 0 2 2"
    message { max-width "72ch"; html-dark-mode "off"; }
    sounds {
        mail.new "chime"
        calendar.reminder "sounds/bell.ogg"
    }
    raw-css "message-list.row" "text-wrap: balance; color: $color.fg;"
}
"##,
    );
    assert_eq!(t.mode, ThemeMode::HighContrast);
    assert_eq!(t.tokens.get("z-index", "popup"), Some("100"));
    assert_eq!(t.tokens.get("radius", "pill"), Some("999px"));
    assert_eq!(t.tokens.get("opacity", "ghost"), Some("0.3"));
    assert_eq!(
        t.tokens.get("border", "strong"),
        Some("2px solid var(--nail-color-fg)")
    );
    assert_eq!(
        t.tokens.get("color", "link"),
        Some("var(--nail-color-accent)")
    );
    // `modes` overrides a default token's dark value and, later in the file,
    // this file's own inline high-contrast value.
    assert_eq!(t.modes["dark"].get("color", "bg"), Some("#000000"));
    assert_eq!(t.modes["high-contrast"].get("color", "bg"), Some("#000000"));
    assert_eq!(
        t.modes["high-contrast"].get("color", "link"),
        Some("#ff00ff")
    );

    let hover = t
        .parts
        .iter()
        .find(|r| r.states == ["hover", "focus-visible"])
        .unwrap();
    assert_eq!(hover.declarations["outline-width"], "2px");
    assert!(t.parts.iter().any(|r| {
        r.conditions
            .first()
            .is_some_and(|c| c.key == "encrypted" && c.value.is_none())
    }));
    assert_eq!(t.icons["dot"].view_box, "0 0 2 2");
    assert_eq!(t.message.max_width, "72ch");
    assert_eq!(t.message.html_dark_mode, HtmlDarkMode::Off);
    assert_eq!(
        t.message.font_size, "var(--nail-font-size-md)",
        "default message fields stay"
    );
    assert_eq!(t.sounds["mail.new"], "chime");
    assert_eq!(t.sounds["calendar.reminder"], "sounds/bell.ogg");
    assert_eq!(t.raw_css[0].selector, "message-list.row");
    assert_eq!(
        t.raw_css[0].css,
        "text-wrap: balance; color: var(--nail-color-fg);"
    );
}

#[test]
fn modes_only_list_values_that_differ() {
    let t =
        theme(r##"theme "x" extends="default" { tokens { color "same" "#123" dark="#123"; }; }"##);
    assert_eq!(t.modes["dark"].get("color", "same"), None);
}

#[test]
fn tokens_can_reference_tokens_defined_later() {
    let t = theme(
        r##"theme "x" extends="default" { tokens { color "a" "$color.b"; color "b" "red"; }; }"##,
    );
    assert_eq!(t.tokens.get("color", "a"), Some("var(--nail-color-b)"));
}

#[test]
fn syntax_errors() {
    assert_error("theme \"x\" {\n  tokens {\n", 2, 10, "syntax error");
    let err = error("theme \"x\" {\n  tokens { color \"a\" 1. }\n}");
    assert_eq!(err.kind, ConfigErrorKind::Syntax);
    assert_eq!(err.line, 2);
}

#[test]
fn structural_errors_point_at_the_node() {
    assert_error("", 1, 1, "file is empty");
    assert_error("layout \"x\"", 1, 1, "unexpected `layout`");
    assert_error("theme \"a\"\ntheme \"b\"", 2, 1, "only one `theme`");
    assert_error("theme \"x\" extends=\"nord\"", 1, 11, "unknown base `nord`");
    assert_error("theme \"x\" mode=\"dim\"", 1, 11, "unknown mode `dim`");
    assert_error(
        "theme \"x\" {\n  colours {}\n}",
        2,
        3,
        "unknown theme section `colours`",
    );
    assert_error("theme 5", 1, 7, "`theme` #1 must be a string");
    assert_error(
        "theme \"x\" extends=\"default\" {\n  tokens { colour \"a\" \"red\"; }\n}",
        2,
        12,
        "unknown token category `colour`",
    );
    assert_error(
        "theme \"x\" extends=\"default\" {\n  tokens { color \"A\" \"red\"; }\n}",
        2,
        18,
        "invalid token name",
    );
    assert_error(
        "theme \"x\" extends=\"default\" {\n  tokens { color \"a\"; }\n}",
        2,
        12,
        "needs a value",
    );
    assert_error(
        "theme \"x\" extends=\"default\" {\n  tokens { color \"a\" \"red\" dim=\"x\"; }\n}",
        2,
        28,
        "unknown property `dim`",
    );
    assert_error(
        "theme \"x\" extends=\"default\" {\n  modes { dim { } }\n}",
        2,
        11,
        "unknown mode `dim`",
    );
}

#[test]
fn part_errors() {
    assert_error(
        "theme \"x\" extends=\"default\" {\n  part \"Row\" { color \"red\"; }\n}",
        2,
        8,
        "invalid part name",
    );
    assert_error(
        "theme \"x\" extends=\"default\" {\n  part \"row\" {\n    colour \"red\"\n  }\n}",
        3,
        5,
        "unsupported CSS property `colour`",
    );
    assert_error(
        "theme \"x\" extends=\"default\" {\n  part \"row\" { color #true; }\n}",
        2,
        22,
        "must be a string or a number",
    );
    assert_error(
        "theme \"x\" extends=\"default\" {\n  part \"row\" { }\n}",
        2,
        3,
        "has no declarations",
    );
    assert_error(
        "theme \"x\" extends=\"default\" {\n  part \"row\" { color; }\n}",
        2,
        16,
        "needs a value",
    );
    assert_error(
        "theme \"x\" extends=\"default\" {\n  part \"row\" { background \"url(https://evil.example/x.png)\"; }\n}",
        2,
        27,
        "cannot load external resources",
    );
    assert_error(
        "theme \"x\" extends=\"default\" {\n  part \"row\" { color \"red; x: y\"; }\n}",
        2,
        22,
        "`;` is not allowed",
    );
    assert_error(
        "theme \"x\" extends=\"default\" {\n  parts { icon \"a\" svg=\"M0 0\"; }\n}",
        2,
        11,
        "may only contain `part`",
    );
}

#[test]
fn token_reference_errors_point_at_the_value() {
    assert_error(
        "theme \"x\" extends=\"default\" {\n  part \"row\" {\n    border \"1px solid $color.nope\"\n  }\n}",
        3,
        12,
        "unknown token `$color.nope`",
    );
    assert_error(
        "theme \"x\" extends=\"default\" {\n  tokens { color \"a\" \"$colour.b\"; }\n}",
        2,
        22,
        "unknown token category `colour`",
    );
    assert_error(
        "theme \"x\" extends=\"default\" {\n  tokens {\n    color \"a\" \"$color.b\"\n    color \"b\" \"$color.a\"\n  }\n}",
        4,
        15,
        "token reference cycle: $color.a → $color.b → $color.a",
    );
    assert_error(
        "theme \"x\" extends=\"default\" {\n  modes { dark { color \"nope\" \"red\"; } }\n}",
        2,
        31,
        "mode override for unknown token `color.nope`",
    );
    assert_error(
        "theme \"x\" extends=\"default\" {\n  message { font-family \"$font.nope\"; }\n}",
        2,
        25,
        "unknown token `$font.nope`",
    );
}

#[test]
fn icon_message_sound_and_raw_css_errors() {
    let base = "theme \"x\" extends=\"default\" {\n  ";
    let check = |body: &str, column: u32, needle: &str| {
        assert_error(&format!("{base}{body}\n}}"), 2, column, needle)
    };
    check("icon \"a\"", 3, "needs svg=");
    check(
        "icon \"a\" svg=\"M0 0<script>\"",
        12,
        "`<` is not allowed in SVG path data",
    );
    check(
        "icon \"a\" svg=\"M0 0\" view-box=\"0 0 24\"",
        23,
        "four numbers",
    );
    check("icon \"A\" svg=\"M0 0\"", 8, "invalid icon name");
    check(
        "message { font-weight 700; }",
        13,
        "unknown message setting `font-weight`",
    );
    check(
        "message { html-dark-mode \"dim\"; }",
        28,
        "unknown html-dark-mode `dim`",
    );
    check("message { max-width; }", 13, "needs a value");
    check(
        "message { max-width \"1\" \"2\"; }",
        27,
        "too many arguments",
    );
    check(
        "sounds { mail.new \"../../x.wav\"; }",
        21,
        "inside the config directory",
    );
    check("sounds { Mail \"x\"; }", 12, "invalid event name");
    check(
        "raw-css \"row\" \"background: url(x)\"",
        17,
        "cannot load external resources",
    );
    check(
        "raw-css \"row\" \"} body { color: red\"",
        17,
        "`}` is not allowed",
    );
    check("raw-css \"Row\" \"color: red\"", 11, "invalid part name");
}

#[test]
fn a_file_without_extends_must_be_complete() {
    let err = error("theme \"x\" {\n  tokens { color \"a\" \"red\"; }\n}");
    assert!(err.message.contains("missing `font-family`"), "{err}");
    assert_eq!((err.line, err.column), (1, 7));
    // Without the defaults, default tokens do not exist.
    let err = error("theme \"x\" {\n  part \"row\" { color \"$color.fg\"; }\n}");
    assert!(err.message.contains("unknown token `$color.fg`"), "{err}");
}

#[test]
fn file_names_appear_in_errors_and_sources() {
    let err = parse_theme("theme \"x\" { bogus }", "C:/cfg/theme.kdl").unwrap_err();
    assert_eq!(err.file, "C:/cfg/theme.kdl");
    assert_eq!(
        err.to_string(),
        "C:/cfg/theme.kdl:1:13: unknown theme section `bogus` (expected tokens, modes, part, parts, icon, icons, message, sounds or raw-css)"
    );
}
