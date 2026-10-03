//! Contrast of every built-in theme (M5a WP151, ADR 0102): WCAG 2.2 AA
//! for text (4.5:1) and UI parts (3:1, success criterion 1.4.11) in every
//! theme, AAA (7:1) for High contrast, and AA for the print view of every
//! theme.
//!
//! Each theme is resolved through `nail-config` exactly as the UI gets it
//! (the base `defaults/theme.kdl` plus the theme file), then each colour
//! token is evaluated in the mode the theme renders in: the mode's
//! override (`dark=` etc.) or the base value, following `$color.*`
//! references and `color-mix(in srgb, …)`, as the browser does on `:root`.
//!
//! The pairs below are the colours that meet as text on a background or
//! as a control boundary on a surface. `the_default_parts_only_use_checked_pairs`
//! keeps the table honest: every text colour the default part rules use,
//! and every text / background pair a rule declares together, must be in
//! it, so a new part rule with an unchecked combination fails here.

use nail_api::{ResolvedTheme, ThemeMode};

/// Minimum ratios.
const AA_TEXT: f64 = 4.5;
const AA_UI: f64 = 3.0;
const AAA: f64 = 7.0;

/// The neutral surfaces text sits on.
const SURFACES: &[&str] = &["bg", "bg-subtle", "bg-raised"];
/// Text colours that appear on any surface.
const TEXT_ON_SURFACES: &[&str] = &[
    "fg",
    "fg-muted",
    "fg-subtle",
    "accent",
    "unread",
    "danger",
    "warning",
    "success",
];

/// What a pair has to meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Need {
    /// Readable text (AA 4.5:1; AAA 7:1 in High contrast).
    Text,
    /// A boundary or an indicator that identifies a control (3:1; 7:1 in
    /// High contrast).
    Ui,
}

/// One foreground colour against several backgrounds.
struct Pair {
    fg: &'static str,
    bgs: &'static [&'static str],
    need: Need,
}

const fn text(fg: &'static str, bgs: &'static [&'static str]) -> Pair {
    Pair {
        fg,
        bgs,
        need: Need::Text,
    }
}

const fn ui(fg: &'static str, bgs: &'static [&'static str]) -> Pair {
    Pair {
        fg,
        bgs,
        need: Need::Ui,
    }
}

/// Pairs every theme meets, beyond `TEXT_ON_SURFACES` × `SURFACES`.
const PAIRS: &[Pair] = &[
    // The window background and the navigation rail carry labels too.
    text("fg", &["canvas", "rail"]),
    text("fg-muted", &["canvas", "rail"]),
    text("fg-subtle", &["canvas", "rail"]),
    // Selected items, filled buttons and badges.
    text("selection-fg", &["selection"]),
    text("accent-fg", &["accent", "danger", "warning"]),
    // Pressed chips and primary icon buttons on the soft accent.
    text("accent", &["accent-soft"]),
    text("fg", &["accent-soft"]),
    // Control outlines, focus rings and indicators (unread dot, flag,
    // account status) on every surface they appear on.
    ui("border-strong", &["bg", "bg-subtle", "bg-raised", "canvas"]),
    ui(
        "focus-ring",
        &[
            "bg",
            "bg-subtle",
            "bg-raised",
            "canvas",
            "rail",
            "accent-soft",
        ],
    ),
    ui("flagged", &["bg", "bg-subtle", "bg-raised"]),
    // Avatar initials and file-type badges (M5a WP155).
    text(
        "avatar-fg",
        &[
            "avatar-1",
            "avatar-2",
            "avatar-3",
            "avatar-4",
            "avatar-5",
            "avatar-6",
            "avatar-7",
            "avatar-8",
            "file-pdf",
            "file-image",
            "file-document",
            "file-spreadsheet",
            "file-archive",
            "file-calendar",
            "file-other",
        ],
    ),
];

/// Text on a *soft* selection (a tint of the accent, as in Frost, Sage,
/// Graphite and Nocturne): secondary text, dates and state colours stay
/// as they are inside a selected row, so they must be readable on it.
/// High contrast uses a solid highlight instead, where everything inside a
/// selected item switches to `selection-fg` (the default parts set
/// `color: inherit` under `:selected`).
const ON_SOFT_SELECTION: &[&str] = &[
    "fg",
    "fg-muted",
    "fg-subtle",
    "accent",
    "unread",
    "danger",
    "warning",
    "success",
];

/// Indicators on a soft selection: the focus ring of the selected row and
/// its flag. On the solid High contrast highlight the ring is drawn
/// outside the item, against the surface, and flagged rows are underlined.
const UI_ON_SOFT_SELECTION: &[&str] = &["focus-ring", "flagged"];

/// Text of the print view on its paper.
const PRINT_TEXT: &[&str] = &[
    "fg",
    "fg-muted",
    "fg-subtle",
    "accent",
    "danger",
    "warning",
    "success",
];

// ---------------------------------------------------------------------------
// Colour evaluation

/// An sRGB colour, channels 0..=255 (fractional after mixing).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rgb([f64; 3]);

impl Rgb {
    /// WCAG 2.x relative luminance.
    fn luminance(self) -> f64 {
        let lin = |c: f64| {
            let c = c / 255.0;
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let [r, g, b] = self.0.map(lin);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    fn hex(self) -> String {
        let [r, g, b] = self.0.map(|c| c.round() as u8);
        format!("#{r:02x}{g:02x}{b:02x}")
    }
}

/// WCAG contrast ratio of two colours (1..=21).
fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (a.luminance(), b.luminance());
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn parse_hex(value: &str) -> Option<Rgb> {
    let digits = value.strip_prefix('#')?;
    let channel = |s: &str| u8::from_str_radix(s, 16).ok().map(f64::from);
    match digits.len() {
        3 => {
            let mut out = [0.0; 3];
            for (i, c) in digits.chars().enumerate() {
                out[i] = channel(&format!("{c}{c}"))?;
            }
            Some(Rgb(out))
        }
        6 => Some(Rgb([
            channel(&digits[0..2])?,
            channel(&digits[2..4])?,
            channel(&digits[4..6])?,
        ])),
        _ => None,
    }
}

/// Splits at commas that are not inside parentheses.
fn split_top_level(s: &str) -> Vec<&str> {
    let (mut depth, mut start, mut out) = (0usize, 0usize, Vec::new());
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(s[start..].trim());
    out
}

/// One `color-mix` argument: a colour and an optional percentage.
fn mix_arg(arg: &str) -> (&str, Option<f64>) {
    if let Some((color, pct)) = arg.rsplit_once(' ')
        && let Some(n) = pct.strip_suffix('%')
        && let Ok(n) = n.parse::<f64>()
    {
        return (color.trim(), Some(n / 100.0));
    }
    (arg, None)
}

/// Evaluates colour tokens of one theme in one mode.
struct Colors<'a> {
    theme: &'a ResolvedTheme,
    mode: &'a str,
}

impl Colors<'_> {
    /// The value of `color.<name>` in this mode (the mode's override, else
    /// the base value).
    fn raw(&self, name: &str) -> Result<&str, String> {
        self.theme
            .modes
            .get(self.mode)
            .and_then(|m| m.get("color", name))
            .or_else(|| self.theme.tokens.get("color", name))
            .ok_or_else(|| format!("color.{name} is not defined"))
    }

    fn get(&self, name: &str) -> Result<Rgb, String> {
        let raw = self.raw(name)?;
        self.eval(raw, 0)
            .map_err(|e| format!("color.{name} = `{raw}`: {e}"))
    }

    fn eval(&self, value: &str, depth: usize) -> Result<Rgb, String> {
        if depth > 16 {
            return Err("references nest too deeply".into());
        }
        let value = value.trim();
        if let Some(rgb) = parse_hex(value) {
            return Ok(rgb);
        }
        if let Some(name) = value
            .strip_prefix("var(--nail-color-")
            .and_then(|r| r.strip_suffix(')'))
        {
            let raw = self.raw(name)?;
            return self.eval(raw, depth + 1);
        }
        if let Some(body) = value
            .strip_prefix("color-mix(")
            .and_then(|r| r.strip_suffix(')'))
        {
            let args = split_top_level(body);
            let [space, a, b] = args.as_slice() else {
                return Err("color-mix needs a colour space and two colours".into());
            };
            if *space != "in srgb" {
                return Err(format!("color-mix in `{space}` is not checked (use srgb)"));
            }
            let ((a, pa), (b, pb)) = (mix_arg(a), mix_arg(b));
            let (wa, wb) = match (pa, pb) {
                (None, None) => (0.5, 0.5),
                (Some(p), None) => (p, 1.0 - p),
                (None, Some(p)) => (1.0 - p, p),
                // Below 100 % in total CSS makes the result translucent.
                (Some(p), Some(q)) if p + q < 1.0 => {
                    return Err("color-mix percentages below 100 % are translucent".into());
                }
                (Some(p), Some(q)) => (p / (p + q), q / (p + q)),
            };
            let (ca, cb) = (self.eval(a, depth + 1)?, self.eval(b, depth + 1)?);
            let mut out = [0.0; 3];
            for (i, c) in out.iter_mut().enumerate() {
                *c = ca.0[i] * wa + cb.0[i] * wb;
            }
            return Ok(Rgb(out));
        }
        Err(
            "only opaque #rgb / #rrggbb, $color references and color-mix(in srgb) are checked"
                .into(),
        )
    }
}

// ---------------------------------------------------------------------------
// The themes under test

struct Case {
    /// `default` (the base) or a built-in theme id.
    id: &'static str,
    theme: ResolvedTheme,
}

fn cases() -> Vec<Case> {
    let mut out = vec![Case {
        id: "default",
        theme: nail_config::default_theme(),
    }];
    for builtin in nail_config::BUILTIN_THEME_FILES {
        let theme = nail_config::resolve_builtin_theme(builtin.id)
            .expect("listed")
            .unwrap_or_else(|e| panic!("{}: {e}", builtin.id));
        out.push(Case {
            id: builtin.id,
            theme,
        });
    }
    out
}

/// The colour modes a theme renders in: its own mode, or all three for a
/// theme that follows the system (the base).
fn screen_modes(theme: &ResolvedTheme) -> &'static [&'static str] {
    match theme.mode {
        ThemeMode::Light => &["light"],
        ThemeMode::Dark => &["dark"],
        ThemeMode::HighContrast => &["high-contrast"],
        ThemeMode::System => &["light", "dark", "high-contrast"],
    }
}

/// Every failing pair of one theme in one mode, as readable lines.
fn failures(id: &str, theme: &ResolvedTheme, mode: &str) -> Vec<String> {
    let colors = Colors { theme, mode };
    let high_contrast = mode == "high-contrast";
    let mut checks: Vec<(&str, &str, Need)> = Vec::new();
    for fg in TEXT_ON_SURFACES {
        for bg in SURFACES {
            checks.push((fg, bg, Need::Text));
        }
    }
    for pair in PAIRS {
        for bg in pair.bgs {
            checks.push((pair.fg, bg, pair.need));
        }
    }
    if !high_contrast {
        for fg in ON_SOFT_SELECTION {
            checks.push((fg, "selection", Need::Text));
        }
        for fg in UI_ON_SOFT_SELECTION {
            checks.push((fg, "selection", Need::Ui));
        }
    }
    let mut out = Vec::new();
    for (fg, bg, need) in checks {
        let min = match (high_contrast, need) {
            (true, _) => AAA,
            (false, Need::Text) => AA_TEXT,
            (false, Need::Ui) => AA_UI,
        };
        match (colors.get(fg), colors.get(bg)) {
            (Ok(f), Ok(b)) => {
                let ratio = contrast(f, b);
                if ratio < min {
                    out.push(format!(
                        "{id} [{mode}] {fg} {} on {bg} {}: {ratio:.2} < {min}",
                        f.hex(),
                        b.hex()
                    ));
                }
            }
            (Err(e), _) | (_, Err(e)) => out.push(format!("{id} [{mode}]: {e}")),
        }
    }
    out
}

fn print_failures(id: &str, theme: &ResolvedTheme) -> Vec<String> {
    let colors = Colors {
        theme,
        mode: "print",
    };
    let mut out = Vec::new();
    for fg in PRINT_TEXT {
        for bg in ["bg", "bg-subtle"] {
            match (colors.get(fg), colors.get(bg)) {
                (Ok(f), Ok(b)) => {
                    let ratio = contrast(f, b);
                    if ratio < AA_TEXT {
                        out.push(format!(
                            "{id} [print] {fg} {} on {bg} {}: {ratio:.2} < {AA_TEXT}",
                            f.hex(),
                            b.hex()
                        ));
                    }
                }
                (Err(e), _) | (_, Err(e)) => out.push(format!("{id} [print]: {e}")),
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests

#[test]
fn every_theme_meets_wcag_contrast_on_screen() {
    let mut all = Vec::new();
    for case in cases() {
        for mode in screen_modes(&case.theme) {
            all.extend(failures(case.id, &case.theme, mode));
        }
    }
    assert!(all.is_empty(), "contrast failures:\n{}", all.join("\n"));
}

#[test]
fn every_theme_prints_black_on_white_readably() {
    let mut all = Vec::new();
    for case in cases() {
        let colors = Colors {
            theme: &case.theme,
            mode: "print",
        };
        // Paper stays white whatever the screen theme is.
        for bg in ["bg", "bg-subtle", "bg-raised", "canvas", "rail"] {
            let paper = colors
                .get(bg)
                .unwrap_or_else(|e| panic!("{}: {e}", case.id));
            assert_eq!(paper.hex(), "#ffffff", "{} [print] {bg}", case.id);
        }
        all.extend(print_failures(case.id, &case.theme));
    }
    assert!(
        all.is_empty(),
        "print contrast failures:\n{}",
        all.join("\n")
    );
}

#[test]
fn every_builtin_theme_has_text_and_background_colours() {
    for builtin in nail_config::BUILTIN_THEME_FILES {
        let theme = nail_config::resolve_builtin_theme(builtin.id)
            .expect("listed")
            .unwrap_or_else(|e| panic!("{}: {e}", builtin.id));
        for token in ["bg", "fg", "accent", "accent-fg", "border", "focus-ring"] {
            assert!(
                theme.tokens.get("color", token).is_some(),
                "{}: color.{token}",
                builtin.id
            );
        }
    }
}

/// Each built-in theme file sets every semantic colour of the base itself
/// (so a theme never shows a stray colour of another palette), and the
/// light / dark themes set colours only: spacing, type, corners,
/// elevation and density are the base's (ADR 0102). High contrast also
/// enlarges text, thickens borders and focus rings and stops motion.
#[test]
fn builtin_theme_files_are_colour_palettes_on_the_base() {
    const SEMANTIC: &[&str] = &[
        "bg",
        "bg-subtle",
        "bg-raised",
        "canvas",
        "rail",
        "fg",
        "fg-muted",
        "fg-subtle",
        "border",
        "border-strong",
        "accent",
        "accent-fg",
        "accent-soft",
        "selection",
        "selection-fg",
        "unread",
        "focus-ring",
        "danger",
        "flagged",
        "warning",
        "success",
    ];
    let base = nail_config::default_theme();
    for name in SEMANTIC {
        assert!(
            base.tokens.get("color", name).is_some(),
            "base: color.{name}"
        );
    }
    for builtin in nail_config::BUILTIN_THEME_FILES {
        let file = nail_config::ThemeFile::parse(builtin.kdl, builtin.file)
            .unwrap_or_else(|e| panic!("{}: {e}", builtin.id));
        let colors = file.tokens.get("color").expect("a colour palette");
        for name in SEMANTIC {
            assert!(
                colors.get(*name).is_some_and(|d| d.value.is_some()),
                "{}: sets no color.{name}",
                builtin.id
            );
        }
        if builtin.id == nail_api::appearance::HIGH_CONTRAST_THEME {
            continue;
        }
        let categories: Vec<_> = file.tokens.keys().map(String::as_str).collect();
        assert_eq!(categories, ["color"], "{}: colours only", builtin.id);
        assert!(file.parts.is_empty(), "{}: no part rules", builtin.id);
        assert!(file.icons.is_empty(), "{}: no icons", builtin.id);
    }
}

/// Light themes are light and dark themes are dark, as their `mode` says
/// (Follow Windows picks them by kind).
#[test]
fn theme_modes_match_their_palettes() {
    for case in cases() {
        for mode in screen_modes(&case.theme) {
            let colors = Colors {
                theme: &case.theme,
                mode,
            };
            let bg = colors.get("bg").unwrap().luminance();
            let fg = colors.get("fg").unwrap().luminance();
            match *mode {
                "light" => assert!(bg > fg, "{} [{mode}] is not light", case.id),
                _ => assert!(bg < fg, "{} [{mode}] is not dark", case.id),
            }
        }
    }
}

/// The accent override (`color "accent"` in the user's theme.kdl) restyles
/// what derives from the accent: the soft accent, the selection, unread
/// marks and (outside High contrast) the focus ring.
#[test]
fn derived_colours_follow_the_accent() {
    for builtin in nail_config::BUILTIN_THEME_FILES {
        let theme = nail_config::resolve_builtin_theme(builtin.id)
            .unwrap()
            .unwrap();
        let mut derived = vec!["unread"];
        if builtin.id != nail_api::appearance::HIGH_CONTRAST_THEME {
            derived.extend(["accent-soft", "selection", "focus-ring"]);
        }
        for name in derived {
            let value = theme.tokens.get("color", name).unwrap();
            assert!(
                value.contains("var(--nail-color-accent)"),
                "{}: color.{name} = `{value}` does not derive from the accent",
                builtin.id
            );
        }
    }
    // And the derived colours really change with it.
    let text = r##"theme "mine" extends="default" mode="light" {
        tokens { color "accent" "#aa0000"; }
    }"##;
    let theme = nail_config::parse_theme(text, "theme.kdl").unwrap();
    let colors = Colors {
        theme: &theme,
        mode: "light",
    };
    assert_eq!(colors.get("unread").unwrap().hex(), "#aa0000");
    assert_eq!(colors.get("focus-ring").unwrap().hex(), "#aa0000");
    // 16 % of #aa0000 on white.
    assert_eq!(colors.get("selection").unwrap().hex(), "#f1d6d6");
}

/// Controls that take input are outlined with `border-strong` (3:1 against
/// the surfaces, checked above), not with the faint divider colour.
#[test]
fn input_controls_use_the_strong_outline() {
    // `form.input` / `form.select` / `form.textarea` / `search-bar.field`
    // have no default rule since M5a: the UI skin (ui/src/theme/skin.ts)
    // outlines them with `var(--nail-border-control)` (checked there).
    const CONTROLS: &[&str] = &[
        "composer.identity",
        "composer.body",
        "composer.html.source",
        "composer.link.input",
        "rules.text.source",
        "merge-dialog.text",
        "key-binding-editor.capture",
        "plugin.input",
        "plugin.textarea",
        "plugin.select",
        "plugin.date",
        "pim.datetime.input",
        "palette.input",
        "folder-picker.input",
    ];
    let theme = nail_config::default_theme();
    let control = theme
        .tokens
        .get("border", "control")
        .expect("border.control");
    assert!(
        control.contains("var(--nail-color-border-strong)"),
        "{control}"
    );
    for selector in CONTROLS {
        let rule = theme
            .parts
            .iter()
            .find(|r| {
                r.selector == *selector
                    && r.states.is_empty()
                    && r.conditions.is_empty()
                    && r.ancestors.is_empty()
            })
            .unwrap_or_else(|| panic!("no default rule for {selector}"));
        assert_eq!(
            rule.declarations.get("border").map(String::as_str),
            Some("var(--nail-border-control)"),
            "{selector}"
        );
    }
}

/// Every text colour the default part rules use is checked above, and so
/// is every text / background pair a rule sets together.
#[test]
fn the_default_parts_only_use_checked_pairs() {
    fn token(value: &str) -> Option<&str> {
        value
            .trim()
            .strip_prefix("var(--nail-color-")
            .and_then(|r| r.strip_suffix(')'))
    }
    let mut checked: Vec<(&str, &str)> = Vec::new();
    for fg in TEXT_ON_SURFACES {
        for bg in SURFACES {
            checked.push((fg, bg));
        }
    }
    for pair in PAIRS.iter().filter(|p| p.need == Need::Text) {
        for bg in pair.bgs {
            checked.push((pair.fg, bg));
        }
    }
    for fg in ON_SOFT_SELECTION {
        checked.push((fg, "selection"));
    }
    let text_colours: Vec<&str> = checked.iter().map(|(fg, _)| *fg).collect();

    let theme = nail_config::default_theme();
    let mut problems = Vec::new();
    for rule in &theme.parts {
        let fg = rule.declarations.get("color").map(String::as_str);
        let bg = rule
            .declarations
            .get("background-color")
            .map(String::as_str);
        let Some(fg_value) = fg else { continue };
        let Some(fg) = token(fg_value) else {
            // `inherit`, `currentColor` and the like take a checked colour
            // from the parent.
            assert!(
                !fg_value.contains("var(--nail-color-"),
                "{}: unexpected colour value `{fg_value}`",
                rule.selector
            );
            continue;
        };
        if !text_colours.contains(&fg) {
            problems.push(format!(
                "{}: text colour {fg} is not checked",
                rule.selector
            ));
        }
        if let Some(bg) = bg.and_then(token)
            && !checked.contains(&(fg, bg))
        {
            problems.push(format!("{}: {fg} on {bg} is not checked", rule.selector));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

// ---------------------------------------------------------------------------
// The checker itself

#[test]
fn contrast_matches_the_wcag_reference_values() {
    let black = parse_hex("#000").unwrap();
    let white = parse_hex("#ffffff").unwrap();
    assert!((contrast(black, white) - 21.0).abs() < 1e-9);
    assert!((contrast(white, white) - 1.0).abs() < 1e-9);
    // #767676 on white is the classic 4.54:1 grey.
    let grey = parse_hex("#767676").unwrap();
    assert!((contrast(grey, white) - 4.54).abs() < 0.01);
    assert!((contrast(white, grey) - contrast(grey, white)).abs() < 1e-12);
    assert!(parse_hex("#12345").is_none());
    assert!(parse_hex("red").is_none());
}

#[test]
fn colour_values_are_evaluated_like_css_on_root() {
    let text = r##"theme "t" extends="default" mode="dark" {
        tokens {
            color "bg" "#000000" dark="#202020"
            color "accent" "#ffffff"
            color "accent-soft" "color-mix(in srgb, $color.accent 25%, $color.bg)"
            color "selection" "color-mix(in srgb, $color.accent, $color.bg)"
            color "unread" "$color.accent-soft"
            color "scrim" "rgb(0 0 0 / 0.5)"
        }
    }"##;
    let theme = nail_config::parse_theme(text, "theme.kdl").unwrap();
    let dark = Colors {
        theme: &theme,
        mode: "dark",
    };
    // The mode override wins, and references see the mode's values.
    assert_eq!(dark.get("bg").unwrap().hex(), "#202020");
    // 25 % white + 75 % #202020 = 0x20 * 0.75 + 0xff * 0.25 = 87.75 → 0x58.
    assert_eq!(dark.get("accent-soft").unwrap().hex(), "#585858");
    assert_eq!(dark.get("unread").unwrap().hex(), "#585858");
    // No percentages: half and half.
    assert_eq!(dark.get("selection").unwrap().hex(), "#909090");
    let light = Colors {
        theme: &theme,
        mode: "light",
    };
    assert_eq!(light.get("bg").unwrap().hex(), "#000000");
    // Translucent colours and unknown tokens are reported, not guessed.
    assert!(dark.get("scrim").is_err());
    assert!(dark.get("nope").is_err());
    assert!(dark.eval("color-mix(in oklch, #fff, #000)", 0).is_err());
    assert!(
        dark.eval("color-mix(in srgb, #fff 30%, #000 10%)", 0)
            .is_err()
    );
    // Above 100 % in total the percentages are scaled down.
    assert_eq!(
        dark.eval("color-mix(in srgb, #fff 150%, #000 50%)", 0)
            .unwrap()
            .hex(),
        "#bfbfbf"
    );
}
