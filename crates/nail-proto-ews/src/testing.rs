//! A mock EWS SOAP server for tests and the demo-multi seed (feature
//! `testing`; plan M3c WP92, contract ADR 0072).
//!
//! **Stub (WP90).** It binds 127.0.0.1 only (ADR 0057) and answers every
//! `POST /EWS/Exchange.asmx` with a `GetFolder` response for the inbox;
//! WP92 grows it into an in-memory mailbox (folders, items with change
//! keys and sync states, OOF, rules, categories, rooms, calendar,
//! contacts, tasks, streaming notifications, logins, injected faults) with
//! recorded-response fixtures.

use std::net::SocketAddr;

use axum::Router;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::post;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// A running mock EWS server; stops when dropped.
#[derive(Debug)]
pub struct MockEwsServer {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

const GET_FOLDER_RESPONSE: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?>"#,
    r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">"#,
    r#"<s:Header><h:ServerVersionInfo MajorVersion="15" MinorVersion="2" "#,
    r#"MajorBuildNumber="1544" MinorBuildNumber="4" "#,
    r#"xmlns:h="http://schemas.microsoft.com/exchange/services/2006/types"/></s:Header>"#,
    r#"<s:Body><m:GetFolderResponse "#,
    r#"xmlns:m="http://schemas.microsoft.com/exchange/services/2006/messages" "#,
    r#"xmlns:t="http://schemas.microsoft.com/exchange/services/2006/types">"#,
    r#"<m:ResponseMessages><m:GetFolderResponseMessage ResponseClass="Success">"#,
    r#"<m:ResponseCode>NoError</m:ResponseCode><m:Folders><t:Folder>"#,
    r#"<t:FolderId Id="AAMkInbox" ChangeKey="AQAAAA"/>"#,
    r#"<t:DisplayName>Inbox</t:DisplayName><t:TotalCount>0</t:TotalCount>"#,
    r#"<t:UnreadCount>0</t:UnreadCount></t:Folder></m:Folders>"#,
    r#"</m:GetFolderResponseMessage></m:ResponseMessages></m:GetFolderResponse>"#,
    r#"</s:Body></s:Envelope>"#
);

impl MockEwsServer {
    /// Starts a server on a free loopback port.
    pub async fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let app = Router::new().route(
            nail_api::exchange::EWS_PATH,
            post(|| async {
                (
                    [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
                    GET_FOLDER_RESPONSE,
                )
                    .into_response()
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

    /// The EWS endpoint (`http://127.0.0.1:<port>/EWS/Exchange.asmx`).
    pub fn url(&self) -> String {
        format!(
            "http://127.0.0.1:{}{}",
            self.addr.port(),
            nail_api::exchange::EWS_PATH
        )
    }
}

impl Drop for MockEwsServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
