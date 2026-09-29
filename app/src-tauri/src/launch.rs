//! Launch targets: what Windows hands the app when it is started as the
//! handler of a URL scheme or a file type (plan M2b WP44).
//!
//! The installer registers Nail for `mailto:`, `webcal:` / `webcals:` and
//! the `.eml`, `.ics` and `.vcf` file types (`installer/hooks.nsh`); Windows
//! then starts `Nail.exe "<url or path>"`. [`LaunchRequest::parse`] turns
//! such an argument into a typed request. The shell keeps the requests of
//! its own start in [`PendingLaunch`] until the UI takes them
//! (`nail_launch_requests`) and emits later ones on [`LAUNCH_CHANNEL`].
//!
//! What each request does (compose, subscribe, import) is decided by the
//! integrator (M2b WP45), which maps these types onto `nail_api` requests.
//! Nothing here opens a file or fetches a URL.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;

/// Tauri event channel for launch requests that arrive while the app runs.
pub const LAUNCH_CHANNEL: &str = "nail://launch";

/// URL schemes the installer registers, with the request kind they map to.
pub const URL_SCHEMES: [&str; 3] = ["mailto", "webcal", "webcals"];

/// File extensions (without the dot) the installer registers.
pub const FILE_EXTENSIONS: [&str; 3] = ["eml", "ics", "vcf"];

/// Longest launch argument accepted. `mailto:` links from web pages can be
/// long (a body), but nothing legitimate comes close to this.
pub const MAX_ARGUMENT_BYTES: usize = 64 * 1024;

/// A `mailto:` link (RFC 6068), decoded. Addresses are not validated here;
/// the composer does that like for typed input.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailtoLink {
    /// Recipients from the path and every `to=` field.
    pub to: Vec<String>,
    /// `cc=` recipients.
    pub cc: Vec<String>,
    /// `bcc=` recipients (the composer must show them).
    pub bcc: Vec<String>,
    /// `subject=`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// `body=` (plain text; RFC 6068 line breaks are `%0D%0A`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// `in-reply-to=` (a Message-ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<String>,
}

/// One thing the app was asked to open at launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LaunchRequest {
    /// `mailto:`: open the composer with these fields.
    Compose {
        /// The decoded link.
        mailto: MailtoLink,
    },
    /// `webcal:` / `webcals:`: offer to subscribe to a calendar.
    SubscribeCalendar {
        /// The link as given.
        url: String,
        /// The `https:` URL to fetch (both schemes are fetched over HTTPS;
        /// plain HTTP only after the user agrees, decided by the importer).
        https_url: String,
    },
    /// A `.eml` file: show the message.
    OpenMessageFile {
        /// Absolute path as Windows passed it.
        path: PathBuf,
    },
    /// A `.ics` file: offer to import the events.
    ImportCalendarFile {
        /// Absolute path as Windows passed it.
        path: PathBuf,
    },
    /// A `.vcf` file: offer to import the contacts.
    ImportContactsFile {
        /// Absolute path as Windows passed it.
        path: PathBuf,
    },
}

impl LaunchRequest {
    /// Parses one launch argument; `None` for anything that is not a
    /// registered scheme or file type (or is malformed / too long).
    pub fn parse(argument: &str) -> Option<Self> {
        let argument = argument.trim();
        if argument.is_empty() || argument.len() > MAX_ARGUMENT_BYTES {
            return None;
        }
        if let Some((scheme, rest)) = split_scheme(argument) {
            return match scheme.as_str() {
                "mailto" => Some(Self::Compose {
                    mailto: parse_mailto(rest),
                }),
                "webcal" | "webcals" => parse_webcal(argument, rest),
                _ => None,
            };
        }
        let path = Path::new(argument);
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        let path = path.to_path_buf();
        match extension.as_str() {
            "eml" => Some(Self::OpenMessageFile { path }),
            "ics" => Some(Self::ImportCalendarFile { path }),
            "vcf" => Some(Self::ImportContactsFile { path }),
            _ => None,
        }
    }

    /// Every launch request among `arguments` (the program name excluded),
    /// in order. Flags and unknown arguments are skipped, so this also works
    /// on a command line that clap rejected.
    pub fn from_args<I, S>(arguments: I) -> Vec<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        arguments
            .into_iter()
            .filter(|a| !a.as_ref().starts_with('-'))
            .filter_map(|a| Self::parse(a.as_ref()))
            .collect()
    }
}

/// Splits `scheme:rest` for URL-like arguments. A drive letter (`C:\…`) is
/// not a scheme: schemes are at least two characters long.
fn split_scheme(argument: &str) -> Option<(String, &str)> {
    let (scheme, rest) = argument.split_once(':')?;
    let valid = scheme.len() >= 2
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid.then(|| (scheme.to_ascii_lowercase(), rest))
}

/// RFC 6068: `mailto:[addr,…][?hfname=hfvalue&…]`, percent-encoded UTF-8.
/// Unknown header fields are ignored, like `attach=`, `from=` and other
/// fields a web page must not be able to set.
fn parse_mailto(rest: &str) -> MailtoLink {
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut link = MailtoLink {
        to: split_addresses(&percent_decode(path)),
        ..MailtoLink::default()
    };
    for field in query.split('&').filter(|f| !f.is_empty()) {
        let (name, value) = field.split_once('=').unwrap_or((field, ""));
        let value = percent_decode(value);
        match percent_decode(name).to_ascii_lowercase().as_str() {
            "to" => link.to.extend(split_addresses(&value)),
            "cc" => link.cc.extend(split_addresses(&value)),
            "bcc" => link.bcc.extend(split_addresses(&value)),
            "subject" => link.subject = Some(value),
            "body" => link.body = Some(value),
            "in-reply-to" => link.in_reply_to = Some(value),
            _ => {}
        }
    }
    link
}

fn split_addresses(list: &str) -> Vec<String> {
    list.split(',')
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `webcal://host/path` → `https://host/path`. Needs a host.
fn parse_webcal(argument: &str, rest: &str) -> Option<LaunchRequest> {
    let after = rest.strip_prefix("//")?;
    let host = after.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains(char::is_whitespace) {
        return None;
    }
    Some(LaunchRequest::SubscribeCalendar {
        url: argument.to_owned(),
        https_url: format!("https://{after}"),
    })
}

/// Decodes `%XX` escapes as UTF-8 (invalid sequences become U+FFFD).
/// `+` stays a plus sign: RFC 6068 is not form encoding.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(hi), Some(lo)) = (
                bytes.get(i + 1).and_then(|b| (*b as char).to_digit(16)),
                bytes.get(i + 2).and_then(|b| (*b as char).to_digit(16)),
            )
        {
            // Two hex digits make one byte.
            out.push(u8::try_from(hi * 16 + lo).unwrap_or(b'?'));
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Launch requests the UI has not taken yet (Tauri state).
#[derive(Debug, Default)]
pub struct PendingLaunch(Mutex<Vec<LaunchRequest>>);

impl PendingLaunch {
    /// A queue holding `requests`.
    pub fn new(requests: Vec<LaunchRequest>) -> Self {
        Self(Mutex::new(requests))
    }

    /// Adds requests (e.g. from a second start, once single-instance
    /// forwarding exists).
    pub fn push(&self, requests: impl IntoIterator<Item = LaunchRequest>) {
        self.lock().extend(requests);
    }

    /// Takes every pending request; each is handed out once.
    pub fn take(&self) -> Vec<LaunchRequest> {
        std::mem::take(&mut *self.lock())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<LaunchRequest>> {
        // A panic while holding the lock cannot leave the Vec inconsistent.
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn mailto(argument: &str) -> MailtoLink {
        match LaunchRequest::parse(argument) {
            Some(LaunchRequest::Compose { mailto }) => mailto,
            other => panic!("{argument}: {other:?}"),
        }
    }

    #[test]
    fn mailto_with_all_fields() {
        let link = mailto(
            "mailto:ada@example.org,bob@example.org?cc=carol@example.org&bcc=dan@example.org\
             &subject=Hello%20there&body=Line%201%0D%0ALine%202&In-Reply-To=%3Cid%40x%3E",
        );
        assert_eq!(
            link,
            MailtoLink {
                to: vec!["ada@example.org".into(), "bob@example.org".into()],
                cc: vec!["carol@example.org".into()],
                bcc: vec!["dan@example.org".into()],
                subject: Some("Hello there".into()),
                body: Some("Line 1\r\nLine 2".into()),
                in_reply_to: Some("<id@x>".into()),
            }
        );
    }

    #[test]
    fn mailto_decodes_utf8_and_keeps_plus_signs() {
        let link = mailto("MAILTO:a+tag@example.org?subject=Gr%C3%BC%C3%9Fe+1&to=b%40example.org");
        assert_eq!(link.to, ["a+tag@example.org", "b@example.org"]);
        assert_eq!(link.subject.as_deref(), Some("Grüße+1"));
    }

    #[test]
    fn mailto_edge_cases() {
        // No address at all: an empty composer.
        assert_eq!(mailto("mailto:"), MailtoLink::default());
        assert_eq!(mailto("mailto:?subject=x").subject.as_deref(), Some("x"));
        // Broken escapes and invalid UTF-8 do not fail the whole link.
        assert_eq!(
            mailto("mailto:a@x?subject=100%&body=%FF%zz")
                .subject
                .as_deref(),
            Some("100%")
        );
        assert_eq!(
            mailto("mailto:a@x?body=%FF%zz").body.as_deref(),
            Some("\u{FFFD}%zz")
        );
    }

    #[test]
    fn mailto_ignores_fields_a_page_must_not_set() {
        let link = mailto("mailto:a@x?attach=C:%5Csecret.txt&from=boss@x&X-Header=1&subject=s");
        assert_eq!(
            link,
            MailtoLink {
                to: vec!["a@x".into()],
                subject: Some("s".into()),
                ..MailtoLink::default()
            }
        );
    }

    #[test]
    fn webcal_links_become_https() {
        assert_eq!(
            LaunchRequest::parse("webcal://example.org/cal.ics?x=1"),
            Some(LaunchRequest::SubscribeCalendar {
                url: "webcal://example.org/cal.ics?x=1".into(),
                https_url: "https://example.org/cal.ics?x=1".into(),
            })
        );
        assert!(matches!(
            LaunchRequest::parse("WEBCALS://example.org/a"),
            Some(LaunchRequest::SubscribeCalendar { https_url, .. }) if https_url == "https://example.org/a"
        ));
        for bad in [
            "webcal:",
            "webcal:example.org",
            "webcal:///path",
            "webcal://a b/",
        ] {
            assert_eq!(LaunchRequest::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn files_by_extension() {
        assert_eq!(
            LaunchRequest::parse(r"C:\Users\ada\Downloads\Offer.EML"),
            Some(LaunchRequest::OpenMessageFile {
                path: r"C:\Users\ada\Downloads\Offer.EML".into()
            })
        );
        assert!(matches!(
            LaunchRequest::parse(r"D:\cal\team.ics"),
            Some(LaunchRequest::ImportCalendarFile { .. })
        ));
        assert!(matches!(
            LaunchRequest::parse(r"\\server\share\people.vcf"),
            Some(LaunchRequest::ImportContactsFile { .. })
        ));
        for other in [
            r"C:\x\setup.exe",
            r"C:\x\noext",
            "https://example.org/a.eml",
            "x:",
        ] {
            assert_eq!(LaunchRequest::parse(other), None, "{other}");
        }
    }

    #[test]
    fn every_registered_scheme_and_extension_is_understood() {
        for scheme in URL_SCHEMES {
            let argument = if scheme == "mailto" {
                "mailto:a@x".to_owned()
            } else {
                format!("{scheme}://example.org/c.ics")
            };
            assert!(LaunchRequest::parse(&argument).is_some(), "{scheme}");
        }
        for extension in FILE_EXTENSIONS {
            assert!(
                LaunchRequest::parse(&format!(r"C:\f.{extension}")).is_some(),
                "{extension}"
            );
        }
    }

    #[test]
    fn oversized_and_empty_arguments_are_ignored() {
        let long = format!("mailto:a@x?body={}", "x".repeat(MAX_ARGUMENT_BYTES));
        assert_eq!(LaunchRequest::parse(&long), None);
        assert_eq!(LaunchRequest::parse("   "), None);
    }

    #[test]
    fn from_args_skips_flags_and_unknown_arguments() {
        let requests = LaunchRequest::from_args([
            "--autostart",
            "mailto:a@x",
            "--data-dir",
            "notes.txt",
            r"C:\m.eml",
        ]);
        assert_eq!(requests.len(), 2);
        assert!(matches!(requests[0], LaunchRequest::Compose { .. }));
        assert!(matches!(requests[1], LaunchRequest::OpenMessageFile { .. }));
    }

    #[test]
    fn requests_serialize_for_the_webview() {
        let json =
            serde_json::to_value(LaunchRequest::parse("mailto:a@x?subject=s").unwrap()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "type": "compose",
                "mailto": { "to": ["a@x"], "cc": [], "bcc": [], "subject": "s" }
            })
        );
        let json = serde_json::to_value(LaunchRequest::parse(r"C:\a.ics").unwrap()).unwrap();
        assert_eq!(json["type"], "importCalendarFile");
    }

    #[test]
    fn pending_requests_are_handed_out_once() {
        let pending = PendingLaunch::new(LaunchRequest::from_args(["mailto:a@x"]));
        pending.push(LaunchRequest::from_args([r"C:\b.vcf"]));
        assert_eq!(pending.take().len(), 2);
        assert!(pending.take().is_empty());
    }
}
