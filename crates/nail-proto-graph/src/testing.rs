//! A mock Microsoft Graph server for tests and the demo-multi seed
//! (feature `testing`; plan M3c WP91, contract ADR 0072).
//!
//! **Stub (WP90).** It binds 127.0.0.1 only (ADR 0057) and serves `GET
//! /v1.0/me`; WP91 grows it into an in-memory tenant (mailboxes, folders,
//! messages with delta tokens, categories, rules, automatic replies,
//! rooms, calendars, contacts, To Do, throttling and injected errors) with
//! recorded-response fixtures.

use std::net::SocketAddr;

use axum::Json;
use axum::Router;
use axum::routing::get;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// A running mock Graph server; stops when dropped.
#[derive(Debug)]
pub struct MockGraphServer {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl MockGraphServer {
    /// Starts a server on a free loopback port for the user `email`.
    pub async fn start(email: &str) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let email = email.to_owned();
        let app = Router::new().route(
            &format!("{}/me", nail_api::exchange::GRAPH_BASE_PATH),
            get(move || {
                let email = email.clone();
                async move {
                    Json(json!({
                        "id": "00000000-0000-0000-0000-000000000001",
                        "displayName": "Ada Lovelace",
                        "mail": email,
                        "userPrincipalName": email,
                    }))
                }
            }),
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

    /// The API base URL (`http://127.0.0.1:<port>/v1.0`).
    pub fn api_base(&self) -> String {
        crate::api_base("127.0.0.1", self.addr.port(), false)
    }
}

impl Drop for MockGraphServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
