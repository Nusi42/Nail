//! A mock Gmail REST server for tests and the demo-multi seed (feature
//! `testing`; plan M3b WP82, contract ADR 0065).
//!
//! **Stub (WP80).** It binds 127.0.0.1 only (ADR 0057) and serves
//! `users.getProfile`; WP82 grows it into an in-memory Gmail (messages,
//! labels, threads, history, quotas and injected errors, server-side
//! changes) with recorded-response fixtures.

use std::net::SocketAddr;

use axum::Json;
use axum::Router;
use axum::routing::get;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// A running mock Gmail server; stops when dropped.
#[derive(Debug)]
pub struct MockGmailServer {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl MockGmailServer {
    /// Starts a server on a free loopback port for the mailbox `email`.
    pub async fn start(email: &str) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let email = email.to_owned();
        let app = Router::new().route(
            &format!("{}/profile", nail_api::gmail::GMAIL_API_BASE_PATH),
            get(move || {
                let email = email.clone();
                async move {
                    Json(json!({
                        "emailAddress": email,
                        "messagesTotal": 0,
                        "threadsTotal": 0,
                        "historyId": "1",
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

    /// The API base URL (`http://127.0.0.1:<port>/gmail/v1/users/me`).
    pub fn api_base(&self) -> String {
        crate::api_base("127.0.0.1", self.addr.port(), false)
    }
}

impl Drop for MockGmailServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
