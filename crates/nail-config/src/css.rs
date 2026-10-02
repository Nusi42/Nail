//! CSS knowledge used by the theme: the supported property subset, value
//! safety checks and token references.

use nail_api::LocalizedMessage;
use nail_api::config::theme::css_var_name;

use crate::l10n::msg;

/// Token categories (see `nail_api::TokenSet`).
pub const TOKEN_CATEGORIES: &[&str] = &[
    "color",
    "font",
    "font-size",
    "font-weight",
    "line-height",
    "space",
    "size",
    "radius",
    "border",
    "shadow",
    "motion",
    "opacity",
    "z-index",
];

/// Token categories whose unitless numbers are pixel lengths.
pub const PX_TOKEN_CATEGORIES: &[&str] = &["font-size", "space", "size", "radius"];

/// Colour modes that may override tokens.
pub const MODES: &[&str] = &["light", "dark", "high-contrast", "print"];

/// The CSS properties a part rule may set.
pub const PROPERTIES: &[&str] = &[
    // Box and layout
    "display",
    "position",
    "top",
    "right",
    "bottom",
    "left",
    "inset",
    "z-index",
    "overflow",
    "overflow-x",
    "overflow-y",
    "box-sizing",
    "visibility",
    "width",
    "height",
    "min-width",
    "min-height",
    "max-width",
    "max-height",
    "aspect-ratio",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "margin-inline",
    "margin-block",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "padding-inline",
    "padding-block",
    // Flex and grid
    "flex",
    "flex-direction",
    "flex-wrap",
    "flex-grow",
    "flex-shrink",
    "flex-basis",
    "order",
    "align-items",
    "align-self",
    "align-content",
    "justify-content",
    "justify-items",
    "justify-self",
    "gap",
    "row-gap",
    "column-gap",
    "grid-template-columns",
    "grid-template-rows",
    "grid-column",
    "grid-row",
    // Typography
    "color",
    "font",
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "font-variant-numeric",
    "font-feature-settings",
    "line-height",
    "letter-spacing",
    "text-align",
    "text-decoration",
    "text-decoration-color",
    "text-transform",
    "text-overflow",
    "text-wrap",
    "white-space",
    "word-break",
    "overflow-wrap",
    "vertical-align",
    // Background, borders, effects
    "background",
    "background-color",
    "border",
    "border-top",
    "border-right",
    "border-bottom",
    "border-left",
    "border-color",
    "border-width",
    "border-style",
    "border-collapse",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "border-radius",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-bottom-left-radius",
    "border-bottom-right-radius",
    "outline",
    "outline-color",
    "outline-width",
    "outline-style",
    "outline-offset",
    "box-shadow",
    "opacity",
    "accent-color",
    "caret-color",
    "fill",
    "stroke",
    "stroke-width",
    // Motion and interaction
    "transition",
    "transition-property",
    "transition-duration",
    "transition-timing-function",
    "transition-delay",
    "cursor",
    "user-select",
    "pointer-events",
    "scrollbar-width",
    "scrollbar-color",
];

/// Properties whose unitless, non-zero numbers are pixel lengths.
const LENGTH_PROPERTIES: &[&str] = &[
    "top",
    "right",
    "bottom",
    "left",
    "inset",
    "width",
    "height",
    "min-width",
    "min-height",
    "max-width",
    "max-height",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "margin-inline",
    "margin-block",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "padding-inline",
    "padding-block",
    "flex-basis",
    "gap",
    "row-gap",
    "column-gap",
    "font-size",
    "letter-spacing",
    "border-width",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "border-radius",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-bottom-left-radius",
    "border-bottom-right-radius",
    "outline-width",
    "outline-offset",
    "stroke-width",
];

/// Whether unitless numbers for this property get `px`.
pub fn is_length_property(property: &str) -> bool {
    LENGTH_PROPERTIES.contains(&property)
}

/// Adds `px` to a unitless number, except for zero.
pub fn px(number: &str) -> String {
    if number.parse::<f64>().is_ok_and(|n| n == 0.0) {
        number.to_owned()
    } else {
        format!("{number}px")
    }
}

/// Rejects values that could load remote content or break out of a
/// declaration. Returns a readable reason.
pub fn check_value_safety(value: &str) -> Result<(), LocalizedMessage> {
    if let Some(c) = value
        .chars()
        .find(|c| matches!(c, ';' | '{' | '}' | '<' | '>' | '\\'))
    {
        return Err(msg("config-css-character").arg("character", c.to_string()));
    }
    check_no_remote(value)
}

/// Rejects constructs that load remote content (also used for `raw-css`).
pub fn check_no_remote(css: &str) -> Result<(), LocalizedMessage> {
    let lower = css.to_ascii_lowercase();
    for banned in [
        "url(",
        "image-set(",
        "image(",
        "@import",
        "expression(",
        "javascript:",
    ] {
        if lower.contains(banned) {
            return Err(msg("config-css-remote").arg("construct", banned.trim_end_matches('(')));
        }
    }
    Ok(())
}

/// A token reference found in a value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TokenRef {
    /// Token category, e.g. `"color"`.
    pub category: String,
    /// Token name, e.g. `"accent"`.
    pub name: String,
}

/// Rewrites every `$category.name` in `value` to `var(--nail-category-name)`.
/// `$$` stands for a literal `$`. `exists` decides whether a token is
/// defined. Returns the rewritten value and the references found.
pub fn resolve_token_refs(
    value: &str,
    exists: impl Fn(&str, &str) -> bool,
) -> Result<(String, Vec<TokenRef>), LocalizedMessage> {
    let mut out = String::with_capacity(value.len());
    let mut refs = Vec::new();
    let mut rest = value;
    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        if let Some(tail) = after.strip_prefix('$') {
            out.push('$');
            rest = tail;
            continue;
        }
        let ident_len = |s: &str| {
            s.find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
                .unwrap_or(s.len())
        };
        let cat_len = ident_len(after);
        let category = &after[..cat_len];
        let name = after[cat_len..]
            .strip_prefix('.')
            .map(|s| s[..ident_len(s)].trim_end_matches('-'))
            .unwrap_or_default();
        if category.is_empty() || name.is_empty() {
            return Err(msg("config-token-reference-invalid"));
        }
        if !TOKEN_CATEGORIES.contains(&category) {
            return Err(msg("config-token-category-unknown")
                .arg("category", category)
                .arg("token", format!("${category}.{name}"))
                .arg("known", TOKEN_CATEGORIES.join(", ")));
        }
        if !exists(category, name) {
            return Err(msg("config-unknown-token").arg("token", format!("${category}.{name}")));
        }
        out.push_str("var(");
        out.push_str(&css_var_name(category, name));
        out.push(')');
        refs.push(TokenRef {
            category: category.to_owned(),
            name: name.to_owned(),
        });
        rest = &after[cat_len + 1 + name.len()..];
    }
    out.push_str(rest);
    Ok((out, refs))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(_: &str, _: &str) -> bool {
        true
    }

    #[test]
    fn token_refs_become_css_vars() {
        let (out, refs) = resolve_token_refs("1px solid $color.border", all).unwrap();
        assert_eq!(out, "1px solid var(--nail-color-border)");
        assert_eq!(refs.len(), 1);
        let (out, _) = resolve_token_refs("$space.xs $space.sm", all).unwrap();
        assert_eq!(out, "var(--nail-space-xs) var(--nail-space-sm)");
        let (out, _) =
            resolve_token_refs("background-color $motion.fast $motion.ease", all).unwrap();
        assert_eq!(
            out,
            "background-color var(--nail-motion-fast) var(--nail-motion-ease)"
        );
        let (out, refs) = resolve_token_refs("costs $$5", all).unwrap();
        assert_eq!((out.as_str(), refs.len()), ("costs $5", 0));
        let (out, _) = resolve_token_refs("calc($space.sm*2)", all).unwrap();
        assert_eq!(out, "calc(var(--nail-space-sm)*2)");
    }

    #[test]
    fn bad_token_refs_are_reported() {
        let err = resolve_token_refs("$color.nope", |_, n| n != "nope").unwrap_err();
        let err = crate::l10n::english(&err);
        assert!(err.contains("unknown token `$color.nope`"), "{err}");
        let err = resolve_token_refs("$colour.bg", all).unwrap_err();
        let err = crate::l10n::english(&err);
        assert!(err.contains("unknown token category `colour`"), "{err}");
        let err = resolve_token_refs("5$", all).unwrap_err();
        let err = crate::l10n::english(&err);
        assert!(err.contains("$$"), "{err}");
        assert!(resolve_token_refs("$color", all).is_err());
    }

    #[test]
    fn remote_content_is_rejected() {
        assert!(check_value_safety("url(https://x/y.png)").is_err());
        assert!(check_value_safety("URL(x)").is_err());
        assert!(check_value_safety("red; background: blue").is_err());
        assert!(check_value_safety("a } b").is_err());
        assert!(check_no_remote("@import 'x.css'").is_err());
        assert!(check_value_safety("\"Segoe UI\", system-ui").is_ok());
        assert!(check_value_safety("rgb(0 0 0 / 0.12)").is_ok());
    }

    #[test]
    fn px_units() {
        assert_eq!(px("4"), "4px");
        assert_eq!(px("0"), "0");
        assert_eq!(px("1.5"), "1.5px");
        assert!(is_length_property("padding") && !is_length_property("font-weight"));
        for p in LENGTH_PROPERTIES {
            assert!(PROPERTIES.contains(p), "{p} missing from PROPERTIES");
        }
    }
}
