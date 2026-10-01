//! Automatic replies (out of office) of Exchange accounts (M3c, ADR 0072).
//!
//! Exchange keeps them as a mailbox setting (Graph
//! `mailboxSettings/automaticRepliesSetting`, EWS
//! `Get/SetUserOofSettings`): a status (off, on, on for a schedule), a reply
//! for senders inside the organization, one for outside senders, and who
//! outside gets it. For Exchange accounts they replace the Sieve vacation
//! reply ([`crate::sieve::VacationSettings`]; the mapping layer converts
//! between both).
//!
//! The replies are plain text in the contract. Exchange stores HTML: the
//! engines write the text as simple HTML (escaped, line breaks kept) and
//! read the server's HTML back as text, so formatting made in Outlook is
//! lost once the reply is edited in Nail.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::ExchangeSync;
use crate::ids::AccountId;

/// Longest reply text Nail sends (characters; Exchange limits the stored
/// HTML to about 128 KB).
pub const MAX_REPLY_CHARS: usize = 32_000;

/// Whether automatic replies are sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AutomaticRepliesStatus {
    /// Off.
    #[default]
    Disabled,
    /// On until switched off.
    AlwaysEnabled,
    /// On between `start` and `end`.
    Scheduled,
}

/// Which senders outside the organization get the external reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ExternalAudience {
    /// Nobody outside.
    None,
    /// Only senders in the user's contacts.
    ContactsOnly,
    /// Everyone.
    #[default]
    All,
}

/// The automatic-reply settings of an Exchange account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AutomaticRepliesSettings {
    /// Off, on, or on for the schedule.
    pub status: AutomaticRepliesStatus,
    /// Schedule start (RFC 3339 UTC); required for `scheduled`.
    #[serde(default)]
    pub start: Option<String>,
    /// Schedule end (RFC 3339 UTC, after `start`); required for
    /// `scheduled`.
    #[serde(default)]
    pub end: Option<String>,
    /// The reply to senders inside the organization (plain text).
    pub internal_reply: String,
    /// The reply to senders outside (plain text).
    pub external_reply: String,
    /// Who outside gets `external_reply`.
    #[serde(default)]
    pub external_audience: ExternalAudience,
}

impl Default for AutomaticRepliesSettings {
    fn default() -> Self {
        Self {
            status: AutomaticRepliesStatus::Disabled,
            start: None,
            end: None,
            internal_reply: String::new(),
            external_reply: String::new(),
            external_audience: ExternalAudience::All,
        }
    }
}

impl AutomaticRepliesSettings {
    /// Whether replies go out at `now` (RFC 3339 UTC).
    pub fn active_at(&self, now: &str) -> bool {
        match self.status {
            AutomaticRepliesStatus::Disabled => false,
            AutomaticRepliesStatus::AlwaysEnabled => true,
            AutomaticRepliesStatus::Scheduled => match (&self.start, &self.end) {
                (Some(start), Some(end)) => start.as_str() <= now && now < end.as_str(),
                _ => false,
            },
        }
    }

    /// Problems (empty = valid). Disabled settings are still checked for
    /// the schedule's shape, not for empty texts.
    pub fn issues(&self) -> Vec<AutomaticRepliesIssue> {
        let mut out = Vec::new();
        let enabled = self.status != AutomaticRepliesStatus::Disabled;
        if enabled && self.internal_reply.trim().is_empty() {
            out.push(AutomaticRepliesIssue::EmptyInternalReply);
        }
        if enabled
            && self.external_audience != ExternalAudience::None
            && self.external_reply.trim().is_empty()
        {
            out.push(AutomaticRepliesIssue::EmptyExternalReply);
        }
        if self.internal_reply.chars().count() > MAX_REPLY_CHARS
            || self.external_reply.chars().count() > MAX_REPLY_CHARS
        {
            out.push(AutomaticRepliesIssue::ReplyTooLong);
        }
        let start = self.start.as_deref();
        let end = self.end.as_deref();
        if start.is_some_and(|s| !is_utc_time(s)) || end.is_some_and(|e| !is_utc_time(e)) {
            out.push(AutomaticRepliesIssue::InvalidTime);
        } else if self.status == AutomaticRepliesStatus::Scheduled {
            match (start, end) {
                (Some(start), Some(end)) if end <= start => {
                    out.push(AutomaticRepliesIssue::EndBeforeStart);
                }
                (Some(_), Some(_)) => {}
                _ => out.push(AutomaticRepliesIssue::MissingSchedule),
            }
        }
        out
    }
}

/// Whether `text` is RFC 3339 UTC as the contract writes it
/// (`YYYY-MM-DDTHH:MM:SSZ`).
fn is_utc_time(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 20
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b'T',
            13 | 16 => *c == b':',
            19 => *c == b'Z',
            _ => c.is_ascii_digit(),
        })
}

/// A problem with [`AutomaticRepliesSettings`]; the core answers
/// `invalid_request` with [`Self::message_id`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AutomaticRepliesIssue {
    /// On, but the internal reply is empty.
    EmptyInternalReply,
    /// On for outside senders, but the external reply is empty.
    EmptyExternalReply,
    /// A reply is longer than [`MAX_REPLY_CHARS`].
    ReplyTooLong,
    /// `start` / `end` are not RFC 3339 UTC.
    InvalidTime,
    /// `scheduled` without `start` and `end`.
    MissingSchedule,
    /// `end` is not after `start`.
    EndBeforeStart,
}

impl AutomaticRepliesIssue {
    /// Every issue.
    pub const ALL: [AutomaticRepliesIssue; 6] = [
        Self::EmptyInternalReply,
        Self::EmptyExternalReply,
        Self::ReplyTooLong,
        Self::InvalidTime,
        Self::MissingSchedule,
        Self::EndBeforeStart,
    ];

    /// The Fluent id (`exchange-oof-issue-*`, in `core-exchange.ftl`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::EmptyInternalReply => "exchange-oof-issue-empty-internal-reply",
            Self::EmptyExternalReply => "exchange-oof-issue-empty-external-reply",
            Self::ReplyTooLong => "exchange-oof-issue-reply-too-long",
            Self::InvalidTime => "exchange-oof-issue-invalid-time",
            Self::MissingSchedule => "exchange-oof-issue-missing-schedule",
            Self::EndBeforeStart => "exchange-oof-issue-end-before-start",
        }
    }
}

/// The automatic replies of an account
/// ([`crate::Request::GetAutomaticReplies`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AutomaticReplies {
    /// The account.
    pub account_id: AccountId,
    /// The settings (the queued ones while `sync` is `pending`).
    pub settings: AutomaticRepliesSettings,
    /// Whether a change waits in the Exchange operation queue.
    #[serde(default)]
    pub sync: ExchangeSync,
    /// When the settings were last read from the server (RFC 3339 UTC),
    /// `null` = never (then `settings` are the defaults).
    #[serde(default)]
    pub fetched_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn on() -> AutomaticRepliesSettings {
        AutomaticRepliesSettings {
            status: AutomaticRepliesStatus::AlwaysEnabled,
            internal_reply: "Back on Monday.".into(),
            external_reply: "I am out of the office.".into(),
            ..AutomaticRepliesSettings::default()
        }
    }

    #[test]
    fn valid_settings() {
        assert!(AutomaticRepliesSettings::default().issues().is_empty());
        assert!(on().issues().is_empty());
        let scheduled = AutomaticRepliesSettings {
            status: AutomaticRepliesStatus::Scheduled,
            start: Some("2026-10-05T07:00:00Z".into()),
            end: Some("2026-10-09T16:00:00Z".into()),
            ..on()
        };
        assert!(scheduled.issues().is_empty());
        assert!(scheduled.active_at("2026-10-06T12:00:00Z"));
        assert!(!scheduled.active_at("2026-10-09T16:00:00Z"));
        assert!(on().active_at("2000-01-01T00:00:00Z"));
        assert!(!AutomaticRepliesSettings::default().active_at("2026-10-06T12:00:00Z"));
    }

    #[test]
    fn invalid_settings() {
        let empty = AutomaticRepliesSettings {
            status: AutomaticRepliesStatus::AlwaysEnabled,
            ..AutomaticRepliesSettings::default()
        };
        assert_eq!(
            empty.issues(),
            [
                AutomaticRepliesIssue::EmptyInternalReply,
                AutomaticRepliesIssue::EmptyExternalReply
            ]
        );
        let inside_only = AutomaticRepliesSettings {
            external_reply: String::new(),
            external_audience: ExternalAudience::None,
            ..on()
        };
        assert!(inside_only.issues().is_empty());
        let unscheduled = AutomaticRepliesSettings {
            status: AutomaticRepliesStatus::Scheduled,
            ..on()
        };
        assert_eq!(
            unscheduled.issues(),
            [AutomaticRepliesIssue::MissingSchedule]
        );
        let backwards = AutomaticRepliesSettings {
            start: Some("2026-10-09T16:00:00Z".into()),
            end: Some("2026-10-05T07:00:00Z".into()),
            ..unscheduled.clone()
        };
        assert_eq!(backwards.issues(), [AutomaticRepliesIssue::EndBeforeStart]);
        let local = AutomaticRepliesSettings {
            start: Some("2026-10-09 16:00".into()),
            ..unscheduled
        };
        assert_eq!(local.issues(), [AutomaticRepliesIssue::InvalidTime]);
        let long = AutomaticRepliesSettings {
            internal_reply: "x".repeat(MAX_REPLY_CHARS + 1),
            ..on()
        };
        assert_eq!(long.issues(), [AutomaticRepliesIssue::ReplyTooLong]);
        for issue in AutomaticRepliesIssue::ALL {
            assert!(issue.message_id().starts_with("exchange-oof-issue-"));
        }
    }

    #[test]
    fn wire_shape() {
        let value = serde_json::to_value(on()).unwrap();
        assert_eq!(value["status"], json!("always-enabled"));
        assert_eq!(value["external_audience"], json!("all"));
        let parsed: AutomaticRepliesSettings = serde_json::from_value(json!({
            "status": "scheduled", "start": "2026-10-05T07:00:00Z",
            "end": "2026-10-09T16:00:00Z", "internal_reply": "a", "external_reply": "b",
            "external_audience": "contacts-only"
        }))
        .unwrap();
        assert_eq!(parsed.external_audience, ExternalAudience::ContactsOnly);
        let replies: AutomaticReplies = serde_json::from_value(json!({
            "account_id": "work",
            "settings": serde_json::to_value(on()).unwrap()
        }))
        .unwrap();
        assert_eq!(replies.sync, ExchangeSync::Synced);
        assert_eq!(replies.fetched_at, None);
    }
}
