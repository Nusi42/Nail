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
//! revoked tokens get `401` and an audit entry. The listener binds the IPv4
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

async fn authenticate(State(store): State<Store>, mut request: Request, next: Next) -> Response {
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
        let record = McpAuditRecord {
            client_id: None,
            client_name: String::new(),
            at: now(),
            kind: McpCallKind::Connect,
            name: request.uri().path().to_owned(),
            outcome: McpOutcome::Denied,
            detail: Some(detail.to_owned()),
        };
        if let Err(err) = store.call(move |s| s.append_mcp_audit(&record)).await {
            tracing::warn!(%err, "cannot write the MCP audit log");
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
            .layer(middleware::from_fn_with_state(store, authenticate));

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
