//! Deterministic id derivation.
//!
//! Ids are opaque to clients (see `nail_api::ids`), but the core derives them
//! deterministically from server identity, so re-syncing (or re-seeding) the
//! same mailbox yields the same ids and upserts instead of duplicating rows.

use nail_api::{AccountId, FolderId, MessageId};
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
pub fn folder_id(account: &AccountId, path: &str) -> FolderId {
    FolderId::new(format!(
        "f-{}",
        digest(&[account.as_str().as_bytes(), path.as_bytes()])
    ))
}

/// The id of the message with `uid` in the folder at `path`, valid for one
/// `UIDVALIDITY` of that folder.
pub fn message_id(account: &AccountId, path: &str, uid_validity: u32, uid: u32) -> MessageId {
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
    fn parts_are_length_prefixed() {
        assert_ne!(
            folder_id(&AccountId::new("ab"), "c"),
            folder_id(&AccountId::new("a"), "bc")
        );
    }
}
