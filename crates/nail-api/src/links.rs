//! Following a link the user clicked in a message (ADR 0206).
//!
//! The reader never lets a mail frame navigate: a click on a link (also a
//! middle click or Enter on a focused one) becomes
//! [`crate::Request::OpenLink`], and the core decides what happens:
//!
//! * `http:` / `https:` / `tel:` ([`EXTERNAL_LINK_SCHEMES`]): the desktop
//!   app hands the link to the system (the default browser or phone app)
//!   and answers [`LinkOutcome::Opened`]; without the desktop shell (web
//!   mode, every remote session) the answer is
//!   [`LinkOutcome::OpenInBrowser`] and the UI opens a new browser tab
//!   (`noopener`, `noreferrer`). A remote session never reaches the
//!   desktop's browser.
//! * `mailto:` ([`MAILTO_SCHEME`]): [`LinkOutcome::Compose`] with the
//!   decoded link; the UI opens Nail's composer, as for a `mailto:` launch.
//! * Anything else (the sanitizer keeps no other scheme): `invalid_request`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::launch::MailtoLink;

/// Schemes [`crate::Request::OpenLink`] opens outside Nail.
pub const EXTERNAL_LINK_SCHEMES: [&str; 3] = ["http", "https", "tel"];

/// The scheme that opens Nail's composer.
pub const MAILTO_SCHEME: &str = "mailto";

/// Longest link [`crate::Request::OpenLink`] accepts (bytes).
pub const MAX_LINK_BYTES: usize = 64 * 1024;

/// What [`crate::Request::OpenLink`] did, or asks the UI to do (tagged by
/// `type`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum LinkOutcome {
    /// The desktop app handed the link to the system.
    Opened,
    /// Open `url` in a new browser tab (`noopener`, `noreferrer`): web
    /// mode and remote sessions.
    OpenInBrowser {
        /// The link, normalized.
        url: String,
    },
    /// A `mailto:` link: open the composer with these fields.
    Compose {
        /// The decoded link.
        mailto: MailtoLink,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn wire_shapes() {
        assert_eq!(
            serde_json::to_value(LinkOutcome::Opened).unwrap(),
            serde_json::json!({"type": "Opened"})
        );
        assert_eq!(
            serde_json::to_value(LinkOutcome::OpenInBrowser {
                url: "https://example.com/".into()
            })
            .unwrap(),
            serde_json::json!({"type": "OpenInBrowser", "url": "https://example.com/"})
        );
        let compose = LinkOutcome::Compose {
            mailto: MailtoLink {
                to: vec!["a@example.com".into()],
                ..MailtoLink::default()
            },
        };
        let json = serde_json::to_value(&compose).unwrap();
        assert_eq!(json["type"], "Compose");
        assert_eq!(json["mailto"]["to"][0], "a@example.com");
        assert_eq!(
            serde_json::from_value::<LinkOutcome>(json).unwrap(),
            compose
        );
    }
}
