//! Exchange accounts (M3c, contract ADR 0072): the Exchange requests and
//! the account engine.
//!
//! The contract stage answers every Exchange request with `unsupported`,
//! and an Exchange account's engine only reports that it is not wired:
//! the Graph engine (`nail-proto-graph`, WP91) and the EWS engine
//! (`nail-proto-ews`, WP92) — both a
//! `nail_api::exchange::engine::ExchangeEngine` — and the common layer
//! (`nail-exchange`, WP93: Autodiscover, the SSPI `NegotiateAuth`, the
//! mapping between Exchange and Nail models) are wired here by the
//! integration (WP95): per-folder delta sync with `AdaptivePoll` or EWS
//! streaming, op replay with remote ids, the Exchange operation queue,
//! PIM sync of the account's `exchange:<account>` source, sending through
//! the engine, free/busy and rooms. The store already keeps the Exchange
//! state (`nail_store::exchange`).

use std::sync::Arc;

use nail_api::{AccountConfig, AccountState, ApiError, ErrorCode, Request, Response};
use tokio::sync::watch;

use crate::engine::{Signals, stopped};
use crate::error::Result;
use crate::service::Service;

/// Handles one of this module's requests.
pub(crate) async fn handle(_service: &Service, request: Request) -> Result<Response> {
    Err(ApiError::unsupported(format!(
        "{} is not wired yet (M3c WP95)",
        crate::service::request_name(&request)
    ))
    .into())
}

/// The engine of an Exchange account until WP95: reports the account
/// offline with `unsupported` and waits to be stopped.
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
            "the Exchange engines are not wired yet (M3c WP95)".to_owned(),
            None,
        )),
    );
    stopped(&mut stop).await;
}
