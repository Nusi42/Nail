//! Link and attachment safety → [`nail_api::security::MessageWarning`]
//! (M2a WP33, ADR 0032).
//!
//! [`message_warnings`] looks at a reader body and returns warnings; it
//! never changes the body. Links are numbered zero-based in document order
//! among the `<a href>` elements of the sanitized HTML, or among the URLs
//! of the text body for text-only mail (the numbering the UI uses to put a
//! warning on the right link).
//!
//! * **`link-mismatch`**: the visible text of an HTML link is a URL or a
//!   domain whose organizational domain differs from the target's
//!   (`<a href="https://evil.example">https://bank.example</a>`). Text that
//!   is not an address (`Click here`) is not compared.
//! * **`lookalike-domain`**: the target host is an internationalized
//!   domain (`xn--…`) that mixes scripts (`аpple.com` with a Cyrillic `а`)
//!   or whose UTS #39 skeleton is plain ASCII (a whole-script look-alike
//!   such as all-Cyrillic `раураӏ.com`); `resembles` names the imitated
//!   ASCII domain.
//! * **`suspicious-link`**: a raw IP address as host, credentials in the
//!   URL (`https://bank.example@evil.example/`), or a scheme that reaches
//!   the local network or file system (`file:`, `smb:`; only possible in
//!   text mail, the sanitizer keeps http / https / mailto / tel).
//! * **`dangerous-attachment`**: executables and scripts, Office macro
//!   formats, disguised names (double extensions, padding before the
//!   extension, bidirectional control characters), disk images, shortcut
//!   files, and a declared type that contradicts the name. One warning per
//!   attachment, the most severe reason.
//!
//! One link gets at most one warning, in the order look-alike, suspicious,
//! mismatch.

use nail_api::security::{AttachmentRisk, MessageWarning};
use nail_api::{AttachmentInfo, MessageBody};
use unicode_security::MixedScript;
use url::{Host, Url};

use crate::domain;

/// A link of an HTML body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlLink {
    /// The `href`, entities decoded.
    pub href: String,
    /// The visible text (tags removed, entities decoded, whitespace
    /// collapsed).
    pub text: String,
}

/// Decodes the entities the HTML serializer writes (`&amp;` `&lt;` `&gt;`
/// `&quot;` `&nbsp;` and numeric ones); unknown ones stay as written.
fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        let Some(end) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        if let Some(c) = decoded {
            out.push(c);
            rest = &rest[end + 1..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

/// The value of attribute `name` in the inside of a start tag
/// (`a href="…" rel="…"`), as the serializer writes it (double quotes).
fn attribute(tag: &str, name: &str) -> Option<String> {
    // ASCII lowercasing keeps byte offsets, so positions found in `lower`
    // index `tag` too.
    let lower = tag.to_ascii_lowercase();
    let mut offset = 0;
    while let Some(found) = lower[offset..].find(name) {
        let pos = offset + found;
        let before = tag[..pos].chars().next_back();
        let after = &tag[pos + name.len()..];
        if before.is_some_and(char::is_whitespace)
            && let Some(value) = after.trim_start().strip_prefix('=')
        {
            let value = value.trim_start();
            let (quote, body) = match value.chars().next() {
                Some(q @ ('"' | '\'')) => (Some(q), &value[1..]),
                _ => (None, value),
            };
            let end = match quote {
                Some(q) => body.find(q).unwrap_or(body.len()),
                None => body
                    .find(|c: char| c.is_whitespace() || c == '>')
                    .unwrap_or(body.len()),
            };
            return Some(decode_entities(&body[..end]));
        }
        offset = pos + name.len();
    }
    None
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The `<a href>` links of sanitized HTML in document order.
pub fn html_links(html: &str) -> Vec<HtmlLink> {
    let mut out = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut pos = 0;
    while let Some(found) = lower[pos..].find("<a") {
        let start = pos + found;
        let after = lower.as_bytes().get(start + 2).copied();
        if !matches!(after, Some(b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/')) {
            pos = start + 2;
            continue;
        }
        let Some(tag_end) = lower[start..].find('>').map(|e| start + e) else {
            break;
        };
        let tag = &html[start + 2..tag_end];
        let close = lower[tag_end..]
            .find("</a")
            .map_or(html.len(), |e| tag_end + e);
        pos = close.max(tag_end + 1);
        let Some(href) = attribute(&format!(" {tag}"), "href") else {
            continue;
        };
        let inner = &html[tag_end + 1..close];
        // Remove tags from the link text.
        let mut text = String::new();
        let mut in_tag = false;
        for c in inner.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                c if !in_tag => text.push(c),
                _ => {}
            }
        }
        out.push(HtmlLink {
            href,
            text: collapse_whitespace(&decode_entities(&text)),
        });
    }
    out
}

/// Schemes recognized as links in text mail.
const TEXT_SCHEMES: &[&str] = &["https://", "http://", "ftp://", "file://", "smb://"];

/// The URLs of a text body in order (`scheme://…` and `www.…`), with
/// trailing punctuation removed.
pub fn text_links(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < text.len() {
        let next = TEXT_SCHEMES
            .iter()
            .chain(std::iter::once(&"www."))
            .filter_map(|s| lower[pos..].find(s).map(|i| pos + i))
            .filter(|&i| {
                // At a word start.
                text[..i]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !c.is_alphanumeric() && !matches!(c, '.' | '/' | '@' | '-'))
            })
            .min();
        let Some(start) = next else { break };
        let end = text[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\''))
            .map_or(text.len(), |e| start + e);
        let url = text[start..end].trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}']);
        if url.len() > 4 && !TEXT_SCHEMES.iter().any(|s| url.eq_ignore_ascii_case(s)) {
            out.push(if url.to_ascii_lowercase().starts_with("www.") {
                format!("http://{url}")
            } else {
                url.to_owned()
            });
        }
        pos = end.max(start + 1);
    }
    out
}

/// The host a link text shows, if the text is (or starts with) a URL or a
/// domain name.
fn shown_host(text: &str) -> Option<String> {
    let text = text.trim().trim_matches(|c| c == '<' || c == '>');
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("mailto:") || lower.starts_with("tel:") {
        return None;
    }
    if lower.contains("://") {
        return Url::parse(text)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .and_then(|h| domain::normalize_host(&h));
    }
    // A bare domain, optionally with a path: `www.bank.example/login`.
    let host = text.split(['/', '?', '#']).next()?;
    if host.contains('@') || !host.contains('.') {
        return None;
    }
    let labels: Vec<&str> = host.split('.').collect();
    let tld = labels.last()?;
    let valid = labels
        .iter()
        .all(|l| !l.is_empty() && l.chars().all(|c| c.is_alphanumeric() || c == '-'))
        && tld.chars().all(char::is_alphabetic)
        && tld.chars().count() >= 2;
    if !valid {
        return None;
    }
    domain::normalize_host(host)
}

/// A look-alike check of an ASCII (punycode) host: `Some((unicode,
/// resembles))` when it should be flagged.
pub(crate) fn lookalike(ascii_host: &str) -> Option<(String, Option<String>)> {
    if !ascii_host
        .split('.')
        .any(|l| l.to_ascii_lowercase().starts_with("xn--"))
    {
        return None;
    }
    let (unicode, result) = idna::domain_to_unicode(ascii_host);
    if result.is_err() {
        // Invalid punycode is suspicious in itself.
        return Some((unicode, None));
    }
    let mut flagged = false;
    let mut skeleton_labels = Vec::new();
    for label in unicode.split('.') {
        let skeleton: String = unicode_security::skeleton(label).collect();
        if label.is_ascii() {
            skeleton_labels.push(label.to_owned());
            continue;
        }
        if !label.is_single_script() {
            flagged = true;
        }
        if skeleton.is_ascii() && skeleton != label {
            flagged = true;
        }
        skeleton_labels.push(skeleton);
    }
    if !flagged {
        return None;
    }
    let resembles = skeleton_labels.join(".");
    let resembles = (resembles.is_ascii() && resembles != ascii_host).then_some(resembles);
    Some((unicode, resembles))
}

fn is_ip_host(url: &Url) -> bool {
    matches!(url.host(), Some(Host::Ipv4(_) | Host::Ipv6(_)))
}

/// The warning for one link, if any.
fn link_warning(index: u32, href: &str, text: Option<&str>) -> Option<MessageWarning> {
    let url = Url::parse(href.trim()).ok()?;
    let scheme = url.scheme();
    if matches!(scheme, "mailto" | "tel") {
        return None;
    }
    let href = href.trim().to_owned();
    if !matches!(scheme, "http" | "https" | "ftp") {
        return Some(MessageWarning::SuspiciousLink { link: index, href });
    }
    let host = url.host_str()?.to_ascii_lowercase();
    if let Some((unicode_host, resembles)) = lookalike(&host) {
        return Some(MessageWarning::LookalikeDomain {
            link: index,
            href,
            ascii_host: host,
            unicode_host,
            resembles,
        });
    }
    if is_ip_host(&url) || !url.username().is_empty() || url.password().is_some() {
        return Some(MessageWarning::SuspiciousLink { link: index, href });
    }
    let shown = text.and_then(shown_host)?;
    if domain::aligned(&shown, &host) {
        return None;
    }
    Some(MessageWarning::LinkMismatch {
        link: index,
        href,
        shown_host: shown,
        target_host: host,
    })
}

/// Link warnings of a body: the HTML links when there is HTML, else the
/// URLs of the text.
pub fn link_warnings(sanitized_html: Option<&str>, text: &str) -> Vec<MessageWarning> {
    match sanitized_html {
        Some(html) => html_links(html)
            .iter()
            .enumerate()
            .filter_map(|(i, link)| {
                link_warning(u32::try_from(i).ok()?, &link.href, Some(&link.text))
            })
            .collect(),
        None => text_links(text)
            .iter()
            .enumerate()
            .filter_map(|(i, url)| link_warning(u32::try_from(i).ok()?, url, None))
            .collect(),
    }
}

/// Programs, scripts and installers Windows runs on a double click.
const EXECUTABLE: &[&str] = &[
    "exe",
    "com",
    "scr",
    "pif",
    "bat",
    "cmd",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "wsc",
    "ps1",
    "psm1",
    "psd1",
    "ps1xml",
    "psc1",
    "msi",
    "msp",
    "mst",
    "msix",
    "msixbundle",
    "appx",
    "appxbundle",
    "appinstaller",
    "jar",
    "hta",
    "cpl",
    "reg",
    "dll",
    "ocx",
    "sys",
    "drv",
    "application",
    "gadget",
    "msc",
    "inf",
    "sct",
    "chm",
    "xll",
    "xla",
    "vb",
    "vbscript",
    "ws",
    "jnlp",
    "sh",
    "py",
    "pyw",
    "rb",
    "pl",
    "elf",
    "apk",
    "dmg",
    "pkg",
    "app",
    "workflow",
    "action",
    "command",
];

/// Office formats that may carry macros.
const MACRO: &[&str] = &[
    "docm", "dotm", "xlsm", "xltm", "xlam", "pptm", "potm", "ppam", "ppsm", "sldm", "xlsb",
];

/// Disk images and containers that hide their content from scanners and
/// from Mark-of-the-Web.
const CONTAINER: &[&str] = &["iso", "img", "vhd", "vhdx", "udf", "wim"];

/// Shortcuts and link files.
const SHORTCUT: &[&str] = &[
    "lnk",
    "url",
    "scf",
    "website",
    "webloc",
    "desktop",
    "library-ms",
    "searchconnector-ms",
    "settingcontent-ms",
];

/// Extensions a disguise imitates (documents, pictures, media).
const DOCUMENT_LIKE: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "rtf", "txt", "csv", "jpg",
    "jpeg", "png", "gif", "bmp", "tif", "tiff", "mp3", "mp4", "mov", "avi", "wav", "zip", "html",
    "htm", "eml",
];

/// Declared types that mean "program".
const EXECUTABLE_TYPES: &[&str] = &[
    "application/x-msdownload",
    "application/x-dosexec",
    "application/x-msdos-program",
    "application/x-ms-installer",
    "application/x-msi",
    "application/x-executable",
    "application/vnd.microsoft.portable-executable",
    "application/hta",
    "application/x-sh",
    "application/java-archive",
];

/// Expected MIME type prefixes of common harmless extensions.
const EXPECTED_TYPES: &[(&str, &str)] = &[
    ("pdf", "application/pdf"),
    ("jpg", "image/"),
    ("jpeg", "image/"),
    ("png", "image/"),
    ("gif", "image/"),
    ("txt", "text/"),
    ("csv", "text/"),
    ("mp3", "audio/"),
    ("wav", "audio/"),
    ("mp4", "video/"),
];

/// Characters that reorder text on screen or hide characters
/// (`invoice\u{202E}fdp.exe` shows as `invoiceexe.pdf`).
fn has_bidi_control(name: &str) -> bool {
    name.chars().any(|c| {
        matches!(
            c,
            '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}'
        )
    })
}

fn extension(name: &str) -> Option<String> {
    let (_, ext) = name.rsplit_once('.')?;
    let ext = ext.trim().to_ascii_lowercase();
    (!ext.is_empty()).then_some(ext)
}

/// The risk of one attachment, if any.
pub fn attachment_risk(filename: &str, mime: &str) -> Option<AttachmentRisk> {
    let name = filename.trim_end_matches(['.', ' ']);
    if has_bidi_control(name) {
        return Some(AttachmentRisk::DisguisedName);
    }
    let ext = extension(name)?;
    let risky = EXECUTABLE.contains(&ext.as_str())
        || SHORTCUT.contains(&ext.as_str())
        || MACRO.contains(&ext.as_str())
        || CONTAINER.contains(&ext.as_str());
    if risky {
        let stem = &name[..name.len() - ext.len() - 1];
        let inner = extension(stem);
        let padded = stem.ends_with("   ") || stem.ends_with('\u{a0}');
        if padded || inner.is_some_and(|e| DOCUMENT_LIKE.contains(&e.as_str())) {
            return Some(AttachmentRisk::DisguisedName);
        }
    }
    if EXECUTABLE.contains(&ext.as_str()) {
        return Some(AttachmentRisk::Executable);
    }
    if SHORTCUT.contains(&ext.as_str()) {
        return Some(AttachmentRisk::Shortcut);
    }
    if MACRO.contains(&ext.as_str()) {
        return Some(AttachmentRisk::Macro);
    }
    if CONTAINER.contains(&ext.as_str()) {
        return Some(AttachmentRisk::Container);
    }
    let mime = mime.trim().to_ascii_lowercase();
    let mime = mime.split(';').next().unwrap_or_default().trim();
    if EXECUTABLE_TYPES.contains(&mime) {
        return Some(AttachmentRisk::TypeMismatch);
    }
    if let Some((_, expected)) = EXPECTED_TYPES.iter().find(|(e, _)| *e == ext)
        && !mime.is_empty()
        && mime != "application/octet-stream"
        && !mime.starts_with(expected)
    {
        return Some(AttachmentRisk::TypeMismatch);
    }
    None
}

/// Attachment warnings.
pub fn attachment_warnings(attachments: &[AttachmentInfo]) -> Vec<MessageWarning> {
    attachments
        .iter()
        .filter_map(|a| {
            let filename = a.filename.as_deref()?;
            attachment_risk(filename, &a.mime).map(|risk| MessageWarning::DangerousAttachment {
                index: a.index,
                filename: filename.to_owned(),
                risk,
            })
        })
        .collect()
}

/// All warnings of a reader body: a disguised sender first (ADR 0195),
/// then links (in link order), then attachments.
pub fn message_warnings(body: &MessageBody) -> Vec<MessageWarning> {
    let mut out = crate::sender::sender_warnings(&body.headers.from);
    out.extend(link_warnings(body.sanitized_html.as_deref(), &body.text));
    out.extend(attachment_warnings(&body.attachments));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn idna_and_confusable_detection_work() {
        let (unicode, result) = idna::domain_to_unicode("xn--pple-43d.com");
        assert!(result.is_ok());
        assert_eq!(unicode, "\u{430}pple.com");
        assert!(!"p\u{430}ypal".is_single_script());
    }

    #[test]
    fn extracts_html_links_in_order_with_text() {
        let html = "<p>Hi <a href=\"https://a.example/x?y=1&amp;z=2\" rel=\"noopener noreferrer\">\
                    <b>Your</b>  account</a></p><a name=\"top\">anchor</a>\
                    <A HREF=\"http://b.example\">b&nbsp;&#x41;</A><abbr>x</abbr><a href='c'>c</a>";
        let links = html_links(html);
        assert_eq!(
            links,
            vec![
                HtmlLink {
                    href: "https://a.example/x?y=1&z=2".into(),
                    text: "Your account".into()
                },
                HtmlLink {
                    href: "http://b.example".into(),
                    text: "b A".into()
                },
                HtmlLink {
                    href: "c".into(),
                    text: "c".into()
                },
            ]
        );
        assert_eq!(html_links("<a href=\"x\"").len(), 0);
        assert_eq!(
            decode_entities("&bogus; & &#9999999999;"),
            "&bogus; & &#9999999999;"
        );
    }

    #[test]
    fn extracts_text_links() {
        let text = "See https://a.example/path, or (www.b.example). Mail me: x@www.c.example \
                    file://server/share and http:// nothing; notwww.d.example";
        assert_eq!(
            text_links(text),
            [
                "https://a.example/path",
                "http://www.b.example",
                "file://server/share"
            ]
        );
    }

    fn html_warnings(html: &str) -> Vec<MessageWarning> {
        link_warnings(Some(html), "")
    }

    #[test]
    fn display_target_mismatch() {
        let w = html_warnings(
            "<a href=\"https://login.evil.example/bank\">https://www.bank.example/login</a>\
             <a href=\"https://www.bank.example/x\">bank.example</a>\
             <a href=\"https://click.mailer.example/t/1\">Click here</a>\
             <a href=\"https://evil.example\">WWW.BANK.EXAMPLE/offers</a>",
        );
        assert_eq!(
            w,
            vec![
                MessageWarning::LinkMismatch {
                    link: 0,
                    href: "https://login.evil.example/bank".into(),
                    shown_host: "www.bank.example".into(),
                    target_host: "login.evil.example".into(),
                },
                MessageWarning::LinkMismatch {
                    link: 3,
                    href: "https://evil.example".into(),
                    shown_host: "www.bank.example".into(),
                    target_host: "evil.example".into(),
                },
            ]
        );
    }

    #[test]
    fn punycode_lookalikes() {
        // Mixed Latin + Cyrillic.
        let w = html_warnings("<a href=\"https://xn--pple-43d.com/login\">Apple</a>");
        assert_eq!(
            w,
            vec![MessageWarning::LookalikeDomain {
                link: 0,
                href: "https://xn--pple-43d.com/login".into(),
                ascii_host: "xn--pple-43d.com".into(),
                unicode_host: "\u{430}pple.com".into(),
                resembles: Some("apple.com".into()),
            }]
        );
        // A Unicode href is converted to punycode by the URL parser.
        let w = html_warnings("<a href=\"https://p\u{430}ypal.com\">x</a>");
        assert!(matches!(
            &w[0],
            MessageWarning::LookalikeDomain { resembles: Some(r), .. } if r == "paypal.com"
        ));
        // Whole-script Cyrillic look-alike of "paypal".
        let w = link_warnings(
            None,
            "https://\u{440}\u{430}\u{443}\u{440}\u{430}\u{04CF}.com",
        );
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(matches!(&w[0], MessageWarning::LookalikeDomain { .. }));
        // Legitimate IDNs are fine.
        for ok in [
            "https://m\u{fc}nchen.de",
            "https://\u{43f}\u{440}\u{438}\u{43c}\u{435}\u{440}.\u{440}\u{444}",
            "https://b\u{fc}cher.example",
        ] {
            assert!(link_warnings(None, ok).is_empty(), "{ok}");
        }
    }

    #[test]
    fn suspicious_links() {
        let w = link_warnings(
            None,
            "http://192.168.1.10/admin https://[::1]/x https://bank.example@evil.example/ \
             file://attacker/share smb://host/x https://fine.example",
        );
        let links: Vec<u32> = w
            .iter()
            .map(|w| match w {
                MessageWarning::SuspiciousLink { link, .. } => *link,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(links, [0, 1, 2, 3, 4]);
        // mailto / tel links are not links for these checks.
        assert_eq!(
            html_warnings("<a href=\"mailto:a@evil.example\">a@bank.example</a>").len(),
            0
        );
        assert_eq!(html_warnings("<a href=\"not a url\">x</a>").len(), 0);
    }

    #[test]
    fn attachment_risks() {
        use AttachmentRisk as R;
        for (name, mime, risk) in [
            ("setup.exe", "application/octet-stream", Some(R::Executable)),
            ("RUN.PS1", "text/plain", Some(R::Executable)),
            ("invoice.pdf.exe", "application/pdf", Some(R::DisguisedName)),
            ("invoice.pdf          .scr", "", Some(R::DisguisedName)),
            ("photo\u{202E}gpj.exe", "image/jpeg", Some(R::DisguisedName)),
            ("readme\u{202E}txt", "text/plain", Some(R::DisguisedName)),
            ("budget.xlsm", "application/vnd.ms-excel", Some(R::Macro)),
            ("budget.xlsx.docm", "", Some(R::DisguisedName)),
            ("backup.iso", "application/octet-stream", Some(R::Container)),
            ("Report.lnk", "", Some(R::Shortcut)),
            ("login.url", "", Some(R::Shortcut)),
            (
                "notes.pdf",
                "application/x-msdownload",
                Some(R::TypeMismatch),
            ),
            ("notes.pdf", "image/png", Some(R::TypeMismatch)),
            ("notes.pdf", "application/pdf", None),
            ("notes.pdf", "application/octet-stream", None),
            ("photo.JPG", "image/jpeg; name=photo.JPG", None),
            ("archive.tar.gz", "application/gzip", None),
            ("Makefile", "text/plain", None),
            ("trailing.exe.", "", Some(R::Executable)),
        ] {
            assert_eq!(attachment_risk(name, mime), risk, "{name:?} {mime:?}");
        }
    }

    #[test]
    fn message_warnings_combine_links_and_attachments() {
        let body = MessageBody {
            id: "m1".into(),
            headers: nail_api::MessageHeaders::default(),
            text: "ignored when there is HTML: https://1.2.3.4".into(),
            sanitized_html: Some("<a href=\"https://evil.example\">bank.example</a>".into()),
            reader_html: None,
            attachments: vec![
                AttachmentInfo {
                    index: 0,
                    filename: Some("scan.pdf".into()),
                    mime: "application/pdf".into(),
                    size: 10,
                    inline: false,
                    shown_in_body: false,
                },
                AttachmentInfo {
                    index: 1,
                    filename: Some("scan.pdf.js".into()),
                    mime: "application/javascript".into(),
                    size: 10,
                    inline: false,
                    shown_in_body: false,
                },
                AttachmentInfo {
                    index: 2,
                    filename: None,
                    mime: "application/x-msdownload".into(),
                    size: 10,
                    inline: false,
                    shown_in_body: false,
                },
            ],
            remote_content_blocked: false,
            security: None,
            auth_results: None,
            warnings: Vec::new(),
            remote_content: None,
            render_url: None,
            junk: None,
            invitation: None,
        };
        let w = message_warnings(&body);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(matches!(w[0], MessageWarning::LinkMismatch { link: 0, .. }));
        assert_eq!(
            w[1],
            MessageWarning::DangerousAttachment {
                index: 1,
                filename: "scan.pdf.js".into(),
                risk: AttachmentRisk::DisguisedName,
            }
        );
    }
}
