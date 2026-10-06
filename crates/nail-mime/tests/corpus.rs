//! Parses every message of the testkit corpus: snapshot of the full result,
//! plus targeted assertions for the interesting cases.
#![cfg(test)]

use nail_api::{Flags, MessageBody};
use nail_mime::{
    ParsedMessage, SummaryIds, attachment_content, inline_content_by_cid, parse_message,
};
use nail_testkit::{corpus_message, demo_corpus};

fn parse(folder: &str, file: &str) -> ParsedMessage {
    let raw = corpus_message(folder, file).unwrap_or_else(|| panic!("corpus has {folder}/{file}"));
    parse_message(&raw)
}

fn body(parsed: ParsedMessage) -> MessageBody {
    parsed.into_body("m1".into())
}

#[test]
fn corpus_snapshots() {
    for folder in demo_corpus() {
        for message in &folder.messages {
            let parsed = parse_message(&message.raw);
            let summary = parsed.to_summary(
                SummaryIds {
                    id: "m1".into(),
                    folder_id: "f1".into(),
                    account_id: "a1".into(),
                },
                message.api_flags(),
                None,
            );
            let name = format!(
                "{}__{}",
                folder.name,
                message.file_name.trim_end_matches(".eml")
            );
            insta::assert_json_snapshot!(
                name,
                serde_json::json!({ "summary": summary, "body": body(parsed) })
            );
        }
    }
}

#[test]
fn plain_text_message() {
    let parsed = parse("INBOX", "01-welcome.eml");
    assert_eq!(parsed.headers.subject, "Welcome to Nail");
    assert_eq!(parsed.headers.from[0].name.as_deref(), Some("Ada Lovelace"));
    assert_eq!(parsed.headers.from[0].email, "ada@lovelace.example");
    assert_eq!(
        parsed.headers.date.as_deref(),
        Some("2026-09-28T08:15:00+02:00")
    );
    assert_eq!(
        parsed.headers.message_id.as_deref(),
        Some("welcome-0001@lovelace.example")
    );
    assert!(parsed.sanitized_html.is_none());
    assert!(parsed.preview.starts_with("Hi Sam, welcome aboard!"));
    assert!(parsed.preview.chars().count() <= nail_mime::PREVIEW_CHARS);
    assert!(!parsed.has_attachments());
}

#[test]
fn newsletter_remote_content_and_tracking_pixel_are_blocked() {
    let parsed = parse("INBOX", "02-newsletter-tracking.eml");
    assert_eq!(parsed.headers.subject, "Your weekly garden digest 🌱");
    assert!(parsed.remote_content_blocked);
    let html = parsed.sanitized_html.as_deref().unwrap();
    assert!(
        !html.contains("track.garden-weekly.example"),
        "tracking pixel removed: {html}"
    );
    assert!(!html.contains("pumpkins.jpg"));
    assert!(!html.contains("paper.jpg"));
    assert!(!html.contains("leaf.png"));
    // The style block stays (P1 WP190) without its remote background; the
    // remote style sheet link never does.
    assert!(html.starts_with("<style>"), "{html}");
    assert!(!html.contains("bg.png"));
    assert!(!html.contains("garden-weekly.example/mail.css"));
    assert!(!html.contains("<link"));
    assert!(html.contains("Autumn is here"));
    assert!(html.contains(r#"href="https://garden-weekly.example/r/abc123?to=bulbs""#));
    assert!(parsed.text.contains("Autumn is here"));
}

#[test]
fn multipart_alternative_has_text_and_html() {
    let parsed = parse("INBOX", "03-alternative-meeting.eml");
    assert!(parsed.text.contains("* Release candidate on October 5"));
    assert!(!parsed.text.contains("<li>"));
    let html = parsed.sanitized_html.unwrap();
    assert!(html.contains("<li>Release candidate on <b>October 5</b></li>"));
    assert_eq!(parsed.headers.cc.len(), 1);
    assert!(!parsed.remote_content_blocked);
}

#[test]
fn attachments_are_listed_and_decodable() {
    let raw = corpus_message("INBOX", "04-invoice-attachment.eml").unwrap();
    let parsed = parse_message(&raw);
    assert!(parsed.has_attachments());
    assert_eq!(parsed.attachments.len(), 2);
    let pdf = &parsed.attachments[0];
    assert_eq!(pdf.index, 0);
    assert_eq!(pdf.filename.as_deref(), Some("INV-2026-09-5521.pdf"));
    assert_eq!(pdf.mime, "application/pdf");
    assert!(!pdf.inline);
    assert!(!pdf.shown_in_body);
    assert_eq!(parsed.attachments[1].mime, "text/csv");

    let content = attachment_content(&raw, 0).unwrap();
    assert!(content.data.starts_with(b"%PDF-1.4"));
    assert_eq!(content.data.len() as u64, pdf.size);
    assert!(attachment_content(&raw, 9).is_none());
    assert!(
        !parsed.text.contains("date,service"),
        "attachment text is not body text"
    );
}

#[test]
fn inline_cid_image_is_embedded_as_data_url() {
    let raw = corpus_message("INBOX", "05-inline-image.eml").unwrap();
    let parsed = parse_message(&raw);
    let html = parsed.reader_html.as_deref().unwrap();
    // Resolved for the reader when the body is built (P1 WP190): every
    // transport shows it without allowing `cid:` in a CSP.
    assert!(
        html.contains(r#"src="data:image/png;base64,iVBORw0KGgo"#),
        "{html}"
    );
    assert!(!html.contains("cid:"), "{html}");
    // Everything else (quoting, MCP, plug-ins) keeps the small reference
    // (P1 review).
    let portable = parsed.sanitized_html.as_deref().unwrap();
    assert!(portable.contains(r#"src="cid:"#), "{portable}");
    assert!(!portable.contains("data:"), "{portable}");
    assert!(!parsed.remote_content_blocked);
    assert_eq!(parsed.attachments.len(), 1);
    assert!(parsed.attachments[0].inline);
    assert!(parsed.attachments[0].shown_in_body);
    assert_eq!(parsed.attachments[0].mime, "image/png");
    assert!(
        !parsed.has_attachments(),
        "inline images are not attachments"
    );

    let image = inline_content_by_cid(&raw, "cid:bench-photo-1@chem.example").unwrap();
    assert!(image.data.starts_with(b"\x89PNG"));
    assert!(inline_content_by_cid(&raw, "cid:missing@x").is_none());
}

#[test]
fn legacy_charsets_are_decoded() {
    let latin1 = parse("INBOX", "06-latin1-qp.eml");
    assert_eq!(latin1.headers.subject, "Grüße aus München");
    assert_eq!(
        latin1.headers.from[0].name.as_deref(),
        Some("Jürgen Müller")
    );
    assert!(latin1.text.contains("viele Grüße aus München"));

    let cp1252 = parse("INBOX", "07-windows1252-8bit.eml");
    assert_eq!(cp1252.headers.subject, "Café crème à la gare");
    assert_eq!(
        cp1252.headers.from[0].name.as_deref(),
        Some("Françoise Dupont")
    );
    assert!(
        cp1252
            .text
            .contains("on se retrouve au café à côté de la gare ? Je t’offre un crème")
    );
}

#[test]
fn broken_mime_is_best_effort() {
    let parsed = parse("INBOX", "08-broken-mime.eml");
    assert!(parsed.headers.from.is_empty(), "{:?}", parsed.headers.from);
    assert_eq!(parsed.headers.date, None);
    assert_eq!(
        parsed.headers.subject,
        "Broken message (missing From, bad boundaries)"
    );
    let summary = parsed.to_summary(
        SummaryIds {
            id: "m".into(),
            folder_id: "f".into(),
            account_id: "a".into(),
        },
        Flags::default(),
        Some("2026-09-29T00:00:00Z"),
    );
    assert_eq!(summary.from, None);
    assert_eq!(summary.date, "2026-09-29T00:00:00Z");
}

#[test]
fn hostile_html_is_neutralized() {
    let parsed = parse("INBOX", "09-hostile-html.eml");
    let html = parsed
        .sanitized_html
        .as_deref()
        .unwrap()
        .to_ascii_lowercase();
    for needle in [
        "<script",
        "onload",
        "onerror",
        "onclick",
        "javascript:",
        "vbscript:",
        "data:text",
        "<iframe",
        "<object",
        "<embed",
        "<svg",
        "<form",
        "<input",
        "<button",
        "<meta",
        "<base",
        "expression(",
        "evil.example/pixel",
        "evil.example/frame",
        "document.cookie",
        "<math",
        "xlink",
    ] {
        assert!(!html.contains(needle), "{needle} survived: {html}");
    }
    assert!(parsed.remote_content_blocked, "remote pixel reported");
    // Plain https links survive (the UI shows their real target).
    assert!(html.contains(r#"href="https://evil.example/login""#));
    assert!(html.contains("your mailbox is almost full"));
    assert!(!parsed.text.to_ascii_lowercase().contains("document.cookie"));
}

#[test]
fn thread_headers() {
    let first = parse("INBOX", "10-thread-1.eml");
    assert!(first.in_reply_to.is_empty(), "{:?}", first.in_reply_to);
    assert!(first.references.is_empty(), "{:?}", first.references);
    let last = parse("INBOX", "13-thread-7.eml");
    assert_eq!(last.in_reply_to, ["ng-6@nail.example"]);
    assert_eq!(last.references.len(), 6);
    assert_eq!(last.references[0], "ng-1@navy.example");
    let reply = parse("Sent", "01-thread-2.eml");
    assert_eq!(
        reply.preview,
        "Hi Grace, count me in. Should I prepare numbers on the current sync times? Sam"
    );
}

#[test]
fn forwarded_message_is_an_attachment() {
    let parsed = parse("Archive", "02-forwarded-message.eml");
    assert_eq!(parsed.attachments.len(), 1);
    assert_eq!(parsed.attachments[0].mime, "message/rfc822");
    assert!(parsed.has_attachments());
    assert_eq!(parsed.text, "Forwarding the original notes, see attached.");
}

#[test]
fn every_corpus_message_parses_with_subject() {
    for folder in demo_corpus() {
        for message in folder.messages {
            let parsed = parse_message(&message.raw);
            assert!(
                !parsed.headers.subject.is_empty(),
                "{} has a subject",
                message.file_name
            );
            // Truncated input must not panic either.
            for cut in [
                1,
                message.raw.len() / 3,
                message.raw.len() / 2,
                message.raw.len() - 1,
            ] {
                drop(parse_message(&message.raw[..cut]));
            }
        }
    }
}

/// Cheap deterministic fuzzing: flip, drop and duplicate bytes of every
/// corpus message and make sure parsing and sanitizing never panic.
#[test]
fn mutated_corpus_never_panics() {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for folder in demo_corpus() {
        for message in folder.messages {
            for _ in 0..20 {
                let mut raw = message.raw.clone();
                for _ in 0..8 {
                    let pos = usize::try_from(next()).unwrap() % raw.len().max(1);
                    match next() % 3 {
                        0 => raw[pos] = next().to_le_bytes()[0],
                        1 => {
                            raw.remove(pos);
                        }
                        _ => {
                            let byte = raw[pos];
                            raw.insert(pos, byte);
                        }
                    }
                }
                let parsed = parse_message(&raw);
                drop(attachment_content(&raw, 0));
                drop(parsed.to_summary(
                    SummaryIds {
                        id: "m".into(),
                        folder_id: "f".into(),
                        account_id: "a".into(),
                    },
                    Flags::default(),
                    None,
                ));
            }
        }
    }
}

/// Link and attachment safety over the whole demo corpus: no panics, and
/// ordinary mail (newsletters with tracking links, invoices, inline
/// images) raises no warning.
#[test]
fn demo_corpus_raises_no_false_warnings() {
    let mut found = Vec::new();
    for folder in demo_corpus() {
        for message in &folder.messages {
            let body = body(parse_message(&message.raw));
            for warning in nail_mime::links::message_warnings(&body) {
                found.push(format!(
                    "{}/{}: {warning:?}",
                    folder.name, message.file_name
                ));
            }
            // The corpus has no trusted Authentication-Results.
            assert_eq!(
                nail_mime::auth_results::auth_results_of_raw(
                    &message.raw,
                    &["mx.nail.example".to_owned()]
                ),
                None
            );
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

/// A phishing mail end to end: the warnings refer to the links of the
/// sanitized HTML (links the sanitizer removed are not counted).
#[test]
fn phishing_mail_warnings_follow_the_sanitized_html() {
    let raw = "From: Bank <security@bank.example>\r\n\
Subject: Verify your account\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=b\r\n\r\n\
--b\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
<p><a href=\"javascript:alert(1)\">https://bank.example</a>\
<a href=\"https://bank.example.evil.example/login\">https://bank.example/login</a>\
<a href=\"https://xn--pple-43d.example/\">Sign in</a>\
<a href=\"https://www.bank.example/help\">Help</a></p>\r\n\
--b\r\nContent-Type: application/octet-stream\r\n\
Content-Disposition: attachment; filename=\"statement.pdf.exe\"\r\n\r\nMZ\r\n--b--\r\n";
    let body = body(parse_message(raw.as_bytes()));
    let html = body.sanitized_html.as_deref().unwrap();
    let links = nail_mime::links::html_links(html);
    assert!(
        links.iter().all(|l| !l.href.starts_with("javascript")),
        "{html}"
    );
    let warnings = nail_mime::links::message_warnings(&body);
    let kinds: Vec<String> = warnings
        .iter()
        .map(|w| {
            serde_json::to_value(w).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        kinds,
        ["link-mismatch", "lookalike-domain", "dangerous-attachment"],
        "{warnings:#?}"
    );
    // Link numbers index the `<a href>` elements of the sanitized HTML.
    let mismatch = serde_json::to_value(&warnings[0]).unwrap();
    let n = usize::try_from(mismatch["link"].as_u64().unwrap()).unwrap();
    assert_eq!(links[n].href, "https://bank.example.evil.example/login");
}
