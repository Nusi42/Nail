//! Launch requests (M2b; ADRs 0040, 0041): what the desktop app was asked
//! to open when Windows started it as the handler of `mailto:`,
//! `webcal:` / `webcals:` or a `.eml` / `.ics` / `.vcf` file.
//!
//! The desktop shell parses its command line into [`LaunchRequest`]s and
//! hands them to the core; the UI takes them with
//! [`crate::Request::TakeLaunchRequests`] (at start-up and after
//! [`crate::Event::LaunchRequested`]) and acts: a `mailto:` link opens the
//! composer, a `.eml` file is imported and shown
//! ([`crate::Request::ImportMessageFile`]); calendars and contacts answer
//! "not supported yet" until M3.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A `mailto:` link (RFC 6068), decoded. Addresses are not validated here;
/// the composer does that like for typed input.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MailtoLink {
    /// Recipients from the path and every `to=` field.
    pub to: Vec<String>,
    /// `cc=` recipients.
    pub cc: Vec<String>,
    /// `bcc=` recipients (the composer must show them).
    pub bcc: Vec<String>,
    /// `subject=`.
    pub subject: Option<String>,
    /// `body=` (plain text; RFC 6068 line breaks are `%0D%0A`).
    pub body: Option<String>,
    /// `in-reply-to=` (a Message-ID).
    pub in_reply_to: Option<String>,
}

/// One thing the app was asked to open at launch (tagged by `type`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum LaunchRequest {
    /// `mailto:`: open the composer with these fields.
    Compose {
        /// The decoded link.
        mailto: MailtoLink,
    },
    /// `webcal:` / `webcals:`: offer to subscribe to a calendar (M3).
    SubscribeCalendar {
        /// The link as given.
        url: String,
        /// The `https:` URL to fetch.
        https_url: String,
    },
    /// A `.eml` file: import and show the message.
    OpenMessageFile {
        /// Absolute path as Windows passed it.
        path: String,
    },
    /// A `.ics` file: offer to import the events (M3).
    ImportCalendarFile {
        /// Absolute path as Windows passed it.
        path: String,
    },
    /// A `.vcf` file: offer to import the contacts (M3).
    ImportContactsFile {
        /// Absolute path as Windows passed it.
        path: String,
    },
}

/// The local account that files opened in Nail (`.eml`) are imported into.
pub const OPENED_FILES_ACCOUNT_ID: &str = "opened-files";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wire_shapes() {
        let compose = LaunchRequest::Compose {
            mailto: MailtoLink {
                to: vec!["a@x.example".into()],
                subject: Some("Hi".into()),
                ..MailtoLink::default()
            },
        };
        assert_eq!(
            serde_json::to_value(&compose).unwrap(),
            json!({
                "type": "Compose",
                "mailto": {
                    "to": ["a@x.example"], "cc": [], "bcc": [],
                    "subject": "Hi", "body": null, "in_reply_to": null
                }
            })
        );
        let file = LaunchRequest::OpenMessageFile {
            path: r"C:\mail\a.eml".into(),
        };
        let value = serde_json::to_value(&file).unwrap();
        assert_eq!(value["type"], "OpenMessageFile");
        assert_eq!(
            serde_json::from_value::<LaunchRequest>(value).unwrap(),
            file
        );
    }
}
