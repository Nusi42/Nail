//! The MCP client administration requests of the Client API
//! (`ListMcpClients`, `CreateMcpClient`, `UpdateMcpGrants`,
//! `RevokeMcpClient`, `ListMcpAudit`, `GetMcpServerInfo`; ADR 0022) and
//! the client's rate limits (`UpdateMcpRateLimits`, M4a, ADR 0079).
//!
//! The core routes these requests here ([`handle`]); everything they need
//! is the store, the server's state and a way to emit events.

use nail_api::mcp::{McpGrant, McpServerInfo};
use nail_api::{ApiError, Event, LocalizedMessage, Request, Response};
use nail_store::{Store, hash_mcp_token};

use crate::server::now;

/// Prefix of every token (recognizable in configs and secret scanners).
pub const TOKEN_PREFIX: &str = "nail_mcp_";
/// Characters of the token shown as its hint.
pub const HINT_LEN: usize = 4;
/// Longest client name.
pub const MAX_NAME_LEN: usize = 64;
/// Largest audit page.
pub const MAX_AUDIT_PAGE: u32 = 500;

/// A new random bearer token: the prefix and 32 random bytes, base64url.
pub fn new_token() -> Result<String, ApiError> {
    use base64::Engine as _;
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|e| ApiError::internal(format!("no randomness for an MCP token: {e}")))?;
    Ok(format!(
        "{TOKEN_PREFIX}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    ))
}

/// The last characters of a token, shown to tell tokens apart.
pub fn token_hint(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    chars[chars.len().saturating_sub(HINT_LEN)..]
        .iter()
        .collect()
}

/// Whether `request` is one of the requests [`handle`] answers.
pub fn is_admin_request(request: &Request) -> bool {
    matches!(
        request,
        Request::ListMcpClients
            | Request::CreateMcpClient { .. }
            | Request::UpdateMcpGrants { .. }
            | Request::RevokeMcpClient { .. }
            | Request::ListMcpAudit { .. }
            | Request::GetMcpServerInfo
            | Request::UpdateMcpRateLimits { .. }
    )
}

/// Answers one administration request. `server` is the running server's
/// state (`None` = not running); `emit` pushes events to the UIs
/// ([`Event::McpClientsChanged`] after every change).
pub async fn handle(
    store: &Store,
    server: Option<McpServerInfo>,
    request: Request,
    emit: impl Fn(Event),
) -> Result<Response, ApiError> {
    match request {
        Request::ListMcpClients => Ok(Response::McpClients {
            clients: store.call(|s| s.list_mcp_clients()).await?,
        }),
        Request::CreateMcpClient { name, grant } => {
            let name = name.trim().to_owned();
            if name.is_empty() || name.chars().count() > MAX_NAME_LEN {
                return Err(ApiError::invalid_request(format!(
                    "an MCP client name has 1 to {MAX_NAME_LEN} characters"
                ))
                .with_localized(crate::messages::client_name_length(MAX_NAME_LEN)));
            }
            let token = new_token()?;
            let hash = hash_mcp_token(&token);
            let hint = token_hint(&token);
            let client = store
                .call(move |s| s.create_mcp_client(&name, &hash, &hint, &grant, now()))
                .await?;
            emit(Event::McpClientsChanged);
            Ok(Response::McpClientCreated {
                client: Box::new(client),
                token,
            })
        }
        Request::UpdateMcpGrants { client_id, grant } => {
            let grant = normalized(grant);
            let client = store
                .call(move |s| s.update_mcp_grant(&client_id, &grant))
                .await?;
            emit(Event::McpClientsChanged);
            Ok(Response::McpClientSaved {
                client: Box::new(client),
            })
        }
        Request::UpdateMcpRateLimits { client_id, limits } => {
            if let Some(issue) = limits.issues().first() {
                return Err(ApiError::invalid_request(format!(
                    "invalid rate limits ({})",
                    issue.message_id()
                ))
                .with_localized(LocalizedMessage::new(issue.message_id())));
            }
            let client = store
                .call(move |s| s.set_mcp_rate_limits(&client_id, &limits))
                .await?;
            emit(Event::McpClientsChanged);
            Ok(Response::McpClientSaved {
                client: Box::new(client),
            })
        }
        Request::RevokeMcpClient { client_id } => {
            store
                .call(move |s| s.revoke_mcp_client(&client_id, now()))
                .await?;
            emit(Event::McpClientsChanged);
            Ok(Response::Done)
        }
        Request::ListMcpAudit {
            client_id,
            before,
            limit,
        } => {
            let limit = limit.clamp(1, MAX_AUDIT_PAGE);
            let mut entries = store
                .call(move |s| s.list_mcp_audit(client_id.as_ref(), before, limit))
                .await?;
            // The store keeps the English detail; the UI gets its text.
            for entry in &mut entries {
                if entry.localized.is_none() {
                    entry.localized = entry
                        .detail
                        .as_deref()
                        .and_then(crate::messages::audit_detail);
                }
            }
            Ok(Response::McpAudit { entries })
        }
        Request::GetMcpServerInfo => Ok(Response::McpServer {
            info: server.unwrap_or(McpServerInfo {
                running: false,
                url: None,
            }),
        }),
        other => Err(ApiError::invalid_request(format!(
            "not an MCP administration request: {}",
            serde_json::to_value(&other)
                .ok()
                .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_owned))
                .unwrap_or_default()
        ))),
    }
}

/// Removes duplicates (the store keeps each scope, account and folder once).
fn normalized(mut grant: McpGrant) -> McpGrant {
    grant.scopes.sort();
    grant.scopes.dedup();
    grant.accounts.sort();
    grant.accounts.dedup();
    grant.excluded_folders.sort();
    grant.excluded_folders.dedup();
    grant
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use nail_api::mcp::{McpCallKind, McpOutcome, McpScope};
    use nail_api::{ErrorCode, McpClientId};
    use nail_store::McpAuditRecord;

    use super::*;

    #[test]
    fn tokens_are_random_prefixed_and_hinted() {
        let a = new_token().unwrap();
        let b = new_token().unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with(TOKEN_PREFIX));
        assert_eq!(a.len(), TOKEN_PREFIX.len() + 43);
        assert_eq!(token_hint(&a), a[a.len() - 4..]);
        assert_eq!(token_hint("ab"), "ab");
    }

    #[tokio::test]
    async fn clients_are_created_granted_revoked_and_audited() {
        let store = Store::open_in_memory().unwrap();
        let events = Mutex::new(Vec::new());
        let emit = |e: Event| events.lock().unwrap().push(e);

        let Response::McpClientCreated { client, token } = handle(
            &store,
            None,
            Request::CreateMcpClient {
                name: "  Claude  ".into(),
                grant: McpGrant::default(),
            },
            emit,
        )
        .await
        .unwrap() else {
            panic!("created expected");
        };
        assert_eq!(client.name, "Claude");
        assert!(client.grant.is_empty(), "new clients get nothing");
        assert_eq!(client.token_hint, token_hint(&token));
        // Only the hash is stored.
        let found = store
            .mcp_client_by_token(&hash_mcp_token(&token))
            .unwrap()
            .unwrap();
        assert_eq!(found.id, client.id);

        // Same name again: conflict.
        let err = handle(
            &store,
            None,
            Request::CreateMcpClient {
                name: "Claude".into(),
                grant: McpGrant::default(),
            },
            emit,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::Conflict);
        let err = handle(
            &store,
            None,
            Request::CreateMcpClient {
                name: " ".into(),
                grant: McpGrant::default(),
            },
            emit,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest);

        let grant = McpGrant {
            scopes: vec![McpScope::Read, McpScope::Read, McpScope::Context],
            accounts: vec!["work".into()],
            ..McpGrant::default()
        };
        let Response::McpClientSaved { client: saved } = handle(
            &store,
            None,
            Request::UpdateMcpGrants {
                client_id: client.id.clone(),
                grant,
            },
            emit,
        )
        .await
        .unwrap() else {
            panic!("saved expected");
        };
        assert_eq!(saved.grant.scopes, vec![McpScope::Read, McpScope::Context]);

        store
            .append_mcp_audit(&McpAuditRecord {
                client_id: Some(client.id.clone()),
                client_name: "Claude".into(),
                at: 1,
                kind: McpCallKind::Tool,
                name: "get_message".into(),
                outcome: McpOutcome::Allowed,
                detail: None,
            })
            .unwrap();
        let Response::McpAudit { entries } = handle(
            &store,
            None,
            Request::ListMcpAudit {
                client_id: Some(client.id.clone()),
                before: None,
                limit: 0,
            },
            emit,
        )
        .await
        .unwrap() else {
            panic!("audit expected");
        };
        assert_eq!(entries.len(), 1, "a zero limit still returns a page");
        assert_eq!(entries[0].localized, None);

        // Refused connections carry the text of their English detail.
        store
            .append_mcp_audit(&McpAuditRecord {
                client_id: None,
                client_name: String::new(),
                at: 2,
                kind: McpCallKind::Tool,
                name: "connect".into(),
                outcome: McpOutcome::Denied,
                detail: Some(crate::messages::DETAIL_REFUSED_TOKEN.into()),
            })
            .unwrap();
        let Response::McpAudit { entries } = handle(
            &store,
            None,
            Request::ListMcpAudit {
                client_id: None,
                before: None,
                limit: 10,
            },
            emit,
        )
        .await
        .unwrap() else {
            panic!("audit expected");
        };
        assert_eq!(
            entries[0].localized.as_ref().map(|l| l.id.as_str()),
            Some("mcp-core-token-refused")
        );

        assert_eq!(
            handle(
                &store,
                None,
                Request::RevokeMcpClient {
                    client_id: client.id.clone(),
                },
                emit,
            )
            .await
            .unwrap(),
            Response::Done
        );
        assert!(
            store
                .mcp_client_by_token(&hash_mcp_token(&token))
                .unwrap()
                .is_none()
        );
        let Response::McpClients { clients } = handle(&store, None, Request::ListMcpClients, emit)
            .await
            .unwrap()
        else {
            panic!("clients expected");
        };
        assert!(clients[0].revoked_at.is_some());

        let err = handle(
            &store,
            None,
            Request::RevokeMcpClient {
                client_id: McpClientId::new("mcp-99"),
            },
            emit,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);

        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| matches!(e, Event::McpClientsChanged))
                .count(),
            3,
            "create, grant, revoke"
        );
    }

    #[tokio::test]
    async fn server_info_and_foreign_requests() {
        let store = Store::open_in_memory().unwrap();
        let info = McpServerInfo {
            running: true,
            url: Some("http://127.0.0.1:1/mcp".into()),
        };
        assert_eq!(
            handle(
                &store,
                Some(info.clone()),
                Request::GetMcpServerInfo,
                |_| {}
            )
            .await
            .unwrap(),
            Response::McpServer { info }
        );
        assert_eq!(
            handle(&store, None, Request::GetMcpServerInfo, |_| {})
                .await
                .unwrap(),
            Response::McpServer {
                info: McpServerInfo {
                    running: false,
                    url: None
                }
            }
        );
        assert!(is_admin_request(&Request::ListMcpClients));
        assert!(!is_admin_request(&Request::Ping));
        let err = handle(&store, None, Request::Ping, |_| {})
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest);
    }

    #[tokio::test]
    async fn rate_limits_are_validated_and_saved() {
        use nail_api::mcp::McpRateLimits;
        let store = Store::open_in_memory().unwrap();
        let events = Mutex::new(Vec::new());
        let emit = |e: Event| events.lock().unwrap().push(e);
        let Response::McpClientCreated { client, .. } = handle(
            &store,
            None,
            Request::CreateMcpClient {
                name: "Agent".into(),
                grant: McpGrant::default(),
            },
            emit,
        )
        .await
        .unwrap() else {
            panic!("created expected");
        };
        assert_eq!(client.rate_limits, McpRateLimits::default());
        let request = Request::UpdateMcpRateLimits {
            client_id: client.id.clone(),
            limits: McpRateLimits {
                requests_per_minute: Some(0),
                ..McpRateLimits::default()
            },
        };
        assert!(is_admin_request(&request));
        let err = handle(&store, None, request, emit).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest);
        assert_eq!(err.localized.unwrap().id, "mcp-rate-limit-issue-zero");
        let limits = McpRateLimits {
            sends_per_hour: Some(3),
            ..McpRateLimits::unlimited()
        };
        let Response::McpClientSaved { client: saved } = handle(
            &store,
            None,
            Request::UpdateMcpRateLimits {
                client_id: client.id.clone(),
                limits,
            },
            emit,
        )
        .await
        .unwrap() else {
            panic!("saved expected");
        };
        assert_eq!(saved.rate_limits, limits);
        assert!(matches!(
            events.lock().unwrap().last(),
            Some(Event::McpClientsChanged)
        ));
    }
}
