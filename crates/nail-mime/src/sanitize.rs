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
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

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
    /// Normalized Content-IDs (see [`InlineImages`]) of the inline images
    /// the sanitized document refers to (ADR 0162): references that were
    /// kept or embedded, never removed ones. Empty for fragments.
    pub inline_shown: BTreeSet<String>,
}

/// The `data:` URLs of a message's inline image parts by `Content-ID`
/// (without `cid:` and angle brackets, ignoring case), for
/// [`sanitize_mail_html`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InlineImages {
    by_cid: HashMap<String, String>,
}

impl InlineImages {
    /// Adds the `data:` URL of the part with Content-ID `cid`.
    pub fn insert(&mut self, cid: &str, data_url: String) {
        self.by_cid.insert(normalize_cid(cid), data_url);
    }

    /// The `data:` URL of `cid` (with or without `cid:` / angle brackets;
    /// a `cid:` URL's percent escapes are decoded, RFC 2392).
    pub fn get(&self, cid: &str) -> Option<&str> {
        self.by_cid
            .get(&normalize_cid(&percent_decode(cid)))
            .map(String::as_str)
    }

    /// Whether no image is known.
    pub fn is_empty(&self) -> bool {
        self.by_cid.is_empty()
    }
}

/// A Content-ID without `cid:` (any case), angle brackets and surrounding
/// blanks, in lower case: mail clients match Content-IDs ignoring case.
pub(crate) fn normalize_cid(cid: &str) -> String {
    let cid = cid.trim();
    let cid = match cid.get(..4) {
        Some(scheme) if scheme.eq_ignore_ascii_case("cid:") => cid.get(4..).unwrap_or_default(),
        _ => cid,
    };
    cid.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_lowercase()
}

/// `value` with its `%XX` escapes decoded (a `cid:` URL is the percent-encoded
/// Content-ID, RFC 2392). Invalid escapes and non-UTF-8 results stay as
/// written.
fn percent_decode(value: &str) -> Cow<'_, str> {
    if !value.contains('%') {
        return Cow::Borrowed(value);
    }
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        let escaped = (byte == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let Some(decoded) = escaped {
            out.push(decoded);
            i += 3;
        } else {
            out.push(byte);
            i += 1;
        }
    }
    String::from_utf8(out).map_or(Cow::Borrowed(value), Cow::Owned)
}

/// What the filters record and use while one body is sanitized.
pub(crate) struct Context {
    blocked: AtomicBool,
    count: AtomicU32,
    rewriter: Option<ProxyRewriter>,
    /// A whole mail document (P1): classes, ids and style sheets are kept,
    /// `cid:` references to parts in `inline` are kept (or, with `embed`,
    /// resolved to their `data:` URLs); others are removed.
    document: bool,
    inline: InlineImages,
    embed: bool,
    /// The normalized Content-IDs of the inline images kept so far.
    shown: Mutex<BTreeSet<String>>,
}

impl Context {
    fn fragment(rewriter: Option<ProxyRewriter>) -> Self {
        Self {
            blocked: AtomicBool::new(false),
            count: AtomicU32::new(0),
            rewriter,
            document: false,
            inline: InlineImages::default(),
            embed: true,
            shown: Mutex::new(BTreeSet::new()),
        }
    }

    /// The context of [`sanitize_mail_html`] (inline images embedded).
    #[cfg(test)]
    pub(crate) fn document(rewriter: Option<ProxyRewriter>, inline: InlineImages) -> Self {
        Self::document_with(rewriter, inline, true)
    }

    /// The context of [`sanitize_mail_html`] / [`sanitize_mail_html_portable`].
    fn document_with(rewriter: Option<ProxyRewriter>, inline: InlineImages, embed: bool) -> Self {
        Self {
            document: true,
            inline,
            embed,
            ..Self::fragment(rewriter)
        }
    }

    /// Remote references seen so far.
    pub(crate) fn remote_count(&self) -> u32 {
        self.count.load(Ordering::Relaxed)
    }

    /// Whether a remote reference was removed.
    pub(crate) fn blocked(&self) -> bool {
        self.blocked.load(Ordering::Relaxed)
    }

    /// The normalized Content-IDs of the inline images kept so far.
    pub(crate) fn shown(&self) -> BTreeSet<String> {
        self.shown
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

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

    /// What a `cid:` reference becomes: kept as written in a fragment; in a
    /// document the inline image's `data:` URL (`embed`) or the reference
    /// itself (`None` = remove it: the part is missing or too large, and the
    /// reader shows a placeholder).
    fn cid<'u>(&self, value: &'u str) -> Option<Cow<'u, str>> {
        if !self.document {
            return Some(Cow::Borrowed(value));
        }
        let url = self.inline.get(value)?;
        self.shown
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(normalize_cid(&percent_decode(value)));
        Some(if self.embed {
            Cow::Owned(url.to_owned())
        } else {
            Cow::Borrowed(value)
        })
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

/// Attributes allowed on every kept element of a whole mail document, in
/// addition to [`GENERIC_ATTRIBUTES`]: its style sheets select by them.
const DOCUMENT_ATTRIBUTES: &[&str] = &["class", "id"];

/// Sanitizes an HTML fragment (plug-in HTML, Markdown blocks), removing
/// remote content. Style sheets, classes and ids are removed: the result may
/// be shown next to other content. Never panics on malformed input.
pub fn sanitize_html(html: &str) -> SanitizedHtml {
    sanitize_html_with(html, None)
}

/// [`sanitize_html`]; with a `rewriter`, remote references are replaced by
/// what it returns (references it refuses are removed as usual).
pub fn sanitize_html_with(html: &str, rewriter: Option<ProxyRewriter>) -> SanitizedHtml {
    let ctx = Arc::new(Context::fragment(rewriter));
    let html = clean(html, &ctx);
    SanitizedHtml {
        html,
        remote_content_blocked: ctx.blocked(),
        remote_resources: ctx.remote_count(),
        inline_shown: ctx.shown(),
    }
}

/// Sanitizes the HTML body of a mail, which the reader shows as a document
/// of its own in a sandboxed frame (P1 WP190): in addition to
/// [`sanitize_html_with`]'s fragment it keeps `class` / `id` attributes and
/// the mail's `<style>` blocks (filtered by `crate::css`, placed first in a
/// `<style>` element), carries the `<body>` element's `bgcolor`, `text`,
/// `link`, `background` and `style` over as `body` rules, and resolves
/// `cid:` references to the `data:` URLs of `inline` (references to
/// missing parts are removed, so the reader shows a placeholder). Remote
/// references in style sheets count like all others. Never panics.
pub fn sanitize_mail_html(
    html: &str,
    rewriter: Option<ProxyRewriter>,
    inline: InlineImages,
) -> SanitizedHtml {
    sanitize_document(html, Context::document_with(rewriter, inline, true))
}

/// [`sanitize_mail_html`] for everything but the reader (quoting, MCP,
/// plug-ins; P1 review): references to the parts in `inline` stay `cid:`
/// links instead of `data:` URLs of up to megabytes; references to missing
/// parts are removed as there.
pub fn sanitize_mail_html_portable(
    html: &str,
    rewriter: Option<ProxyRewriter>,
    inline: InlineImages,
) -> SanitizedHtml {
    sanitize_document(html, Context::document_with(rewriter, inline, false))
}

fn sanitize_document(html: &str, ctx: Context) -> SanitizedHtml {
    let ctx = Arc::new(ctx);
    let extracted = crate::document::extract(html);
    let mut css = String::new();
    let (body_rules, body_style) = body_rules(&extracted.body, &ctx);
    css.push_str(&body_rules);
    for sheet in &extracted.styles {
        css.push_str(&crate::css::sanitize_stylesheet(sheet, &ctx));
    }
    css.push_str(&body_style);
    let body = clean(html, &ctx);
    let html = if css.is_empty() {
        body
    } else {
        format!("<style>\n{css}</style>{body}")
    };
    SanitizedHtml {
        html,
        remote_content_blocked: ctx.blocked(),
        remote_resources: ctx.remote_count(),
        inline_shown: ctx.shown(),
    }
}

fn clean(html: &str, ctx: &Arc<Context>) -> String {
    builder(Arc::clone(ctx))
        .clean(html)
        .to_string()
        .trim()
        .to_owned()
}

/// A legacy colour attribute value (`#fff`, `white`) or `None`.
fn legacy_color(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()
        && value.len() <= 32
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '#'))
    .then_some(value)
}

/// The `<body>` element's presentational attributes as rules: those before
/// the mail's style sheets (attributes lose against author CSS), and its
/// `style` attribute after them (it wins).
fn body_rules(attributes: &[(String, String)], ctx: &Context) -> (String, String) {
    let mut declarations: Vec<String> = Vec::new();
    let mut link = None;
    let mut style = String::new();
    for (name, value) in attributes {
        match name.as_str() {
            "bgcolor" => {
                if let Some(color) = legacy_color(value) {
                    declarations.push(format!("background-color: {color}"));
                }
            }
            "text" => {
                if let Some(color) = legacy_color(value) {
                    declarations.push(format!("color: {color}"));
                }
            }
            "link" => link = legacy_color(value),
            "background" => {
                let url = match classify_url(value) {
                    UrlKind::Cid => ctx.cid(value).map(Cow::into_owned),
                    UrlKind::DataImage => Some(value.trim().to_owned()),
                    UrlKind::Remote => ctx.replace(value),
                    UrlKind::Link | UrlKind::Other => None,
                };
                if let Some(url) = url.filter(|u| !u.contains(['"', '\\', '(', ')', '<'])) {
                    declarations.push(format!("background-image: url(\"{url}\")"));
                }
            }
            "style" => {
                let kept = filter_declarations(value, ctx);
                if !kept.is_empty() {
                    style = format!("body {{ {} }}\n", kept.join("; "));
                }
            }
            _ => {}
        }
    }
    let mut before = String::new();
    if !declarations.is_empty() {
        before = format!("body {{ {} }}\n", declarations.join("; "));
    }
    if let Some(color) = link {
        before.push_str("a:link { color: ");
        before.push_str(color);
        before.push_str(" }\n");
    }
    before.retain(|c| c != '<');
    style.retain(|c| c != '<');
    (before, style)
}

fn builder(ctx: Arc<Context>) -> Builder<'static> {
    let mut builder = Builder::default();
    let mut generic: HashSet<&'static str> = GENERIC_ATTRIBUTES.iter().copied().collect();
    if ctx.document {
        generic.extend(DOCUMENT_ATTRIBUTES.iter().copied());
    }
    builder
        .add_tags(EXTRA_TAGS)
        .rm_tags(CLEAN_CONTENT_TAGS)
        .clean_content_tags(CLEAN_CONTENT_TAGS.iter().copied().collect())
        .generic_attributes(generic)
        .url_schemes(
            ["http", "https", "mailto", "tel", "cid", "data"]
                .into_iter()
                .collect::<HashSet<_>>(),
        )
        // Relative URLs pass ammonia so `filter_attribute` sees (and reports)
        // protocol-relative `//host` sources; it drops every other relative URL.
        .url_relative(UrlRelative::PassThrough)
        .link_rel(Some("noopener noreferrer"))
        .strip_comments(true);
    for (tag, attributes) in EXTRA_TAG_ATTRIBUTES {
        builder.add_tag_attributes(tag, attributes.iter());
    }
    builder.attribute_filter(move |element, attribute, value| {
        filter_attribute(element, attribute, value, &ctx)
    });
    builder
}

fn filter_attribute<'u>(
    element: &str,
    attribute: &str,
    value: &'u str,
    ctx: &Context,
) -> Option<Cow<'u, str>> {
    match (element, attribute) {
        (_, "src" | "background") => match classify_url(value) {
            UrlKind::Cid => ctx.cid(value),
            UrlKind::DataImage => Some(Cow::Borrowed(value)),
            UrlKind::Remote => ctx.replace(value).map(Cow::Owned),
            UrlKind::Link | UrlKind::Other => None,
        },
        (_, "href") => match classify_url(value) {
            UrlKind::Remote | UrlKind::Link => Some(Cow::Borrowed(value)),
            _ => None,
        },
        (_, "style") => filter_style(value, ctx),
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
fn filter_style<'u>(style: &'u str, ctx: &Context) -> Option<Cow<'u, str>> {
    let kept = filter_declarations(style, ctx);
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

/// Text that never appears in a kept declaration.
const FORBIDDEN_IN_DECLARATIONS: &[&str] = &[
    "expression(",
    "javascript:",
    "vbscript:",
    "behavior",
    "-moz-binding",
    "@import",
    "\\",
    "/*",
    "<",
    // A `<body style>` becomes a rule in the mail's style sheet: a brace
    // would end that rule and start new ones past the checks below.
    "{",
    "}",
];

/// The declarations of a `style` attribute or rule block that may be kept
/// (see the module docs), with remote `url()` references counted and
/// replaced through the rewriter and `cid:` ones resolved in documents.
/// Declarations are split at `;` outside strings and parentheses, so
/// `data:` URLs stay whole.
pub(crate) fn filter_declarations(block: &str, ctx: &Context) -> Vec<String> {
    let mut kept = Vec::new();
    for declaration in split_declarations(block) {
        let declaration = declaration.trim();
        if declaration.is_empty() || !declaration.contains(':') {
            continue;
        }
        let lower = declaration.to_ascii_lowercase();
        if FORBIDDEN_IN_DECLARATIONS.iter().any(|f| lower.contains(f)) {
            continue;
        }
        // An unclosed string would swallow the end of the rule it lands in
        // (escapes are refused above, so counting quotes is enough).
        if lower.matches('"').count() % 2 != 0 || lower.matches('\'').count() % 2 != 0 {
            continue;
        }
        let compact: String = lower.chars().filter(|c| !c.is_whitespace()).collect();
        // Fixed boxes could cover the reader's own controls when framed;
        // generated text could fake a link's address (link warnings).
        if compact.starts_with("position:fixed")
            || (compact.starts_with("content:")
                && !matches!(
                    compact.as_str(),
                    "content:\"\"" | "content:''" | "content:none" | "content:normal"
                ))
        {
            continue;
        }
        let urls = css_urls(&lower);
        let needs_rewrite = urls.iter().any(|url| {
            let kind = classify_url(url);
            kind == UrlKind::Remote || (kind == UrlKind::Cid && ctx.document)
        });
        if needs_rewrite {
            if let Some(rewritten) = rewrite_css_urls(declaration, ctx) {
                kept.push(rewritten);
            }
            continue;
        }
        if urls
            .iter()
            .any(|url| !matches!(classify_url(url), UrlKind::Cid | UrlKind::DataImage))
        {
            continue;
        }
        kept.push(declaration.to_owned());
    }
    kept
}

/// `block` split at `;` outside strings and parentheses.
fn split_declarations(block: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut parens = 0usize;
    let mut start = 0;
    for (i, b) in crate::css::outside_strings(block.as_bytes(), 0, false) {
        match b {
            b'(' => parens += 1,
            b')' => parens = parens.saturating_sub(1),
            b';' if parens == 0 => {
                parts.push(&block[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if let Some(rest) = block.get(start..) {
        parts.push(rest);
    }
    parts
}

/// A declaration with remote (or, in documents, `cid:`) `url(...)`
/// references: every remote one counted and replaced through the rewriter
/// (`url("<replacement>")`), `cid:` ones replaced by the inline image's
/// `data:` URL in documents, `data:` images kept. `None` (drop the
/// declaration) when any reference cannot be kept or replaced, or the
/// declaration uses `image-set()` / `src()`, whose bare strings are URLs too.
fn rewrite_css_urls(declaration: &str, ctx: &Context) -> Option<String> {
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
            ctx.replace("");
            return None;
        };
        let arg_end = arg_start + close;
        let url = declaration[arg_start..arg_end]
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .trim();
        let replacement = match classify_url(url) {
            UrlKind::DataImage => {
                out.push_str(&declaration[start..=arg_end]);
                pos = arg_end + 1;
                continue;
            }
            UrlKind::Cid => match ctx.cid(url) {
                Some(Cow::Borrowed(_)) => {
                    out.push_str(&declaration[start..=arg_end]);
                    pos = arg_end + 1;
                    continue;
                }
                Some(Cow::Owned(data)) => Some(data),
                None => None,
            },
            UrlKind::Remote => ctx.replace(url),
            UrlKind::Link | UrlKind::Other => None,
        };
        match replacement {
            Some(replacement) if !replacement.contains(['"', '\\', '(', ')', '<']) => {
                out.push_str("url(\"");
                out.push_str(&replacement);
                out.push_str("\")");
            }
            _ => ok = false,
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
            drop(clean(input));
        }
    }

    fn mail(html: &str) -> SanitizedHtml {
        let mut images = InlineImages::default();
        images.insert(
            "<logo@shop.example>",
            "data:image/png;base64,iVBORw0KGgo=".to_owned(),
        );
        sanitize_mail_html(html, None, images)
    }

    const NEWSLETTER: &str = r##"<!doctype html><html><head><meta charset="utf-8"><title>Sale</title>
<style type="text/css"><!--
  body { margin: 0; }
  .hero { background-color: #0b7285; color: #ffffff; padding: 24px; }
  .btn { background: url(https://t.example/btn.png); color: #ffffff; }
  @media (max-width: 600px) { .card { width: 100% !important; } }
--></style></head>
<body bgcolor="#f4f4f4" text="#222222" link="#0b7285" style="font-family: Georgia; position: fixed">
<table class="card" id="main" width="600"><tr><td class="hero"><h1>Big October Sale</h1></td></tr>
<tr><td><img src="cid:logo@shop.example" alt="Logo"><img src="cid:gone@shop.example" alt="Gone"><a class="btn" href="https://shop.example/">Shop now</a></td></tr></table>
</body></html>"##;

    #[test]
    fn mail_documents_keep_their_style_sheets_classes_and_body() {
        let out = mail(NEWSLETTER);
        let (css, body) = out
            .html
            .split_once("</style>")
            .expect("a style element first");
        assert_eq!(
            css,
            "<style>\nbody { background-color: #f4f4f4; color: #222222 }\n\
             a:link { color: #0b7285 }\n\
             body { margin: 0 }\n\
             .hero { background-color: #0b7285; color: #ffffff; padding: 24px }\n\
             .btn { color: #ffffff }\n\
             @media (max-width: 600px) {\n.card { width: 100% !important }\n}\n\
             body { font-family: Georgia }\n"
        );
        assert!(
            body.contains(r#"<table class="card" id="main" width="600">"#),
            "{body}"
        );
        assert!(body.contains(r#"<td class="hero"><h1>Big October Sale</h1></td>"#));
        assert!(
            body.contains(
                r#"<a class="btn" href="https://shop.example/" rel="noopener noreferrer">"#
            )
        );
        assert!(!body.contains("<style") && !body.contains("<title"));
        // The style sheet's remote background counts and is blocked.
        assert_eq!(out.remote_resources, 1);
        assert!(out.remote_content_blocked);
    }

    #[test]
    fn cid_images_become_data_urls_and_missing_ones_lose_their_source() {
        let out = mail(NEWSLETTER);
        assert!(
            out.html
                .contains(r#"<img src="data:image/png;base64,iVBORw0KGgo=" alt="Logo">"#),
            "{}",
            out.html
        );
        assert!(out.html.contains(r#"<img alt="Gone">"#));
        assert!(!out.html.contains("cid:"));
        let out = mail(
            r#"<div style="background: url(cid:logo@shop.example) no-repeat; color: red">x</div>"#,
        );
        assert!(out.html.contains(r#"style="background: url(&quot;data:image/png;base64,iVBORw0KGgo=&quot;) no-repeat; color: red""#), "{}", out.html);
        let out = mail(r#"<body background="cid:logo@shop.example"><p>x</p></body>"#);
        assert!(out.html.starts_with("<style>\nbody { background-image: url(\"data:image/png;base64,iVBORw0KGgo=\") }\n</style>"), "{}", out.html);
    }

    #[test]
    fn documents_report_the_inline_images_they_keep() {
        let shown = |html: &str| mail(html).inline_shown.into_iter().collect::<Vec<_>>();
        let logo = vec!["logo@shop.example".to_owned()];
        assert_eq!(shown(NEWSLETTER), logo);
        // Any case, angle brackets, percent escapes; in attributes and styles.
        assert_eq!(shown(r#"<img src="CID:%3CLogo%40Shop.example%3E">"#), logo);
        assert_eq!(
            shown(r#"<div style="background: url(cid:logo@shop.example)">x</div>"#),
            logo
        );
        assert_eq!(
            shown(
                r#"<style>.a { background: url(cid:logo@shop.example) }</style><p class="a">x</p>"#
            ),
            logo
        );
        assert_eq!(
            shown(r#"<body background="cid:logo@shop.example"><p>x</p></body>"#),
            logo
        );
        // Removed content shows nothing; unknown parts are not reported.
        assert_eq!(shown(r#"<script>document.write('<img src="cid:logo@shop.example">')</script><svg><image href="cid:logo@shop.example"/></svg><p>x</p>"#).len(), 0);
        assert_eq!(
            shown(r#"<img src="cid:gone@shop.example"><a href="cid:logo@shop.example">x</a>"#)
                .len(),
            0
        );
        // The portable variant (cid: references kept) reports the same.
        let mut images = InlineImages::default();
        images.insert(
            "<logo@shop.example>",
            "data:image/png;base64,iVBORw0KGgo=".to_owned(),
        );
        let out = sanitize_mail_html_portable(NEWSLETTER, None, images);
        assert_eq!(out.inline_shown.into_iter().collect::<Vec<_>>(), logo);
        // Fragments report nothing.
        assert!(clean(NEWSLETTER).inline_shown.is_empty());
    }

    #[test]
    fn mail_documents_proxy_remote_style_sheet_urls_when_allowed() {
        let rewriter: ProxyRewriter =
            Arc::new(|url: &str| Some(format!("nail-proxy:{}", url.len())));
        let out = sanitize_mail_html(
            r#"<style>.a { background: url(https://t.example/a.png) }</style><body background="https://t.example/bg.png"><p class="a">x</p>"#,
            Some(rewriter),
            InlineImages::default(),
        );
        assert_eq!(out.remote_resources, 2);
        assert!(!out.remote_content_blocked);
        assert!(out.html.starts_with("<style>\nbody { background-image: url(\"nail-proxy:24\") }\n.a { background: url(\"nail-proxy:23\") }\n</style>"), "{}", out.html);
    }

    #[test]
    fn mail_without_style_sheets_is_a_plain_fragment() {
        let out = mail(r#"<p class="x">Hello</p><body bgcolor="javascript:x" text="red;}">"#);
        assert_eq!(out.html, r#"<p class="x">Hello</p>"#);
    }

    #[test]
    fn fragments_still_drop_classes_style_sheets_and_keep_cid() {
        let out = clean(NEWSLETTER);
        assert!(!out.html.contains("<style"));
        assert!(!out.html.contains("class="));
        assert!(!out.html.contains("id="));
        assert!(out.html.contains(r#"src="cid:logo@shop.example""#));
    }

    #[test]
    fn data_urls_with_semicolons_stay_whole_in_styles() {
        let out =
            clean(r#"<p style="background: url(data:image/png;base64,AAAA); color: red">t</p>"#);
        assert_eq!(
            out.html,
            r#"<p style="background: url(data:image/png;base64,AAAA); color: red">t</p>"#
        );
    }

    #[test]
    fn fixed_boxes_and_generated_text_are_dropped_from_styles() {
        let out = clean(r#"<p style="position: fixed; top: 0; content: 'x'; color: red">t</p>"#);
        assert_eq!(out.html, r#"<p style="top: 0; color: red">t</p>"#);
    }

    #[test]
    fn body_style_cannot_start_rules_of_its_own() {
        // P1 review: the body's style attribute lands in the style sheet, so
        // a brace would end `body { … }` and smuggle in fixed boxes and
        // generated link text.
        let out = mail(
            r#"<body style="color:red} .x{position:fixed;top:0} a::after{content:'https://bank.example'"><a class="x" href="https://evil.example">x</a></body>"#,
        );
        let (css, _) = out
            .html
            .split_once("</style>")
            .unwrap_or(("", out.html.as_str()));
        assert_eq!(css.matches('{').count(), css.matches('}').count(), "{css}");
        assert!(!css.contains("position:fixed"), "{css}");
        assert!(!css.contains("bank.example"), "{css}");
        assert!(!css.contains(".x"), "{css}");
        // The same braces in an element's own style attribute are refused too.
        let out = clean(r#"<p style="color: red} p{color: blue">t</p>"#);
        assert!(!out.html.contains('{'), "{}", out.html);
    }

    #[test]
    fn unclosed_strings_are_dropped_from_styles() {
        let out = mail(r#"<body style="font-family: 'Georgia; color: red"><p>t</p></body>"#);
        assert!(!out.html.contains("Georgia"), "{}", out.html);
        let out = clean(r#"<p style="font-family: &quot;Georgia&quot;; color: red">t</p>"#);
        assert!(out.html.contains("Georgia"), "{}", out.html);
    }

    #[test]
    fn inline_images_normalize_their_ids() {
        let mut images = InlineImages::default();
        assert!(images.is_empty());
        images.insert(" <A@b> ", "data:image/gif;base64,R0lG".to_owned());
        assert_eq!(images.get("cid:A@b"), Some("data:image/gif;base64,R0lG"));
        // Case, the scheme's case and RFC 2392 percent escapes do not matter.
        assert_eq!(images.get("a@b"), Some("data:image/gif;base64,R0lG"));
        assert_eq!(images.get("CID:a%40B"), Some("data:image/gif;base64,R0lG"));
        assert_eq!(images.get("cid:b@a"), None);
        assert_eq!(percent_decode("a%4"), "a%4");
        assert_eq!(percent_decode("a%zz%41"), "a%zzA");
        assert_eq!(percent_decode("%ff"), "%ff");
    }
}
