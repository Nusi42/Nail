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

use nail_api::{ApiError, Event, LocalizedMessage, McpSettings, MessageId, Request, Response};
use nail_mcp::{CoreAccess, CoreData, McpServer, ServerOptions};
use nail_store::{BlobStore, MCP_SETTINGS_KEY, Store};

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
    // M2c (ADR 0043): the settings are the core's; the server picks the
    // port up when it (re)starts (wired by WP53 / WP55).
    let running = || {
        info.clone().unwrap_or(nail_api::mcp::McpServerInfo {
            running: false,
            url: None,
        })
    };
    match request {
        Request::GetMcpSettings => {
            let settings: McpSettings = service
                .store()
                .call(|s| s.setting(MCP_SETTINGS_KEY))
                .await?
                .unwrap_or_default();
            return Ok(Response::McpSettings {
                settings,
                info: running(),
            });
        }
        // M4a: a revoked client's pending confirmations are
        // cancelled (its waiting calls answer `expired`), its policies go.
        Request::RevokeMcpClient { ref client_id } => {
            let actor = nail_api::agents::ActionActor::mcp_client(client_id.clone());
            let events = service.clone();
            let response = nail_mcp::admin::handle(service.store(), info, request, move |event| {
                events.broadcast(event);
            })
            .await?;
            service.forget_actor(&actor).await;
            return Ok(response);
        }
        // Review M4a: pending writes were checked under the old grant.
        Request::UpdateMcpGrants { ref client_id, .. } => {
            let actor = nail_api::agents::ActionActor::mcp_client(client_id.clone());
            let events = service.clone();
            let response = nail_mcp::admin::handle(service.store(), info, request, move |event| {
                events.broadcast(event);
            })
            .await?;
            service.cancel_confirmations_of(&actor).await;
            return Ok(response);
        }
        Request::UpdateMcpSettings { settings } => {
            if let Some(issue) = settings.issues().first() {
                return Err(ApiError::invalid_request(format!(
                    "invalid MCP settings ({})",
                    issue.message_id()
                ))
                .with_localized(LocalizedMessage::new(issue.message_id()))
                .into());
            }
            let saved = settings.clone();
            service
                .store()
                .call(move |s| s.set_setting(MCP_SETTINGS_KEY, &saved))
                .await?;
            service.broadcast(Event::McpSettingsChanged);
            return Ok(Response::McpSettings {
                settings,
                info: running(),
            });
        }
        _ => {}
    }
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
    async fn search_local(&self, query: nail_api::SearchQuery) -> Response {
        match self
            .0
            .search_with(query, crate::search::ServerSearchMode::Off)
            .await
        {
            Ok(response) => response,
            Err(err) => ApiError::from(err).into(),
        }
    }

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

    fn handle_write(
        &self,
        by: &nail_api::agents::ActionAttribution,
        request: Request,
    ) -> impl Future<Output = Response> + Send {
        // Writes of agents and plug-ins run as the UI's own requests:
        // through the operation queue, undoable. Who asked is recorded in
        // the confirmation and the audit log.
        tracing::debug!(actor = %by.name, "agent write");
        let service = self.0.clone();
        async move { service.handle(request).await }
    }

    fn confirmations(&self) -> Option<Arc<dyn nail_mcp::broker::ConfirmationBroker>> {
        Some(self.0.confirmation_broker())
    }

    fn rate_limiter(&self) -> Option<Arc<nail_mcp::limits::RateLimiter>> {
        Some(self.0.mcp_rate_limiter())
    }

    fn render(&self, message: &LocalizedMessage) -> String {
        crate::strings::render(self.0.inner.locale.as_deref(), message)
    }

    fn plugin_tools(&self) -> Vec<nail_mcp::PluginTool> {
        self.0
            .inner
            .plugins
            .existing_host()
            .map(|host| {
                host.mcp_tools()
                    .into_iter()
                    .map(|t| nail_mcp::PluginTool {
                        name: t.name,
                        plugin_name: t.plugin_name,
                        description: t.description,
                        input_schema: t.input_schema,
                        scopes: t.scopes,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn call_plugin_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
        client: nail_api::agents::McpClientRef,
    ) -> impl Future<Output = std::result::Result<serde_json::Value, ApiError>> + Send {
        let host = self.0.inner.plugins.existing_host().cloned();
        let name = name.to_owned();
        let store = self.0.store().clone();
        async move {
            let Some(host) = host else {
                return Err(ApiError::not_found(format!("no plug-in tool {name}")));
            };
            // The plug-in serves the client within the client's grant too
            // (review M4a): read fresh, so a narrowed or revoked grant
            // applies at once.
            let id = client.client_id.clone();
            let grant = store
                .call(move |s| s.get_mcp_client(&id))
                .await
                .map(|c| c.grant)
                .map_err(ApiError::from)?;
            let limit = nail_plugins::ClientLimit::from_grant(&grant);
            host.call_mcp_tool(&name, arguments, client, limit).await
        }
    }

    fn is_encrypted(
        &self,
        id: &MessageId,
    ) -> impl Future<Output = std::result::Result<bool, ApiError>> + Send {
        let service = self.0.clone();
        let id = id.clone();
        async move {
            service
                .blocking(move |s| s.message_is_encrypted(&id))
                .await
                .map_err(ApiError::from)
        }
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
