//! Style sheets of HTML mail (P1 WP190).
//!
//! Almost every designed mail (newsletters, Outlook and Gmail HTML) styles
//! itself with `<style>` blocks and classes. The reader shows a message as a
//! document of its own (a sandboxed frame without scripts), so the mail's
//! rules cannot reach the app; they still must not load anything or break
//! out of their `<style>` element. [`sanitize_stylesheet`] keeps what is
//! safe, rule by rule:
//!
//! * comments and the `<!--` / `-->` markers are removed first, so nothing
//!   can be hidden in them (`ur/**/l(` is read as `url(`);
//! * qualified rules keep their selector when it has no `\`, `<`, braces or
//!   `url(`; their declarations pass the same filter as inline `style`
//!   attributes (`crate::sanitize::filter_declarations`: remote `url()`
//!   counted and removed or proxied, `cid:` resolved, no `expression`,
//!   `behavior`, `-moz-binding`, escapes, `position: fixed`);
//! * `@media` and `@supports` blocks are kept (three levels at most) with
//!   their inner rules filtered the same way; every other at-rule is dropped:
//!   `@import`, `@font-face` (fonts would load from anywhere), `@keyframes`,
//!   `@charset`, `@namespace`, `@page`, `@layer`, …;
//! * rules nested inside a declaration block (CSS nesting) are dropped;
//! * the result never contains `<`, so it cannot end its `<style>` element.

use crate::sanitize::{Context, filter_declarations};

/// The most style sheet text read from one message (all blocks together).
pub(crate) const MAX_STYLESHEET_BYTES: usize = 256 * 1024;

/// How deep `@media` / `@supports` blocks may nest.
const MAX_DEPTH: usize = 3;

/// At-rules whose block holds rules that are kept (filtered).
const NESTED_AT_RULES: &[&str] = &["media", "supports"];

/// Words that never appear in a kept selector or at-rule prelude.
const FORBIDDEN_PRELUDE: &[&str] = &[
    "url(",
    "expression",
    "javascript:",
    "vbscript:",
    "image-set(",
];

/// The safe part of a mail's style sheet `css` (see the module docs).
pub(crate) fn sanitize_stylesheet(css: &str, ctx: &Context) -> String {
    let css = strip_comments(truncate(css, MAX_STYLESHEET_BYTES));
    let mut out = String::new();
    rules(&css, ctx, 0, &mut out);
    // Belt and braces: nothing can close the `<style>` element.
    out.retain(|c| c != '<');
    out
}

/// `text` cut to at most `max` bytes at a character boundary.
pub(crate) fn truncate(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let end = text.floor_char_boundary(max);
    &text[..end]
}

/// Removes `/* … */` comments (an unterminated one ends the sheet) and the
/// HTML comment markers mail wraps its styles in.
fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => {
                // Removed without a trace: what is left is what gets
                // checked (`ur/**/l(` is checked as `url(`).
                rest = &rest[start + 2 + end + 2..];
            }
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.replace("<!--", " ").replace("-->", " ")
}

/// Appends the kept rules of `css` to `out`.
fn rules(css: &str, ctx: &Context, depth: usize, out: &mut String) {
    let bytes = css.as_bytes();
    let mut pos = 0;
    while pos < css.len() {
        let Some((stop, found)) = scan(bytes, pos) else {
            break;
        };
        if found != b'{' {
            // A statement at-rule (`@import …;`), a stray `}` or garbage.
            pos = stop + 1;
            continue;
        }
        let end = block_end(bytes, stop + 1);
        let prelude = css[pos..stop].trim();
        let block = &css[stop + 1..end];
        pos = end + 1;
        if let Some(at) = prelude.strip_prefix('@') {
            let name = at
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect::<String>()
                .to_ascii_lowercase();
            if depth < MAX_DEPTH
                && NESTED_AT_RULES.contains(&name.as_str())
                && safe_prelude(prelude)
            {
                let mut inner = String::new();
                rules(block, ctx, depth + 1, &mut inner);
                if !inner.is_empty() {
                    out.push_str(prelude);
                    out.push_str(" {\n");
                    out.push_str(&inner);
                    out.push_str("}\n");
                }
            }
            continue;
        }
        if !safe_prelude(prelude) || block.contains('{') {
            continue;
        }
        let declarations = filter_declarations(block, ctx);
        if declarations.is_empty() {
            continue;
        }
        out.push_str(prelude);
        out.push_str(" { ");
        out.push_str(&declarations.join("; "));
        out.push_str(" }\n");
    }
}

/// Whether a selector list or at-rule prelude may be kept.
fn safe_prelude(prelude: &str) -> bool {
    if prelude.is_empty() || prelude.len() > 4096 || prelude.contains(['\\', '<', '{', '}', ';']) {
        return false;
    }
    let lower = prelude.to_ascii_lowercase();
    !FORBIDDEN_PRELUDE.iter().any(|word| lower.contains(word))
}

/// The bytes of CSS text outside quoted strings from `start`, as
/// `(index, byte)`: quotes and string contents are skipped, as is the byte
/// after a `\` (inside strings always, outside them with `escapes`).
pub(crate) fn outside_strings(
    bytes: &[u8],
    start: usize,
    escapes: bool,
) -> impl Iterator<Item = (usize, u8)> + '_ {
    let mut quote: Option<u8> = None;
    let mut i = start;
    std::iter::from_fn(move || {
        while i < bytes.len() {
            let (at, b) = (i, bytes[i]);
            i += 1;
            match quote {
                Some(q) => {
                    if b == b'\\' {
                        i += 1;
                    } else if b == q {
                        quote = None;
                    }
                }
                None => match b {
                    b'"' | b'\'' => quote = Some(b),
                    b'\\' if escapes => i += 1,
                    _ => return Some((at, b)),
                },
            }
        }
        None
    })
}

/// The first `{`, `;` or `}` at or after `pos` outside strings and
/// parentheses, with the byte found.
fn scan(bytes: &[u8], pos: usize) -> Option<(usize, u8)> {
    let mut parens = 0usize;
    for (i, b) in outside_strings(bytes, pos, true) {
        match b {
            b'(' => parens += 1,
            b')' => parens = parens.saturating_sub(1),
            b'{' | b';' | b'}' if parens == 0 => return Some((i, b)),
            _ => {}
        }
    }
    None
}

/// The index of the `}` closing the block whose content starts at `start`
/// (the end of the text for an unterminated block, as CSS reads it).
fn block_end(bytes: &[u8], start: usize) -> usize {
    let mut depth = 1usize;
    for (i, b) in outside_strings(bytes, start, true) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sanitize::{Context, InlineImages};
    use std::sync::Arc;

    #[test]
    fn scanning_skips_strings_and_escapes() {
        let text = br#"a"x;\"y;"\;b;c"#;
        let seen: Vec<u8> = outside_strings(text, 0, true).map(|(_, b)| b).collect();
        assert_eq!(seen, b"ab;c");
        let raw: Vec<u8> = outside_strings(text, 0, false).map(|(_, b)| b).collect();
        assert_eq!(raw, br"a\;b;c");
        assert_eq!(scan(b"x(;);{", 0), Some((4, b';')));
        assert_eq!(block_end(b"a{b}c}d", 0), 5);
        assert_eq!(block_end(b"a{b", 0), 3);
    }

    fn sheet(css: &str) -> (String, Arc<Context>) {
        let ctx = Arc::new(Context::document(None, InlineImages::default()));
        (sanitize_stylesheet(css, &ctx), ctx)
    }

    #[test]
    fn keeps_ordinary_rules() {
        let (out, ctx) = sheet(
            ".hero { background-color: #0b7285; color: #ffffff; padding: 24px }\n\
             body { margin: 0; background: #f4f4f4 }\ntable.card td.x > a:hover { color: red; }",
        );
        assert_eq!(
            out,
            ".hero { background-color: #0b7285; color: #ffffff; padding: 24px }\n\
             body { margin: 0; background: #f4f4f4 }\n\
             table.card td.x > a:hover { color: red }\n"
        );
        assert_eq!(ctx.remote_count(), 0);
    }

    #[test]
    fn keeps_media_and_supports_blocks() {
        let (out, _) = sheet(
            "@media screen and (max-width: 600px) { .col { width: 100% !important } @supports (display: grid) { .g { display: grid } } }",
        );
        assert_eq!(
            out,
            "@media screen and (max-width: 600px) {\n.col { width: 100% !important }\n\
             @supports (display: grid) {\n.g { display: grid }\n}\n}\n"
        );
    }

    #[test]
    fn drops_loading_and_unknown_at_rules() {
        let (out, _) = sheet(
            "@import url(https://x.example/a.css); @import 'b.css';\n\
             @font-face { font-family: X; src: url(https://x.example/f.woff2) }\n\
             @keyframes spin { from { transform: none } to { transform: rotate(1turn) } }\n\
             @charset \"utf-8\"; @namespace svg url(http://www.w3.org/2000/svg);\n\
             p { color: blue }",
        );
        assert_eq!(out, "p { color: blue }\n");
    }

    #[test]
    fn remote_urls_are_counted_and_removed() {
        let (out, ctx) = sheet(
            ".a { background: url(https://t.example/bg.png); color: red } .b { background-image: url('//t.example/x.gif') }",
        );
        assert_eq!(out, ".a { color: red }\n");
        assert_eq!(ctx.remote_count(), 2);
        assert!(ctx.blocked());
    }

    #[test]
    fn remote_urls_are_proxied_when_allowed() {
        let rewriter: crate::ProxyRewriter =
            Arc::new(|url: &str| Some(format!("nail-proxy:{}", url.len())));
        let ctx = Arc::new(Context::document(Some(rewriter), InlineImages::default()));
        let out = sanitize_stylesheet(".a { background: url(https://t.example/bg.png) }", &ctx);
        assert_eq!(out, ".a { background: url(\"nail-proxy:24\") }\n");
        assert!(!ctx.blocked());
    }

    #[test]
    fn cid_urls_resolve_to_inline_images() {
        let mut images = InlineImages::default();
        images.insert("bg@x", "data:image/png;base64,AAAA".to_owned());
        let ctx = Arc::new(Context::document(None, images));
        let out = sanitize_stylesheet(
            ".a { background: url(cid:bg@x) } .b { background: url(cid:missing@x); color: red }",
            &ctx,
        );
        assert_eq!(
            out,
            ".a { background: url(\"data:image/png;base64,AAAA\") }\n.b { color: red }\n"
        );
    }

    #[test]
    fn dangerous_declarations_and_selectors_are_dropped() {
        let (out, _) = sheet(
            "p { width: expression(alert(1)); color: red }\n\
             .x { behavior: url(x.htc) } .y { -moz-binding: url(x.xml#a) }\n\
             .z { position: fixed; top: 0 } .w { position : FIXED }\n\
             a[href^=\"https\"]::after { content: \"https://bank.example\" }\n\
             .e\\31 { color: red } .f { color: r\\65 d }\n\
             div:has(> url(x)) { color: red }\n\
             .v { background: url(javascript:alert(1)) }",
        );
        assert_eq!(out, "p { color: red }\n.z { top: 0 }\n");
    }

    #[test]
    fn comments_cannot_hide_anything() {
        let (out, ctx) = sheet(
            "<!-- .a { color: red } /* .hidden { x: y } */ .b { background: ur/**/l(https://t.example/a.png) } -->",
        );
        assert_eq!(out, ".a { color: red }\n");
        assert_eq!(ctx.remote_count(), 1);
        let (out, _) = sheet(".a { color: red } /* unterminated .b { color: blue }");
        assert_eq!(out, ".a { color: red }\n");
    }

    #[test]
    fn nothing_closes_the_style_element() {
        let (out, _) = sheet(
            "p { color: red } </style><script>alert(1)</script> .x { font-family: \"</style>\" } .y[title=\"<b>\"] { color: blue }",
        );
        assert!(!out.contains('<'), "{out}");
        assert!(out.contains("p { color: red }"));
    }

    #[test]
    fn nested_rules_and_garbage_are_dropped() {
        let (out, _) = sheet(".a { color: red; .b { color: blue } } } ; .c { color: green }");
        assert_eq!(out, ".c { color: green }\n");
        for garbage in [
            "",
            "{",
            "}",
            "@",
            "@media",
            "@media {",
            "a {",
            "a { b",
            "\"",
            "'{'",
            "(((",
            "@media x { @media y { @media z { @media w { p { color: red } } } } }",
        ] {
            drop(sheet(garbage));
        }
        let (out, _) =
            sheet("@media a { @media b { @media c { @media d { p { color: red } } } } }");
        assert_eq!(out, "");
    }

    #[test]
    fn strings_with_braces_and_semicolons_stay_in_their_rule() {
        let (out, _) = sheet(".q { font-family: \"A;B\", 'C D'; color: red } .r { color: blue }");
        assert_eq!(
            out,
            ".q { font-family: \"A;B\", 'C D'; color: red }\n.r { color: blue }\n"
        );
        // A brace, even inside a string, drops the declaration (P1 review:
        // the same filter guards the `<body style>` rule); the rules stay.
        let (out, _) = sheet(".q { font-family: 'C}D'; color: red } .r { color: blue }");
        assert_eq!(out, ".q { color: red }\n.r { color: blue }\n");
    }

    #[test]
    fn long_sheets_are_cut_at_a_character_boundary() {
        assert_eq!(truncate("ab€", 3), "ab");
        assert_eq!(truncate("ab€", 5), "ab€");
        let big = format!("p {{ color: red }}\n{}", "é".repeat(MAX_STYLESHEET_BYTES));
        let (out, _) = sheet(&big);
        assert!(out.starts_with("p { color: red }"));
    }
}
