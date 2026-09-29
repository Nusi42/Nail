//! Requests of M1b WP23 (MCP): ListMcpClients, CreateMcpClient,
//! UpdateMcpGrants, RevokeMcpClient, ListMcpAudit, GetMcpServerInfo.
//!
//! [`Service::dispatch`](crate::service::Service) routes exactly these
//! requests here (one match arm), so the package owning this file never
//! has to edit `service.rs`. Until the package lands every request is
//! answered with `unsupported` (ADR 0022).

use nail_api::{ApiError, Request, Response};

use crate::error::Result;
use crate::service::Service;

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    let _ = service;
    Err(ApiError::unsupported(format!(
        "{} arrives with M1b WP23 (MCP)",
        crate::service::request_name(&request)
    ))
    .into())
}
