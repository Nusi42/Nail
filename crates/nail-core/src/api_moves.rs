//! Moves of API-engine messages out of their account (M4c, WP123
//! leftovers; ADR 0099): Exchange ↔ Exchange (another account) and
//! Exchange ↔ Gmail / Gmail ↔ Gmail, through
//! [`nail_exchange::transfer::transfer`] (copy with the flags into the
//! target, then remove from the source; a source that is gone is not
//! copied). Moves between mailboxes of one Exchange account use the
//! engine's [`ExchangeMailEngine::move_to_mailbox`](nail_api::exchange::engine::ExchangeMailEngine::move_to_mailbox) in the Exchange
//! executor. Tags (categories / labels) are not carried over.

use std::sync::Arc;

use nail_api::accounts::IncomingServer;
use nail_api::engine::{EngineError, MailApiEngine};
use nail_api::exchange::engine::ExchangeEngine;
use nail_api::{ErrorCode, FolderId, MessageId};
use nail_exchange::transfer::{ApiMailbox, TransferOutcome};

use crate::error::CoreError;
use crate::service::Service;

/// Where a moved message goes: a folder of an API account.
pub(crate) enum ApiTarget {
    /// An Exchange account's folder.
    Exchange {
        engine: Arc<dyn ExchangeEngine>,
        mailbox: Option<String>,
        folder: String,
    },
    /// A Gmail account's folder (its label path).
    Gmail {
        engine: Arc<nail_proto_gmail::GmailEngine>,
        folder: String,
    },
}

impl ApiTarget {
    fn mailbox(&self) -> ApiMailbox<'_> {
        match self {
            Self::Exchange {
                engine, mailbox, ..
            } => ApiMailbox::Exchange {
                engine: engine.as_ref(),
                mailbox: mailbox.as_deref(),
            },
            Self::Gmail { engine, .. } => ApiMailbox::Api(engine.as_ref()),
        }
    }

    fn folder(&self) -> &str {
        match self {
            Self::Exchange { folder, .. } | Self::Gmail { folder, .. } => folder,
        }
    }
}

fn engine_err(err: CoreError) -> EngineError {
    err.into()
}

/// The API target of a folder in another account; `None` when that
/// account is not an API account (IMAP, POP3, local).
pub(crate) async fn api_target(
    service: &Service,
    folder: &FolderId,
) -> Result<Option<ApiTarget>, EngineError> {
    let id = folder.clone();
    let (local, record) = service
        .store()
        .call(move |s| Ok((s.get_folder(&id)?, s.exchange_folder(&id)?)))
        .await
        .map_err(|e| engine_err(e.into()))?;
    let Some(config) = service
        .inner
        .accounts
        .config()
        .account(&local.account_id)
        .cloned()
    else {
        return Ok(None);
    };
    Ok(match &config.incoming {
        IncomingServer::Exchange { .. } => {
            let Some(record) = record else {
                return Ok(None);
            };
            let engine = crate::exchange::engine_of(service, &config).map_err(engine_err)?;
            Some(ApiTarget::Exchange {
                engine,
                mailbox: record.mailbox,
                folder: record.remote_id,
            })
        }
        IncomingServer::Gmail { .. } => {
            let engine = crate::gmail::engine_for(service, &config).map_err(engine_err)?;
            Some(ApiTarget::Gmail {
                engine: Arc::new(engine),
                folder: local.path,
            })
        }
        IncomingServer::Imap { .. } | IncomingServer::Pop3 { .. } => None,
    })
}

/// Moves `remote_id` from `source` into `target`; records the new remote
/// id on the (already locally moved) message.
pub(crate) async fn move_out(
    service: &Service,
    message: &MessageId,
    source: ApiMailbox<'_>,
    remote_id: &str,
    target: &ApiTarget,
) -> Result<(), EngineError> {
    let id = message.clone();
    let flags = service
        .store()
        .call(move |s| Ok(s.get_message(&id)?.summary.flags))
        .await
        .map_err(|e| engine_err(e.into()))?;
    match nail_exchange::transfer::transfer(
        source,
        remote_id,
        target.mailbox(),
        target.folder(),
        flags,
    )
    .await?
    {
        TransferOutcome::Moved {
            remote_id: Some(new),
        } => {
            let id = message.clone();
            service
                .store()
                .call(move |s| s.set_message_remote_id(&id, Some(&new)))
                .await
                .map_err(|e| engine_err(e.into()))?;
            Ok(())
        }
        TransferOutcome::Moved { remote_id: None } | TransferOutcome::Gone => Ok(()),
    }
}

/// The error of a move into an account without an API engine.
pub(crate) fn unsupported_target() -> EngineError {
    EngineError::permanent(
        ErrorCode::Unsupported,
        "moving messages from this account into IMAP or local folders is not supported yet",
    )
}

/// Files a copy of a local message's bytes in an API target (moves from
/// IMAP / POP3 accounts into Exchange or Gmail); answers the new remote id.
pub(crate) async fn append_raw(
    target: &ApiTarget,
    raw: Vec<u8>,
    flags: nail_api::Flags,
) -> Result<Option<String>, EngineError> {
    let write = nail_api::engine::RemoteWrite::Append {
        folder: target.folder().to_owned(),
        raw,
        flags,
    };
    let outcome = match target {
        ApiTarget::Exchange {
            engine, mailbox, ..
        } => engine.write(mailbox.as_deref(), &write).await?,
        ApiTarget::Gmail { engine, .. } => MailApiEngine::write(engine.as_ref(), &write).await?,
    };
    Ok(match outcome {
        nail_api::engine::WriteOutcome::Done { remote_id } => remote_id,
        nail_api::engine::WriteOutcome::Gone => None,
    })
}
