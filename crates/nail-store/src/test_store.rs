//! Stores for the unit tests, built from `nail_api::testing`'s samples.

use nail_api::{Account, AccountKind, Flags, Folder, FolderRole, MessageSummary, testing};

use crate::{Store, StoredMessage};

/// An in-memory store holding `account` and `folders`.
pub(crate) fn store_with(account: &Account, folders: &[Folder]) -> Store {
    let store = Store::open_in_memory().unwrap();
    store.upsert_account(account).unwrap();
    for folder in folders {
        store.upsert_folder(folder).unwrap();
    }
    store
}

/// Stores a message.
pub(crate) fn add(store: &Store, summary: MessageSummary) {
    store.upsert_message(&StoredMessage::new(summary)).unwrap();
}

/// Stores messages `(id, seen, tags)` in `folder` of `account`.
pub(crate) fn add_tagged(
    store: &Store,
    account: &str,
    folder: &str,
    messages: &[(&str, bool, &[&str])],
) {
    for &(id, seen, tags) in messages {
        add(
            store,
            MessageSummary {
                flags: Flags {
                    seen,
                    ..Flags::default()
                },
                tags: tags.iter().map(|t| (*t).to_owned()).collect(),
                ..testing::summary(id, account, folder)
            },
        );
    }
}

/// A store with the Exchange account `work` and its inbox `x-inbox`.
pub(crate) fn exchange_store() -> Store {
    store_with(
        &testing::account("work", AccountKind::Exchange),
        &[testing::folder(
            "x-inbox",
            "work",
            "Inbox",
            FolderRole::Inbox,
        )],
    )
}
