//! The HTTP/WebSocket server of `nail serve`.
//!
//! Three routes, as in design.md ("Remote UI security"):
//!
//! * `/api/ws` — one WebSocket carrying the Nail protocol: the client sends
//!   [`nail_api::RpcRequest`] frames, the server answers with
//!   [`nail_api::ServerMessage`] frames (responses by `id`, plus pushed
//!   events). See `docs/adr/0002-rpc-protocol.md`.
//! * `/api/blob/{hash}` — a raw message from the blob store by SHA-256.
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

use nail_core::Service;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

pub use http::router;

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerOptions {
    /// Listen address; must be a loopback address. Port 0 picks a free port.
    pub addr: SocketAddr,
    /// Directory of the built UI (`ui/dist`); `None` serves the API only.
    pub ui_dir: Option<PathBuf>,
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

        let app = router(service, options.ui_dir, local_addr.port());
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
