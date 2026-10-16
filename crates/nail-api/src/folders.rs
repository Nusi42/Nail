//! Folder management (ADR 0196): create, rename, move and delete folders.
//!
//! [`crate::Request::CreateFolder`], [`crate::Request::RenameFolder`],
//! [`crate::Request::MoveFolder`] and [`crate::Request::DeleteFolder`] are
//! checked at once (a refusal is `invalid_request` with a
//! [`FolderIssue::message_id`] as `localized`), then queued like message
//! writes and answered with [`crate::Response::Queued`]. The account's
//! engine replays them before its message writes; the folder tree changes
//! when the server confirmed (offline, they wait). Progress arrives as
//! [`crate::Event::OpStatus`] with the op id from the answer.
//!
//! Renaming or moving keeps the folder's id and its messages; deleting moves
//! the folder into the account's Trash unless `permanent`. Gmail accounts
//! manage labels instead; folders of system roles cannot be changed.

/// Longest folder name, in characters.
pub const MAX_FOLDER_NAME: usize = 200;

labeled_enum! {
    /// Why a folder change is refused (`folder-issue-*`, core-folders.ftl).
    pub enum FolderIssue : "folder-issue" {
        /// The name is empty.
        MissingName = "missing-name",
        /// The name is longer than [`MAX_FOLDER_NAME`].
        NameTooLong = "name-too-long",
        /// The name contains the server's hierarchy separator or control
        /// characters.
        InvalidName = "invalid-name",
        /// A folder of that name exists at that place.
        NameTaken = "name-taken",
        /// Inbox, Sent, Drafts, Trash, … cannot be renamed, moved or deleted.
        SystemFolder = "system-folder",
        /// A folder cannot be moved into itself or one of its subfolders.
        IntoItself = "into-itself",
        /// The target folder belongs to another account.
        OtherAccount = "other-account",
        /// The server has no folder hierarchy (no subfolders).
        NoHierarchy = "no-hierarchy",
        /// The account has no Trash (or the folder is in it already): only a
        /// permanent delete is possible.
        NoTrash = "no-trash",
        /// Gmail accounts use labels.
        GmailLabels = "gmail-labels",
        /// The account's server cannot manage folders through Nail (yet).
        Unsupported = "unsupported",
    }
}

impl FolderIssue {
    /// The issue of a folder name, if any (the separator is checked by the
    /// core, which knows the server's).
    pub fn of_name(name: &str) -> Option<Self> {
        let name = name.trim();
        if name.is_empty() {
            Some(Self::MissingName)
        } else if name.chars().count() > MAX_FOLDER_NAME {
            Some(Self::NameTooLong)
        } else if name.chars().any(char::is_control) {
            Some(Self::InvalidName)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        assert_eq!(FolderIssue::of_name("  "), Some(FolderIssue::MissingName));
        assert_eq!(
            FolderIssue::of_name(&"x".repeat(MAX_FOLDER_NAME + 1)),
            Some(FolderIssue::NameTooLong)
        );
        assert_eq!(FolderIssue::of_name("a\tb"), Some(FolderIssue::InvalidName));
        assert_eq!(FolderIssue::of_name(" Receipts 2026 "), None);
        assert_eq!(
            FolderIssue::NameTaken.message_id(),
            "folder-issue-name-taken"
        );
    }
}
