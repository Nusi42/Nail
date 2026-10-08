//! Sync policies for Exchange folders (plan M4c WP123; design "Account
//! behaviour": full bodies, headers only, or bodies for the last N days).
//!
//! The policy comes from the account's `sync` block like for IMAP
//! ([`SyncPolicy::effective`] by the folder's path and role). Exchange
//! lists every message of a folder either way (metadata is cheap and the
//! per-folder delta cursor must cover the whole folder); the policy decides
//! per message whether the sync downloads the MIME ([`BodyFetch::Full`])
//! or only the headers ([`BodyFetch::HeadersOnly`]: Graph
//! `internetMessageHeaders`, EWS `PR_TRANSPORT_MESSAGE_HEADERS`, through
//! the engines' `fetch_headers`). Headers-only messages are stored with
//! `headers_only` and their bodies are fetched on demand when opened, as
//! for IMAP. Messages whose body was fetched once stay `on-demand`; the
//! store trims them when they leave the window.

use nail_api::FolderRole;
use nail_api::accounts::{BodyPolicy, EffectiveFolderPolicy, SyncPolicy};

/// Seconds per day.
const DAY: i64 = 24 * 3600;

/// What the sync downloads for one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyFetch {
    /// The whole message (MIME).
    Full,
    /// Only its headers; the body comes when it is opened.
    HeadersOnly,
}

/// The effective policy of an Exchange folder: its path as Nail shows it
/// (other mailboxes' folders below `@<address>/`) and its role.
pub fn effective(sync: &SyncPolicy, path: &str, role: FolderRole) -> EffectiveFolderPolicy {
    sync.effective(path, role)
}

/// What to download for a message received at `internal_date` (Unix
/// seconds; unknown dates count as recent) under `policy` at `now`.
pub fn body_fetch(policy: BodyPolicy, internal_date: Option<i64>, now: i64) -> BodyFetch {
    match policy {
        BodyPolicy::Full => BodyFetch::Full,
        BodyPolicy::HeadersOnly => BodyFetch::HeadersOnly,
        BodyPolicy::Recent { days } => {
            let cutoff = now - i64::from(days.max(1)) * DAY;
            match internal_date {
                Some(date) if date < cutoff => BodyFetch::HeadersOnly,
                _ => BodyFetch::Full,
            }
        }
    }
}

/// The oldest receive time whose body the policy keeps (`None` = all or
/// none: [`BodyPolicy::Full`] / [`BodyPolicy::HeadersOnly`]); what the
/// store trims bodies against.
pub fn body_window_start(policy: BodyPolicy, now: i64) -> Option<i64> {
    match policy {
        BodyPolicy::Recent { days } => Some(now - i64::from(days.max(1)) * DAY),
        BodyPolicy::Full | BodyPolicy::HeadersOnly => None,
    }
}

/// The header section of a raw message (up to and including the empty
/// line), for engines without a headers-only call and for trimming bodies.
pub use nail_util::mime_text::header_section;

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::accounts::{FolderMatch, FolderPolicy};

    use nail_util::time::FIXTURE_NOW as NOW;

    #[test]
    fn bodies_by_policy_and_age() {
        assert_eq!(body_fetch(BodyPolicy::Full, Some(0), NOW), BodyFetch::Full);
        assert_eq!(
            body_fetch(BodyPolicy::HeadersOnly, Some(NOW), NOW),
            BodyFetch::HeadersOnly
        );
        let recent = BodyPolicy::Recent { days: 30 };
        assert_eq!(
            body_fetch(recent, Some(NOW - 29 * DAY), NOW),
            BodyFetch::Full
        );
        assert_eq!(
            body_fetch(recent, Some(NOW - 30 * DAY), NOW),
            BodyFetch::Full
        );
        assert_eq!(
            body_fetch(recent, Some(NOW - 30 * DAY - 1), NOW),
            BodyFetch::HeadersOnly
        );
        assert_eq!(body_fetch(recent, None, NOW), BodyFetch::Full);
        // Zero days means one.
        assert_eq!(
            body_fetch(BodyPolicy::Recent { days: 0 }, Some(NOW - 2 * DAY), NOW),
            BodyFetch::HeadersOnly
        );
        assert_eq!(body_window_start(recent, NOW), Some(NOW - 30 * DAY));
        assert_eq!(body_window_start(BodyPolicy::Full, NOW), None);
    }

    #[test]
    fn folder_rules_apply_to_exchange_paths() {
        let sync = SyncPolicy {
            bodies: Some(BodyPolicy::Recent { days: 90 }),
            folders: vec![
                FolderPolicy {
                    folder: FolderMatch::Path("@support@contoso.example/Inbox".into()),
                    bodies: Some(BodyPolicy::HeadersOnly),
                    enabled: None,
                    idle: None,
                },
                FolderPolicy {
                    folder: FolderMatch::Role(FolderRole::Archive),
                    bodies: None,
                    enabled: Some(false),
                    idle: None,
                },
            ],
        };
        let shared = effective(&sync, "@support@contoso.example/Inbox", FolderRole::Other);
        assert_eq!(shared.bodies, BodyPolicy::HeadersOnly);
        let inbox = effective(&sync, "Inbox", FolderRole::Inbox);
        assert_eq!(inbox.bodies, BodyPolicy::Recent { days: 90 });
        assert!(inbox.enabled);
        assert!(!effective(&sync, "Archive", FolderRole::Archive).enabled);
    }

    #[test]
    fn header_sections() {
        assert_eq!(header_section(b"A: 1\r\n\r\nbody"), b"A: 1\r\n\r\n");
        assert_eq!(header_section(b"A: 1\n\nbody"), b"A: 1\n\n");
        assert_eq!(header_section(b"A: 1"), b"A: 1");
    }
}
