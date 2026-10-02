//! Moves between mailboxes and accounts of API engines (plan M4c WP123).
//!
//! A move inside one mailbox is the engine's own `move`
//! ([`nail_api::engine::RemoteWrite::Move`]). Graph cannot move between
//! mailboxes, EWS only where the server allows it, and nothing moves
//! between accounts — so a cross-mailbox or cross-account move is
//! **fetch MIME → append in the target → delete at the source**, written so
//! a crash at any point replays safely: appends find an earlier copy by its
//! `Message-ID` (Graph, EWS and Gmail engines are idempotent there), and a
//! source that is already gone is not an error.
//!
//! [`ApiMailbox`] names either side: an Exchange mailbox (an
//! [`ExchangeMailEngine`] plus the shared mailbox, `None` = the user's) or
//! an account of a [`MailApiEngine`] (Gmail). The core runs [`transfer`]
//! from the operation queue (one queued move = one transfer).

use nail_api::ErrorCode;
use nail_api::domain::Flags;
use nail_api::engine::{EngineError, MailApiEngine, RemoteWrite, WriteOutcome};
use nail_api::exchange::engine::ExchangeMailEngine;

/// One side of a transfer.
#[derive(Clone, Copy)]
pub enum ApiMailbox<'a> {
    /// A mailbox of an Exchange account.
    Exchange {
        /// The account's engine.
        engine: &'a dyn ExchangeMailEngine,
        /// A shared / delegate mailbox (`None` = the user's).
        mailbox: Option<&'a str>,
    },
    /// An account of a REST mail engine (Gmail).
    Api(&'a dyn MailApiEngine),
}

impl std::fmt::Debug for ApiMailbox<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exchange { mailbox, .. } => f
                .debug_struct("Exchange")
                .field("mailbox", mailbox)
                .finish_non_exhaustive(),
            Self::Api(_) => f.write_str("Api"),
        }
    }
}

impl ApiMailbox<'_> {
    async fn fetch(&self, remote_id: &str) -> Result<Vec<u8>, EngineError> {
        match self {
            Self::Exchange { engine, mailbox } => engine.fetch_mime(*mailbox, remote_id).await,
            Self::Api(engine) => engine.fetch_raw(remote_id).await,
        }
    }

    async fn write(&self, write: &RemoteWrite) -> Result<WriteOutcome, EngineError> {
        match self {
            Self::Exchange { engine, mailbox } => engine.write(*mailbox, write).await,
            Self::Api(engine) => engine.write(write).await,
        }
    }
}

/// What a [`transfer`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferOutcome {
    /// The message is in the target now (with its new remote id, when the
    /// engine reported one) and gone from the source.
    Moved {
        /// The remote id in the target.
        remote_id: Option<String>,
    },
    /// The source message no longer exists: nothing was copied (the next
    /// sync removes the local copy, like [`WriteOutcome::Gone`]).
    Gone,
}

/// Moves a message from `source` to `folder` of `target` (see the module
/// docs). Flags travel with the copy; tags (categories / labels) are the
/// core's to reapply through the target's queue.
pub async fn transfer(
    source: ApiMailbox<'_>,
    remote_id: &str,
    target: ApiMailbox<'_>,
    folder: &str,
    flags: Flags,
) -> Result<TransferOutcome, EngineError> {
    let raw = match source.fetch(remote_id).await {
        Ok(raw) => raw,
        Err(e) if e.code == ErrorCode::NotFound => return Ok(TransferOutcome::Gone),
        Err(e) => return Err(e),
    };
    let appended = target
        .write(&RemoteWrite::Append {
            folder: folder.to_owned(),
            raw,
            flags,
        })
        .await?;
    let new_id = match appended {
        WriteOutcome::Done { remote_id } => remote_id,
        // The target folder vanished: keep the source.
        WriteOutcome::Gone => {
            return Err(EngineError::permanent(
                ErrorCode::NotFound,
                format!("the target folder {folder} is gone"),
            ));
        }
    };
    match source
        .write(&RemoteWrite::Delete {
            remote_id: remote_id.to_owned(),
        })
        .await
    {
        Ok(_) => {}
        Err(e) if e.code == ErrorCode::NotFound => {}
        Err(e) => return Err(e),
    }
    Ok(TransferOutcome::Moved { remote_id: new_id })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use nail_api::engine::{BoxFuture, DeltaOutcome, FullSyncPage, RemoteFolder, RemoteWrite as W};
    use nail_api::exchange::InferenceClassification;
    use nail_api::exchange::engine::{ExchangeServerInfo, HierarchyOutcome};

    use super::*;

    type Items = BTreeMap<(Option<String>, String), (String, Vec<u8>, Flags)>;

    /// An in-memory store: (mailbox, id) → (folder, raw, flags).
    #[derive(Debug, Default)]
    struct Fake {
        items: Mutex<Items>,
        next: Mutex<u32>,
        fail_delete: bool,
        writes: Mutex<Vec<String>>,
    }

    impl Fake {
        fn put(&self, mailbox: Option<&str>, id: &str, folder: &str, raw: &[u8]) {
            self.items.lock().unwrap().insert(
                (mailbox.map(str::to_owned), id.to_owned()),
                (folder.to_owned(), raw.to_vec(), Flags::default()),
            );
        }

        fn get(&self, mailbox: Option<&str>, id: &str) -> Option<(String, Vec<u8>, Flags)> {
            self.items
                .lock()
                .unwrap()
                .get(&(mailbox.map(str::to_owned), id.to_owned()))
                .cloned()
        }

        fn fetch(&self, mailbox: Option<&str>, id: &str) -> Result<Vec<u8>, EngineError> {
            self.get(mailbox, id)
                .map(|(_, raw, _)| raw)
                .ok_or_else(|| EngineError::permanent(ErrorCode::NotFound, "gone"))
        }

        fn apply(&self, mailbox: Option<&str>, write: &W) -> Result<WriteOutcome, EngineError> {
            let key = |id: &str| (mailbox.map(str::to_owned), id.to_owned());
            match write {
                W::Append { folder, raw, flags } => {
                    self.writes.lock().unwrap().push("append".into());
                    // Idempotent by content (stands in for Message-ID).
                    let mut items = self.items.lock().unwrap();
                    if let Some(((_, id), _)) = items.iter().find(|((m, _), (f, r, _))| {
                        m.as_deref() == mailbox && f == folder && r == raw
                    }) {
                        return Ok(WriteOutcome::Done {
                            remote_id: Some(id.clone()),
                        });
                    }
                    let mut next = self.next.lock().unwrap();
                    *next += 1;
                    let id = format!("new-{next}");
                    items.insert(key(&id), (folder.clone(), raw.clone(), *flags));
                    Ok(WriteOutcome::Done {
                        remote_id: Some(id),
                    })
                }
                W::Delete { remote_id } => {
                    self.writes.lock().unwrap().push("delete".into());
                    if self.fail_delete {
                        return Err(EngineError::temporary(ErrorCode::Network, "offline"));
                    }
                    match self.items.lock().unwrap().remove(&key(remote_id)) {
                        Some(_) => Ok(WriteOutcome::Done { remote_id: None }),
                        None => Err(EngineError::permanent(ErrorCode::NotFound, "gone")),
                    }
                }
                _ => Err(EngineError::permanent(ErrorCode::Unsupported, "fake")),
            }
        }
    }

    impl ExchangeMailEngine for Fake {
        fn server_info(&self) -> BoxFuture<'_, Result<ExchangeServerInfo, EngineError>> {
            Box::pin(async { Ok(ExchangeServerInfo::default()) })
        }
        fn folder_hierarchy<'a>(
            &'a self,
            _: Option<&'a str>,
            _: Option<&'a str>,
        ) -> BoxFuture<'a, Result<HierarchyOutcome, EngineError>> {
            Box::pin(async { Ok(HierarchyOutcome::CursorExpired) })
        }
        fn folder_items<'a>(
            &'a self,
            _: Option<&'a str>,
            _: &'a str,
            _: Option<&'a str>,
            _: Option<i64>,
        ) -> BoxFuture<'a, Result<DeltaOutcome, EngineError>> {
            Box::pin(async { Ok(DeltaOutcome::CursorExpired) })
        }
        fn fetch_mime<'a>(
            &'a self,
            mailbox: Option<&'a str>,
            remote_id: &'a str,
        ) -> BoxFuture<'a, Result<Vec<u8>, EngineError>> {
            Box::pin(async move { self.fetch(mailbox, remote_id) })
        }
        fn write<'a>(
            &'a self,
            mailbox: Option<&'a str>,
            write: &'a W,
        ) -> BoxFuture<'a, Result<WriteOutcome, EngineError>> {
            Box::pin(async move { self.apply(mailbox, write) })
        }
        fn classify<'a>(
            &'a self,
            _: Option<&'a str>,
            _: &'a [String],
            _: InferenceClassification,
            _: bool,
        ) -> BoxFuture<'a, Result<(), EngineError>> {
            Box::pin(async { Ok(()) })
        }
        fn wait_for_changes<'a>(
            &'a self,
            _: Option<&'a str>,
            _: &'a [String],
            _: u32,
        ) -> BoxFuture<'a, Result<Option<Vec<String>>, EngineError>> {
            Box::pin(async { Ok(None) })
        }
    }

    impl MailApiEngine for Fake {
        fn folders(&self) -> BoxFuture<'_, Result<Vec<RemoteFolder>, EngineError>> {
            Box::pin(async { Ok(Vec::new()) })
        }
        fn full_sync_page<'a>(
            &'a self,
            _: Option<&'a str>,
            _: Option<i64>,
        ) -> BoxFuture<'a, Result<FullSyncPage, EngineError>> {
            Box::pin(async { Ok(FullSyncPage::default()) })
        }
        fn changes<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<DeltaOutcome, EngineError>> {
            Box::pin(async { Ok(DeltaOutcome::CursorExpired) })
        }
        fn fetch_raw<'a>(
            &'a self,
            remote_id: &'a str,
        ) -> BoxFuture<'a, Result<Vec<u8>, EngineError>> {
            Box::pin(async move { self.fetch(None, remote_id) })
        }
        fn write<'a>(&'a self, write: &'a W) -> BoxFuture<'a, Result<WriteOutcome, EngineError>> {
            Box::pin(async move { self.apply(None, write) })
        }
    }

    const RAW: &[u8] = b"Subject: x\r\nMessage-ID: <x@y>\r\n\r\nbody\r\n";

    fn seen() -> Flags {
        Flags {
            seen: true,
            ..Flags::default()
        }
    }

    #[tokio::test]
    async fn moves_between_mailboxes_of_one_account() {
        let exchange = Fake::default();
        exchange.put(None, "m1", "inbox", RAW);
        let outcome = transfer(
            ApiMailbox::Exchange {
                engine: &exchange,
                mailbox: None,
            },
            "m1",
            ApiMailbox::Exchange {
                engine: &exchange,
                mailbox: Some("support@contoso.example"),
            },
            "inbox",
            seen(),
        )
        .await
        .unwrap();
        let TransferOutcome::Moved {
            remote_id: Some(id),
        } = outcome
        else {
            panic!("{outcome:?}");
        };
        assert!(exchange.get(None, "m1").is_none());
        let (folder, raw, flags) = exchange.get(Some("support@contoso.example"), &id).unwrap();
        assert_eq!(
            (folder.as_str(), raw.as_slice(), flags),
            ("inbox", RAW, seen())
        );
    }

    #[tokio::test]
    async fn moves_between_accounts_and_replays_safely() {
        let exchange = Fake::default();
        let gmail = Fake {
            fail_delete: true,
            ..Fake::default()
        };
        gmail.put(None, "g1", "INBOX", RAW);
        let from_gmail = ApiMailbox::Api(&gmail);
        let to_exchange = ApiMailbox::Exchange {
            engine: &exchange,
            mailbox: None,
        };
        // The delete at the source fails: the move fails, the copy stays.
        let err = transfer(from_gmail, "g1", to_exchange, "inbox", Flags::default())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Network);
        assert!(gmail.get(None, "g1").is_some());
        let copies = exchange.items.lock().unwrap().len();
        assert_eq!(copies, 1);
        // Replayed with the source back online: no second copy.
        let gmail_ok = Fake::default();
        gmail_ok.put(None, "g1", "INBOX", RAW);
        let outcome = transfer(
            ApiMailbox::Api(&gmail_ok),
            "g1",
            to_exchange,
            "inbox",
            Flags::default(),
        )
        .await
        .unwrap();
        assert!(matches!(
            outcome,
            TransferOutcome::Moved { remote_id: Some(_) }
        ));
        assert_eq!(
            exchange.items.lock().unwrap().len(),
            1,
            "the append was idempotent"
        );
        assert!(gmail_ok.get(None, "g1").is_none());
        // And the other way round.
        let id = exchange
            .items
            .lock()
            .unwrap()
            .keys()
            .next()
            .unwrap()
            .1
            .clone();
        let back = transfer(
            to_exchange,
            &id,
            ApiMailbox::Api(&gmail_ok),
            "INBOX",
            seen(),
        )
        .await
        .unwrap();
        assert!(matches!(back, TransferOutcome::Moved { .. }));
        assert!(exchange.items.lock().unwrap().is_empty());
        assert_eq!(*gmail_ok.writes.lock().unwrap(), ["delete", "append"]);
    }

    #[tokio::test]
    async fn a_gone_source_copies_nothing() {
        let exchange = Fake::default();
        let gmail = Fake::default();
        let outcome = transfer(
            ApiMailbox::Api(&gmail),
            "missing",
            ApiMailbox::Exchange {
                engine: &exchange,
                mailbox: None,
            },
            "inbox",
            Flags::default(),
        )
        .await
        .unwrap();
        assert_eq!(outcome, TransferOutcome::Gone);
        assert!(exchange.writes.lock().unwrap().is_empty());
        assert!(format!("{:?}", ApiMailbox::Api(&gmail)).contains("Api"));
    }
}
