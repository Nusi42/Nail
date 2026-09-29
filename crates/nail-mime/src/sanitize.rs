//! HTML sanitizing for mail bodies.
//!
//! Incoming HTML is hostile. The sanitizer (built on `ammonia`) keeps the
//! formatting subset mail needs (text, tables, images, inline styles) and
//! removes everything that executes, submits, embeds or phones home:
//!
//! * scripts, `<style>` blocks, frames, objects, embeds, SVG/MathML, forms and
//!   form controls are removed together with their content;
//! * event handler attributes (`on*`), `class`/`id`, `srcset` and other
//!   attributes outside the allow list are dropped;
//! * links keep only `http`, `https`, `mailto` and `tel` targets and get
//!   `rel="noopener noreferrer"`; relative URLs are dropped;
//! * images keep only `cid:` (inline parts) and `data:image/{png,gif,jpeg,webp}`
//!   sources; remote sources (`http:`, `https:`, `//`) are removed and
//!   reported via [`SanitizedHtml::remote_content_blocked`] — this also kills
//!   tracking pixels;
//! * inline `style` attributes are filtered per declaration: declarations with
//!   remote `url(...)` are removed (and reported), declarations with
//!   `expression(`, `javascript:`, `behavior`, `-moz-binding`, `@import`,
//!   CSS escapes or comments are removed.
//!
//! The result must still only be rendered inside the sandboxed reader iframe
//! with a restrictive CSP; the sanitizer is one layer, not the only one.
//!
//! Every remote reference is counted ([`SanitizedHtml::remote_resources`]).
//! When the user allowed remote content, [`sanitize_html_with`] replaces
//! each one with the URL a [`ProxyRewriter`] returns (a signed
//! `nail-proxy:` URL, [`crate::remote_content`]) instead of removing it.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use ammonia::{Builder, UrlRelative};

use crate::remote_content::{ProxyRewriter, remote_target};

/// The result of [`sanitize_html`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizedHtml {
    /// The sanitized HTML fragment.
    pub html: String,
    /// `true` when remote content (images, backgrounds) was removed.
    pub remote_content_blocked: bool,
    /// Remote references found (`src` / `background` attributes and CSS
    /// `url(...)` in `style`), whether removed or rewritten.
    pub remote_resources: u32,
}

/// What the attribute filter records and uses about remote content.
struct Remote {
    blocked: AtomicBool,
    count: AtomicU32,
    rewriter: Option<ProxyRewriter>,
}

impl Remote {
    /// Counts one remote reference and returns its replacement, or marks
    /// the content blocked and returns `None`.
    fn replace(&self, value: &str) -> Option<String> {
        self.count.fetch_add(1, Ordering::Relaxed);
        let replaced = self
            .rewriter
            .as_ref()
            .and_then(|rewrite| remote_target(value).and_then(|url| rewrite(&url)));
        if replaced.is_none() {
            self.blocked.store(true, Ordering::Relaxed);
        }
        replaced
    }
}

/// Tags removed together with everything inside them.
const CLEAN_CONTENT_TAGS: &[&str] = &[
    "script", "style", "title", "head", "noscript", "template", "iframe", "frame", "frameset",
    "object", "embed", "applet", "svg", "math", "form", "button", "select", "textarea", "option",
    "canvas", "audio", "video",
];

/// Tags kept in addition to ammonia's defaults.
const EXTRA_TAGS: &[&str] = &["font", "big", "section", "main"];

/// Attributes allowed on every kept element.
const GENERIC_ATTRIBUTES: &[&str] = &["lang", "title", "dir", "style", "align"];

/// Per-tag attributes allowed in addition to ammonia's defaults.
const EXTRA_TAG_ATTRIBUTES: &[(&str, &[&str])] = &[
    (
        "img",
        &[
            "src", "alt", "width", "height", "border", "hspace", "vspace",
        ],
    ),
    (
        "table",
        &[
            "width",
            "height",
            "border",
            "cellpadding",
            "cellspacing",
            "bgcolor",
            "background",
        ],
    ),
    ("tr", &["valign", "bgcolor", "height"]),
    (
        "td",
        &[
            "valign",
            "bgcolor",
            "background",
            "width",
            "height",
            "nowrap",
        ],
    ),
    (
        "th",
        &[
            "valign",
            "bgcolor",
            "background",
            "width",
            "height",
            "nowrap",
        ],
    ),
    ("font", &["color", "face", "size"]),
    ("a", &["name"]),
    ("div", &["bgcolor"]),
];

/// Image MIME types allowed in `data:` URLs (no SVG: it can carry script).
const DATA_IMAGE_PREFIXES: &[&str] = &[
    "data:image/png",
    "data:image/gif",
    "data:image/jpeg",
    "data:image/jpg",
    "data:image/webp",
];

/// Sanitizes an HTML mail body, removing remote content. Never panics on
/// malformed input.
pub fn sanitize_html(html: &str) -> SanitizedHtml {
    sanitize_html_with(html, None)
}

/// Sanitizes an HTML mail body; with a `rewriter`, remote references are
/// replaced by what it returns (references it refuses are removed as
/// usual). Never panics on malformed input.
pub fn sanitize_html_with(html: &str, rewriter: Option<ProxyRewriter>) -> SanitizedHtml {
    let remote = Arc::new(Remote {
        blocked: AtomicBool::new(false),
        count: AtomicU32::new(0),
        rewriter,
    });
    let html = builder(Arc::clone(&remote))
        .clean(html)
        .to_string()
        .trim()
        .to_owned();
    SanitizedHtml {
        html,
        remote_content_blocked: remote.blocked.load(Ordering::Relaxed),
        remote_resources: remote.count.load(Ordering::Relaxed),
    }
}

fn builder(remote: Arc<Remote>) -> Builder<'static> {
    let mut builder = Builder::default();
    builder
        .add_tags(EXTRA_TAGS)
        .rm_tags(CLEAN_CONTENT_TAGS)
        .clean_content_tags(CLEAN_CONTENT_TAGS.iter().copied().collect())
        .generic_attributes(GENERIC_ATTRIBUTES.iter().copied().collect())
        .url_schemes(["http", "https", "mailto", "tel", "cid", "data"].into_iter().collect::<HashSet<_>>())
        // Relative URLs pass ammonia so `filter_attribute` sees (and reports)
        // protocol-relative `//host` sources; it drops every other relative URL.
        .url_relative(UrlRelative::PassThrough)
        .link_rel(Some("noopener noreferrer"))
        .strip_comments(true);
    for (tag, attributes) in EXTRA_TAG_ATTRIBUTES {
        builder.add_tag_attributes(tag, attributes.iter());
    }
    builder.attribute_filter(move |element, attribute, value| {
        filter_attribute(element, attribute, value, &remote)
    });
    builder
}

fn filter_attribute<'u>(
    element: &str,
    attribute: &str,
    value: &'u str,
    remote: &Remote,
) -> Option<Cow<'u, str>> {
    match (element, attribute) {
        (_, "src" | "background") => match classify_url(value) {
            UrlKind::Cid | UrlKind::DataImage => Some(Cow::Borrowed(value)),
            UrlKind::Remote => remote.replace(value).map(Cow::Owned),
            UrlKind::Link | UrlKind::Other => None,
        },
        (_, "href") => match classify_url(value) {
            UrlKind::Remote | UrlKind::Link => Some(Cow::Borrowed(value)),
            _ => None,
        },
        (_, "style") => filter_style(value, remote),
        _ => Some(Cow::Borrowed(value)),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum UrlKind {
    /// `cid:` reference to an inline MIME part.
    Cid,
    /// An allowed `data:image/...` URL.
    DataImage,
    /// `http:`, `https:` or protocol-relative `//`.
    Remote,
    /// `mailto:` or `tel:`.
    Link,
    /// Anything else (`javascript:`, `data:text/html`, relative paths, ...).
    Other,
}

fn classify_url(value: &str) -> UrlKind {
    // Browsers ignore embedded whitespace/control characters in schemes.
    let normalized: String = value
        .trim()
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && !c.is_control())
        .take(64)
        .collect::<String>()
        .to_ascii_lowercase();
    if normalized.starts_with("cid:") {
        UrlKind::Cid
    } else if DATA_IMAGE_PREFIXES
        .iter()
        .any(|p| normalized.starts_with(p))
    {
        UrlKind::DataImage
    } else if normalized.starts_with("http:")
        || normalized.starts_with("https:")
        || normalized.starts_with("//")
    {
        UrlKind::Remote
    } else if normalized.starts_with("mailto:") || normalized.starts_with("tel:") {
        UrlKind::Link
    } else {
        UrlKind::Other
    }
}

/// Filters an inline `style` attribute declaration by declaration.
fn filter_style<'u>(style: &'u str, remote: &Remote) -> Option<Cow<'u, str>> {
    const FORBIDDEN: &[&str] = &[
        "expression(",
        "javascript:",
        "vbscript:",
        "behavior",
        "-moz-binding",
        "@import",
        "\\",
        "/*",
    ];
    let mut kept: Vec<Cow<'u, str>> = Vec::new();
    for declaration in style.split(';') {
        let declaration = declaration.trim();
        if declaration.is_empty() {
            continue;
        }
        let lower = declaration.to_ascii_lowercase();
        if FORBIDDEN.iter().any(|f| lower.contains(f)) {
            continue;
        }
        let urls = css_urls(&lower);
        if urls.iter().any(|url| classify_url(url) == UrlKind::Remote) {
            if let Some(rewritten) = rewrite_css_urls(declaration, remote) {
                kept.push(Cow::Owned(rewritten));
            }
            continue;
        }
        if urls
            .iter()
            .any(|url| !matches!(classify_url(url), UrlKind::Cid | UrlKind::DataImage))
        {
            continue;
        }
        kept.push(Cow::Borrowed(declaration));
    }
    if kept.is_empty() {
        return None;
    }
    let joined = kept.join("; ");
    if joined == style {
        Some(Cow::Borrowed(style))
    } else {
        Some(Cow::Owned(joined))
    }
}

/// A declaration with remote `url(...)` references: every remote one
/// counted and replaced through the rewriter (`url("<replacement>")`),
/// `cid:` / `data:` images kept. `None` (drop the declaration) when any
/// reference cannot be kept or replaced, or the declaration uses
/// `image-set()` / `src()`, whose bare strings are URLs too.
fn rewrite_css_urls(declaration: &str, remote: &Remote) -> Option<String> {
    let lower = declaration.to_ascii_lowercase();
    let mut ok = !(lower.contains("image-set(") || lower.contains("src("));
    let mut out = String::with_capacity(declaration.len());
    let mut pos = 0;
    while let Some(found) = lower[pos..].find("url(") {
        let start = pos + found;
        out.push_str(&declaration[pos..start]);
        let arg_start = start + 4;
        let Some(close) = declaration[arg_start..].find(')') else {
            // `url(` without a closing parenthesis: never kept.
            remote.replace("");
            return None;
        };
        let arg_end = arg_start + close;
        let url = declaration[arg_start..arg_end]
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .trim();
        match classify_url(url) {
            UrlKind::Cid | UrlKind::DataImage => out.push_str(&declaration[start..=arg_end]),
            UrlKind::Remote => match remote.replace(url) {
                Some(replacement) if !replacement.contains(['"', '\\', '(', ')']) => {
                    out.push_str("url(\"");
                    out.push_str(&replacement);
                    out.push_str("\")");
                }
                _ => ok = false,
            },
            UrlKind::Link | UrlKind::Other => ok = false,
        }
        pos = arg_end + 1;
    }
    out.push_str(&declaration[pos..]);
    ok.then_some(out)
}

/// Extracts the arguments of `url(...)` and `image-set(...)`-style
/// references from a (lower-cased) CSS declaration. A `url(` without a
/// closing parenthesis yields its remainder, so it is still checked.
fn css_urls(declaration: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut rest = declaration;
    while let Some(start) = rest.find("url(") {
        let after = &rest[start + 4..];
        let end = after.find(')').unwrap_or(after.len());
        let url = after[..end]
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .trim();
        urls.push(url.to_owned());
        rest = &after[end..];
    }
    // `image-set("a.png" 1x)` and `src()` accept bare strings as URLs.
    for function in ["image-set(", "src("] {
        if declaration.contains(function) {
            urls.push("unknown:".to_owned());
        }
    }
    urls
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(html: &str) -> SanitizedHtml {
        sanitize_html(html)
    }

    #[test]
    fn removes_scripts_and_handlers() {
        let out = clean(
            r#"<p onclick="x()">Hi<script>alert(1)</script></p><img src="x" onerror="alert(1)">"#,
        );
        assert_eq!(out.html, "<p>Hi</p><img>");
        assert!(!out.remote_content_blocked);
    }

    #[test]
    fn blocks_remote_images_and_tracking_pixels() {
        let out = clean(r#"<img src="https://t.example/p.gif" width="1" height="1"><p>x</p>"#);
        assert!(out.remote_content_blocked);
        assert!(!out.html.contains("t.example"));
        assert!(out.html.contains(r#"width="1""#));
        let out = clean(r#"<img src="//t.example/p.gif">"#);
        assert!(out.remote_content_blocked);
        let out =
            clean(r#"<table background="http://x.example/bg.jpg"><tr><td>a</td></tr></table>"#);
        assert!(out.remote_content_blocked);
        assert!(!out.html.contains("x.example"));
    }

    #[test]
    fn keeps_cid_and_data_images() {
        let out =
            clean(r#"<img src="cid:logo@x" alt="Logo"><img src="data:image/png;base64,AAAA">"#);
        assert_eq!(
            out.html,
            r#"<img src="cid:logo@x" alt="Logo"><img src="data:image/png;base64,AAAA">"#
        );
        assert!(!out.remote_content_blocked);
        let out = clean(r#"<img src="data:image/svg+xml;base64,AAAA">"#);
        assert_eq!(out.html, "<img>");
    }

    #[test]
    fn filters_link_schemes() {
        let out = clean(
            r#"<a href="javascript:alert(1)">a</a><a href="data:text/html,x">b</a><a href="https://ok.example/">c</a><a href="mailto:a@b.example">d</a><a href="rel/path">e</a><a href=" JaVa&#x09;script:alert(1)">f</a>"#,
        );
        assert_eq!(
            out.html,
            r#"<a rel="noopener noreferrer">a</a><a rel="noopener noreferrer">b</a><a href="https://ok.example/" rel="noopener noreferrer">c</a><a href="mailto:a@b.example" rel="noopener noreferrer">d</a><a rel="noopener noreferrer">e</a><a rel="noopener noreferrer">f</a>"#
        );
        assert!(!out.remote_content_blocked, "links are not remote content");
    }

    #[test]
    fn removes_embedding_and_forms_with_content() {
        let out = clean(
            r#"<iframe src="https://e.example"></iframe><object data="x">o</object><form action="https://e.example"><input name="pw"><button>Go</button></form><svg onload="x"><text>s</text></svg><style>p{}</style><p>kept</p>"#,
        );
        assert_eq!(out.html, "<p>kept</p>");
    }

    #[test]
    fn filters_inline_styles() {
        let out = clean(
            r#"<p style="color: red; background-image: url('https://x.example/a.png'); width: expression(alert(1))">t</p>"#,
        );
        assert_eq!(out.html, r#"<p style="color: red">t</p>"#);
        assert!(out.remote_content_blocked);
        let out = clean(r#"<p style="background:url(javascript:alert(1))">t</p>"#);
        assert_eq!(out.html, "<p>t</p>");
        let out = clean(r#"<p style="background: u\72l(https://x.example)">t</p>"#);
        assert_eq!(out.html, "<p>t</p>");
        let out = clean(r#"<p style="background: url(cid:bg@x); color: blue">t</p>"#);
        assert_eq!(
            out.html,
            r#"<p style="background: url(cid:bg@x); color: blue">t</p>"#
        );
        let out = clean(r#"<p style="background: url(https://x.example/a.png">t</p>"#);
        assert!(out.remote_content_blocked);
        assert!(!out.html.contains("x.example"));
    }

    #[test]
    fn keeps_mail_formatting() {
        let html = r##"<table width="600" cellpadding="0" bgcolor="#ffffff"><tr><td align="center" valign="top"><font color="red" face="Arial">Hi</font></td></tr></table>"##;
        let out = clean(html);
        assert_eq!(
            out.html,
            r##"<table width="600" cellpadding="0" bgcolor="#ffffff"><tbody><tr><td align="center" valign="top"><font color="red" face="Arial">Hi</font></td></tr></tbody></table>"##
        );
    }

    #[test]
    fn survives_garbage() {
        for input in [
            "",
            "<",
            "<<<>>>",
            "<p",
            "</div></div>",
            "<a href=",
            "\u{0}\u{1}<b>x",
            "<img src=\"cid:",
        ] {
            let _ = clean(input);
        }
    }
}
