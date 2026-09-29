//! MCP (M1b WP23, ADR 0025): the client settings requests (ListMcpClients,
//! CreateMcpClient, UpdateMcpGrants, RevokeMcpClient, ListMcpAudit,
//! GetMcpServerInfo) and the running server with the core as its data
//! provider.
//!
//! The server itself lives in `nail-mcp`. [`Service::start_mcp_server`]
//! starts it on loopback with [`nail_mcp::CoreData`] over the service,
//! forwards the core's events (new mail, mail and account changes) and UI
//! context reports to its subscriptions, and makes its address available
//! to `GetMcpServerInfo`. The returned [`McpRunning`] owns the server; the
//! transports (`nail serve`, the desktop shell) keep it for their lifetime.

use std::path::PathBuf;
use std::sync::Arc;

use nail_api::{Event, Request, Response};
use nail_mcp::{CoreAccess, CoreData, McpServer, ServerOptions};
use nail_store::{BlobStore, Store};

use crate::error::Result;
use crate::service::{Service, lock};

/// Where the running server's address is written for `nail mcp --client`
/// (`<data-dir>/mcp-endpoint.json`).
pub fn mcp_endpoint_path(data_dir: &std::path::Path) -> PathBuf {
    nail_mcp::endpoint::endpoint_path(data_dir)
}

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    let info = lock(&service.inner.mcp_info).clone();
    let events = service.clone();
    Ok(
        nail_mcp::admin::handle(service.store(), info, request, move |event| {
            events.broadcast(event);
        })
        .await?,
    )
}

/// The service as the MCP server's data provider.
#[derive(Clone)]
pub struct McpCore(pub Service);

impl CoreAccess for McpCore {
    fn handle(&self, request: Request) -> impl Future<Output = Response> + Send {
        let service = self.0.clone();
        async move { service.handle(request).await }
    }

    fn store(&self) -> &Store {
        self.0.store()
    }

    fn blobs(&self) -> &BlobStore {
        self.0.blobs()
    }

    fn emit(&self, event: Event) {
        self.0.broadcast(event);
    }
}

type Server = McpServer<CoreData<McpCore>>;

/// A running MCP server. Dropping it stops the server and the event
/// forwarding, and `GetMcpServerInfo` reports it as stopped again.
pub struct McpRunning {
    server: Option<Arc<Server>>,
    forward: tokio::task::JoinHandle<()>,
    service: Service,
}

impl McpRunning {
    /// The Streamable HTTP URL (`http://127.0.0.1:<port>/mcp`).
    pub fn url(&self) -> String {
        self.server
            .as_ref()
            .map(|s| s.url().to_owned())
            .unwrap_or_default()
    }

    /// Stops the server and waits until it is down.
    pub async fn shutdown(mut self) {
        self.forward.abort();
        *lock(&self.service.inner.mcp_info) = None;
        if let Some(server) = self.server.take() {
            match Arc::try_unwrap(server) {
                Ok(server) => server.shutdown().await,
                Err(shared) => drop(shared),
            }
        }
    }
}

impl Drop for McpRunning {
    fn drop(&mut self) {
        self.forward.abort();
        *lock(&self.service.inner.mcp_info) = None;
    }
}

impl std::fmt::Debug for McpRunning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpRunning")
            .field("url", &self.url())
            .finish_non_exhaustive()
    }
}

impl Service {
    /// Starts the MCP server on `127.0.0.1:<port>` (0 = a free port),
    /// writing the endpoint file (for `nail mcp --client`) when given.
    /// Must be called within a tokio runtime.
    pub async fn start_mcp_server(
        &self,
        port: u16,
        endpoint_file: Option<PathBuf>,
    ) -> Result<McpRunning> {
        let data = Arc::new(CoreData(McpCore(self.clone())));
        let server = Arc::new(
            McpServer::start(
                self.store().clone(),
                data,
                ServerOptions {
                    port,
                    endpoint_file,
                },
            )
            .await?,
        );
        *lock(&self.inner.mcp_info) = Some(server.info());
        let forward = tokio::spawn(forward_events(
            self.subscribe(),
            self.inner.ui_context_changed.subscribe(),
            Arc::downgrade(&server),
        ));
        Ok(McpRunning {
            server: Some(server),
            forward,
            service: self.clone(),
        })
    }
}

/// Feeds core events and UI context reports to the server's subscriptions.
async fn forward_events(
    mut events: tokio::sync::broadcast::Receiver<Event>,
    mut context: tokio::sync::watch::Receiver<u64>,
    server: std::sync::Weak<Server>,
) {
    use tokio::sync::broadcast::error::RecvError;
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(event) => {
                    let Some(server) = server.upgrade() else { return };
                    server.on_event(&event).await;
                }
                // Missed events: tell subscribers that everything may have
                // changed.
                Err(RecvError::Lagged(_)) => {
                    let Some(server) = server.upgrade() else { return };
                    server.on_event(&Event::AccountsChanged).await;
                }
                Err(RecvError::Closed) => return,
            },
            changed = context.changed() => {
                if changed.is_err() {
                    return;
                }
                let Some(server) = server.upgrade() else { return };
                server.ui_context_changed().await;
            }
        }
    }
}
