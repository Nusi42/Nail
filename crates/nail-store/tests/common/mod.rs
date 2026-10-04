//! Fixtures shared by the integration tests.

use nail_api::{Account, AccountKind, Address, Flags, Folder, FolderRole, MessageSummary};
use nail_store::{Store, StoredMessage};

pub(crate) fn account(id: &str) -> Account {
    Account {
        id: id.into(),
        name: format!("Account {id}"),
        email: format!("{id}@example.org"),
        kind: AccountKind::Imap,
    }
}

pub(crate) fn folder(
    id: &str,
    account: &str,
    path: &str,
    parent: Option<&str>,
    role: FolderRole,
) -> Folder {
    Folder {
        id: id.into(),
        account_id: account.into(),
        parent_id: parent.map(Into::into),
        name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        path: path.to_owned(),
        role,
        unread: 0,
        total: 0,
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

/// A store with account `a1` and folders `inbox` (INBOX) and `archive`.
pub(crate) fn seeded_store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store.upsert_account(&account("a1")).unwrap();
    store
        .upsert_folder(&folder("inbox", "a1", "INBOX", None, FolderRole::Inbox))
        .unwrap();
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
