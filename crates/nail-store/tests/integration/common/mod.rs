//! Fixtures shared by the integration tests.

use nail_api::{Account, AccountKind, Address, Flags, Folder, FolderRole, MessageSummary};
use nail_store::{Store, StoredMessage};

/// An IMAP account `id` (`nail_api::testing::account`).
pub(crate) fn account(id: &str) -> Account {
    nail_api::testing::account(id, AccountKind::Imap)
}

pub(crate) fn folder(
    id: &str,
    account: &str,
    path: &str,
    parent: Option<&str>,
    role: FolderRole,
) -> Folder {
    Folder {
        parent_id: parent.map(Into::into),
        ..nail_api::testing::folder(id, account, path, role)
    }
}

pub(crate) fn message(id: &str, account: &str, folder: &str, date: &str) -> StoredMessage {
    StoredMessage::new(MessageSummary {
        id: id.into(),
        folder_id: folder.into(),
        account_id: account.into(),
        subject: format!("Subject {id}"),
        from: Some(Address {
            name: Some("Ada Lovelace".into()),
            email: "ada@example.org".into(),
        }),
        to: vec![
            Address {
                name: None,
                email: "bob@example.org".into(),
            },
            Address {
                name: Some("Carol".into()),
                email: "carol@example.org".into(),
            },
        ],
        date: date.to_owned(),
        flags: Flags::default(),
        has_attachments: false,
        preview: format!("Preview of {id}"),
        thread_id: None,
        tags: Vec::new(),
        duplicates: Vec::new(),
        headers_only: false,
        gmail: None,
        exchange: None,
    })
}

pub(crate) use nail_store::testing::add_inbox;

/// A store with account `a1` and folders `inbox` (INBOX) and `archive`.
pub(crate) fn seeded_store() -> Store {
    let store = Store::open_in_memory().unwrap();
    add_inbox(&store, "a1", "inbox");
    store
        .upsert_folder(&folder(
            "archive",
            "a1",
            "Archive",
            None,
            FolderRole::Archive,
        ))
        .unwrap();
    store
}

/// A store in `<dir>/nail.sqlite3` with account `a1` and its inbox.
pub(crate) fn file_store(dir: &std::path::Path) -> (std::path::PathBuf, Store) {
    let path = dir.join("nail.sqlite3");
    let store = Store::open(&path).unwrap();
    add_inbox(&store, "a1", "inbox");
    (path, store)
}
