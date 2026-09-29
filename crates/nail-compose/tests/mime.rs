//! Golden-file and round-trip tests of the built messages.
//!
//! Each case builds a message, normalizes the random MIME boundaries and
//! compares the bytes with `tests/golden/<case>.eml`. Regenerate the files
//! with `NAIL_UPDATE_GOLDEN=1 cargo test -p nail-compose --test mime` and
//! review the diff. Every case is also parsed back with `nail-mime`.

use std::collections::HashMap;
use std::path::PathBuf;

use nail_api::accounts::{ComposerMode, Signature};
use nail_api::{Address, Draft, DraftAttachment, DraftContext, DraftContextMode, Identity};
use nail_compose::{
    ComposeError, MailStyle, MessageMeta, OutgoingMessage, ReplyHeaders, build_message,
};
use pretty_assertions::assert_eq;

const DATE: &str = "Tue, 29 Sep 2026 10:00:00 +0200";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR fake image";

fn identity() -> Identity {
    Identity {
        id: "ada@work.example".into(),
        account_id: "work".into(),
        email: "ada@work.example".into(),
        name: Some("Ada Lovelace".into()),
        reply_to: Some("replies@work.example".into()),
        bcc: vec!["archive@work.example".into()],
        signature: Some(Signature {
            text: Some("Ada".into()),
            html: None,
            markdown: None,
        }),
        composer_mode: None,
        posting: None,
        mail_style: None,
    }
}

fn addr(name: Option<&str>, email: &str) -> Address {
    Address {
        name: name.map(Into::into),
        email: email.into(),
    }
}

fn base_draft() -> Draft {
    Draft {
        to: vec![addr(Some("Bob, the Builder"), "bob@example.org")],
        cc: vec![addr(Some("Zoë Ärger"), "zoe@example.org")],
        bcc: vec![addr(None, "dave@example.org")],
        subject: "Grüße – Pläne für Q4".into(),
        ..Draft::new("ada@work.example")
    }
}

fn blobs() -> HashMap<String, Vec<u8>> {
    HashMap::from([
        ("blob-logo".to_owned(), PNG.to_vec()),
        ("blob-pdf".to_owned(), b"%PDF-1.4 fake".to_vec()),
        (
            "blob-notes".to_owned(),
            "Notizen: äöü\n".as_bytes().to_vec(),
        ),
    ])
}

fn build(draft: &Draft, reply: Option<&ReplyHeaders>) -> Result<OutgoingMessage, ComposeError> {
    let blobs = blobs();
    let source = move |hash: &str| blobs.get(hash).cloned();
    build_message(
        draft,
        &identity(),
        reply,
        MessageMeta {
            date: DATE,
            message_id: "<fixed-id@work.example>",
        },
        &MailStyle::default(),
        &source,
    )
}

/// Replaces the random boundaries by `BOUNDARY-1`, `BOUNDARY-2`, … in order
/// of appearance.
fn normalize(raw: &[u8]) -> String {
    let mut text = String::from_utf8(raw.to_vec()).expect("ASCII output");
    let mut n = 0;
    while let Some(at) = text.find("boundary=\"") {
        let start = at + "boundary=\"".len();
        let end = start + text[start..].find('"').expect("closing quote");
        let boundary = text[start..end].to_owned();
        n += 1;
        text = text.replace(&boundary, &format!("BOUNDARY-{n}"));
        text = text.replacen("boundary=\"BOUNDARY", "boundary=\u{0}BOUNDARY", 1);
    }
    text.replace('\u{0}', "\"")
}

fn golden(name: &str, message: &OutgoingMessage) {
    let actual = normalize(&message.raw);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.eml"));
    if std::env::var_os("NAIL_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual.as_bytes()).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing {}: run with NAIL_UPDATE_GOLDEN=1", path.display()));
    // Git may check the files out with LF line ends.
    assert_eq!(actual.replace("\r\n", "\n"), expected.replace("\r\n", "\n"));
}

fn check_wire_format(message: &OutgoingMessage) {
    let text = String::from_utf8(message.raw.clone()).expect("7-bit output");
    assert!(!text.replace("\r\n", "").contains('\n'), "bare LF");
    for line in text.split("\r\n") {
        assert!(line.len() <= 998, "line too long: {line}");
        assert!(line.is_ascii(), "non-ASCII line: {line}");
    }
    assert!(
        !text.to_ascii_lowercase().contains("\r\nbcc:"),
        "Bcc header leaked"
    );
    assert!(
        text.starts_with("Date: Tue, 29 Sep 2026 10:00:00 +0200\r\n"),
        "{text}"
    );
    assert!(
        text.contains("\r\nMessage-ID: <fixed-id@work.example>\r\n"),
        "{text}"
    );
    assert!(text.contains("\r\nMIME-Version: 1.0\r\n"), "{text}");
    assert_eq!(message.message_id, "fixed-id@work.example");
    assert_eq!(message.envelope.from, "ada@work.example");
    assert_eq!(
        message.envelope.to,
        [
            "bob@example.org",
            "zoe@example.org",
            "dave@example.org",
            "archive@work.example"
        ]
    );
}

fn check_headers(parsed: &nail_mime::ParsedMessage) {
    let h = &parsed.headers;
    assert_eq!(h.subject, "Grüße – Pläne für Q4");
    assert_eq!(h.from, [addr(Some("Ada Lovelace"), "ada@work.example")]);
    assert_eq!(h.to, [addr(Some("Bob, the Builder"), "bob@example.org")]);
    assert_eq!(h.cc, [addr(Some("Zoë Ärger"), "zoe@example.org")]);
    assert_eq!(h.reply_to, [addr(None, "replies@work.example")]);
    assert_eq!(h.message_id.as_deref(), Some("fixed-id@work.example"));
}

#[test]
fn plain_text_reply_is_flowed_and_threaded() {
    let long = "This paragraph is long enough to be wrapped with soft line breaks so that \
                receiving clients can reflow it to their window width.";
    let draft = Draft {
        text: format!("Hallo Bob,\n\n{long}\n\nOn Monday, Bob wrote:\n> Plans?\n\n-- \nAda"),
        context: Some(DraftContext {
            mode: DraftContextMode::Reply,
            message_id: "m1".into(),
        }),
        ..base_draft()
    };
    let reply = ReplyHeaders {
        message_id: Some("<parent@example.org>".into()),
        references: vec!["root@example.org".into(), "parent@example.org".into()],
    };
    let message = build(&draft, Some(&reply)).unwrap();
    check_wire_format(&message);
    golden("plain-reply", &message);
    let raw = String::from_utf8(message.raw.clone()).unwrap();
    assert!(
        raw.contains("Content-Type: text/plain; charset=\"utf-8\"; format=\"flowed\"")
            || raw.contains("Content-Type: text/plain; charset=utf-8; format=flowed"),
        "{raw}"
    );

    let parsed = nail_mime::parse_message(&message.raw);
    check_headers(&parsed);
    assert_eq!(parsed.in_reply_to, ["parent@example.org"]);
    assert_eq!(
        parsed.references,
        ["root@example.org", "parent@example.org"]
    );
    assert_eq!(parsed.sanitized_html, None);
    assert!(parsed.attachments.is_empty());
    let text = parsed.text.replace("\r\n", "\n");
    assert!(
        text.starts_with("Hallo Bob,\n\nThis paragraph is long enough"),
        "{text}"
    );
    assert!(text.contains("> Plans?\n\n-- \nAda"), "{text}");
    // Unflowing the soft breaks gives the paragraph back.
    let unflowed = text.replace(" \n", " ");
    assert!(unflowed.contains(long), "{unflowed}");
}

#[test]
fn html_with_inline_image_and_attachments() {
    let draft = Draft {
        mode: ComposerMode::Html,
        html: Some(
            "<h1>Plan</h1><p>See <b>the logo</b>:</p><p><img src=\"cid:logo@nail\" alt=\"Logo\"></p>\
             <script>alert(1)</script><blockquote><p>quoted</p></blockquote>"
                .into(),
        ),
        attachments: vec![
            DraftAttachment {
                filename: "logo.png".into(),
                mime: "image/png".into(),
                size: PNG.len() as u64,
                blob: "blob-logo".into(),
                content_id: Some("logo@nail".into()),
            },
            DraftAttachment {
                filename: "Angebot März.pdf".into(),
                mime: "application/pdf".into(),
                size: 13,
                blob: "blob-pdf".into(),
                content_id: None,
            },
            DraftAttachment {
                filename: "unused.png".into(),
                mime: "image/png".into(),
                size: PNG.len() as u64,
                blob: "blob-logo".into(),
                content_id: Some("unused@nail".into()),
            },
        ],
        ..base_draft()
    };
    let message = build(&draft, None).unwrap();
    check_wire_format(&message);
    golden("html-related-mixed", &message);
    let raw = normalize(&message.raw);
    let mixed = raw.find("multipart/mixed").unwrap();
    let alternative = raw.find("multipart/alternative").unwrap();
    let related = raw.find("multipart/related").unwrap();
    assert!(mixed < alternative && alternative < related, "{raw}");
    assert!(!raw.contains("<script"), "sanitized: {raw}");

    let parsed = nail_mime::parse_message(&message.raw);
    check_headers(&parsed);
    let html = parsed.sanitized_html.as_deref().unwrap();
    assert!(html.contains("the logo"), "{html}");
    assert!(
        html.contains("border-left: 3px solid"),
        "stylesheet inlined: {html}"
    );
    assert!(parsed.text.contains("See the logo:"), "{}", parsed.text);
    assert!(parsed.text.contains("> quoted"), "{}", parsed.text);
    let names: Vec<(Option<&str>, &str, bool)> = parsed
        .attachments
        .iter()
        .map(|a| (a.filename.as_deref(), a.mime.as_str(), a.inline))
        .collect();
    assert_eq!(
        names,
        [
            (Some("logo.png"), "image/png", true),
            (Some("Angebot März.pdf"), "application/pdf", false),
            (Some("unused.png"), "image/png", false),
        ]
    );
    let image = nail_mime::inline_content_by_cid(&message.raw, "logo@nail").unwrap();
    assert_eq!(image.data, PNG);
    let pdf = nail_mime::attachment_content(&message.raw, 1).unwrap();
    assert_eq!(pdf.data, b"%PDF-1.4 fake");
}

#[test]
fn markdown_with_markdown_part_and_highlighted_code() {
    let draft = Draft {
        mode: ComposerMode::Markdown,
        markdown: Some(
            "# Plan\n\n- [x] draft\n- [ ] send\n\n```rust\nfn main() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n-- \nAda"
                .into(),
        ),
        markdown_part: true,
        attachments: vec![DraftAttachment {
            filename: "notes.txt".into(),
            mime: "text/plain".into(),
            size: 14,
            blob: "blob-notes".into(),
            content_id: None,
        }],
        ..base_draft()
    };
    let message = build(&draft, None).unwrap();
    check_wire_format(&message);
    golden("markdown-alternative", &message);
    let raw = normalize(&message.raw);
    let text = raw.find("Content-Type: text/plain").unwrap();
    let markdown = raw.find("Content-Type: text/markdown").unwrap();
    let html = raw.find("Content-Type: text/html").unwrap();
    assert!(
        text < markdown && markdown < html,
        "order text, markdown, html: {raw}"
    );
    assert!(
        raw.contains("variant=\"GFM\"") || raw.contains("variant=GFM"),
        "{raw}"
    );

    let parsed = nail_mime::parse_message(&message.raw);
    check_headers(&parsed);
    assert!(
        parsed.text.starts_with("# Plan"),
        "source as text: {}",
        parsed.text
    );
    let html = parsed.sanitized_html.as_deref().unwrap();
    assert!(html.contains("<h1"), "{html}");
    assert!(html.contains("<table"), "{html}");
    assert!(
        html.contains("color:#") || html.contains("color: #"),
        "highlighted: {html}"
    );
    // mail-parser lists the text/markdown alternative among the attachments
    // (a nail-mime matter on the receiving side); the file is the last one.
    let last = parsed.attachments.last().unwrap();
    assert_eq!(last.filename.as_deref(), Some("notes.txt"));
    assert!(parsed.attachments.len() <= 2, "{:?}", parsed.attachments);
}

#[test]
fn missing_blobs_and_senders_are_errors() {
    let draft = Draft {
        attachments: vec![DraftAttachment {
            filename: "gone.bin".into(),
            mime: "not a type".into(),
            size: 1,
            blob: "nope".into(),
            content_id: None,
        }],
        ..base_draft()
    };
    let error = build(&draft, None).unwrap_err();
    assert!(
        matches!(&error, ComposeError::MissingBlob { filename, blob } if filename == "gone.bin" && blob == "nope"),
        "{error}"
    );
    let no_sender = Identity {
        email: " ".into(),
        ..identity()
    };
    let error = build_message(
        &base_draft(),
        &no_sender,
        None,
        MessageMeta {
            date: DATE,
            message_id: "x@y",
        },
        &MailStyle::default(),
        &|_: &str| None,
    )
    .unwrap_err();
    assert!(matches!(error, ComposeError::NoSender));
}

#[test]
fn plain_messages_send_inline_images_as_attachments_and_fix_bad_types() {
    let draft = Draft {
        text: "See attached".into(),
        attachments: vec![DraftAttachment {
            filename: "x\r\ny.png".into(),
            mime: "image/png; evil=\"1\"".into(),
            size: PNG.len() as u64,
            blob: "blob-logo".into(),
            content_id: Some("logo@nail".into()),
        }],
        ..base_draft()
    };
    let message = build(&draft, None).unwrap();
    check_wire_format(&message);
    let parsed = nail_mime::parse_message(&message.raw);
    assert_eq!(parsed.attachments.len(), 1);
    assert_eq!(parsed.attachments[0].mime, "application/octet-stream");
    assert!(!parsed.attachments[0].inline);
    assert_eq!(parsed.attachments[0].filename.as_deref(), Some("x  y.png"));
}

#[test]
fn message_ids_are_unique_and_in_the_sender_domain() {
    let a = nail_compose::new_message_id("ada@work.example", b"seed");
    let b = nail_compose::new_message_id("ada@work.example", b"seed");
    assert_ne!(a, b);
    assert!(a.ends_with("@work.example"));
    assert!(nail_compose::new_message_id("nobody", b"").ends_with("@nail.invalid"));
}

#[test]
fn rule_mail_is_marked_as_automatic() {
    let plain = normalize(&build(&base_draft(), None).unwrap().raw);
    assert!(!plain.contains("Auto-Submitted"), "{plain}");
    assert!(!plain.contains(nail_api::LOOP_HEADER), "{plain}");
    for (automatic, value) in [
        (nail_api::AutomaticMail::Reply, "auto-replied"),
        (nail_api::AutomaticMail::Forward, "auto-generated"),
    ] {
        let draft = Draft {
            automatic: Some(automatic),
            ..base_draft()
        };
        let raw = normalize(&build(&draft, None).unwrap().raw);
        assert!(
            raw.contains(&format!("Auto-Submitted: {value}\r\n")),
            "{raw}"
        );
        assert!(raw.contains("X-Nail-Loop: ada@work.example\r\n"), "{raw}");
        let parsed = nail_mime::header_fields(raw.as_bytes());
        assert!(parsed.iter().any(|(n, _)| n == "X-Nail-Loop"), "{parsed:?}");
    }
}

#[test]
fn redirects_prepend_resent_headers_and_the_loop_marker() {
    let original = b"From: Bob <bob@example.org>\r\nSubject: Hi\r\n\r\nBody\r\n";
    let bytes = nail_compose::resent_message(
        original,
        "ada@work.example",
        &["desk@example.org".into(), "x@example.org".into()],
        MessageMeta {
            date: DATE,
            message_id: "<r1@work.example>",
        },
    );
    let text = String::from_utf8(bytes).unwrap();
    assert_eq!(
        text,
        "Resent-From: ada@work.example\r\nResent-To: desk@example.org, x@example.org\r\n\
         Resent-Date: Tue, 29 Sep 2026 10:00:00 +0200\r\nResent-Message-ID: <r1@work.example>\r\n\
         X-Nail-Loop: ada@work.example\r\n\
         From: Bob <bob@example.org>\r\nSubject: Hi\r\n\r\nBody\r\n"
    );
}
