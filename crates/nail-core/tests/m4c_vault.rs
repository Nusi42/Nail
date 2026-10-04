//! WP125 (ADRs 0095, 0099): the vault recovery key through the core —
//! create (with the recovery kit), verify, the locked data directory after
//! the keychain entry was lost, restore and reopen, remove.

#![cfg(test)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use nail_api::engine::{MemorySecretStore, SecretStore};
use nail_api::security::VaultState;
use nail_api::{ErrorCode, Request, Response, SecretRef};
use nail_core::{Service, ServiceOptions};
use pretty_assertions::assert_eq;

fn open(data: &Path, config: &Path, secrets: &Arc<MemorySecretStore>) -> Service {
    let mut options = ServiceOptions::new(data, config);
    options.watch_config = false;
    options.start_engines = false;
    options.secrets = Some(secrets.clone());
    Service::open(nail_core::demo::demo_options(options)).unwrap()
}

async fn status(service: &Service) -> nail_api::security::VaultStatus {
    let Response::VaultStatus { status } = service.handle(Request::GetVaultStatus).await else {
        panic!("vault status expected");
    };
    status
}

fn error_of(response: Response) -> (ErrorCode, Option<String>) {
    match response {
        Response::Error {
            code, localized, ..
        } => (code, localized.map(|l| l.id)),
        other => panic!("error expected, got {other:?}"),
    }
}

async fn encrypted(service: &Service) {
    service.start();
    let Response::VaultStatus { .. } = service
        .handle(Request::SetVaultEncryption { enabled: true })
        .await
    else {
        panic!("vault status expected");
    };
    let deadline = tokio::time::Instant::now() + Duration::from_mins(1);
    while status(service).await.state != VaultState::Encrypted {
        assert!(
            tokio::time::Instant::now() < deadline,
            "encryption finishes"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_recovery_key_opens_the_data_after_the_keychain_entry_is_lost() {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let secrets = Arc::new(MemorySecretStore::new());
    let service = open(data.path(), config.path(), &secrets);
    // Plain data: no recovery key.
    assert_eq!(
        error_of(
            service
                .handle(Request::CreateRecoveryKey {
                    save_to: None,
                    overwrite: false,
                })
                .await
        ),
        (ErrorCode::Conflict, Some("vault-not-encrypted".into()))
    );
    let seed = nail_core::demo::seed_demo(service.store(), service.blobs()).unwrap();
    encrypted(&service).await;

    let kit = data.path().join("kit").join("recovery-kit.txt");
    let Response::RecoveryKeyCreated {
        key,
        saved_to,
        status: recovery,
    } = service
        .handle(Request::CreateRecoveryKey {
            save_to: Some(kit.display().to_string()),
            overwrite: false,
        })
        .await
    else {
        panic!("a recovery key expected");
    };
    assert!(recovery.configured);
    assert_eq!(
        saved_to.as_deref(),
        Some(kit.display().to_string().as_str())
    );
    let text = std::fs::read_to_string(&kit).unwrap();
    assert!(text.contains(&key), "the kit holds the key");
    assert!(text.starts_with("Nail recovery key"));
    // Replacing the key with a kit that cannot be written (the file exists)
    // fails before the old key is replaced: the key the user holds still
    // opens the data and the status is unchanged.
    assert_eq!(
        error_of(
            service
                .handle(Request::CreateRecoveryKey {
                    save_to: Some(kit.display().to_string()),
                    overwrite: false,
                })
                .await
        ),
        (
            ErrorCode::Conflict,
            Some("portability-error-target-exists".into())
        )
    );
    assert_eq!(std::fs::read_to_string(&kit).unwrap(), text);
    let after = status(&service).await.recovery;
    assert_eq!(after.created_at, recovery.created_at);
    let Response::RecoveryKeyVerified { valid, status: s } = service
        .handle(Request::VerifyRecoveryKey { key: key.clone() })
        .await
    else {
        panic!("verified expected");
    };
    assert!(valid);
    assert!(s.verified_at.is_some());
    // A mistyped key is a typing problem, a wrong one is not valid.
    assert_eq!(
        error_of(
            service
                .handle(Request::VerifyRecoveryKey { key: "abc".into() })
                .await
        )
        .0,
        ErrorCode::InvalidRequest
    );
    let other = nail_api::recovery::format_recovery_key(&[7u8; 32]);
    let Response::RecoveryKeyVerified { valid, .. } = service
        .handle(Request::VerifyRecoveryKey { key: other.clone() })
        .await
    else {
        panic!("verified expected");
    };
    assert!(!valid);
    service.shutdown().await;
    drop(service);

    // The keychain entry is lost (a new Windows profile).
    let file = nail_vault::VaultFile::load(&data.path().join(nail_vault::VAULT_FILE_NAME)).unwrap();
    let key_ref = SecretRef(file.key_ref.clone().unwrap());
    assert!(secrets.delete(&key_ref).unwrap());

    let locked = open(data.path(), config.path(), &secrets);
    let s = status(&locked).await;
    assert!(s.needs_recovery, "{s:?}");
    assert_eq!(s.state, VaultState::Encrypted);
    // Nothing of the data is readable; the engines stay off.
    let Response::Accounts { accounts } = locked.handle(Request::ListAccounts).await else {
        panic!("accounts expected");
    };
    assert!(!accounts.iter().any(|a| a.id == seed.account.id));
    assert_eq!(
        error_of(locked.handle(Request::RestoreVaultKey { key: other }).await),
        (ErrorCode::AuthFailed, Some("recovery-key-wrong".into()))
    );
    let Response::VaultStatus { status: after } = locked
        .handle(Request::RestoreVaultKey { key: key.clone() })
        .await
    else {
        panic!("status expected");
    };
    assert!(!after.needs_recovery);
    assert_eq!(
        after.localized.map(|l| l.id).as_deref(),
        Some("vault-restart-needed")
    );
    drop(locked);

    // After the restart the data is there again.
    let reopened = open(data.path(), config.path(), &secrets);
    let s = status(&reopened).await;
    assert!(!s.needs_recovery && s.error.is_none(), "{s:?}");
    let Response::Accounts { accounts } = reopened.handle(Request::ListAccounts).await else {
        panic!("accounts expected");
    };
    assert!(accounts.iter().any(|a| a.id == seed.account.id));
    let Response::VaultStatus { status: removed } =
        reopened.handle(Request::RemoveRecoveryKey).await
    else {
        panic!("status expected");
    };
    assert!(!removed.recovery.configured);
}

/// Post-M4c: `nail serve --seed demo-multi` passes the data directory's
/// file keychain; demo-multi's options keep it (instead of swapping in the
/// in-memory one), so the data key of encrypted data survives a restart.
#[tokio::test(flavor = "multi_thread")]
async fn demo_multi_keeps_the_file_keychain_across_a_restart() {
    use nail_core::demo::{DEMO_KEYCHAIN_FILE, FileSecretStore, MULTI_WORK_ID};
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let keychain = data.path().join(DEMO_KEYCHAIN_FILE);
    let open_multi = |demo: &nail_core::demo::DemoMulti| {
        let mut options = ServiceOptions::new(data.path(), config.path());
        options.secrets = Some(Arc::new(FileSecretStore::open(&keychain)));
        let mut options = demo.service_options(options);
        options.watch_config = false;
        options.start_engines = false;
        Service::open(options).unwrap()
    };

    let demo = nail_core::demo::start_demo_multi().await.unwrap();
    let service = open_multi(&demo);
    // The demo accounts' passwords went into the file, not only memory.
    let password = SecretRef::default_for(
        &nail_api::AccountId::new(MULTI_WORK_ID),
        nail_api::accounts::SecretKind::Password,
    );
    assert!(
        FileSecretStore::open(&keychain)
            .get(&password)
            .unwrap()
            .is_some()
    );
    let seed = nail_core::demo::seed_demo(service.store(), service.blobs()).unwrap();
    encrypted(&service).await;
    let file = nail_vault::VaultFile::load(&data.path().join(nail_vault::VAULT_FILE_NAME)).unwrap();
    let key_ref = SecretRef(file.key_ref.clone().unwrap());
    assert!(
        FileSecretStore::open(&keychain)
            .get(&key_ref)
            .unwrap()
            .is_some(),
        "the data key is in the file keychain"
    );
    assert!(
        demo.secrets.get(&key_ref).unwrap().is_none(),
        "not in the in-memory one"
    );
    service.shutdown().await;
    drop(service);
    drop(demo);

    // A restart: new mock servers, the same data directory and keychain file.
    let demo = nail_core::demo::start_demo_multi().await.unwrap();
    let reopened = open_multi(&demo);
    let s = status(&reopened).await;
    assert!(!s.needs_recovery && s.error.is_none(), "{s:?}");
    assert_eq!(s.state, VaultState::Encrypted);
    let Response::Accounts { accounts } = reopened.handle(Request::ListAccounts).await else {
        panic!("accounts expected");
    };
    assert!(accounts.iter().any(|a| a.id == seed.account.id));
    reopened.shutdown().await;
}

/// Post-M4c: a kit file the user chose to replace in the Save dialog
/// (`overwrite`) is replaced atomically with the new key's kit.
#[tokio::test(flavor = "multi_thread")]
async fn a_confirmed_overwrite_replaces_the_recovery_kit() {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let secrets = Arc::new(MemorySecretStore::new());
    let service = open(data.path(), config.path(), &secrets);
    encrypted(&service).await;
    let out = tempfile::tempdir().unwrap();
    let kit = out.path().join("recovery-kit.txt");
    std::fs::write(&kit, "an older kit").unwrap();
    let create = |overwrite: bool| Request::CreateRecoveryKey {
        save_to: Some(kit.display().to_string()),
        overwrite,
    };
    assert_eq!(
        error_of(service.handle(create(false)).await),
        (
            ErrorCode::Conflict,
            Some("portability-error-target-exists".into())
        )
    );
    assert_eq!(std::fs::read_to_string(&kit).unwrap(), "an older kit");
    let Response::RecoveryKeyCreated { key, saved_to, .. } = service.handle(create(true)).await
    else {
        panic!("a recovery key expected");
    };
    assert_eq!(saved_to, Some(kit.display().to_string()));
    let text = std::fs::read_to_string(&kit).unwrap();
    assert!(text.contains(&key), "the new kit holds the new key");
    assert_eq!(
        std::fs::read_dir(out.path()).unwrap().count(),
        1,
        "no temporary file left"
    );
    service.shutdown().await;
}
