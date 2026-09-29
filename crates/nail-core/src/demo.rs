//! Demo data: the `nail-testkit` corpus as a local account, and an
//! in-process mock IMAP server serving the same corpus.
//!
//! Used by `nail serve --seed demo` (UI development, Playwright) and
//! `nail serve --seed demo-imap` (the full sync path without a real server).
//! No network access is involved in either.

use nail_api::{Account, AccountId, AccountKind, Folder};
use nail_proto_imap::{Credentials, ImapConfig, Security};
use nail_store::{BlobStore, Store};
use nail_testkit::{MockImapServer, demo_corpus};

use crate::error::Result;
use crate::ids;
use crate::ingest::{IncomingMessage, ingest};
use crate::sync::ImapAccount;

/// Id of the local demo account created by [`seed_demo`].
pub const DEMO_ACCOUNT_ID: &str = "demo";
/// Id of the IMAP demo account served by [`start_demo_imap`].
pub const DEMO_IMAP_ACCOUNT_ID: &str = "demo-imap";

/// The address the demo corpus is written to.
const DEMO_EMAIL: &str = "sam@nail.example";

/// What [`seed_demo`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoSeed {
    /// The demo account.
    pub account: Account,
    /// Its folders, in corpus order (INBOX first).
    pub folders: Vec<Folder>,
    /// Number of messages written.
    pub messages: usize,
}

/// Writes the demo corpus into `store` and `blobs` as a local account
/// ([`DEMO_ACCOUNT_ID`]). Idempotent: ids are derived from the corpus, so
/// seeding again updates the same rows. Blocking.
pub fn seed_demo(store: &Store, blobs: &BlobStore) -> Result<DemoSeed> {
    let account = Account {
        id: AccountId::new(DEMO_ACCOUNT_ID),
        name: "Demo".to_owned(),
        email: DEMO_EMAIL.to_owned(),
        kind: AccountKind::Local,
    };
    store.upsert_account(&account)?;

    let mut folders = Vec::new();
    let mut messages = 0;
    for corpus_folder in demo_corpus() {
        let folder = Folder {
            id: ids::folder_id(&account.id, &corpus_folder.name),
            account_id: account.id.clone(),
            parent_id: None,
            name: corpus_folder.name.clone(),
            path: corpus_folder.name.clone(),
            role: corpus_folder.role,
            unread: 0,
            total: 0,
        };
        store.upsert_folder(&folder)?;
        let incoming: Vec<IncomingMessage> = corpus_folder
            .messages
            .iter()
            .zip(1u32..)
            .map(|(message, uid)| IncomingMessage {
                uid,
                flags: message.api_flags(),
                internal_date: None,
                raw: message.raw.clone(),
            })
            .collect();
        messages += ingest(store, blobs, &folder, 1, &incoming)?;
        folders.push(store.get_folder(&folder.id)?);
    }
    tracing::info!(folders = folders.len(), messages, "demo data seeded");
    Ok(DemoSeed {
        account,
        folders,
        messages,
    })
}

/// A running mock IMAP server with the demo corpus, plus the account and
/// settings to sync it. The server stops when this is dropped.
#[derive(Debug)]
pub struct DemoImap {
    /// The server (plaintext, on `127.0.0.1`).
    pub server: MockImapServer,
    /// The account to register ([`DEMO_IMAP_ACCOUNT_ID`]).
    pub account: Account,
    /// Settings pointing at the server.
    pub settings: ImapAccount,
}

/// Starts an in-process mock IMAP server serving the demo corpus.
pub async fn start_demo_imap() -> Result<DemoImap> {
    let server = MockImapServer::start_demo()
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let settings = ImapAccount {
        config: ImapConfig::new(server.host())
            .with_port(server.port())
            .with_security(Security::InsecurePlaintext),
        credentials: Credentials::new(&server.config().username, &server.config().password),
    };
    let account = Account {
        id: AccountId::new(DEMO_IMAP_ACCOUNT_ID),
        name: "Demo (IMAP)".to_owned(),
        email: server.config().username.clone(),
        kind: AccountKind::Imap,
    };
    Ok(DemoImap {
        server,
        account,
        settings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::FolderRole;

    #[test]
    fn seeds_the_corpus_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let blobs = BlobStore::open(dir.path().join("blobs")).unwrap();

        let seed = seed_demo(&store, &blobs).unwrap();
        let corpus = demo_corpus();
        let expected: usize = corpus.iter().map(|f| f.messages.len()).sum();
        assert_eq!(seed.messages, expected);
        assert_eq!(seed.folders[0].role, FolderRole::Inbox);
        assert_eq!(
            seed.folders.iter().map(|f| f.total as usize).sum::<usize>(),
            expected
        );

        let again = seed_demo(&store, &blobs).unwrap();
        assert_eq!(again, seed);
        assert_eq!(store.list_accounts().unwrap(), vec![seed.account]);
    }
}
