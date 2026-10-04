//! The desktop's endpoint for `nail plugin dev` (M4b WP113, ADR 0087).
//!
//! Off by default (`DesktopSettings::plugin_dev_endpoint`). When on, the
//! shell listens on `127.0.0.1` (never another address, ADR 0057) on a
//! free port and writes `<data-dir>/ui-endpoint.json`
//! (`UiEndpointFile {url, pid, token}`), which `nail plugin dev` reads.
//!
//! * The WebSocket is `/api/ws`; the upgrade must carry `Authorization:
//!   Bearer <token>` (a fresh random token per start, compared in constant
//!   time). Requests with an `Origin` header (browsers) are refused, so a
//!   web page cannot reach it.
//! * Only `LoadDevPlugin` is served (`nail_api::desktop::dev_endpoint_allows`);
//!   everything else answers `forbidden`. Only plug-in log lines and
//!   plug-in list changes are forwarded (`dev_endpoint_forwards`).
//! * Stopping removes the file.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response as HttpResponse};
use futures_util::{SinkExt, StreamExt};
use nail_api::desktop::{
    UI_ENDPOINT_FILE, UiEndpointFile, dev_endpoint_allows, dev_endpoint_forwards,
};
use nail_api::{ApiError, ErrorCode, Response, RpcRequest, ServerMessage};
use nail_core::Service;
use tokio::sync::broadcast;

/// The running endpoint; stopped (and its file removed) when dropped.
#[derive(Debug)]
pub struct DevEndpoint {
    url: String,
    file: PathBuf,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

#[derive(Clone)]
struct EndpointState {
    service: Service,
    token: String,
}

impl DevEndpoint {
    /// Binds `127.0.0.1:0`, writes the endpoint file and serves on
    /// `runtime`.
    pub fn start(
        service: Service,
        data_dir: &Path,
        runtime: &tokio::runtime::Handle,
    ) -> Result<Self, String> {
        let token = crate::win::random_token()?;
        let listener = std::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .map_err(|e| format!("cannot listen on 127.0.0.1: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        let url = format!("http://{address}/");
        let file = data_dir.join(UI_ENDPOINT_FILE);
        let content = UiEndpointFile {
            url: url.clone(),
            pid: std::process::id(),
            token: Some(token.clone()),
        };
        std::fs::write(
            &file,
            serde_json::to_string(&content).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
        let listener = {
            let _guard = runtime.enter();
            tokio::net::TcpListener::from_std(listener).map_err(|e| e.to_string())?
        };
        let router = axum::Router::new()
            .route("/api/ws", axum::routing::get(upgrade))
            .with_state(EndpointState { service, token });
        let (shutdown, stopped) = tokio::sync::oneshot::channel::<()>();
        runtime.spawn(async move {
            let served = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await;
            if let Err(err) = served {
                tracing::warn!(%err, "the plug-in dev endpoint stopped");
            }
        });
        tracing::info!(%url, "plug-in dev endpoint running");
        Ok(Self {
            url,
            file,
            shutdown: Some(shutdown),
        })
    }

    /// The base URL (`http://127.0.0.1:<port>/`).
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for DevEndpoint {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        drop(std::fs::remove_file(&self.file));
    }
}

/// Whether the upgrade request may connect: no browser `Origin`, the
/// right bearer token.
pub fn authorized(headers: &HeaderMap, token: &str) -> Result<(), StatusCode> {
    if headers.contains_key(header::ORIGIN) {
        return Err(StatusCode::FORBIDDEN);
    }
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if crate::win::secrets_equal(given.as_bytes(), token.as_bytes()) {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

async fn upgrade(
    State(state): State<EndpointState>,
    headers: HeaderMap,
    socket: WebSocketUpgrade,
) -> HttpResponse {
    if let Err(status) = authorized(&headers, &state.token) {
        return status.into_response();
    }
    socket.on_upgrade(move |socket| session(state.service, socket))
}

/// The answer to one request on the dev endpoint.
pub async fn answer(service: &Service, request: RpcRequest) -> ServerMessage {
    let response = if dev_endpoint_allows(&request.request) {
        service.handle(request.request).await
    } else {
        Response::from(ApiError::new(
            ErrorCode::Forbidden,
            "the plug-in dev endpoint only loads development folders",
        ))
    };
    ServerMessage::Response {
        id: request.id,
        response,
    }
}

async fn session(service: Service, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let (out, mut outgoing) = tokio::sync::mpsc::channel::<ServerMessage>(256);
    let mut events = service.subscribe();
    let forward = out.clone();
    let events_task = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) if dev_endpoint_forwards(&event) => {
                    if forward.send(ServerMessage::Event { event }).await.is_err() {
                        break;
                    }
                }
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    let writer = tokio::spawn(async move {
        while let Some(message) = outgoing.recv().await {
            let Ok(text) = serde_json::to_string(&message) else {
                continue;
            };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });
    while let Some(Ok(frame)) = stream.next().await {
        let Message::Text(text) = frame else {
            if matches!(frame, Message::Close(_)) {
                break;
            }
            continue;
        };
        let Ok(request) = serde_json::from_str::<RpcRequest>(text.as_str()) else {
            continue;
        };
        let service = service.clone();
        let out = out.clone();
        tokio::spawn(async move {
            drop(out.send(answer(&service, request).await).await);
        });
    }
    events_task.abort();
    drop(out);
    drop(writer.await);
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use nail_api::{Event, Request};
    use pretty_assertions::assert_eq;
    use tokio_tungstenite::tungstenite;
    use tungstenite::client::IntoClientRequest;

    fn open(data: &Path, config: &Path) -> Service {
        let args = crate::AppArgs {
            data_dir: Some(data.into()),
            config_dir: Some(config.into()),
            seed: Some("demo".into()),
            ..crate::AppArgs::default()
        };
        crate::open_service(&args).unwrap()
    }

    #[test]
    fn only_a_bearer_with_the_token_and_no_origin_gets_in() {
        let mut headers = HeaderMap::new();
        assert_eq!(authorized(&headers, "t0k"), Err(StatusCode::UNAUTHORIZED));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer nope"),
        );
        assert_eq!(authorized(&headers, "t0k"), Err(StatusCode::UNAUTHORIZED));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer t0k"),
        );
        assert_eq!(authorized(&headers, "t0k"), Ok(()));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.example"),
        );
        assert_eq!(authorized(&headers, "t0k"), Err(StatusCode::FORBIDDEN));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_endpoint_serves_only_dev_loading_with_the_token() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let service = open(data.path(), config.path());
        let endpoint = DevEndpoint::start(
            service.clone(),
            data.path(),
            &tokio::runtime::Handle::current(),
        )
        .unwrap();
        // The file `nail plugin dev` reads.
        let file: UiEndpointFile = serde_json::from_str(
            &std::fs::read_to_string(data.path().join(UI_ENDPOINT_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(file.url, endpoint.url());
        assert!(file.url.starts_with("http://127.0.0.1:"), "{}", file.url);
        assert_eq!(file.pid, std::process::id());
        let token = file.token.clone().unwrap();
        let socket = format!("{}api/ws", file.url.replace("http://", "ws://"));

        // Without the token: refused.
        let err = tokio_tungstenite::connect_async(socket.as_str())
            .await
            .unwrap_err();
        assert!(
            matches!(&err, tungstenite::Error::Http(r) if r.status() == 401),
            "{err:?}"
        );
        // A browser (Origin) with the token: refused.
        let mut request = socket.as_str().into_client_request().unwrap();
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
        request
            .headers_mut()
            .insert("origin", "http://evil.example".parse().unwrap());
        assert!(tokio_tungstenite::connect_async(request).await.is_err());

        // Like `nail plugin dev`: the token as a bearer.
        let mut request = socket.as_str().into_client_request().unwrap();
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
        let (mut ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let send = |id: u64, request: Request| {
            tungstenite::Message::Text(
                serde_json::to_string(&RpcRequest { id, request })
                    .unwrap()
                    .into(),
            )
        };
        ws.send(send(1, Request::ListAccounts)).await.unwrap();
        let reply = next_response(&mut ws).await;
        assert!(
            matches!(
                reply,
                ServerMessage::Response {
                    id: 1,
                    response: Response::Error {
                        code: ErrorCode::Forbidden,
                        ..
                    }
                }
            ),
            "{reply:?}"
        );
        // A development folder without a manifest: the core answers
        // (with its own error), so the request went through.
        let folder = tempfile::tempdir().unwrap();
        ws.send(send(
            2,
            Request::LoadDevPlugin {
                folder: folder.path().display().to_string(),
                grants: vec![],
            },
        ))
        .await
        .unwrap();
        let reply = next_response(&mut ws).await;
        let ServerMessage::Response { id: 2, response } = reply else {
            panic!("{reply:?}")
        };
        assert!(
            !matches!(
                response,
                Response::Error {
                    code: ErrorCode::Forbidden,
                    ..
                }
            ),
            "{response:?}"
        );
        // Events: plug-in changes are forwarded, mail changes are not.
        service.broadcast(Event::MailChanged { folder_ids: vec![] });
        service.broadcast(Event::PluginsChanged);
        loop {
            let frame = tokio::time::timeout(std::time::Duration::from_secs(10), ws.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let tungstenite::Message::Text(text) = frame else {
                continue;
            };
            match serde_json::from_str::<ServerMessage>(text.as_str()).unwrap() {
                ServerMessage::Event {
                    event: Event::PluginsChanged,
                } => break,
                ServerMessage::Event { event } => {
                    assert!(dev_endpoint_forwards(&event), "{event:?} was forwarded");
                }
                ServerMessage::Response { .. } => {}
            }
        }
        drop(ws);

        // Stopping removes the file and closes the port.
        drop(endpoint);
        assert!(!data.path().join(UI_ENDPOINT_FILE).exists());
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while tokio::net::TcpStream::connect(
                file.url.trim_start_matches("http://").trim_end_matches('/'),
            )
            .await
            .is_ok()
            {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        service.shutdown().await;
    }

    async fn next_response<S>(ws: &mut S) -> ServerMessage
    where
        S: futures_util::Stream<Item = Result<tungstenite::Message, tungstenite::Error>> + Unpin,
    {
        loop {
            let frame = tokio::time::timeout(std::time::Duration::from_secs(20), ws.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            if let tungstenite::Message::Text(text) = frame {
                let message: ServerMessage = serde_json::from_str(text.as_str()).unwrap();
                if matches!(message, ServerMessage::Response { .. }) {
                    return message;
                }
            }
        }
    }
}
