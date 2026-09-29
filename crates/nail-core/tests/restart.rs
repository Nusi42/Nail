//! Carry-over from M2a (ADR 0041): with encryption at rest on, a restarted
//! core with the same keychain reads its data again — mail, search and
//! the vault state — and the files on disk stay sealed.

use std::sync::Arc;
use std::time::Duration;

use nail_api::engine::{MemorySecretStore, SecretStore};
use nail_api::security::VaultState;
use nail_api::{AccountId, MessageBody, Request, Response, SearchQuery};
use nail_core::demo::{DEMO_ACCOUNT_ID, demo_options, seed_demo};
use nail_core::{Service, ServiceOptions};
use pretty_assertions::assert_eq;

fn open(
    data: &std::path::Path,
    config: &std::path::Path,
    secrets: &Arc<dyn SecretStore>,
) -> Service {
    let mut options = ServiceOptions::new(data, config);
    options.watch_config = false;
    options.secrets = Some(Arc::clone(secrets));
    Service::open(demo_options(options)).unwrap()
}

async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    for _ in 0..300 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn vault(service: &Service) -> VaultState {
    match service.handle(Request::GetVaultStatus).await {
        Response::VaultStatus { status } => status.state,
        other => panic!("{other:?}"),
    }
}

async fn hits(service: &Service, text: &str) -> u32 {
    match service
        .handle(Request::Search {
            query: SearchQuery {
                text: text.into(),
                account_id: None,
                folder_id: None,
                offset: 0,
                limit: 50,
                sort: Default::default(),
            },
        })
        .await
    {
        Response::SearchResults { total, .. } => total,
        _ => 0,
    }
}

async fn read(service: &Service, id: &nail_api::MessageId) -> MessageBody {
    match service.handle(Request::GetMessage { id: id.clone() }).await {
        Response::Message { message } => *message,
        other => panic!("{other:?}"),
    }
}

fn files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(files(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_encrypted_profile_reads_again_after_a_restart() {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());

    let first = open(data.path(), config.path(), &secrets);
    seed_demo(first.store(), first.blobs()).unwrap();
    first.rebuild_search_index().unwrap();
    let folder = first
        .store()
        .list_folders(&AccountId::new(DEMO_ACCOUNT_ID))
        .unwrap()
        .remove(0);
    let message = first
        .store()
        .list_messages(&folder.id, 0, 1)
        .unwrap()
        .messages
        .remove(0);
    let before = read(&first, &message.id).await;
    let word = before
        .headers
        .subject
        .split_whitespace()
        .max_by_key(|w| w.len())
        .unwrap()
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_owned();
    eventually("the index", || async { hits(&first, &word).await > 0 }).await;

    first
        .handle(Request::SetVaultEncryption { enabled: true })
        .await;
    eventually("encrypted", || async {
        vault(&first).await == VaultState::Encrypted
    })
    .await;
    eventually("the index answers again", || async {
        hits(&first, &word).await > 0
    })
    .await;
    first.shutdown().await;
    drop(first);

    // Every blob on disk is sealed.
    let blobs = files(&data.path().join("blobs"));
    assert!(!blobs.is_empty());
    assert!(
        blobs.iter().all(|p| std::fs::read(p)
            .unwrap()
            .starts_with(&nail_store::SEALED_MAGIC)),
        "{blobs:?}"
    );

    // Same data directory, same keychain: everything reads again.
    let second = open(data.path(), config.path(), &secrets);
    assert_eq!(vault(&second).await, VaultState::Encrypted);
    assert_eq!(read(&second, &message.id).await, before);
    eventually("the sealed index after the restart", || async {
        hits(&second, &word).await > 0
    })
    .await;
    second.shutdown().await;
    drop(second);

    // Without the key (another keychain) nothing is readable.
    let stranger: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    let third = open(data.path(), config.path(), &stranger);
    let locked = third
        .handle(Request::GetMessage {
            id: message.id.clone(),
        })
        .await;
    assert!(matches!(locked, Response::Error { .. }), "{locked:?}");
    third.shutdown().await;
}
