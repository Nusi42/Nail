//! The HTTP/WebSocket server of `nail serve`.
//!
//! Three routes, as in design.md ("Remote UI security"):
//!
//! * `/api/ws` — one WebSocket carrying the Nail protocol: the client sends
//!   [`nail_api::RpcRequest`] frames, the server answers with
//!   [`nail_api::ServerMessage`] frames (responses by `id`, plus pushed
//!   events). See `docs/adr/0002-rpc-protocol.md`.
//! * `/api/blob/{hash}` — a raw message from the blob store by SHA-256.
//! * `/api/test/{command}` — only when [`ServerOptions::test_control`] is
//!   set (`nail serve --seed demo-multi`): `POST` a JSON body to drive the
//!   in-process mock servers from end-to-end tests. Never present in
//!   normal runs.
//! * everything else — the UI bundle (`ui/dist`) with a single-page-app
//!   fallback to `index.html`.
//!
//! In M0 the server binds to loopback addresses only and has no
//! authentication; [`Server::bind`] refuses anything else. Every route
//! accepts only `Host` (and `Origin`) values naming a loopback address with
//! the bound port, which defeats DNS rebinding. Remote access
//! with TLS, pairing and passkeys arrives in M2.

#![warn(missing_docs)]

mod guard;
mod http;
mod ws;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use nail_core::Service;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

pub use http::{router, router_with_control};

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
    /// The address is not a loopback address (M0 serves localhost only).
    #[error("refusing to listen on {0}: only loopback addresses are allowed")]
    NotLoopback(SocketAddr),
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
    /// Listen address; must be a loopback address. Port 0 picks a free port.
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

/// A running server. Stops on [`Server::shutdown`] or when dropped.
#[derive(Debug)]
pub struct Server {
    local_addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<std::io::Result<()>>>,
}

impl Server {
    /// Binds the listener and starts serving `service`.
    pub async fn bind(service: Service, options: ServerOptions) -> Result<Self, ServerError> {
        if !options.addr.ip().is_loopback() {
            return Err(ServerError::NotLoopback(options.addr));
        }
        let listener =
            TcpListener::bind(options.addr)
                .await
                .map_err(|source| ServerError::Bind {
                    addr: options.addr,
                    source,
                })?;
        let local_addr = listener.local_addr().map_err(|source| ServerError::Bind {
            addr: options.addr,
            source,
        })?;
        if let Some(dir) = &options.ui_dir
            && !dir.join("index.html").is_file()
        {
            tracing::warn!(dir = %dir.display(), "no index.html in the UI directory; run `npm run build -w ui`");
        }

        let app = router_with_control(
            service,
            options.ui_dir,
            local_addr.port(),
            options.test_control,
        );
        let (tx, rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = rx.await;
                })
                .await
        });
        tracing::info!(%local_addr, "serving");
        Ok(Self {
            local_addr,
            shutdown: Some(tx),
            task: Some(task),
        })
    }

    /// The address the server listens on.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// `http://<address>/`.
    pub fn url(&self) -> String {
        format!("http://{}/", self.local_addr)
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
            let _ = tx.send(());
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

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
