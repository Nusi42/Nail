//! What a mail's HTML says about the whole document (P1 WP190): its
//! `<style>` blocks (in `<head>` or anywhere in `<body>`) and the attributes
//! of its `<body>` element. The sanitizer parses the body as a fragment,
//! which drops both, so they are read here first, with html5ever's
//! tokenizer (the same tokenizer the sanitizer's parser uses).
//!
//! Only text is collected; [`crate::css`] filters the style sheets and
//! [`crate::sanitize`] the body attributes. Style blocks inside comments
//! (Outlook's `<!--[if mso]>`), `<noscript>`, `<script>` or other raw-text
//! elements are not collected.

use std::cell::{Cell, RefCell};

use html5ever::buffer_queue::BufferQueue;
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts};

use crate::css::{MAX_STYLESHEET_BYTES, truncate};

/// The document-level parts of a mail's HTML.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Extracted {
    /// The text of every `<style>` element, in document order (at most
    /// [`MAX_STYLESHEET_BYTES`] together).
    pub(crate) styles: Vec<String>,
    /// The attributes of the first `<body>` start tag (lower-case names).
    pub(crate) body: Vec<(String, String)>,
}

/// Reads the style blocks and body attributes of `html`.
pub(crate) fn extract(html: &str) -> Extracted {
    let sink = Sink::default();
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(html));
    // The sink never asks for a script to run, so feeding finishes at once.
    drop(tokenizer.feed(&input));
    tokenizer.end();
    let sink = tokenizer.sink;
    Extracted {
        styles: sink.styles.take(),
        body: sink.body.take().unwrap_or_default(),
    }
}

#[derive(Default)]
struct Sink {
    styles: RefCell<Vec<String>>,
    collected: Cell<usize>,
    current: RefCell<Option<String>>,
    body: RefCell<Option<Vec<(String, String)>>>,
}

impl Sink {
    fn finish_style(&self) {
        if let Some(text) = self.current.borrow_mut().take() {
            let room = MAX_STYLESHEET_BYTES.saturating_sub(self.collected.get());
            let text = truncate(&text, room);
            if !text.trim().is_empty() {
                self.collected.set(self.collected.get() + text.len());
                self.styles.borrow_mut().push(text.to_owned());
            }
        }
    }
}

impl TokenSink for Sink {
    type Handle = ();

    fn process_token(&self, token: Token, _line_number: u64) -> TokenSinkResult<()> {
        match token {
            Token::TagToken(tag) => match tag.kind {
                TagKind::StartTag => match &*tag.name {
                    "style" => {
                        self.finish_style();
                        *self.current.borrow_mut() = Some(String::new());
                        return TokenSinkResult::RawData(RawKind::Rawtext);
                    }
                    "body" => {
                        let mut body = self.body.borrow_mut();
                        if body.is_none() {
                            *body = Some(
                                tag.attrs
                                    .iter()
                                    .map(|a| {
                                        (
                                            a.name.local.to_ascii_lowercase().to_string(),
                                            a.value.to_string(),
                                        )
                                    })
                                    .collect(),
                            );
                        }
                    }
                    "script" => return TokenSinkResult::RawData(RawKind::ScriptData),
                    "title" | "textarea" => return TokenSinkResult::RawData(RawKind::Rcdata),
                    "xmp" | "iframe" | "noembed" | "noframes" | "noscript" => {
                        return TokenSinkResult::RawData(RawKind::Rawtext);
                    }
                    "plaintext" => return TokenSinkResult::Plaintext,
                    _ => {}
                },
                TagKind::EndTag => {
                    if &*tag.name == "style" {
                        self.finish_style();
                    }
                }
            },
            Token::CharacterTokens(text) => {
                if let Some(current) = self.current.borrow_mut().as_mut() {
                    current.push_str(&text);
                }
            }
            Token::EOFToken => self.finish_style(),
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_head_and_body_styles_and_body_attributes() {
        let out = extract(
            "<!doctype html><html><head><title>a { x: y }</title><style type=\"text/css\">.a { color: red }</style></head>\
             <BODY BGCOLOR=\"#f4f4f4\" text=white><style>.b > p { color: blue }</style><p>x</p><body bgcolor=red></body></html>",
        );
        assert_eq!(out.styles, [".a { color: red }", ".b > p { color: blue }"]);
        assert_eq!(
            out.body,
            [
                ("bgcolor".to_owned(), "#f4f4f4".to_owned()),
                ("text".to_owned(), "white".to_owned())
            ]
        );
    }

    #[test]
    fn ignores_styles_in_comments_scripts_and_raw_text() {
        let out = extract(
            "<!--[if mso]><style>.mso { x: y }</style><![endif]--><script>var s = '<style>.s{}</style>';</script>\
             <noscript><style>.n { x: y }</style></noscript><textarea><style>.t{}</style></textarea><p>x</p>",
        );
        assert!(out.styles.is_empty(), "{:?}", out.styles);
        assert!(out.body.is_empty());
    }

    #[test]
    fn an_unterminated_style_ends_with_the_document() {
        let out = extract("<p>a</p><style>.a { color: red }");
        assert_eq!(out.styles, [".a { color: red }"]);
    }

    #[test]
    fn style_text_is_capped() {
        let big = "a".repeat(MAX_STYLESHEET_BYTES);
        let out = extract(&format!("<style>{big}</style><style>.b {{}}</style>"));
        assert_eq!(out.styles.len(), 1);
        assert_eq!(out.styles[0].len(), MAX_STYLESHEET_BYTES);
    }

    #[test]
    fn survives_garbage() {
        for input in [
            "",
            "<",
            "<style",
            "<style>",
            "</style>",
            "<body",
            "<body a",
            "\u{0}<style>\u{0}</style>",
        ] {
            drop(extract(input));
        }
    }
}
