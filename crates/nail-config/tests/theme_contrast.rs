//! Contrast of every built-in theme (M5a; slot of WP151, contract ADR 0102):
//! WCAG AA for text and UI parts in every theme, AAA for High contrast.
//!
//! WP151 fills this in (colour parsing, relative luminance, the pairs of
//! tokens that meet as text on background or as UI part boundaries). The
//! contract stage only checks that every built-in theme resolves with the
//! colour tokens the check needs.

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
