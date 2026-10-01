//! Requests of wave M3a for calendars, contacts and tasks (contract
//! ADR 0058).
//!
//! The contract stage answers every PIM request with `unsupported`: the
//! store APIs (`nail-store`, WP73), iCalendar / vCard (`nail-ical`, WP71),
//! the CalDAV / CardDAV client (`nail-dav`, WP72) and the reminder
//! scheduler (`nail-triage`, WP73) are wired here by the integration
//! (WP75), which replaces this module's body.

use nail_api::{ApiError, Request, Response};

use crate::error::Result;
use crate::service::Service;

/// Handles one of this module's requests.
pub(crate) async fn handle(_service: &Service, request: Request) -> Result<Response> {
    Err(ApiError::unsupported(format!(
        "{} is not wired yet (M3a WP75)",
        crate::service::request_name(&request)
    ))
    .into())
}
