//! Requests of wave M2c for per-folder sync policies and bodies
//! (ADR 0043): `GetFolderSyncPolicy`, `SetFolderSyncPolicy`,
//! `FetchMessageBodies`.
//!
//! The policy lives in `accounts.kdl`; `SetFolderSyncPolicy` edits the
//! account's `sync` block through [`Service::save_account`] (which also
//! reloads the engines and announces `AccountsChanged`). The IMAP engines
//! honour `headers-only` / `recent` (`sync.rs`); bodies are fetched on
//! demand (`bodies.rs`).

use nail_api::accounts::{AccountConfig, FolderMatch, FolderPolicy, IncomingServer};
use nail_api::sync::FolderSyncPolicy;
use nail_api::{ApiError, Folder, Request, Response};

use crate::error::Result;
use crate::service::Service;

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    Ok(match request {
        Request::GetFolderSyncPolicy { folder_id } => {
            let folder = service
                .store()
                .call(move |s| s.get_folder(&folder_id))
                .await?;
            let config = service.inner.accounts.config();
            let account = config.account(&folder.account_id).ok_or_else(|| {
                ApiError::unsupported(format!(
                    "folder {} does not belong to a configured account",
                    folder.id
                ))
            })?;
            Response::FolderSyncPolicy {
                policy: resolve(account, &folder),
            }
        }
        Request::SetFolderSyncPolicy {
            folder_id,
            bodies,
            enabled,
            idle,
        } => {
            if let Some(nail_api::accounts::BodyPolicy::Recent { days: 0 }) = bodies {
                return Err(ApiError::invalid_request("`recent` needs at least 1 day").into());
            }
            let folder = service
                .store()
                .call(move |s| s.get_folder(&folder_id))
                .await?;
            let config = service.inner.accounts.config();
            let mut account = config
                .account(&folder.account_id)
                .ok_or_else(|| {
                    ApiError::unsupported(format!(
                        "folder {} does not belong to a configured account",
                        folder.id
                    ))
                })?
                .clone();
            set_rule(&mut account, &folder.path, bodies, enabled, idle);
            let saved = service.save_account(account, &[], false).await?;
            Response::FolderSyncPolicy {
                policy: resolve(&saved, &folder),
            }
        }
        Request::FetchMessageBodies { message_ids } => {
            let (fetched, failed) = service.fetch_bodies(message_ids).await?;
            Response::BodiesFetched { fetched, failed }
        }
        other => {
            return Err(ApiError::internal(format!(
                "{} is not a sync-policy request",
                crate::service::request_name(&other)
            ))
            .into());
        }
    })
}

/// The resolved policy of `folder` in `account`.
pub(crate) fn resolve(account: &AccountConfig, folder: &Folder) -> FolderSyncPolicy {
    let effective = account.sync.effective(&folder.path, folder.role);
    let rule = account
        .sync
        .folders
        .iter()
        .rev()
        .find(|r| matches!(&r.folder, FolderMatch::Path(p) if p == &folder.path))
        .cloned();
    let imap = matches!(account.incoming, IncomingServer::Imap { .. });
    FolderSyncPolicy {
        folder_id: folder.id.clone(),
        account_id: folder.account_id.clone(),
        path: folder.path.clone(),
        bodies: if imap {
            effective.bodies
        } else {
            nail_api::accounts::BodyPolicy::Full
        },
        enabled: effective.enabled,
        idle: effective.idle,
        rule,
        on_demand: imap,
    }
}

/// Replaces the rules written for exactly `path` with one rule (or none
/// when every field is `None`).
pub(crate) fn set_rule(
    account: &mut AccountConfig,
    path: &str,
    bodies: Option<nail_api::accounts::BodyPolicy>,
    enabled: Option<bool>,
    idle: Option<bool>,
) {
    let folders = &mut account.sync.folders;
    let position = folders
        .iter()
        .position(|r| matches!(&r.folder, FolderMatch::Path(p) if p == path));
    folders.retain(|r| !matches!(&r.folder, FolderMatch::Path(p) if p == path));
    if bodies.is_none() && enabled.is_none() && idle.is_none() {
        return;
    }
    let rule = FolderPolicy {
        folder: FolderMatch::Path(path.to_owned()),
        bodies,
        enabled,
        idle,
    };
    let at = position.unwrap_or(folders.len()).min(folders.len());
    folders.insert(at, rule);
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::accounts::{
        AuthConfig, BodyPolicy, ImapOptions, ServerEndpoint, SyncPolicy, TlsMode,
    };
    use nail_api::domain::FolderRole;

    fn account() -> AccountConfig {
        AccountConfig {
            id: "work".into(),
            name: "Work".into(),
            email: "ada@example.org".into(),
            enabled: true,
            incoming: IncomingServer::Imap {
                server: ServerEndpoint {
                    host: "imap.example.org".into(),
                    port: 993,
                    tls: TlsMode::Implicit,
                    auth: AuthConfig::None,
                },
                options: ImapOptions::default(),
            },
            outgoing: None,
            identities: vec![],
            sync: SyncPolicy {
                bodies: Some(BodyPolicy::Recent { days: 30 }),
                folders: vec![FolderPolicy {
                    folder: FolderMatch::Role(FolderRole::Archive),
                    bodies: Some(BodyPolicy::HeadersOnly),
                    enabled: None,
                    idle: None,
                }],
            },
            save_sent_copy: None,
            authserv_ids: vec![],
            sieve: None,
        }
    }

    fn folder(path: &str, role: FolderRole) -> Folder {
        Folder {
            id: "f1".into(),
            account_id: "work".into(),
            parent_id: None,
            name: path.into(),
            path: path.into(),
            role,
            unread: 0,
            total: 0,
        }
    }

    #[test]
    fn resolves_and_edits_the_folder_rule() {
        let mut a = account();
        let inbox = folder("INBOX", FolderRole::Inbox);
        let policy = resolve(&a, &inbox);
        assert_eq!(policy.bodies, BodyPolicy::Recent { days: 30 });
        assert!(policy.idle && policy.enabled && policy.on_demand);
        assert_eq!(policy.rule, None);

        set_rule(&mut a, "INBOX", Some(BodyPolicy::Full), None, Some(false));
        let policy = resolve(&a, &inbox);
        assert_eq!(policy.bodies, BodyPolicy::Full);
        assert!(!policy.idle);
        assert_eq!(
            policy.rule.as_ref().map(|r| &r.folder),
            Some(&FolderMatch::Path("INBOX".into()))
        );
        // Editing again replaces the rule in place; all `None` removes it.
        set_rule(&mut a, "INBOX", Some(BodyPolicy::HeadersOnly), None, None);
        assert_eq!(a.sync.folders.len(), 2);
        assert_eq!(resolve(&a, &inbox).bodies, BodyPolicy::HeadersOnly);
        set_rule(&mut a, "INBOX", None, None, None);
        assert_eq!(a.sync.folders.len(), 1);

        let archive = folder("Archive", FolderRole::Archive);
        assert_eq!(resolve(&a, &archive).bodies, BodyPolicy::HeadersOnly);
    }
}
