//! `/api/ws`: the Nail protocol over one WebSocket.
//!
//! In the remote UI the upgrade passed the session middleware, so the
//! socket belongs to one remote session: every request goes through
//! [`Service::handle_remote`] (authorization, grant narrowing), the session
//! is re-checked every [`REFRESH_EVERY`] (idle / absolute expiry, revoked
//! device, disabled user; a narrowed grant applies from then on), and the
//! socket closes when the session ends (`RemoteSessionEnded`).

use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use nail_api::{ApiError, Event, RpcRequest, ServerMessage};
use nail_core::Service;
use nail_remote_auth::RemoteSessionCtx;
use tokio::sync::{broadcast, mpsc, watch};

/// How often an idle remote session's socket re-checks its session (each
/// request re-checks it too). An open socket counts as activity.
const REFRESH_EVERY: Duration = Duration::from_mins(1);

/// Largest accepted request frame. Requests are small JSON objects.
const MAX_FRAME: usize = 1 << 20;
/// Frames queued for one client before request handlers wait.
const OUTBOX: usize = 256;

/// Upgrades to a WebSocket. `Host` and `Origin` were already checked
/// against the loopback allowlist by [`crate::guard`] (no cross-site
/// WebSocket hijacking, no DNS rebinding).
pub(crate) async fn upgrade(
    State(service): State<Service>,
    session: Option<axum::Extension<RemoteSessionCtx>>,
    ws: WebSocketUpgrade,
) -> Response {
    let session = session.map(|axum::Extension(ctx)| Arc::new(RwLock::new(ctx)));
    ws.max_message_size(MAX_FRAME)
        .max_frame_size(MAX_FRAME)
        .on_upgrade(move |socket| serve_socket(socket, service, session))
}

type Session = Arc<RwLock<RemoteSessionCtx>>;

fn current(session: &Session) -> RemoteSessionCtx {
    session
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

async fn serve_socket(socket: WebSocket, service: Service, session: Option<Session>) {
    let (mut sink, mut stream) = socket.split();
    // Turns true when a remote session ends: the socket closes.
    let (end_tx, mut end_rx) = watch::channel(false);
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
        drop(sink.close().await);
    });

    // Errors found before this client connected (e.g. at start-up).
    for event in service.config_error_events() {
        drop(tx.send(event.into()).await);
    }

    // Events: forward what the core broadcasts; a remote session only what
    // its (current) grant covers (`Service::remote_event`).
    let forwarder = {
        let tx = tx.clone();
        let service = service.clone();
        let mut events = service.subscribe();
        let own_session = session.as_ref().map(|s| current(s).session.id);
        let session = session.clone();
        let end_tx = end_tx.clone();
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
                    let ends = matches!(
                        (&event, &own_session),
                        (Event::RemoteSessionEnded { session_id, .. }, Some(own)) if session_id == own
                    );
                    let event = match &session {
                        Some(session) => {
                            let ctx = current(session);
                            match service.remote_event(&ctx, event).await {
                                Some(event) => event,
                                None => continue,
                            }
                        }
                        None => event,
                    };
                    if tx.send(event.into()).await.is_err() {
                        return;
                    }
                    if ends {
                        let _ = end_tx.send(true);
                        return;
                    }
                }
            }
        })
    };

    // A remote session is re-checked periodically.
    let refresher = session.clone().map(|session| {
        let service = service.clone();
        let end_tx = end_tx.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(REFRESH_EVERY);
            every.tick().await;
            loop {
                every.tick().await;
                let ctx = current(&session);
                if let Some(fresh) = service.remote_auth().refresh(&ctx) {
                    *session
                        .write()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = fresh;
                } else {
                    let _ = end_tx.send(true);
                    return;
                }
            }
        })
    });

    // Requests: each is answered concurrently; responses carry the id.
    loop {
        let frame = tokio::select! {
            frame = stream.next() => frame,
            _ = end_rx.wait_for(|ended| *ended) => break,
        };
        let Some(frame) = frame else { break };
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
                // Every request re-reads the session: a re-authentication
                // or a narrowed grant applies at once, an ended session
                // closes the socket.
                let ctx = match &session {
                    Some(session) => {
                        if let Some(fresh) = service.remote_auth().refresh(&current(session)) {
                            *session
                                .write()
                                .unwrap_or_else(std::sync::PoisonError::into_inner) = fresh.clone();
                            Some(fresh)
                        } else {
                            let response = ApiError::new(
                                nail_api::ErrorCode::AuthFailed,
                                "the remote session has ended",
                            )
                            .into();
                            drop(tx.send(ServerMessage::Response { id, response }).await);
                            break;
                        }
                    }
                    None => None,
                };
                tokio::spawn(async move {
                    let response = match ctx {
                        Some(ctx) => service.handle_remote(&ctx, request).await,
                        None => service.handle(request).await,
                    };
                    drop(tx.send(ServerMessage::Response { id, response }).await);
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
                    drop(tx.send(ServerMessage::Response { id, response }).await);
                }
            }
        }
    }

    forwarder.abort();
    if let Some(refresher) = refresher {
        refresher.abort();
    }
    drop(tx);
    // In-flight handlers hold senders; the writer ends once they are done.
    drop(writer.await);
}
