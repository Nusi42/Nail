//! Bodies fetched on demand (ADR 0048; sync policies of ADR 0043, store
//! and IMAP parts of ADR 0046): messages synced "headers only" (or aged
//! out of "recent N days") get their body from the server when opened
//! (`GetMessage`) or on request (`FetchMessageBodies`). The fetch uses a
//! connection of its own, so it does not wait for a running sync pass.

use std::collections::BTreeMap;

use nail_api::{AccountId, Event, FolderId, MessageId};
use nail_search::IndexDocument;
use nail_store::BodyFetchTarget;

use crate::error::Result;
use crate::service::Service;
use crate::sync::connect;

impl Service {
    /// Fetches the bodies of `ids` that are not stored. Returns the
    /// messages whose body is stored now (fetched, or stored before) and
    /// those that could not be fetched (offline, gone, not IMAP).
    pub(crate) async fn fetch_bodies(
        &self,
        ids: Vec<MessageId>,
    ) -> Result<(Vec<MessageId>, Vec<MessageId>)> {
        let lookup = ids.clone();
        let (targets, known) = self
            .store()
            .call(move |s| {
                let targets = s.body_fetch_targets(&lookup)?;
                let known: Vec<MessageId> = lookup
                    .iter()
                    .filter(|id| s.get_message(id).is_ok())
                    .cloned()
                    .collect();
                Ok((targets, known))
            })
            .await?;
        let mut fetched: Vec<MessageId> = known
            .iter()
            .filter(|id| !targets.iter().any(|t| &t.message_id == *id))
            .cloned()
            .collect();
        let mut failed: Vec<MessageId> = ids
            .iter()
            .filter(|id| !known.contains(id))
            .cloned()
            .collect();
        let mut by_account: BTreeMap<AccountId, Vec<BodyFetchTarget>> = BTreeMap::new();
        for target in targets {
            by_account
                .entry(target.account_id.clone())
                .or_default()
                .push(target);
        }
        let mut changed: Vec<FolderId> = Vec::new();
        for (account, targets) in by_account {
            let (ok, bad) = self
                .fetch_account_bodies(&account, targets, &mut changed)
                .await;
            fetched.extend(ok);
            failed.extend(bad);
        }
        if !changed.is_empty() {
            self.broadcast(Event::MailChanged {
                folder_ids: changed,
            });
        }
        Ok((fetched, failed))
    }

    async fn fetch_account_bodies(
        &self,
        account: &AccountId,
        targets: Vec<BodyFetchTarget>,
        changed: &mut Vec<FolderId>,
    ) -> (Vec<MessageId>, Vec<MessageId>) {
        let all: Vec<MessageId> = targets.iter().map(|t| t.message_id.clone()).collect();
        // M4c: Exchange folders with a headers-only policy fetch the MIME.
        if let Some(config) = self.inner.accounts.config().account(account).cloned()
            && matches!(
                config.incoming,
                nail_api::accounts::IncomingServer::Exchange { .. }
            )
        {
            return self.fetch_exchange_bodies(&config, targets, changed).await;
        }
        let Some(settings) = self.imap_settings(account) else {
            return (Vec::new(), all);
        };
        let mut client = match connect(&settings).await {
            Ok(client) => client,
            Err(err) => {
                tracing::info!(%account, %err, "cannot connect to fetch bodies");
                return (Vec::new(), all);
            }
        };
        let (mut ok, mut bad) = (Vec::new(), Vec::new());
        for target in targets {
            let Some(uid) = target.uid else {
                bad.push(target.message_id);
                continue;
            };
            let message = match client.fetch_body(&target.folder_path, uid).await {
                Ok(Some(message)) if !message.headers_only => message,
                Ok(_) => {
                    bad.push(target.message_id);
                    continue;
                }
                Err(err) => {
                    tracing::info!(%account, %err, "cannot fetch a body");
                    bad.push(target.message_id);
                    if client.is_broken() {
                        break;
                    }
                    continue;
                }
            };
            let id = target.message_id.clone();
            let blobs = self.blobs().clone();
            let index = self.inner.search.get();
            let account_id = account.clone();
            let stored = self
                .store()
                .call(move |s| {
                    let hash = blobs.put(&message.raw)?;
                    let old = s.attach_body(&id, &hash, message.raw.len() as u64)?;
                    s.remove_unreferenced_blobs(&blobs, &old.into_iter().collect::<Vec<_>>())?;
                    let doc = IndexDocument::from_raw(id.clone(), account_id, &message.raw);
                    if let Err(err) = index.add(&[doc]) {
                        tracing::warn!(%err, "cannot index a fetched body");
                    }
                    Ok(())
                })
                .await;
            match stored {
                Ok(()) => {
                    if !changed.contains(&target.folder_id) {
                        changed.push(target.folder_id.clone());
                    }
                    ok.push(target.message_id);
                }
                Err(err) => {
                    tracing::warn!(%err, "cannot store a fetched body");
                    bad.push(target.message_id);
                }
            }
        }
        drop(client.logout().await);
        (ok, bad)
    }

    /// Bodies of an Exchange account's headers-only messages (M4c): the
    /// MIME by remote id through the account's engine.
    async fn fetch_exchange_bodies(
        &self,
        account: &nail_api::accounts::AccountConfig,
        targets: Vec<BodyFetchTarget>,
        changed: &mut Vec<FolderId>,
    ) -> (Vec<MessageId>, Vec<MessageId>) {
        let all: Vec<MessageId> = targets.iter().map(|t| t.message_id.clone()).collect();
        let engine = match crate::exchange::engine_of(self, account) {
            Ok(engine) => engine,
            Err(err) => {
                tracing::info!(account = %account.id, %err, "no Exchange engine to fetch bodies");
                return (Vec::new(), all);
            }
        };
        let (mut ok, mut bad) = (Vec::new(), Vec::new());
        for target in targets {
            let lookup = (target.message_id.clone(), target.folder_id.clone());
            let remote = self
                .store()
                .call(move |s| {
                    Ok((
                        s.message_remote_id(&lookup.0)?,
                        s.exchange_folder(&lookup.1)?.and_then(|f| f.mailbox),
                    ))
                })
                .await;
            let Ok((Some(remote_id), mailbox)) = remote else {
                bad.push(target.message_id);
                continue;
            };
            let raw = match engine.fetch_mime(mailbox.as_deref(), &remote_id).await {
                Ok(raw) => raw,
                Err(err) => {
                    tracing::info!(account = %account.id, %err, "cannot fetch a body");
                    bad.push(target.message_id);
                    continue;
                }
            };
            let id = target.message_id.clone();
            let blobs = self.blobs().clone();
            let index = self.inner.search.get();
            let account_id = account.id.clone();
            let stored = self
                .store()
                .call(move |s| {
                    let hash = blobs.put(&raw)?;
                    let old = s.attach_body(&id, &hash, raw.len() as u64)?;
                    s.remove_unreferenced_blobs(&blobs, &old.into_iter().collect::<Vec<_>>())?;
                    let doc = IndexDocument::from_raw(id.clone(), account_id, &raw);
                    if let Err(err) = index.add(&[doc]) {
                        tracing::warn!(%err, "cannot index a fetched body");
                    }
                    Ok(())
                })
                .await;
            match stored {
                Ok(()) => {
                    if !changed.contains(&target.folder_id) {
                        changed.push(target.folder_id.clone());
                    }
                    ok.push(target.message_id);
                }
                Err(err) => {
                    tracing::warn!(%err, "cannot store a fetched body");
                    bad.push(target.message_id);
                }
            }
        }
        (ok, bad)
    }
}
