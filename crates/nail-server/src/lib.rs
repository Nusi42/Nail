//! The HTTP/WebSocket server of `nail serve` and the remote UI.
//!
//! Routes of the app origin, as in design.md ("Remote UI security"):
//!
//! * `/api/ws` — one WebSocket carrying the Nail protocol: the client sends
//!   [`nail_api::RpcRequest`] frames, the server answers with
//!   [`nail_api::ServerMessage`] frames (responses by `id`, plus pushed
//!   events). See `docs/adr/0002-rpc-protocol.md`.
//! * `/api/blob/{hash}` — a raw message from the blob store by SHA-256.
//! * `/api/proxy/{token}` — the remote-content proxy
//!   ([`nail_api::remote_content::PROXY_PATH`]), when
//!   [`TransportOptions::proxy`] is set: signed URLs only
//!   ([`nail_proxy`]).
//! * `/api/test/{command}` — only when [`ServerOptions::test_control`] is
//!   set (`nail serve --seed demo-multi`): `POST` a JSON body to drive the
//!   in-process mock servers from end-to-end tests. Never present in
//!   normal runs.
//! * `/.well-known/acme-challenge/{token}` — only while an ACME order is
//!   running ([`acme`]; internet mode, locked in this build).
//! * everything else — the UI bundle (`ui/dist`) with a single-page-app
//!   fallback to `index.html`.
//!
//! **Transport and browser hardening** (plan M2b WP42, ADR
//! 0038): [`Server::bind`] serves loopback over plain HTTP
//! (the local `nail serve`). [`Server::bind_with`] adds TLS ([`tls`]:
//! TLS 1.3, TLS 1.2 only with forward-secret AEAD suites; certificates
//! from [`cert`] or [`acme`]), which it requires for any non-loopback
//! address, the allowed networks, the host names browsers use, the proxy
//! and the **sandbox origin** ([`sandbox`]: mail HTML on its own port,
//! never on the app's origin). Every route checks `Host` and `Origin`
//! against the names the server is reachable as, plus a fetch-metadata
//! policy (`guard`, DNS-rebinding and cross-site protection), and every
//! response carries the security headers of [`headers`] (strict CSP
//! without inline script, Trusted Types, COOP/COEP/CORP,
//! `frame-ancestors 'none'`, HSTS with TLS).

#![warn(missing_docs)]

pub mod acme;
pub mod cert;
mod guard;
pub mod headers;
mod http;
mod lan;
pub mod proxy_limit;
pub mod remote_ui;
pub mod sandbox;
pub mod tls;
mod ws;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use ipnet::IpNet;
use nail_core::Service;
use nail_proxy::Proxy;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

pub use headers::HeaderProfile;
pub use http::{router, router_with_control};
pub use remote_ui::{RemoteUiHost, RemoteUiOptions};
pub use sandbox::MailSource;
pub use tls::ClientAddr;

use crate::acme::AcmeChallenges;
use crate::guard::HostAllowlist;

/// Test hooks behind `POST /api/test/{command}` (end-to-end tests against
/// mock servers). Only servers given one expose the route.
pub trait TestControl: Send + Sync + 'static {
    /// Runs `command` with JSON `args`; `Err` answers `400` with the text.
    fn control(&self, command: &str, args: &serde_json::Value)
    -> Result<serde_json::Value, String>;
}

/// Errors starting the server.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServerError {
    /// The address is not a loopback address ([`Server::bind`] serves
    /// localhost only).
    #[error("refusing to listen on {0}: only loopback addresses are allowed")]
    NotLoopback(SocketAddr),
    /// A non-loopback address without TLS.
    #[error("refusing to listen on {0} without TLS")]
    TlsRequired(SocketAddr),
    /// A sandbox origin needs the proxy's signer (render tokens).
    #[error("the sandbox origin needs the remote-content proxy")]
    SandboxNeedsProxy,
    /// Binding the listener failed.
    #[error("cannot listen on {addr}: {source}")]
    Bind {
        /// The requested address.
        addr: SocketAddr,
        /// The cause.
        #[source]
        source: std::io::Error,
    },
}

/// What to serve and where.
#[derive(Clone)]
pub struct ServerOptions {
    /// Listen address. [`Server::bind`] requires a loopback address;
    /// [`Server::bind_with`] allows others with TLS. Port 0 picks a free
    /// port.
    pub addr: SocketAddr,
    /// Directory of the built UI (`ui/dist`); `None` serves the API only.
    pub ui_dir: Option<PathBuf>,
    /// Test hooks (`/api/test/…`); `None` in normal runs.
    pub test_control: Option<Arc<dyn TestControl>>,
}

impl std::fmt::Debug for ServerOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerOptions")
            .field("addr", &self.addr)
            .field("ui_dir", &self.ui_dir)
            .field("test_control", &self.test_control.is_some())
            .finish()
    }
}

/// The sandbox origin (mail HTML) of a server.
#[derive(Clone)]
pub struct SandboxOptions {
    /// Its port on the same address (0 = a free port). Must differ from
    /// the app's.
    pub port: u16,
    /// Where mail HTML comes from; `None` = the service
    /// (`GetMessage` → `sanitized_html`).
    pub source: Option<Arc<dyn MailSource>>,
}

impl std::fmt::Debug for SandboxOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SandboxOptions")
            .field("port", &self.port)
            .finish_non_exhaustive()
    }
}

/// Transport and browser hardening for [`Server::bind_with`]. The default
/// is the loopback behaviour of [`Server::bind`].
#[derive(Clone, Default)]
pub struct TransportOptions {
    /// TLS configuration ([`cert::ServerIdentity::tls_config`]); `None` =
    /// plain HTTP, loopback only.
    pub tls: Option<Arc<rustls::ServerConfig>>,
    /// Names browsers use besides the loopback names: the host name, the
    /// machine's LAN addresses. `Host` / `Origin` must name one of them.
    pub host_names: Vec<String>,
    /// Peers allowed to connect (loopback always may); empty = any peer
    /// from a local network (or anyone, with `public_peers`).
    pub allowed_networks: Vec<IpNet>,
    /// Whether peers outside local networks may connect (internet mode,
    /// locked in M2). Off: only loopback, RFC 1918, link-local, CGNAT and
    /// IPv6 unique-local peers ([`nail_api::remote::is_local_address`]).
    pub public_peers: bool,
    /// The remote-content proxy (route on both origins).
    pub proxy: Option<Arc<Proxy>>,
    /// The sandbox origin for mail HTML (needs `proxy`).
    pub sandbox: Option<SandboxOptions>,
    /// Header profile; non-loopback addresses always use
    /// [`HeaderProfile::Strict`].
    pub headers: HeaderProfile,
    /// Serve ACME HTTP-01 challenges ([`acme`]; internet mode only).
    pub acme_challenges: Option<AcmeChallenges>,
    /// Remote-UI authentication: mounts `POST /api/auth` and lets only
    /// requests with a valid session reach the WebSocket, blobs and the
    /// proxy; every WebSocket request is authorized
    /// ([`nail_core::Service::handle_remote`]). `None` = the local UI
    /// (loopback `nail serve`, no login).
    pub auth: Option<nail_remote_auth::RemoteAuth>,
}

impl std::fmt::Debug for TransportOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransportOptions")
            .field("tls", &self.tls.is_some())
            .field("host_names", &self.host_names)
            .field("allowed_networks", &self.allowed_networks)
            .field("public_peers", &self.public_peers)
            .field("proxy", &self.proxy.is_some())
            .field("sandbox", &self.sandbox)
            .field("headers", &self.headers)
            .field("auth", &self.auth.is_some())
            .finish_non_exhaustive()
    }
}

/// A running server. Stops on [`Server::shutdown`] or when dropped.
#[derive(Debug)]
pub struct Server {
    local_addr: SocketAddr,
    sandbox_addr: Option<SocketAddr>,
    tls: bool,
    shutdown: Option<watch::Sender<bool>>,
    task: Option<JoinHandle<std::io::Result<()>>>,
}

async fn listen(addr: SocketAddr) -> Result<(TcpListener, SocketAddr), ServerError> {
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|source| ServerError::Bind { addr, source })?;
    let local = listener
        .local_addr()
        .map_err(|source| ServerError::Bind { addr, source })?;
    Ok((listener, local))
}

/// Serves `app` on `listener` until `stop` turns `true`.
async fn serve(
    listener: TcpListener,
    tls: Option<Arc<rustls::ServerConfig>>,
    peers: tls::PeerPolicy,
    app: axum::Router,
    mut stop: watch::Receiver<bool>,
) -> std::io::Result<()> {
    let incoming = tls::Incoming::start(listener, tls, peers)?;
    axum::serve(
        incoming,
        app.into_make_service_with_connect_info::<ClientAddr>(),
    )
    .with_graceful_shutdown(async move {
        drop(stop.wait_for(|stopped| *stopped).await);
    })
    .await
}

impl Server {
    /// Binds a loopback address over plain HTTP and starts serving
    /// `service` (the local `nail serve`).
    pub async fn bind(service: Service, options: ServerOptions) -> Result<Self, ServerError> {
        if !options.addr.ip().is_loopback() {
            return Err(ServerError::NotLoopback(options.addr));
        }
        Self::bind_with(service, options, TransportOptions::default()).await
    }

    /// Binds with transport and browser hardening (see
    /// [`TransportOptions`]). Refuses a non-loopback address without TLS.
    pub async fn bind_with(
        service: Service,
        options: ServerOptions,
        transport: TransportOptions,
    ) -> Result<Self, ServerError> {
        let loopback = options.addr.ip().is_loopback();
        if !loopback && transport.tls.is_none() {
            return Err(ServerError::TlsRequired(options.addr));
        }
        if transport.sandbox.is_some() && transport.proxy.is_none() {
            return Err(ServerError::SandboxNeedsProxy);
        }
        let (listener, local_addr) = listen(options.addr).await?;
        let sandbox_listener = match &transport.sandbox {
            Some(sandbox) => {
                let addr = SocketAddr::new(options.addr.ip(), sandbox.port);
                Some(listen(addr).await?)
            }
            None => None,
        };
        if let Some(dir) = &options.ui_dir
            && !dir.join("index.html").is_file()
        {
            tracing::warn!(dir = %dir.display(), "no index.html in the UI directory; run `npm run build -w ui`");
        }

        let tls = transport.tls.is_some();
        let scheme = if tls { "https" } else { "http" };
        let hosts = HostAllowlist::with_names(scheme, &transport.host_names, local_addr.port());
        let sandbox_hosts = sandbox_listener
            .as_ref()
            .map(|(_, addr)| HostAllowlist::with_names(scheme, &transport.host_names, addr.port()));
        let profile = if loopback {
            transport.headers
        } else {
            HeaderProfile::Strict
        };
        let app = http::app_router(
            service.clone(),
            http::AppConfig {
                ui_dir: options.ui_dir,
                control: options.test_control,
                hosts: hosts.clone(),
                profile,
                tls,
                sandbox_origins: sandbox_hosts
                    .as_ref()
                    .map(HostAllowlist::origins)
                    .unwrap_or_default(),
                proxy: transport.proxy.clone(),
                acme: transport.acme_challenges.clone(),
                auth: transport.auth.clone(),
            },
        );

        let (tx, rx) = watch::channel(false);
        let peers = tls::PeerPolicy {
            networks: transport.allowed_networks.clone(),
            public_peers: transport.public_peers,
        };
        let app_serve = serve(
            listener,
            transport.tls.clone(),
            peers.clone(),
            app,
            rx.clone(),
        );
        let mut sandbox_addr = None;
        let sandbox_serve = match (sandbox_listener, sandbox_hosts, &transport.proxy) {
            (Some((listener, addr)), Some(sandbox_hosts), Some(proxy)) => {
                sandbox_addr = Some(addr);
                let source = transport
                    .sandbox
                    .as_ref()
                    .and_then(|s| s.source.clone())
                    .unwrap_or_else(|| Arc::new(service.clone()));
                let router = sandbox_app(
                    sandbox::SandboxState {
                        proxy: Arc::clone(proxy),
                        source,
                        frame_ancestors: headers::csp_sources(&hosts.origins())
                            .collect::<Vec<_>>()
                            .join(" "),
                        scheme,
                    },
                    sandbox_hosts,
                    tls,
                );
                Some(serve(listener, transport.tls.clone(), peers, router, rx))
            }
            _ => None,
        };
        let task = tokio::spawn(async move {
            match sandbox_serve {
                Some(sandbox) => tokio::try_join!(app_serve, sandbox).map(|_| ()),
                None => app_serve.await,
            }
        });
        tracing::info!(%local_addr, ?sandbox_addr, tls, "serving");
        Ok(Self {
            local_addr,
            sandbox_addr,
            tls,
            shutdown: Some(tx),
            task: Some(task),
        })
    }

    /// The address the server listens on.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// The sandbox origin's address, if it has one.
    pub fn sandbox_addr(&self) -> Option<SocketAddr> {
        self.sandbox_addr
    }

    /// `http://<address>/` or `https://<address>/`.
    pub fn url(&self) -> String {
        format!("{}://{}/", self.scheme(), self.local_addr)
    }

    /// The sandbox origin (`https://<address>`), if any: the app frames
    /// `<origin>/render/<token>`.
    pub fn sandbox_origin(&self) -> Option<String> {
        self.sandbox_addr
            .map(|addr| format!("{}://{addr}", self.scheme()))
    }

    fn scheme(&self) -> &'static str {
        if self.tls { "https" } else { "http" }
    }

    /// Waits until the server stops (it only stops on error or shutdown).
    pub async fn wait(mut self) -> std::io::Result<()> {
        match self.task.take() {
            Some(task) => task.await.map_err(std::io::Error::other)?,
            None => Ok(()),
        }
    }

    /// Serves until `signal` completes (e.g. Ctrl+C), then shuts down.
    /// Returns early if the server fails.
    pub async fn run_until(mut self, signal: impl Future<Output = ()>) -> std::io::Result<()> {
        let Some(mut task) = self.task.take() else {
            return Ok(());
        };
        tokio::select! {
            result = &mut task => result.map_err(std::io::Error::other)?,
            () = signal => {
                self.task = Some(task);
                self.shutdown().await
            }
        }
    }

    /// Stops accepting connections, lets open requests finish and waits.
    /// Open WebSockets are closed.
    pub async fn shutdown(mut self) -> std::io::Result<()> {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(true);
        }
        match self.task.take() {
            // WebSockets never finish on their own; don't wait forever.
            Some(task) => {
                match tokio::time::timeout(std::time::Duration::from_secs(5), task).await {
                    Ok(result) => result.map_err(std::io::Error::other)?,
                    Err(_) => Ok(()),
                }
            }
            None => Ok(()),
        }
    }
}

/// The sandbox origin's router with its guard (Host only, no fetch
/// metadata: every route is a capability URL) and baseline headers.
fn sandbox_app(state: sandbox::SandboxState, hosts: HostAllowlist, tls: bool) -> axum::Router {
    use axum::middleware;
    let set = Arc::new(headers::HeaderSet::sandbox(tls));
    sandbox::router(state)
        .layer(middleware::from_fn_with_state(
            guard::Guard {
                hosts,
                fetch_metadata: false,
            },
            guard::check,
        ))
        .layer(middleware::from_fn_with_state(set, headers::apply))
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(true);
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
