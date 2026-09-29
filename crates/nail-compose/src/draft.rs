//! Drafts: new messages, replies and forwards with everything filled in
//! (`NewDraft`), and mode switches (`ConvertDraft`).

use nail_api::accounts::ComposerMode;
use nail_api::{
    Address, Draft, DraftAttachment, DraftContext, DraftContextMode, Identity, MessageId,
    SignatureChoice,
};

use crate::convert::{has_formatting, html_to_markdown, html_to_text};
use crate::markdown::render_markdown;
use crate::quote::{Posting, forward_body, reply_body};
use crate::signature::signature_block;
use crate::style::MailStyle;
use crate::text_to_html;

/// Composer settings of the sending identity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComposeSettings {
    /// Mail stylesheet and code theme.
    pub style: MailStyle,
    /// Top or bottom posting.
    pub posting: Posting,
}

/// Localized texts for quoting, filled in by the caller (the core formats
/// them from its Fluent catalog with the original's date and sender).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuoteStrings {
    /// Reply quote header, e.g. "On 1 May 2026, Bob wrote:".
    pub reply_header: String,
    /// First line of a forwarded message, e.g. "Forwarded message".
    pub forward_intro: String,
    /// Header fields shown in a forward: (label, value).
    pub forward_fields: Vec<(String, String)>,
}

/// The message being replied to or forwarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginalMessage {
    /// Its id in the store.
    pub id: MessageId,
    /// `From`.
    pub from: Vec<Address>,
    /// `To`.
    pub to: Vec<Address>,
    /// `Cc`.
    pub cc: Vec<Address>,
    /// `Reply-To`.
    pub reply_to: Vec<Address>,
    /// Subject.
    pub subject: String,
    /// Plain-text body.
    pub text: String,
    /// Sanitized HTML body, if any.
    pub html: Option<String>,
    /// Attachments, already copied into the blob store (for forwards).
    pub attachments: Vec<DraftAttachment>,
}

/// `Re: subject` (not doubled).
pub fn reply_subject(subject: &str) -> String {
    prefixed(subject, "Re: ", &["re:"])
}

/// `Fwd: subject` (not doubled).
pub fn forward_subject(subject: &str) -> String {
    prefixed(subject, "Fwd: ", &["fwd:", "fw:"])
}

fn prefixed(subject: &str, prefix: &str, known: &[&str]) -> String {
    let lower = subject.trim_start().to_lowercase();
    if known.iter().any(|k| lower.starts_with(k)) {
        subject.trim_start().to_owned()
    } else {
        format!("{prefix}{}", subject.trim_start())
    }
}

fn is_own(address: &Address, own: &[String]) -> bool {
    own.iter()
        .any(|o| o.eq_ignore_ascii_case(address.email.trim()))
}

fn push_unique(list: &mut Vec<Address>, address: &Address) {
    if !list
        .iter()
        .any(|a| a.email.eq_ignore_ascii_case(&address.email))
    {
        list.push(address.clone());
    }
}

/// `(to, cc)` of a reply: the sender (or `Reply-To`); for reply-all also
/// every other recipient, without the user's own addresses and duplicates.
pub fn reply_recipients(
    original: &OriginalMessage,
    mode: DraftContextMode,
    own: &[String],
) -> (Vec<Address>, Vec<Address>) {
    let answered = if original.reply_to.is_empty() {
        &original.from
    } else {
        &original.reply_to
    };
    let mut to = Vec::new();
    for a in answered.iter().filter(|a| !is_own(a, own)) {
        push_unique(&mut to, a);
    }
    if to.is_empty() {
        // Replying to one's own message goes to its recipients.
        for a in &original.to {
            push_unique(&mut to, a);
        }
    }
    let mut cc = Vec::new();
    if mode == DraftContextMode::ReplyAll {
        for a in original.to.iter().chain(&original.cc) {
            if !is_own(a, own) && !to.iter().any(|t| t.email.eq_ignore_ascii_case(&a.email)) {
                push_unique(&mut cc, a);
            }
        }
    }
    (to, cc)
}

fn set_body(draft: &mut Draft, mode: ComposerMode, body: String) {
    draft.mode = mode;
    match mode {
        ComposerMode::Plain => {
            draft.text = body;
            draft.html = None;
            draft.markdown = None;
        }
        ComposerMode::Html => {
            draft.text = String::new();
            draft.html = Some(body);
            draft.markdown = None;
        }
        ComposerMode::Markdown => {
            draft.text = String::new();
            draft.html = None;
            draft.markdown = Some(body);
        }
    }
}

/// The body source of a draft in its own mode.
pub fn body_of(draft: &Draft) -> &str {
    match draft.mode {
        ComposerMode::Plain => &draft.text,
        ComposerMode::Html => draft.html.as_deref().unwrap_or_default(),
        ComposerMode::Markdown => draft.markdown.as_deref().unwrap_or_default(),
    }
}

/// A new draft from `identity`: its mode (or `mode`), its signature and,
/// for a reply or forward, subject, recipients, quote and forwarded
/// attachments. `own` lists the user's addresses (left out of replies).
pub fn new_draft(
    identity: &Identity,
    mode: Option<ComposerMode>,
    settings: &ComposeSettings,
    context: Option<(DraftContextMode, &OriginalMessage, &QuoteStrings)>,
    own: &[String],
) -> Draft {
    let mode = mode.or(identity.composer_mode).unwrap_or_default();
    let mut draft = Draft::new(identity.id.clone());
    draft.signature = SignatureChoice::Identity;
    let signature = signature_block(identity.signature.as_ref(), mode, &settings.style);
    let body = match context {
        None => match (mode, &signature) {
            (_, None) => String::new(),
            (ComposerMode::Html, Some(sig)) => format!("<p></p>{sig}"),
            (_, Some(sig)) => format!("\n\n{sig}"),
        },
        Some((kind, original, strings)) => {
            draft.context = Some(DraftContext {
                mode: kind,
                message_id: original.id.clone(),
            });
            let html = original.html.as_deref();
            if kind == DraftContextMode::Forward {
                draft.subject = forward_subject(&original.subject);
                draft.attachments = original.attachments.clone();
                forward_body(
                    mode,
                    &strings.forward_intro,
                    &strings.forward_fields,
                    &original.text,
                    html,
                    signature.as_deref(),
                )
            } else {
                draft.subject = reply_subject(&original.subject);
                let (to, cc) = reply_recipients(original, kind, own);
                draft.to = to;
                draft.cc = cc;
                reply_body(
                    mode,
                    settings.posting,
                    &strings.reply_header,
                    &original.text,
                    html,
                    signature.as_deref(),
                )
            }
        }
    };
    set_body(&mut draft, mode, body);
    draft
}

/// A draft switched to another mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Converted {
    /// The converted draft.
    pub draft: Draft,
    /// What could not be carried over (empty = lossless). Keys as in
    /// [`crate::Conversion::dropped`], plus `formatting` when a rich body
    /// becomes plain text.
    pub dropped: Vec<&'static str>,
}

impl Converted {
    /// Whether the switch loses formatting.
    pub fn lossy(&self) -> bool {
        !self.dropped.is_empty()
    }
}

/// Switches a draft to `mode`, converting its body. Plain → anything and
/// Markdown → HTML are lossless; HTML → Markdown is best effort; rich →
/// plain keeps the text but loses the formatting.
pub fn convert_draft(draft: &Draft, mode: ComposerMode, style: &MailStyle) -> Converted {
    let mut out = draft.clone();
    if draft.mode == mode {
        return Converted {
            draft: out,
            dropped: Vec::new(),
        };
    }
    let source = body_of(draft).to_owned();
    let mut dropped = Vec::new();
    let body = match (draft.mode, mode) {
        (ComposerMode::Plain, ComposerMode::Html) => text_to_html(&source),
        (ComposerMode::Plain, ComposerMode::Markdown) => source,
        (ComposerMode::Markdown, ComposerMode::Html) => render_markdown(&source, style),
        (ComposerMode::Markdown, ComposerMode::Plain) => {
            if has_formatting(&render_markdown(&source, style)) {
                dropped.push("formatting");
            }
            source
        }
        (ComposerMode::Html, ComposerMode::Markdown) => {
            let conversion = html_to_markdown(&source);
            dropped = conversion.dropped;
            conversion.markdown
        }
        (ComposerMode::Html, ComposerMode::Plain) => {
            if has_formatting(&source) {
                dropped.push("formatting");
            }
            html_to_text(&source)
        }
        (_, _) => source,
    };
    out.markdown_part = out.markdown_part && mode == ComposerMode::Markdown;
    set_body(&mut out, mode, body);
    Converted {
        draft: out,
        dropped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::accounts::Signature;
    use pretty_assertions::assert_eq;

    fn addr(name: &str, email: &str) -> Address {
        Address {
            name: (!name.is_empty()).then(|| name.to_owned()),
            email: email.into(),
        }
    }

    fn identity(mode: Option<ComposerMode>) -> Identity {
        Identity {
            id: "ada@work.example".into(),
            account_id: "work".into(),
            email: "ada@work.example".into(),
            name: Some("Ada".into()),
            reply_to: None,
            bcc: vec![],
            signature: Some(Signature {
                text: Some("Ada".into()),
                html: None,
                markdown: Some("**Ada**".into()),
            }),
            composer_mode: mode,
            posting: None,
            mail_style: None,
        }
    }

    fn original() -> OriginalMessage {
        OriginalMessage {
            id: "m1".into(),
            from: vec![addr("Bob", "bob@x.org")],
            to: vec![addr("Ada", "ADA@work.example"), addr("", "carol@x.org")],
            cc: vec![addr("", "dave@x.org"), addr("Bob again", "bob@x.org")],
            reply_to: vec![],
            subject: "Plans".into(),
            text: "Hi Ada".into(),
            html: Some("<p>Hi <b>Ada</b></p>".into()),
            attachments: vec![DraftAttachment {
                filename: "a.pdf".into(),
                mime: "application/pdf".into(),
                size: 3,
                blob: "b1".into(),
                content_id: None,
            }],
        }
    }

    fn strings() -> QuoteStrings {
        QuoteStrings {
            reply_header: "Bob wrote:".into(),
            forward_intro: "Forwarded".into(),
            forward_fields: vec![("From".into(), "Bob <bob@x.org>".into())],
        }
    }

    #[test]
    fn subjects_are_not_doubled() {
        assert_eq!(reply_subject("Plans"), "Re: Plans");
        assert_eq!(reply_subject("RE: Plans"), "RE: Plans");
        assert_eq!(forward_subject("Fw: x"), "Fw: x");
        assert_eq!(forward_subject("x"), "Fwd: x");
    }

    #[test]
    fn reply_all_recipients_skip_own_addresses_and_duplicates() {
        let own = vec!["ada@work.example".to_owned()];
        let (to, cc) = reply_recipients(&original(), DraftContextMode::ReplyAll, &own);
        assert_eq!(to, [addr("Bob", "bob@x.org")]);
        assert_eq!(cc, [addr("", "carol@x.org"), addr("", "dave@x.org")]);
        let (_, cc) = reply_recipients(&original(), DraftContextMode::Reply, &own);
        assert!(cc.is_empty());
        let mut mine = original();
        mine.from = vec![addr("Ada", "ada@work.example")];
        mine.reply_to = vec![];
        let (to, _) = reply_recipients(&mine, DraftContextMode::Reply, &own);
        assert_eq!(to[1], addr("", "carol@x.org"));
    }

    #[test]
    fn new_drafts_carry_the_signature_of_their_mode() {
        let settings = ComposeSettings::default();
        let plain = new_draft(&identity(None), None, &settings, None, &[]);
        assert_eq!(plain.mode, ComposerMode::Plain);
        assert_eq!(plain.text, "\n\n-- \nAda");
        let md = new_draft(
            &identity(Some(ComposerMode::Markdown)),
            None,
            &settings,
            None,
            &[],
        );
        assert_eq!(md.markdown.as_deref(), Some("\n\n-- \n**Ada**"));
        let html = new_draft(
            &identity(None),
            Some(ComposerMode::Html),
            &settings,
            None,
            &[],
        );
        assert!(
            html.html
                .as_deref()
                .unwrap()
                .contains("<strong>Ada</strong>")
        );
        assert_eq!(html.text, "");
    }

    #[test]
    fn replies_and_forwards_are_filled_in() {
        let settings = ComposeSettings::default();
        let own = vec!["ada@work.example".to_owned()];
        let reply = new_draft(
            &identity(Some(ComposerMode::Markdown)),
            None,
            &settings,
            Some((DraftContextMode::Reply, &original(), &strings())),
            &own,
        );
        assert_eq!(reply.subject, "Re: Plans");
        assert_eq!(reply.to, [addr("Bob", "bob@x.org")]);
        assert_eq!(reply.replied_message(), Some(&MessageId::from("m1")));
        assert_eq!(
            reply.markdown.as_deref(),
            Some("Bob wrote:\n> Hi **Ada**\n\n\n\n-- \n**Ada**")
        );
        let forward = new_draft(
            &identity(None),
            Some(ComposerMode::Html),
            &settings,
            Some((DraftContextMode::Forward, &original(), &strings())),
            &own,
        );
        assert_eq!(forward.subject, "Fwd: Plans");
        assert!(forward.to.is_empty());
        assert_eq!(forward.attachments.len(), 1);
        assert_eq!(forward.replied_message(), None);
        assert!(
            forward
                .html
                .as_deref()
                .unwrap()
                .contains("<p>Hi <b>Ada</b></p>")
        );
    }

    #[test]
    fn conversions_report_losses() {
        let style = MailStyle::default();
        let mut draft = Draft::new("ada");
        draft.text = "Hi <you>\nline".into();
        let html = convert_draft(&draft, ComposerMode::Html, &style);
        assert!(!html.lossy());
        assert_eq!(
            html.draft.html.as_deref(),
            Some("<p>Hi &lt;you&gt;<br>line</p>")
        );
        assert_eq!(html.draft.text, "");
        let md = convert_draft(&html.draft, ComposerMode::Markdown, &style);
        assert!(!md.lossy(), "{:?}", md.dropped);
        assert_eq!(md.draft.markdown.as_deref(), Some("Hi \\<you>\\\nline"));
        let back = convert_draft(&md.draft, ComposerMode::Html, &style);
        assert_eq!(
            back.draft.html.as_deref(),
            Some("<p>Hi &lt;you&gt;<br>\nline</p>\n")
        );

        let mut rich = Draft::new("ada");
        rich.mode = ComposerMode::Html;
        rich.html = Some("<p><u>x</u> <b>y</b></p>".into());
        let to_md = convert_draft(&rich, ComposerMode::Markdown, &style);
        assert_eq!(to_md.dropped, ["underline"]);
        assert_eq!(to_md.draft.markdown.as_deref(), Some("x **y**"));
        let to_plain = convert_draft(&rich, ComposerMode::Plain, &style);
        assert_eq!(to_plain.dropped, ["formatting"]);
        assert_eq!(to_plain.draft.text, "x y");
        assert_eq!(to_plain.draft.html, None);

        let mut markdown = Draft::new("ada");
        markdown.mode = ComposerMode::Markdown;
        markdown.markdown = Some("plain words".into());
        markdown.markdown_part = true;
        let plain = convert_draft(&markdown, ComposerMode::Plain, &style);
        assert!(!plain.lossy());
        assert!(!plain.draft.markdown_part);
        markdown.markdown = Some("*em*".into());
        assert!(convert_draft(&markdown, ComposerMode::Plain, &style).lossy());
        assert_eq!(
            convert_draft(&markdown, ComposerMode::Markdown, &style).draft,
            markdown
        );
    }
}
