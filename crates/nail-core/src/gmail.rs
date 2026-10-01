//! Gmail accounts (M3b, contract ADR 0065): the label requests and the
//! account engine.
//!
//! The contract stage answers every label request with `unsupported`, and
//! a Gmail account's engine only reports that it is not wired: the Gmail
//! API engine (`nail-proto-gmail`, WP82, a `nail_api::engine::MailApiEngine`)
//! is wired here by the integration (WP85) — sync loop with
//! `AdaptivePoll`, op replay as `RemoteWrite`s, label queue, sending
//! through the API. The store already keeps the Gmail state
//! (`nail_store::gmail`).

use std::sync::Arc;

use nail_api::{AccountConfig, AccountState, ApiError, ErrorCode, Request, Response};
use tokio::sync::watch;

use crate::engine::{Signals, stopped};
use crate::error::Result;
use crate::service::Service;

/// Handles one of this module's requests.
pub(crate) async fn handle(_service: &Service, request: Request) -> Result<Response> {
    Err(ApiError::unsupported(format!(
        "{} is not wired yet (M3b WP85)",
        crate::service::request_name(&request)
    ))
    .into())
}

/// The engine of a Gmail account until WP85: reports the account offline
/// with `unsupported` and waits to be stopped.
pub(crate) async fn run(
    service: Service,
    account: AccountConfig,
    _signals: Arc<Signals>,
    mut stop: watch::Receiver<bool>,
) {
    service.set_status(
        &account.id,
        AccountState::Offline,
        Some((
            ErrorCode::Unsupported,
            "the Gmail API engine is not wired yet (M3b WP85)".to_owned(),
            None,
        )),
    );
    stopped(&mut stop).await;
}
