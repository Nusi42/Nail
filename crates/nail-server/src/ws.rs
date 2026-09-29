//! `/api/ws`: the Nail protocol over one WebSocket.

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use nail_api::{ApiError, RpcRequest, ServerMessage};
use nail_core::Service;
use tokio::sync::{broadcast, mpsc};

/// Largest accepted request frame. Requests are small JSON objects.
const MAX_FRAME: usize = 1 << 20;
/// Frames queued for one client before request handlers wait.
const OUTBOX: usize = 256;

/// Upgrades to a WebSocket after checking that a browser's `Origin` is this
/// server (no cross-site WebSocket hijacking). Clients without an `Origin`
/// header (tools, tests) are allowed: the server binds to loopback only.
pub(crate) async fn upgrade(
    State(service): State<Service>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !origin_allowed(&headers) {
        tracing::warn!(origin = ?headers.get(header::ORIGIN), "WebSocket from a foreign origin refused");
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.max_message_size(MAX_FRAME)
        .max_frame_size(MAX_FRAME)
        .on_upgrade(move |socket| serve_socket(socket, service))
}

/// Whether the `Origin` header is absent or names the same host (and port)
/// as the `Host` header.
fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let (Ok(origin), Some(Ok(host))) = (
        origin.to_str(),
        headers.get(header::HOST).map(|h| h.to_str()),
    ) else {
        return false;
    };
    let authority = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"));
    authority.is_some_and(|a| a.eq_ignore_ascii_case(host))
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
        let mut events = service.subscribe();
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) => {
                        if tx.send(event.into()).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(missed)) => {
                        tracing::warn!(missed, "a WebSocket client missed events");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(origin: Option<&str>, host: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(o) = origin {
            h.insert(header::ORIGIN, HeaderValue::from_str(o).unwrap());
        }
        if let Some(o) = host {
            h.insert(header::HOST, HeaderValue::from_str(o).unwrap());
        }
        h
    }

    #[test]
    fn origin_check() {
        assert!(origin_allowed(&headers(None, Some("127.0.0.1:4780"))));
        assert!(origin_allowed(&headers(
            Some("http://127.0.0.1:4780"),
            Some("127.0.0.1:4780")
        )));
        assert!(origin_allowed(&headers(
            Some("http://LOCALHOST:4780"),
            Some("localhost:4780")
        )));
        assert!(!origin_allowed(&headers(
            Some("http://evil.example"),
            Some("127.0.0.1:4780")
        )));
        assert!(!origin_allowed(&headers(
            Some("http://127.0.0.1:9999"),
            Some("127.0.0.1:4780")
        )));
        assert!(!origin_allowed(&headers(
            Some("null"),
            Some("127.0.0.1:4780")
        )));
        assert!(!origin_allowed(&headers(
            Some("http://127.0.0.1:4780"),
            None
        )));
    }
}
