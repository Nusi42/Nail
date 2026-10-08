//! The engine of one POP3 account: a scheduled fetch (and "sync now").
//!
//! Each fetch replays the account's (local) writes first, so deletions the
//! user made are known, then:
//!
//! 1. `UIDL` / `LIST` the server;
//! 2. download what is not known yet into the local Inbox
//!    ([`nail_proto_pop3::plan_downloads`]);
//! 3. delete from the server what the leave-on-server policy says
//!    ([`nail_proto_pop3::plan_deletions`]) and commit with `QUIT`;
//! 4. forget bookkeeping of messages gone from the server.
//!
//! POP3 has no folders: the account's folders (Inbox, Sent, Archive,
//! Trash) are local and created with the account.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use nail_api::accounts::Pop3Options;
use nail_api::{AccountConfig, AccountId, AccountState, Event, Folder, FolderRole, IncomingServer};
use nail_proto_pop3::{LeavePolicy, Pop3Client, plan_deletions, plan_downloads, vanished};
use tokio::sync::watch;

use crate::creds::{EndpointCredentials, pop3_config, pop3_credentials};
use crate::engine::{Signals, stopped};
use crate::error::{CoreError, Result};
use crate::ids;
use crate::ingest::{IncomingMessage, ingest};
use crate::replay::replay_locally;
use crate::service::Service;
use nail_util::time::unix_now;

/// The local folders of a POP3 account: path, role.
pub(crate) const POP3_FOLDERS: [(&str, FolderRole); 4] = [
    ("INBOX", FolderRole::Inbox),
    ("Sent", FolderRole::Sent),
    ("Archive", FolderRole::Archive),
    ("Trash", FolderRole::Trash),
];

/// The local folders of a POP3 account as store rows.
pub(crate) fn pop3_folders(account: &AccountId) -> Vec<Folder> {
    POP3_FOLDERS
        .iter()
        .map(|(path, role)| Folder {
            id: ids::folder_id(account, path),
            account_id: account.clone(),
            parent_id: None,
            name: (*path).to_owned(),
            path: (*path).to_owned(),
            role: *role,
            unread: 0,
            total: 0,
        })
        .collect()
}

/// What one fetch did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FetchReport {
    pub downloaded: usize,
    pub deleted: usize,
}

/// One fetch (see the module docs).
pub(crate) async fn fetch(
    service: &Service,
    account: &AccountId,
    server: &nail_api::ServerEndpoint,
    options: &Pop3Options,
    credentials: &EndpointCredentials,
) -> Result<FetchReport> {
    let login = credentials.login(false).await?;
    let mut client = Pop3Client::connect(&pop3_config(server)).await?;
    client.login(&pop3_credentials(login)).await?;
    let listing = client.messages().await?;
    let state = {
        let account = account.clone();
        service
            .store()
            .call(move |s| s.pop3_state(&account))
            .await?
    };
    let inbox = pop3_folders(account).remove(0);
    let mut report = FetchReport::default();
    let mut downloaded = state.downloaded.clone();
    let mut new_ids = Vec::new();
    let downloads: Vec<_> = plan_downloads(&listing, &state.known)
        .into_iter()
        .cloned()
        .collect();
    let total = u32::try_from(downloads.len())
        .unwrap_or(u32::MAX)
        .saturating_add(1);
    service.broadcast(Event::SyncProgress {
        account_id: account.clone(),
        done: 0,
        total,
    });
    for (n, message) in downloads.iter().enumerate() {
        let raw = client.retr(message.number).await?;
        let id = ids::pop3_message_id(account, &message.uid);
        let now = unix_now();
        let incoming = IncomingMessage {
            uid: None,
            id: Some(id.clone()),
            raw,
            ..IncomingMessage::default()
        };
        let folder = inbox.clone();
        let blobs = service.blobs().clone();
        let index = service.inner.search.get();
        let (account_id, uid) = (account.clone(), message.uid.clone());
        let stored = service
            .store()
            .call(move |s| {
                let report = ingest(s, &blobs, Some(&index), &folder, 0, &[incoming])?;
                s.pop3_record(&account_id, &uid, &id, now)?;
                Ok(report)
            })
            .await?;
        downloaded.insert(message.uid.clone(), now);
        new_ids.extend(stored.new);
        report.downloaded += 1;
        service.broadcast(Event::SyncProgress {
            account_id: account.clone(),
            done: u32::try_from(n + 1).unwrap_or(u32::MAX),
            total,
        });
    }
    let policy = LeavePolicy::from(options);
    let deletions: Vec<_> = plan_deletions(
        &policy,
        &listing,
        &downloaded,
        &state.locally_deleted,
        unix_now(),
    )
    .into_iter()
    .cloned()
    .collect();
    for message in &deletions {
        client.dele(message.number).await?;
    }
    client.quit().await?;
    report.deleted = deletions.len();
    let known: Vec<String> = downloaded.keys().cloned().collect();
    let mut forget = vanished(&known, &listing);
    let deleted: HashSet<&str> = deletions.iter().map(|m| m.uid.as_str()).collect();
    forget.extend(deleted.iter().map(|u| (*u).to_owned()));
    if !forget.is_empty() {
        let account = account.clone();
        service
            .store()
            .call(move |s| s.pop3_forget(&account, &forget))
            .await?;
    }
    if report.downloaded > 0 {
        service.broadcast(Event::MailChanged {
            folder_ids: vec![inbox.id.clone()],
        });
    }
    if !new_ids.is_empty() && !state.known.is_empty() {
        let queued = new_ids.clone();
        crate::rules::queue_arrivals(service, queued).await;
        service.broadcast(Event::NewMail {
            account_id: account.clone(),
            folder_id: inbox.id.clone(),
            message_ids: new_ids,
        });
    }
    service.broadcast(Event::SyncProgress {
        account_id: account.clone(),
        done: total,
        total,
    });
    Ok(report)
}

/// Runs the engine until `stop` is set.
pub(crate) async fn run(
    service: Service,
    account: AccountConfig,
    signals: Arc<Signals>,
    mut stop: watch::Receiver<bool>,
) {
    let IncomingServer::Pop3 { server, options } = &account.incoming else {
        return;
    };
    let credentials = service.endpoint_credentials(&account.id, &server.auth);
    let timing = service.inner.timing;
    let poll = timing.poll_override.unwrap_or_else(|| {
        Duration::from_secs(u64::from(
            options
                .poll_interval_secs
                .unwrap_or(Pop3Options::DEFAULT_POLL_INTERVAL_SECS),
        ))
    });
    let mut backoff = timing.reconnect_min;
    let mut first = true;
    loop {
        if *stop.borrow() {
            break;
        }
        if first {
            service.set_status(&account.id, AccountState::Connecting, None);
            first = false;
        }
        replay_locally(&service, &account.id).await;
        let result = tokio::select! {
            result = fetch(&service, &account.id, server, options, &credentials) => result,
            () = stopped(&mut stop) => break,
        };
        let delay = match result {
            Ok(report) => {
                tracing::info!(account = %account.id, ?report, "POP3 fetch finished");
                backoff = timing.reconnect_min;
                service.set_status(&account.id, AccountState::Online, None);
                poll
            }
            Err(err) => {
                let err: CoreError = err;
                tracing::info!(account = %account.id, %err, "POP3 fetch failed");
                service.report_sync_failure(&account.id, &err);
                let delay = backoff;
                backoff = (backoff * 2).min(timing.reconnect_max);
                delay
            }
        };
        // Queued writes are replayed at once; a fetch waits for the timer
        // or "sync now".
        let deadline = tokio::time::Instant::now() + delay;
        loop {
            tokio::select! {
                () = tokio::time::sleep_until(deadline) => break,
                () = signals.sync.notified() => break,
                () = signals.replay.notified() => replay_locally(&service, &account.id).await,
                () = stopped(&mut stop) => return,
            }
        }
    }
    tracing::info!(account = %account.id, "engine stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pop3_accounts_get_local_folders() {
        let folders = pop3_folders(&AccountId::new("home"));
        assert_eq!(folders.len(), 4);
        assert_eq!(folders[0].role, FolderRole::Inbox);
        assert_eq!(folders[3].path, "Trash");
        assert_eq!(folders[0].id, ids::folder_id(&"home".into(), "INBOX"));
    }
}
