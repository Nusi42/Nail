//! Test support (feature `testing`; the crate's own tests use it too):
//! the one mailbox fixture of the rules tests.

use nail_api::FolderRole;

use crate::{AccountInfo, FolderInfo, StaticMailbox};

/// Two real accounts (`work`, `home`), a virtual account `everything` over
/// both, and their folders (`w-inbox`, `w-archive`, `w-trash`,
/// `w-reading`, `w-rust` = `Lists/Rust`, `h-inbox`, `h-reading`).
pub fn mailbox() -> StaticMailbox {
    let folder = |id: &str, account: &str, path: &str, role| FolderInfo {
        id: id.into(),
        account_id: account.into(),
        path: path.into(),
        name: path.rsplit('/').next().unwrap_or(path).into(),
        role,
    };
    StaticMailbox {
        accounts: vec![
            AccountInfo {
                id: "work".into(),
                name: "Work".into(),
                email: "me@work.example".into(),
                members: vec![],
            },
            AccountInfo {
                id: "home".into(),
                name: "Home".into(),
                email: "me@home.example".into(),
                members: vec![],
            },
            AccountInfo {
                id: "everything".into(),
                name: "Everything".into(),
                email: String::new(),
                members: vec!["work".into(), "home".into()],
            },
        ],
        folders: vec![
            folder("w-inbox", "work", "INBOX", FolderRole::Inbox),
            folder("w-archive", "work", "Archive", FolderRole::Archive),
            folder("w-trash", "work", "Trash", FolderRole::Trash),
            folder("w-reading", "work", "Reading", FolderRole::Other),
            folder("w-rust", "work", "Lists/Rust", FolderRole::Other),
            folder("h-inbox", "home", "INBOX", FolderRole::Inbox),
            folder("h-reading", "home", "Reading", FolderRole::Other),
        ],
    }
}
