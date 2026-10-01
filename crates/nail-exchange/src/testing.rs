//! A mock Autodiscover server for tests and the demo-multi seed (feature
//! `testing`; plan M3c WP93, contract ADR 0072).
//!
//! **Stub (WP90).** It binds 127.0.0.1 only (ADR 0057) and answers
//! Autodiscover v2 (`/autodiscover/autodiscover.json/v1.0/<address>`)
//! with the EWS URL it was started with; WP93 adds POX answers, redirects,
//! logins and automapped mailboxes.

use std::net::SocketAddr;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query};
use axum::routing::get;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// A running mock Autodiscover server; stops when dropped.
#[derive(Debug)]
pub struct MockAutodiscoverServer {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl MockAutodiscoverServer {
    /// Starts a server on a free loopback port that points every address
    /// to `ews_url`.
    pub async fn start(ews_url: &str) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let ews_url = ews_url.to_owned();
        let app = Router::new().route(
            "/autodiscover/autodiscover.json/v1.0/{address}",
            get(
                move |Path(_address): Path<String>,
                      Query(query): Query<std::collections::HashMap<String, String>>| {
                    let ews_url = ews_url.clone();
                    async move {
                        let protocol = query.get("Protocol").cloned().unwrap_or_default();
                        let answer: Value = json!({
                            "Protocol": protocol,
                            "Url": ews_url,
                        });
                        Json(answer)
                    }
                },
            ),
        );
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Self { addr, task })
    }

    /// The server's address (127.0.0.1 and its port).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The host to ask (`127.0.0.1:<port>`).
    pub fn host(&self) -> String {
        format!("127.0.0.1:{}", self.addr.port())
    }
}

impl Drop for MockAutodiscoverServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
