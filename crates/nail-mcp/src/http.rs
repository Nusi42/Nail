//! The Streamable HTTP server on `127.0.0.1` (design.md, "MCP server",
//! Transport).
//!
//! * `/mcp` — the MCP endpoint (rmcp's Streamable HTTP service). Legacy
//!   sessions (protocol ≤ 2025-11-25) and stateless 2026-07-28 requests.
//! * `/whoami` — the calling client's id and name (the stdio bridge checks
//!   `--client <name>` against its token with it).
//!
//! Every request needs `Authorization: Bearer <token>`; the token's SHA-256
//! is looked up in the store ([`nail_store::hash_mcp_token`]). Unknown or
//! revoked tokens get `401` and an audit entry (at most
//! [`DENIED_AUDITS_PER_MINUTE`] a minute; the rest are summed up). The listener binds the IPv4
//! loopback address only; rmcp additionally checks the `Host` header
//! (DNS rebinding) and refuses browser `Origin`s.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use nail_api::Event;
use nail_api::mcp::{McpCallKind, McpOutcome, McpServerInfo};
use nail_store::{McpAuditRecord, Store, hash_mcp_token};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use serde_json::json;

use crate::data::McpData;
use crate::endpoint::{Endpoint, remove_endpoint, write_endpoint};
use crate::server::{AuthedClient, ClientSource, NailMcp, Shared, now};
use crate::subscriptions::Hub;
use crate::uri::ResourceRef;

/// Path of the MCP endpoint.
pub const MCP_PATH: &str = "/mcp";
/// Path of the "who am I" check.
pub const WHOAMI_PATH: &str = "/whoami";

/// How to start the server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerOptions {
    /// Port on 127.0.0.1; 0 picks a free one.
    pub port: u16,
    /// Where to write the endpoint file (usually
    /// [`crate::endpoint::endpoint_path`] of the data directory); `None`
    /// writes none.
    pub endpoint_file: Option<PathBuf>,
}

/// A running MCP server. Dropping it stops the server.
pub struct McpServer<D: McpData> {
    shared: Arc<Shared<D>>,
    url: String,
    /// Cancels rmcp's sessions (its token type is not re-exported).
    cancel_sessions: Box<dyn Fn() + Send + Sync>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
    endpoint_file: Option<PathBuf>,
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim())
        .filter(|t| !t.is_empty())
}

/// Refused connections written to the audit log per minute; more are
/// counted and summed up in one entry, so a local process (or a web page
/// probing the loopback port) cannot grow the database without limit.
const DENIED_AUDITS_PER_MINUTE: u32 = 20;

/// Rate limit of the "refused connection" audit entries.
#[derive(Debug, Default)]
pub(crate) struct DeniedAudits {
    window_start: i64,
    written: u32,
    dropped: u32,
}

impl DeniedAudits {
    /// Whether to write an entry for a refusal at `now` (Unix seconds);
    /// the second value is the number of refusals not written in the
    /// previous window, to sum up.
    pub(crate) fn admit(&mut self, now: i64) -> (bool, u32) {
        let mut dropped = 0;
        if now - self.window_start >= 60 {
            dropped = std::mem::take(&mut self.dropped);
            self.window_start = now;
            self.written = 0;
        }
        if self.written < DENIED_AUDITS_PER_MINUTE {
            self.written += 1;
            (true, dropped)
        } else {
            self.dropped += 1;
            (false, dropped)
        }
    }
}

/// The request path as audited: the known endpoints as they are, anything
/// else shortened (the path is chosen by the caller).
pub(crate) fn audited_path(path: &str) -> String {
    const MAX: usize = 64;
    if path == MCP_PATH || path == WHOAMI_PATH || path.len() <= MAX {
        return path.to_owned();
    }
    let mut end = MAX;
    while !path.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &path[..end])
}

#[derive(Clone)]
struct AuthState {
    store: Store,
    denied: Arc<std::sync::Mutex<DeniedAudits>>,
}

async fn authenticate(State(auth): State<AuthState>, mut request: Request, next: Next) -> Response {
    let store = auth.store.clone();
    let client = match bearer(request.headers()) {
        Some(token) => {
            let hash = hash_mcp_token(token);
            store
                .call(move |s| s.mcp_client_by_token(&hash))
                .await
                .unwrap_or_else(|err| {
                    tracing::warn!(%err, "cannot look up an MCP token");
                    None
                })
        }
        None => None,
    };
    let Some(client) = client else {
        let detail = if bearer(request.headers()).is_some() {
            "unknown or revoked token"
        } else {
            "missing bearer token"
        };
        let at = now();
        let (write, dropped) = auth
            .denied
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .admit(at);
        let mut records = Vec::new();
        if dropped > 0 {
            records.push(McpAuditRecord {
                client_id: None,
                client_name: String::new(),
                at,
                kind: McpCallKind::Connect,
                name: MCP_PATH.to_owned(),
                outcome: McpOutcome::Denied,
                detail: Some(format!("{dropped} more refused connections not logged")),
            });
        }
        if write {
            records.push(McpAuditRecord {
                client_id: None,
                client_name: String::new(),
                at,
                kind: McpCallKind::Connect,
                name: audited_path(request.uri().path()),
                outcome: McpOutcome::Denied,
                detail: Some(detail.to_owned()),
            });
        }
        for record in records {
            if let Err(err) = store.call(move |s| s.append_mcp_audit(&record)).await {
                tracing::warn!(%err, "cannot write the MCP audit log");
            }
        }
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer realm=\"nail\"")],
            Json(json!({"error": "unauthorized", "detail": detail})),
        )
            .into_response();
    };
    let id = client.id.clone();
    if let Err(err) = store.call(move |s| s.touch_mcp_client(&id, now())).await {
        tracing::debug!(%err, "cannot record MCP client use");
    }
    request.extensions_mut().insert(AuthedClient(client));
    next.run(request).await
}

async fn whoami(
    Extension(AuthedClient(client)): Extension<AuthedClient>,
) -> Json<serde_json::Value> {
    Json(json!({ "id": client.id, "name": client.name }))
}

impl<D: McpData> McpServer<D> {
    /// Binds `127.0.0.1:<port>` and serves until dropped or
    /// [`Self::shutdown`].
    pub async fn start(
        store: Store,
        data: Arc<D>,
        options: ServerOptions,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            store: store.clone(),
            data,
            hub: Arc::new(Hub::default()),
        });
        let config = StreamableHttpServerConfig::default().enforce_origin_validation();
        let sessions_ct = config.cancellation_token.clone();
        let factory_shared = shared.clone();
        let service: StreamableHttpService<NailMcp<D>, LocalSessionManager> =
            StreamableHttpService::new(
                move || Ok(NailMcp::new(factory_shared.clone(), ClientSource::Http)),
                Arc::default(),
                config,
            );
        let router = Router::new()
            .nest_service(MCP_PATH, service)
            .route(WHOAMI_PATH, get(whoami))
            .layer(middleware::from_fn_with_state(
                AuthState {
                    store,
                    denied: Arc::default(),
                },
                authenticate,
            ));

        let listener =
            tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, options.port)))
                .await?;
        let addr = listener.local_addr()?;
        let url = format!("http://{addr}{MCP_PATH}");
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let serve = axum::serve(listener, router).with_graceful_shutdown(async move {
                let _ = rx.await;
            });
            if let Err(err) = serve.await {
                tracing::warn!(%err, "MCP server stopped");
            }
        });
        if let Some(path) = &options.endpoint_file {
            let endpoint = Endpoint {
                url: url.clone(),
                pid: std::process::id(),
            };
            if let Err(err) = write_endpoint(path, &endpoint) {
                tracing::warn!(%err, path = %path.display(), "cannot write the MCP endpoint file");
            }
        }
        tracing::info!(%url, "MCP server listening");
        Ok(Self {
            shared,
            url,
            cancel_sessions: Box::new(move || sessions_ct.cancel()),
            shutdown: Some(tx),
            task: Some(task),
            endpoint_file: options.endpoint_file,
        })
    }

    /// The Streamable HTTP URL (`http://127.0.0.1:<port>/mcp`).
    pub fn url(&self) -> &str {
        &self.url
    }

    /// For [`nail_api::Request::GetMcpServerInfo`].
    pub fn info(&self) -> McpServerInfo {
        McpServerInfo {
            running: true,
            url: Some(self.url.clone()),
        }
    }

    /// Feeds a core event to the subscriptions: new mail updates the
    /// account's `folders` and the folder's `messages`, other mail changes
    /// the folders' `messages`, account changes `nail://accounts`.
    pub async fn on_event(&self, event: &Event) {
        match event {
            Event::NewMail {
                account_id,
                folder_id,
                ..
            } => {
                self.shared
                    .notify(&ResourceRef::Folders(account_id.clone()))
                    .await;
                self.shared.notify(&folder_messages(folder_id)).await;
            }
            Event::MailChanged { folder_ids } => {
                for folder in folder_ids {
                    self.shared.notify(&folder_messages(folder)).await;
                }
            }
            Event::AccountsChanged => self.shared.notify(&ResourceRef::Accounts).await,
            _ => {}
        }
    }

    /// The UI reported a new context: notifies `nail://ui/context`
    /// subscribers.
    pub async fn ui_context_changed(&self) {
        self.shared.notify(&ResourceRef::UiContext).await;
    }

    /// Number of subscribed sessions and streams (diagnostics, tests).
    pub fn subscriber_count(&self) -> usize {
        self.shared.hub.len()
    }

    /// Stops the server and waits for it.
    pub async fn shutdown(mut self) {
        self.stop();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }

    fn stop(&mut self) {
        (self.cancel_sessions)();
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(path) = self.endpoint_file.take() {
            remove_endpoint(&path, &self.url);
        }
    }
}

fn folder_messages(folder: &nail_api::FolderId) -> ResourceRef {
    ResourceRef::FolderMessages {
        folder: folder.clone(),
        offset: 0,
        limit: crate::uri::DEFAULT_PAGE,
    }
}

impl<D: McpData> Drop for McpServer<D> {
    fn drop(&mut self) {
        self.stop();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refused_connections_are_audited_at_a_bounded_rate() {
        let mut denied = DeniedAudits::default();
        let written = (0..100).filter(|_| denied.admit(1000).0).count();
        assert_eq!(written, DENIED_AUDITS_PER_MINUTE as usize);
        // The next window writes again and sums up what was dropped.
        assert_eq!(denied.admit(1030), (false, 0));
        assert_eq!(denied.admit(1060), (true, 81));
        assert_eq!(denied.admit(1061), (true, 0));
    }

    #[test]
    fn audited_paths_are_shortened() {
        assert_eq!(audited_path("/mcp"), "/mcp");
        let long = format!("/{}", "ä".repeat(100));
        let short = audited_path(&long);
        assert!(short.len() <= 64 + "…".len(), "{short}");
        assert!(short.ends_with('…'));
    }

    #[test]
    fn bearer_tokens_are_parsed() {
        let mut headers = HeaderMap::new();
        assert_eq!(bearer(&headers), None);
        headers.insert(header::AUTHORIZATION, "Bearer abc".parse().unwrap());
        assert_eq!(bearer(&headers), Some("abc"));
        headers.insert(header::AUTHORIZATION, "bearer  xyz ".parse().unwrap());
        assert_eq!(bearer(&headers), Some("xyz"));
        headers.insert(header::AUTHORIZATION, "Basic abc".parse().unwrap());
        assert_eq!(bearer(&headers), None);
        headers.insert(header::AUTHORIZATION, "Bearer ".parse().unwrap());
        assert_eq!(bearer(&headers), None);
    }
}
