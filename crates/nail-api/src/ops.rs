//! Message writes and their lifecycle in the core's operation queue.
//!
//! A write request ([`crate::Request::UpdateFlags`],
//! [`crate::Request::MoveMessages`], [`crate::Request::DeleteMessages`]) is
//! applied to the local store at once and answered with
//! [`crate::Response::Queued`]. The core then replays each queued operation
//! to the server. Its progress reaches clients as a queue summary
//! ([`OpsSummary`], pushed with [`crate::Event::OpsChanged`] at most once per
//! enqueued batch and once per replayed group, and answered by
//! [`crate::Request::GetOpsSummary`]); folder changes still report each
//! operation with [`crate::Event::OpStatus`] (ADR 0230).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::{Flags, FolderRole};
use crate::error::ErrorCode;
use crate::ids::{AccountId, FolderId, OpId};
use crate::localized::LocalizedMessage;

/// A change to system flags and tags; `null` / empty leaves a value as it is.
/// Changes are absolute ("set \Seen"), never toggles, so replaying one twice
/// is harmless.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct FlagChange {
    /// New `\Seen` state.
    pub seen: Option<bool>,
    /// New `\Flagged` state.
    pub flagged: Option<bool>,
    /// New `\Answered` state.
    pub answered: Option<bool>,
    /// New `\Draft` state.
    pub draft: Option<bool>,
    /// Tags (IMAP keywords) to add.
    pub add_tags: Vec<String>,
    /// Tags to remove.
    pub remove_tags: Vec<String>,
}

impl FlagChange {
    /// Returns `flags` with the system-flag part of this change applied.
    pub fn apply_to(&self, flags: Flags) -> Flags {
        Flags {
            seen: self.seen.unwrap_or(flags.seen),
            flagged: self.flagged.unwrap_or(flags.flagged),
            answered: self.answered.unwrap_or(flags.answered),
            draft: self.draft.unwrap_or(flags.draft),
        }
    }

    /// Applies the tag part: removes `remove_tags`, then adds `add_tags`
    /// (no duplicates). The result is sorted.
    pub fn apply_tags(&self, tags: &mut Vec<String>) {
        tags.retain(|t| !self.remove_tags.contains(t));
        for tag in &self.add_tags {
            if !tag.is_empty() && !tags.contains(tag) {
                tags.push(tag.clone());
            }
        }
        tags.sort();
    }

    /// Whether the change sets nothing.
    pub fn is_empty(&self) -> bool {
        self.seen.is_none()
            && self.flagged.is_none()
            && self.answered.is_none()
            && self.draft.is_none()
            && !self.changes_tags()
    }

    /// Whether the change touches tags.
    pub fn changes_tags(&self) -> bool {
        !self.add_tags.is_empty() || !self.remove_tags.is_empty()
    }
}

/// Where [`crate::Request::MoveMessages`] moves messages.
///
/// JSON: `{"folder_id": "f-…"}` or `{"role": "archive"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MoveTarget {
    /// A specific folder. When it belongs to another account than a
    /// message, the message is copied there and deleted from its source as
    /// one queued operation. A virtual folder resolves to the matching
    /// folder of each message's source account.
    FolderId(FolderId),
    /// The folder with this role in each message's own account (e.g. Archive,
    /// Junk). Fails with `not_found` for an account without such a folder.
    Role(FolderRole),
}

/// Lifecycle state of a queued operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum OpState {
    /// Applied locally, waiting to be replayed (e.g. while offline).
    Pending,
    /// Being replayed to the server.
    InFlight,
    /// Replayed successfully.
    Done,
    /// Given up. The local change stays until the next sync brings the
    /// server's state (server wins for flags).
    Failed,
}

/// Progress of one queued operation, pushed as [`crate::Event::OpStatus`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OpStatus {
    /// The operation (as returned in [`crate::Response::Queued`]).
    pub op_id: OpId,
    /// The account it is replayed to.
    pub account_id: AccountId,
    /// New state.
    pub state: OpState,
    /// Error class when `failed` (or when a retry is pending), else `null`.
    pub error: Option<ErrorCode>,
    /// The user-facing text (Fluent id + arguments) when the core has a more
    /// specific one than the error class; else `null` (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
    /// Diagnostic detail (English), else `null`.
    pub message: Option<String>,
}

/// The operation queue of one account (ADR 0230).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OpsAccountSummary {
    /// The account.
    pub account_id: AccountId,
    /// Writes applied locally and waiting to be replayed.
    pub pending: u32,
    /// Writes being replayed now.
    pub in_flight: u32,
    /// Writes the core gave up on (kept until retried or dismissed).
    pub failed: u32,
}

impl OpsAccountSummary {
    /// Writes of this account not yet on the server (`pending` +
    /// `in_flight`); [`OpsSummary::waiting`] is their sum.
    pub fn waiting(&self) -> u32 {
        waiting(self.pending, self.in_flight)
    }
}

/// The "waiting" rule (pending + in flight) of [`OpsAccountSummary::waiting`]
/// and [`OpsSummary::waiting`]; the UI's `opsWaiting` is pinned to it by the
/// `ops` ts-vectors.
const fn waiting(pending: u32, in_flight: u32) -> u32 {
    pending.saturating_add(in_flight)
}

/// The newest write the core gave up on, for the status bar's text and its
/// Retry (ADR 0230).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OpFailure {
    /// The operation (for [`crate::Request::RetryOp`]).
    pub op_id: OpId,
    /// Its account.
    pub account_id: AccountId,
    /// Error class (the UI shows a localized text).
    pub code: ErrorCode,
    /// The user-facing text (Fluent id + arguments) when the core has a more
    /// specific one than the error class; else `null` (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
    /// Diagnostic detail (English), else `null`.
    pub message: Option<String>,
    /// When it failed (RFC 3339, UTC).
    pub failed_at: String,
}

/// The real state of the core's queue of message writes, in total and per
/// account (ADR 0230). Answers [`crate::Request::GetOpsSummary`] and is
/// pushed with [`crate::Event::OpsChanged`]. The totals are the sums of
/// `accounts`; an account without queued writes is left out.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct OpsSummary {
    /// Writes waiting to be replayed, all accounts.
    pub pending: u32,
    /// Writes being replayed now, all accounts.
    pub in_flight: u32,
    /// Failed writes, all accounts.
    pub failed: u32,
    /// Per account, in account order of the first queued write.
    pub accounts: Vec<OpsAccountSummary>,
    /// The newest failure, `null` when nothing failed.
    pub newest_failure: Option<OpFailure>,
}

impl OpsSummary {
    /// A summary from per-account rows: the totals are their sums, rows
    /// with nothing queued are dropped.
    pub fn from_accounts(
        accounts: impl IntoIterator<Item = OpsAccountSummary>,
        newest_failure: Option<OpFailure>,
    ) -> Self {
        let accounts: Vec<OpsAccountSummary> = accounts
            .into_iter()
            .filter(|a| a.pending + a.in_flight + a.failed > 0)
            .collect();
        Self {
            pending: accounts.iter().map(|a| a.pending).sum(),
            in_flight: accounts.iter().map(|a| a.in_flight).sum(),
            failed: accounts.iter().map(|a| a.failed).sum(),
            accounts,
            newest_failure,
        }
    }

    /// Writes not yet on the server (`pending` + `in_flight`): the number
    /// the status bar shows as "Saving N changes".
    pub fn waiting(&self) -> u32 {
        waiting(self.pending, self.in_flight)
    }

    /// The same summary restricted to the accounts `keep` allows (the
    /// remote UI's narrowed grants); totals and the newest failure follow.
    #[must_use]
    pub fn retain_accounts(self, keep: impl Fn(&AccountId) -> bool) -> Self {
        let failure = self.newest_failure.filter(|f| keep(&f.account_id));
        Self::from_accounts(
            self.accounts.into_iter().filter(|a| keep(&a.account_id)),
            failure,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flag_change_applies_and_detects_empty() {
        let change = FlagChange {
            seen: Some(true),
            ..FlagChange::default()
        };
        assert!(!change.is_empty());
        assert!(FlagChange::default().is_empty());
        let flags = change.apply_to(Flags {
            flagged: true,
            ..Flags::default()
        });
        assert!(flags.seen && flags.flagged && !flags.answered);
        let tags_only = FlagChange {
            add_tags: vec!["work".into()],
            ..FlagChange::default()
        };
        assert!(!tags_only.is_empty());
        let parsed: FlagChange = serde_json::from_value(json!({"flagged": false})).unwrap();
        assert_eq!(parsed.flagged, Some(false));
        assert!(parsed.add_tags.is_empty(), "{:?}", parsed.add_tags);
    }

    #[test]
    fn move_targets_are_externally_tagged() {
        assert_eq!(
            serde_json::to_value(MoveTarget::Role(FolderRole::Archive)).unwrap(),
            json!({"role": "archive"})
        );
        assert_eq!(
            serde_json::to_value(MoveTarget::FolderId("f1".into())).unwrap(),
            json!({"folder_id": "f1"})
        );
        assert_eq!(
            serde_json::to_value(OpState::InFlight).unwrap(),
            json!("in-flight")
        );
    }

    fn row(account: &str, pending: u32, in_flight: u32, failed: u32) -> OpsAccountSummary {
        OpsAccountSummary {
            account_id: AccountId::new(account),
            pending,
            in_flight,
            failed,
        }
    }

    #[test]
    fn ops_summary_sums_accounts_and_narrows() {
        let failure = OpFailure {
            op_id: "op-7".into(),
            account_id: AccountId::new("b"),
            code: ErrorCode::NotFound,
            localized: None,
            message: None,
            failed_at: "2026-11-21T10:00:00Z".into(),
        };
        let summary = OpsSummary::from_accounts(
            [row("a", 3, 1, 0), row("b", 0, 0, 2), row("c", 0, 0, 0)],
            Some(failure),
        );
        assert_eq!(
            (summary.pending, summary.in_flight, summary.failed),
            (3, 1, 2)
        );
        assert_eq!(summary.waiting(), 4);
        assert_eq!(
            summary.accounts[0].waiting(),
            4,
            "one account: the same rule"
        );
        assert_eq!(
            summary.accounts[1].waiting(),
            0,
            "failed writes do not wait"
        );
        assert_eq!(summary.accounts.len(), 2, "empty rows are dropped");
        let narrowed = summary.clone().retain_accounts(|a| a.as_str() == "a");
        assert_eq!(narrowed.failed, 0);
        assert_eq!(narrowed.newest_failure, None);
        assert_eq!(narrowed.waiting(), 4);
        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["accounts"][0]["in_flight"], json!(1));
        assert_eq!(json["newest_failure"]["code"], json!("not_found"));
        assert!(json["newest_failure"].get("localized").is_none());
    }
}
