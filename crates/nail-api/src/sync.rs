//! Per-folder sync policies and on-demand bodies (design.md, "Account
//! behaviour": full bodies, headers only, or bodies for the last N days;
//! "Storage defaults"; M2c, ADR 0043).
//!
//! The policy itself lives in `accounts.kdl` (the account's `sync` block,
//! [`crate::accounts::SyncPolicy`]); [`crate::Request::SetFolderSyncPolicy`]
//! writes a `folder "<path>" …` rule there for one folder.
//! [`FolderSyncPolicy`] is the resolved view of one folder. Messages whose
//! body is not stored locally are marked in their summary
//! ([`crate::MessageSummary::headers_only`]); opening one fetches the body
//! on demand ([`crate::Request::GetMessage`] does so itself), and
//! [`crate::Request::FetchMessageBodies`] downloads bodies ahead of time.
//! Search falls back to the server (IMAP `SEARCH`) for such messages and
//! merges the hits ([`crate::search::ServerSearch`]).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::accounts::{BodyPolicy, FolderPolicy};
use crate::ids::{AccountId, FolderId};

/// The resolved sync policy of one folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FolderSyncPolicy {
    /// The folder.
    pub folder_id: FolderId,
    /// Its account.
    pub account_id: AccountId,
    /// Its server path (what `folder "<path>"` rules match).
    pub path: String,
    /// What is downloaded (after every matching rule).
    pub bodies: BodyPolicy,
    /// Whether the folder is synchronized.
    pub enabled: bool,
    /// Whether it is watched with IMAP IDLE.
    pub idle: bool,
    /// The rule written for exactly this folder's path, if any (what
    /// [`crate::Request::SetFolderSyncPolicy`] edits); rules by role or the
    /// account default may still apply.
    pub rule: Option<FolderPolicy>,
    /// Whether the account can fetch bodies on demand (IMAP: yes; POP3:
    /// no — POP3 accounts always keep full bodies).
    pub on_demand: bool,
}

/// Where a message's body is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum BodyState {
    /// Downloaded by the sync (kept by the policy).
    Synced,
    /// Not stored locally (headers-only policy, or older than the policy's
    /// days); fetched when opened.
    HeadersOnly,
    /// Downloaded on demand; may be dropped again when the store trims
    /// bodies outside the policy.
    OnDemand,
}

impl BodyState {
    /// The store's column value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Synced => "synced",
            Self::HeadersOnly => "headers-only",
            Self::OnDemand => "on-demand",
        }
    }

    /// Parses [`Self::as_str`].
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "synced" => Some(Self::Synced),
            "headers-only" => Some(Self::HeadersOnly),
            "on-demand" => Some(Self::OnDemand),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::FolderMatch;
    use serde_json::json;

    #[test]
    fn body_states_round_trip() {
        for state in [
            BodyState::Synced,
            BodyState::HeadersOnly,
            BodyState::OnDemand,
        ] {
            assert_eq!(BodyState::parse(state.as_str()), Some(state));
            assert_eq!(serde_json::to_value(state).unwrap(), json!(state.as_str()));
        }
        assert_eq!(BodyState::parse("x"), None);
    }

    #[test]
    fn policy_serializes() {
        let policy = FolderSyncPolicy {
            folder_id: "f1".into(),
            account_id: "work".into(),
            path: "Archive/2020".into(),
            bodies: BodyPolicy::Recent { days: 30 },
            enabled: true,
            idle: false,
            rule: Some(FolderPolicy {
                folder: FolderMatch::Path("Archive/2020".into()),
                bodies: Some(BodyPolicy::HeadersOnly),
                enabled: None,
                idle: None,
            }),
            on_demand: true,
        };
        let value = serde_json::to_value(&policy).unwrap();
        assert_eq!(value["bodies"], json!({"mode": "recent", "days": 30}));
        assert_eq!(value["rule"]["folder"], json!({"path": "Archive/2020"}));
        assert_eq!(
            serde_json::from_value::<FolderSyncPolicy>(value).unwrap(),
            policy
        );
    }
}
