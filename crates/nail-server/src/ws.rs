//! `/api/ws`: the Nail protocol over one WebSocket.

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use nail_api::{ApiError, RpcRequest, ServerMessage};
use nail_core::Service;
use tokio::sync::{broadcast, mpsc};

/// Largest accepted request frame. Requests are small JSON objects.
const MAX_FRAME: usize = 1 << 20;
/// Frames queued for one client before request handlers wait.
const OUTBOX: usize = 256;

/// Upgrades to a WebSocket. `Host` and `Origin` were already checked
/// against the loopback allowlist by [`crate::guard`] (no cross-site
/// WebSocket hijacking, no DNS rebinding).
pub(crate) async fn upgrade(State(service): State<Service>, ws: WebSocketUpgrade) -> Response {
    ws.max_message_size(MAX_FRAME)
        .max_frame_size(MAX_FRAME)
        .on_upgrade(move |socket| serve_socket(socket, service))
}

async fn serve_socket(socket: WebSocket, service: Service) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(OUTBOX);

    // Writer: the only task that touches the sink.
    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            let text = match serde_json::to_string(&message) {
                Ok(text) => text,
                Err(err) => {
                    tracing::error!(%err, "cannot serialize a server message");
                    continue;
                }
            };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    // Errors found before this client connected (e.g. at start-up).
    for event in service.config_error_events() {
        let _ = tx.send(event.into()).await;
    }

    // Events: forward everything the core broadcasts.
    let forwarder = {
        let tx = tx.clone();
        let service = service.clone();
        let mut events = service.subscribe();
        tokio::spawn(async move {
            loop {
                let batch = match events.recv().await {
                    Ok(event) => vec![event],
                    Err(broadcast::error::RecvError::Lagged(missed)) => {
                        // The client missed changes; tell it the current
                        // state so it refetches instead of staying stale.
                        tracing::warn!(missed, "a WebSocket client missed events; resyncing it");
                        service.resync_events().await
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                for event in batch {
                    if tx.send(event.into()).await.is_err() {
                        return;
                    }
                }
            }
        })
    };

    // Requests: each is answered concurrently; responses carry the id.
    while let Some(frame) = stream.next().await {
        let text = match frame {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(Message::Binary(_)) => {
                tracing::debug!("ignoring a binary WebSocket frame");
                continue;
            }
            Ok(Message::Ping(_) | Message::Pong(_)) => continue,
        };
        match serde_json::from_str::<RpcRequest>(text.as_str()) {
            Ok(RpcRequest { id, request }) => {
                let service = service.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let response = service.handle(request).await;
                    let _ = tx.send(ServerMessage::Response { id, response }).await;
                });
            }
            Err(err) => {
                // Answer malformed requests whose id is readable, so the
                // client does not wait forever; drop the rest.
                let id = serde_json::from_str::<serde_json::Value>(text.as_str())
                    .ok()
                    .and_then(|v| v.get("id").and_then(serde_json::Value::as_u64));
                tracing::debug!(%err, ?id, "malformed request frame");
                if let Some(id) = id {
                    let response = ApiError::invalid_request(err.to_string()).into();
                    let _ = tx.send(ServerMessage::Response { id, response }).await;
                }
            }
        }
    }

    forwarder.abort();
    drop(tx);
    // In-flight handlers hold senders; the writer ends once they are done.
    let _ = writer.await;
}
