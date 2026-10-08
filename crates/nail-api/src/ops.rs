//! Message writes and their lifecycle in the core's operation queue.
//!
//! A write request ([`crate::Request::UpdateFlags`],
//! [`crate::Request::MoveMessages`], [`crate::Request::DeleteMessages`]) is
//! applied to the local store at once and answered with
//! [`crate::Response::Queued`]. The core then replays each queued operation
//! to the server and reports its progress with [`crate::Event::OpStatus`].

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
}
