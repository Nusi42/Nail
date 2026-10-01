//! Requests of wave M3b for invitations, scheduling and free/busy
//! (contract ADR 0065).
//!
//! The contract stage answers every scheduling request with
//! `unsupported`: iTIP / iMIP (`nail-itip`, WP81), CalDAV scheduling and
//! free/busy (`nail-dav`, WP83; the store's `pim/freebusy.rs`) and the
//! invitation card in the reader (WP84) are wired here by the integration
//! (WP85), which replaces this module's body. The store already keeps the
//! scheduling state (`nail_store::scheduling`).

use nail_api::{ApiError, Request, Response};

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
