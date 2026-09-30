//! Sync policies for bodies and on-demand body fetches (M2c WP53; ADR
//! 0043: `bodies` of a folder's sync policy — full, headers only, or
//! full bodies for the last N days).
//!
//! A sync plans as before ([`ImapClient::plan_fetch`]) and fetches each
//! batch with [`ImapClient::fetch_batch_with`] and the folder's
//! [`BodyFetch`]: full messages, header blocks only (`BODY.PEEK[HEADER]`),
//! or full messages for mail that arrived since a day (`UID SEARCH …
//! SINCE`) and header blocks for older mail. Opening a message whose body
//! was not fetched calls [`ImapClient::fetch_body`].
//!
//! A separate replay connection needs nothing at the protocol level: it
//! is another [`ImapClient::connect`] with the same configuration, and
//! IMAP servers accept several connections per user.

use std::fmt;

use nail_api::accounts::BodyPolicy;

use crate::client::{FetchedMessage, ImapClient, uid_set};
use crate::error::Result;

const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// A calendar day as IMAP search keys take it (`SINCE 1-Feb-2026`).
///
/// IMAP compares dates without time or zone (the server's `INTERNALDATE`
/// day for `SINCE` / `BEFORE`, the `Date:` header's day for `SENTSINCE` /
/// `SENTBEFORE`), so a day is precise to at most one day either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImapDate {
    /// Year.
    pub year: i32,
    /// Month 1–12.
    pub month: u8,
    /// Day 1–31.
    pub day: u8,
}

impl ImapDate {
    /// The UTC day of a Unix time (seconds).
    pub fn from_unix(secs: i64) -> Self {
        // Howard Hinnant's civil_from_days.
        let z = secs.div_euclid(86_400) + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        Self {
            year: i32::try_from(year).unwrap_or(i32::MAX),
            month: u8::try_from(month).unwrap_or(1),
            day: u8::try_from(day).unwrap_or(1),
        }
    }

    /// The day `days` days before the UTC day of `now` (Unix seconds).
    pub fn days_before(now: i64, days: u32) -> Self {
        Self::from_unix(now.saturating_sub(i64::from(days).saturating_mul(86_400)))
    }
}

impl fmt::Display for ImapDate {
    /// `date` of RFC 3501: `1-Feb-2026`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let month = MONTH_NAMES
            .get(usize::from(self.month.saturating_sub(1)))
            .copied()
            .unwrap_or("Jan");
        write!(f, "{}-{month}-{:04}", self.day, self.year)
    }
}

/// What a batch fetch downloads (a folder's body policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyFetch {
    /// Whole messages.
    Full,
    /// Header blocks only; bodies are fetched on demand.
    HeadersOnly,
    /// Whole messages that arrived on or after this day (`INTERNALDATE`),
    /// header blocks for older ones.
    Since(ImapDate),
}

impl BodyFetch {
    /// The fetch for a folder's [`BodyPolicy`] at `now` (Unix seconds):
    /// "recent N days" keeps bodies of mail since the day N days ago.
    pub fn from_policy(policy: &BodyPolicy, now: i64) -> Self {
        match policy {
            BodyPolicy::Full => Self::Full,
            BodyPolicy::HeadersOnly => Self::HeadersOnly,
            BodyPolicy::Recent { days } => Self::Since(ImapDate::days_before(now, *days)),
        }
    }
}

impl ImapClient {
    /// [`ImapClient::fetch_batch`] under a body policy: whole messages,
    /// header blocks ([`FetchedMessage::headers_only`]), or whole messages
    /// since a day and header blocks for the rest. The result is in UID
    /// order either way. Call [`ImapClient::examine`] (or
    /// [`ImapClient::plan_fetch`]) first.
    pub async fn fetch_batch_with(
        &mut self,
        uids: &[u32],
        bodies: BodyFetch,
    ) -> Result<Vec<FetchedMessage>> {
        match bodies {
            BodyFetch::Full => self.fetch_uids(uids, false).await,
            BodyFetch::HeadersOnly => self.fetch_uids(uids, true).await,
            BodyFetch::Since(day) => {
                let mut wanted = uids.to_vec();
                wanted.sort_unstable();
                wanted.dedup();
                if wanted.is_empty() {
                    return Ok(Vec::new());
                }
                let set = uid_set(&wanted);
                let recent = self
                    .search(&format!("UID {set} SINCE {day}"), |uid| {
                        wanted.binary_search(&uid).is_ok()
                    })
                    .await?;
                let older: Vec<u32> = wanted
                    .iter()
                    .copied()
                    .filter(|uid| recent.binary_search(uid).is_err())
                    .collect();
                let mut messages = self.fetch_uids(&recent, false).await?;
                messages.extend(self.fetch_uids(&older, true).await?);
                messages.sort_by_key(|m| m.uid);
                Ok(messages)
            }
        }
    }

    /// Fetches one whole message from `path` (a body not fetched by the
    /// sync policy, opened by the user). Examines `path` unless it is
    /// selected; `None` when the message is gone (expunged).
    pub async fn fetch_body(&mut self, path: &str, uid: u32) -> Result<Option<FetchedMessage>> {
        self.ensure_selected(path, false).await?;
        Ok(self.fetch_uids(&[uid], false).await?.into_iter().next())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_imap_dates() {
        assert_eq!(ImapDate::from_unix(0).to_string(), "1-Jan-1970");
        // 2026-09-30T12:00:00Z
        let now = 1_790_769_600;
        assert_eq!(
            ImapDate::from_unix(now),
            ImapDate {
                year: 2026,
                month: 9,
                day: 30
            }
        );
        assert_eq!(ImapDate::days_before(now, 30).to_string(), "31-Aug-2026");
        assert_eq!(ImapDate::days_before(now, 365).to_string(), "30-Sep-2025");
        assert_eq!(ImapDate::from_unix(951_782_400).to_string(), "29-Feb-2000");
        assert!(ImapDate::from_unix(0) < ImapDate::from_unix(now));
    }

    #[test]
    fn policies_become_fetches() {
        let now = 1_790_769_600;
        assert_eq!(
            BodyFetch::from_policy(&BodyPolicy::Full, now),
            BodyFetch::Full
        );
        assert_eq!(
            BodyFetch::from_policy(&BodyPolicy::HeadersOnly, now),
            BodyFetch::HeadersOnly
        );
        assert_eq!(
            BodyFetch::from_policy(&BodyPolicy::Recent { days: 7 }, now),
            BodyFetch::Since(ImapDate {
                year: 2026,
                month: 9,
                day: 23
            })
        );
    }
}
