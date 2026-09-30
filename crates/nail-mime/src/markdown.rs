//! `text/markdown` body parts (RFC 7763; M2c WP53).
//!
//! Nail's composer sends Markdown mail as `multipart/alternative` of
//! `text/plain`, an optional `text/markdown` source and `text/html`; other
//! clients send a lone `text/markdown` body. `mail-parser` knows only
//! `text/plain` and `text/html` as bodies and lists every other text part
//! as an attachment. Here a Markdown part is a **body alternative** when it
//! is the message's own body or a child of a `multipart/alternative`, and
//! is not marked `Content-Disposition: attachment`:
//!
//! * it is never listed as an attachment;
//! * without a `text/plain` alternative its source is the text body (and
//!   the preview), instead of text converted from HTML;
//! * without an HTML alternative it is rendered (CommonMark + GFM, raw
//!   HTML not passed through) and then sanitized like any mail HTML.
//!
//! Markdown files attached to a message stay attachments.

use mail_parser::{Message, MessagePart, MimeHeaders, PartType};

/// Whether a part is `text/markdown` (or the older `text/x-markdown`).
fn is_markdown(part: &MessagePart<'_>) -> bool {
    part.content_type().is_some_and(|ct| {
        ct.c_type.eq_ignore_ascii_case("text")
            && ct.c_subtype.as_deref().is_some_and(|s| {
                s.eq_ignore_ascii_case("markdown") || s.eq_ignore_ascii_case("x-markdown")
            })
    })
}

fn is_attachment(part: &MessagePart<'_>) -> bool {
    part.content_disposition()
        .is_some_and(|d| d.c_type.eq_ignore_ascii_case("attachment"))
}

fn is_alternative(part: &MessagePart<'_>) -> bool {
    part.content_type().is_some_and(|ct| {
        ct.c_type.eq_ignore_ascii_case("multipart")
            && ct
                .c_subtype
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case("alternative"))
    })
}

/// The Markdown body alternatives of a message: `(part id, source)`, in
/// MIME order. Nested `message/rfc822` parts are separate messages and are
/// not looked into.
pub(crate) fn alternatives(message: &Message<'_>) -> Vec<(u32, String)> {
    let text_of = |id: u32| -> Option<(u32, String)> {
        let part = message.parts.get(usize::try_from(id).ok()?)?;
        if !is_markdown(part) || is_attachment(part) {
            return None;
        }
        match &part.body {
            PartType::Text(text) => Some((id, text.to_string())),
            _ => None,
        }
    };
    let mut out = Vec::new();
    // The message's own body is a lone Markdown part.
    if let Some(root) = message.parts.first()
        && !matches!(root.body, PartType::Multipart(_))
    {
        out.extend(text_of(0));
    }
    for part in &message.parts {
        if let PartType::Multipart(children) = &part.body
            && is_alternative(part)
        {
            out.extend(children.iter().filter_map(|id| text_of(*id)));
        }
    }
    out
}

/// Markdown rendered for the reader: CommonMark with the GFM extensions
/// Nail's composer writes; raw HTML in the source is not passed through
/// (the result is sanitized afterwards anyway).
pub(crate) fn to_html(source: &str) -> String {
    let mut options = comrak::Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options.render.r#unsafe = false;
    comrak::markdown_to_html(source, &options)
}
