//! Deterministic id derivation.
//!
//! Ids are opaque to clients (see `nail_api::ids`), but the core derives them
//! deterministically from server identity, so re-syncing (or re-seeding) the
//! same mailbox yields the same ids and upserts instead of duplicating rows.

use nail_api::{AccountId, FolderId, MessageId, OpId};
use sha2::{Digest, Sha256};

/// Length of the hash part of derived ids, in hex characters (64 bits).
const HASH_CHARS: usize = 16;

fn digest(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        // Length-prefix every part so ("ab", "c") and ("a", "bc") differ.
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    let mut hex = hex::encode(hasher.finalize());
    hex.truncate(HASH_CHARS);
    hex
}

/// The id of the folder at `path` in `account`.
pub(crate) fn folder_id(account: &AccountId, path: &str) -> FolderId {
    FolderId::new(format!(
        "f-{}",
        digest(&[account.as_str().as_bytes(), path.as_bytes()])
    ))
}

/// The id for a new folder at `path`: [`folder_id`], unless a folder
/// renamed away from that path still has it (ids stay on rename, ADR 0196);
/// then the first free variant.
pub(crate) fn unique_folder_id(
    store: &nail_store::Store,
    account: &AccountId,
    path: &str,
) -> nail_store::Result<FolderId> {
    let mut n = 0u32;
    loop {
        let id = if n == 0 {
            folder_id(account, path)
        } else {
            folder_id(account, &format!("{path}\u{1}{n}"))
        };
        match store.get_folder(&id) {
            Err(err) if err.is_not_found() => return Ok(id),
            Ok(existing) if existing.path == path => return Ok(id),
            Ok(_) => n = n.saturating_add(1),
            Err(err) => return Err(err),
        }
    }
}

/// The id of the message with `uid` in the folder at `path`, valid for one
/// `UIDVALIDITY` of that folder.
pub(crate) fn message_id(
    account: &AccountId,
    path: &str,
    uid_validity: u32,
    uid: u32,
) -> MessageId {
    MessageId::new(format!(
        "m-{}",
        digest(&[
            account.as_str().as_bytes(),
            path.as_bytes(),
            &uid_validity.to_le_bytes(),
            &uid.to_le_bytes(),
        ])
    ))
}

/// The id of a message downloaded from a POP3 server (by its `UIDL`).
pub(crate) fn pop3_message_id(account: &AccountId, uidl: &str) -> MessageId {
    MessageId::new(format!(
        "m-{}",
        digest(&[account.as_str().as_bytes(), b"\0pop3", uidl.as_bytes()])
    ))
}

/// The id of a message the core created locally (e.g. a Sent copy of a
/// POP3 account), from its content hash and a folder.
pub(crate) fn local_message_id(folder: &FolderId, content: &str) -> MessageId {
    MessageId::new(format!(
        "m-{}",
        digest(&[folder.as_str().as_bytes(), b"\0local", content.as_bytes()])
    ))
}

/// The wire form of a queued operation's id (`op-42`).
pub(crate) fn op_id(id: nail_store::OpId) -> OpId {
    OpId::new(format!("op-{}", id.0))
}

/// The store's id of a wire op id; `None` when it is not one of ours.
pub(crate) fn parse_op_id(id: &OpId) -> Option<nail_store::OpId> {
    id.as_str()
        .strip_prefix("op-")
        .and_then(|n| n.parse().ok())
        .map(nail_store::OpId)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_and_distinct() {
        let account = AccountId::new("a1");
        assert_eq!(folder_id(&account, "INBOX"), folder_id(&account, "INBOX"));
        assert_ne!(folder_id(&account, "INBOX"), folder_id(&account, "Sent"));
        assert_ne!(
            folder_id(&account, "INBOX"),
            folder_id(&AccountId::new("a2"), "INBOX")
        );
        assert!(folder_id(&account, "INBOX").as_str().starts_with("f-"));
        assert_eq!(folder_id(&account, "INBOX").as_str().len(), 2 + HASH_CHARS);

        let m = message_id(&account, "INBOX", 7, 1);
        assert_eq!(m, message_id(&account, "INBOX", 7, 1));
        assert_ne!(m, message_id(&account, "INBOX", 8, 1));
        assert_ne!(m, message_id(&account, "INBOX", 7, 2));
        assert_ne!(m, message_id(&account, "Sent", 7, 1));
    }

    #[test]
    fn pop3_and_op_ids() {
        let a = AccountId::new("home");
        assert_eq!(pop3_message_id(&a, "u1"), pop3_message_id(&a, "u1"));
        assert_ne!(pop3_message_id(&a, "u1"), pop3_message_id(&a, "u2"));
        assert_ne!(
            pop3_message_id(&a, "u1"),
            pop3_message_id(&AccountId::new("work"), "u1")
        );
        let op = op_id(nail_store::OpId(42));
        assert_eq!(op.as_str(), "op-42");
        assert_eq!(parse_op_id(&op), Some(nail_store::OpId(42)));
        assert_eq!(parse_op_id(&OpId::new("op-x")), None);
        assert_eq!(parse_op_id(&OpId::new("42")), None);
        let f = FolderId::new("f-1");
        assert_ne!(local_message_id(&f, "a"), local_message_id(&f, "b"));
    }

    /// Review M1a: the domain separators are written as `\0` escapes, not
    /// raw NUL bytes (which made git treat the file as binary); the
    /// derived ids must not change.
    #[test]
    fn separators_are_escaped_and_ids_unchanged() {
        let source = include_bytes!("ids.rs");
        assert!(!source.contains(&0), "raw NUL byte in ids.rs");
        let a = AccountId::new("home");
        let pop3 = [0u8, b'p', b'o', b'p', b'3'];
        assert_eq!(
            pop3_message_id(&a, "u1").as_str(),
            format!("m-{}", digest(&[b"home", &pop3, b"u1"]))
        );
        let local = [0u8, b'l', b'o', b'c', b'a', b'l'];
        let f = FolderId::new("f-1");
        assert_eq!(
            local_message_id(&f, "x").as_str(),
            format!("m-{}", digest(&[b"f-1", &local, b"x"]))
        );
    }

    #[test]
    fn parts_are_length_prefixed() {
        assert_ne!(
            folder_id(&AccountId::new("ab"), "c"),
            folder_id(&AccountId::new("a"), "bc")
        );
    }
}
