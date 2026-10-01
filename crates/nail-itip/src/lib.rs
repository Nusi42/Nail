//! iTIP (RFC 5546) and iMIP (RFC 6047) for Nail (plan M3b WP81; contract
//! ADR 0065; design `docs/adr/0066-itip.md`).
//!
//! Pure logic, no I/O: the core (WP85) reads the stored event, the
//! scheduling state and the user's identities, calls into this crate and
//! writes the results through the PIM queue and the outbox.
//!
//! * [`object`] — reading iTIP calendar data: method (`METHOD` wins over
//!   the MIME parameter), the event's components with `SEQUENCE`,
//!   `DTSTAMP`, `RECURRENCE-ID`, `COMMENT`, `SENT-BY`, `SCHEDULE-AGENT`;
//!   problems of the message ([`nail_api::scheduling::ItipIssue`]).
//! * [`address`] — calendar user addresses and the user's identities.
//! * [`order`] — `SEQUENCE` / `DTSTAMP` ordering (stale messages are
//!   ignored) and the organizer's significant-change rules.
//! * [`card`] — the invitation card: role (organizer / attendee /
//!   observer) from the user's identities, sender checks (spoofing),
//!   state against the stored copy, the actions the card offers.
//! * [`apply`] — `REQUEST` / `PUBLISH` / `ADD` / `CANCEL` / `REPLY` on
//!   the stored event, per occurrence; the attendee's own answer,
//!   delegation, accepting a `COUNTER`.
//! * [`build`] — outgoing `REQUEST`, `CANCEL`, `REPLY`, `COUNTER`,
//!   `REFRESH`, `DECLINECOUNTER`, delegated `REQUEST`.
//! * [`imip`] — finding the calendar part of received mail; building
//!   `multipart/alternative` iMIP mail with a readable summary.
//! * [`texts`] — the summary's subject / text / HTML from `itip.ftl`.
//! * [`conflicts`] — busy instances / periods overlapping invited times.
//! * [`server`] — RFC 6638: when the CalDAV server schedules, so Nail
//!   must not send iMIP mail.

#![warn(missing_docs)]

pub mod address;
pub mod apply;
pub mod build;
pub mod card;
pub mod conflicts;
pub mod imip;
pub mod object;
pub mod order;
pub mod server;
pub mod texts;

pub use address::{UserAddress, UserAddresses};
pub use card::{CardInput, StoredCopy, build_card};
pub use imip::{OutgoingImip, ReceivedImip, find_itip};
pub use object::{ItipObject, parse_itip};
pub use order::Version;
pub use texts::{Localizer, MailText, Summary};

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

/// The MIME type of iTIP data (RFC 6047 §2.4).
pub const ITIP_MIME_TYPE: &str = "text/calendar";

/// The Fluent catalog file of the texts this crate builds
/// (`locales/<lang>/itip.ftl`).
pub const CATALOG: &str = "itip.ftl";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "nail-itip");
        assert_eq!(super::ITIP_MIME_TYPE, "text/calendar");
        assert_eq!(super::CATALOG, "itip.ftl");
    }
}
