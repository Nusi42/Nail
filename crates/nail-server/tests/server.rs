//! The server over real sockets (WebSocket round trips) and through the
//! router (static files, blobs, headers).

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode, header};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use nail_api::{Event, Request, Response, RpcRequest, ServerMessage};
use nail_core::demo::seed_demo;
use nail_core::{Service, ServiceOptions};
use nail_server::{Server, ServerError, ServerOptions, router};
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tower::ServiceExt;

const TIMEOUT: Duration = Duration::from_secs(10);

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Fixture {
    service: Service,
    _data: TempDir,
    config: TempDir,
}

fn service() -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    let service = Service::open(options).unwrap();
    seed_demo(service.store(), service.blobs()).unwrap();
    Fixture {
        service,
        _data: data,
        config,
    }
}

async fn start(service: &Service) -> Server {
    Server::bind(
        service.clone(),
        ServerOptions {
            addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            ui_dir: None,
        },
    )
    .await
    .unwrap()
}

async fn connect(server: &Server) -> Socket {
    let url = format!("ws://{}/api/ws", server.local_addr());
    let (socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    socket
}

async fn send(socket: &mut Socket, id: u64, request: Request) {
    let frame = serde_json::to_string(&RpcRequest { id, request }).unwrap();
    socket.send(Message::Text(frame.into())).await.unwrap();
}

async fn receive(socket: &mut Socket) -> ServerMessage {
    loop {
        let frame = tokio::time::timeout(TIMEOUT, socket.next())
            .await
            .expect("no frame within the timeout")
            .expect("socket closed")
            .unwrap();
        if let Message::Text(text) = frame {
            return serde_json::from_str(text.as_str()).unwrap();
        }
    }
}

async fn response(socket: &mut Socket, id: u64) -> Response {
    loop {
        if let ServerMessage::Response { id: got, response } = receive(socket).await {
            assert_eq!(got, id);
            return response;
        }
    }
}

#[tokio::test]
async fn websocket_round_trip() {
    let f = service();
    let server = start(&f.service).await;
    let mut socket = connect(&server).await;

    send(&mut socket, 1, Request::Ping).await;
    assert!(matches!(
        response(&mut socket, 1).await,
        Response::Pong { protocol, .. } if protocol == nail_api::PROTOCOL_VERSION
    ));

    send(&mut socket, 2, Request::ListAccounts).await;
    let Response::Accounts { accounts } = response(&mut socket, 2).await else {
        panic!("expected accounts");
    };
    assert_eq!(accounts.len(), 1);

    send(
        &mut socket,
        3,
        Request::ListFolders {
            account_id: "nope".into(),
        },
    )
    .await;
    assert!(matches!(
        response(&mut socket, 3).await,
        Response::Error {
            code: nail_api::ErrorCode::NotFound,
            ..
        }
    ));

    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn concurrent_requests_are_correlated_by_id() {
    let f = service();
    let server = start(&f.service).await;
    let mut socket = connect(&server).await;
    for id in 10..20 {
        send(&mut socket, id, Request::Ping).await;
    }
    let mut ids = Vec::new();
    while ids.len() < 10 {
        if let ServerMessage::Response { id, response } = receive(&mut socket).await {
            assert!(matches!(response, Response::Pong { .. }));
            ids.push(id);
        }
    }
    ids.sort_unstable();
    assert_eq!(ids, (10..20).collect::<Vec<_>>());
}

#[tokio::test]
async fn malformed_frames_get_an_error_when_the_id_is_readable() {
    let f = service();
    let server = start(&f.service).await;
    let mut socket = connect(&server).await;
    socket
        .send(Message::Text(
            r#"{"id": 5, "request": {"type": "Nope"}}"#.into(),
        ))
        .await
        .unwrap();
    assert!(matches!(
        response(&mut socket, 5).await,
        Response::Error {
            code: nail_api::ErrorCode::InvalidRequest,
            ..
        }
    ));
    // Garbage without an id is dropped; the connection stays usable.
    socket.send(Message::Text("not json".into())).await.unwrap();
    send(&mut socket, 6, Request::Ping).await;
    assert!(matches!(
        response(&mut socket, 6).await,
        Response::Pong { .. }
    ));
}

#[tokio::test]
async fn events_are_pushed_to_every_client() {
    let f = service();
    let server = start(&f.service).await;
    let mut a = connect(&server).await;
    let mut b = connect(&server).await;
    // Make sure both connections are fully set up (subscribed) first.
    send(&mut a, 1, Request::Ping).await;
    response(&mut a, 1).await;
    send(&mut b, 1, Request::Ping).await;
    response(&mut b, 1).await;

    let event = Event::MailChanged {
        folder_ids: vec!["f1".into()],
    };
    f.service.broadcast(event.clone());
    for socket in [&mut a, &mut b] {
        assert_eq!(
            receive(socket).await,
            ServerMessage::Event {
                event: event.clone()
            }
        );
    }
}

#[tokio::test]
async fn config_errors_are_sent_on_connect() {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    std::fs::write(
        config.path().join("layout.kdl"),
        "layout \"x\" {\n  window\n",
    )
    .unwrap();
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    let service = Service::open(options).unwrap();
    let server = start(&service).await;
    let mut socket = connect(&server).await;
    match receive(&mut socket).await {
        ServerMessage::Event {
            event: Event::ConfigError { file, .. },
        } => assert_eq!(file, "layout.kdl"),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn config_file_changes_reach_the_socket() {
    let f = service();
    f.service.watch_config().unwrap();
    let server = start(&f.service).await;
    let mut socket = connect(&server).await;
    send(&mut socket, 1, Request::Ping).await;
    response(&mut socket, 1).await;

    std::fs::write(
        f.config.path().join("theme.kdl"),
        "theme \"t\" extends=\"default\" {\n    tokens {\n        color \"accent\" \"#123456\"\n    }\n}\n",
    )
    .unwrap();
    match receive(&mut socket).await {
        ServerMessage::Event {
            event: Event::ConfigChanged { theme, .. },
        } => assert!(format!("{theme:?}").contains("#123456")),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn foreign_origins_are_refused() {
    let f = service();
    let server = start(&f.service).await;
    let url = format!("ws://{}/api/ws", server.local_addr());

    let mut request = url.clone().into_client_request().unwrap();
    request
        .headers_mut()
        .insert("Origin", "http://evil.example".parse().unwrap());
    let err = tokio_tungstenite::connect_async(request).await.unwrap_err();
    match err {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        other => panic!("unexpected {other:?}"),
    }

    let mut request = url.into_client_request().unwrap();
    let origin = format!("http://{}", server.local_addr());
    request
        .headers_mut()
        .insert("Origin", origin.parse().unwrap());
    assert!(tokio_tungstenite::connect_async(request).await.is_ok());
}

#[tokio::test]
async fn refuses_non_loopback_addresses() {
    let f = service();
    let err = Server::bind(
        f.service.clone(),
        ServerOptions {
            addr: SocketAddr::from(([0, 0, 0, 0], 0)),
            ui_dir: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ServerError::NotLoopback(_)));
}

// ---------------------------------------------------------------------------
// HTTP through the router

fn ui_dir() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("index.html"),
        "<!doctype html><title>t</title>",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets").join("app.js"), "console.log(1)").unwrap();
    dir
}

async fn get(service: &Service, ui: Option<&Path>, uri: &str) -> axum::response::Response {
    router(service.clone(), ui.map(Path::to_path_buf))
        .oneshot(HttpRequest::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body(response: axum::response::Response) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}

#[tokio::test]
async fn serves_the_ui_with_spa_fallback_and_security_headers() {
    let f = service();
    let ui = ui_dir();

    let index = get(&f.service, Some(ui.path()), "/").await;
    assert_eq!(index.status(), StatusCode::OK);
    let csp = index.headers()[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(csp.contains("script-src 'self'"));
    assert!(csp.contains("frame-ancestors 'none'"));
    assert_eq!(index.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(body(index).await, b"<!doctype html><title>t</title>");

    let asset = get(&f.service, Some(ui.path()), "/assets/app.js").await;
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(body(asset).await, b"console.log(1)");

    let deep = get(&f.service, Some(ui.path()), "/folders/inbox").await;
    assert_eq!(deep.status(), StatusCode::OK);
    assert_eq!(body(deep).await, b"<!doctype html><title>t</title>");

    let api = get(&f.service, Some(ui.path()), "/api/nope").await;
    assert_eq!(api.status(), StatusCode::NOT_FOUND);

    let traversal = get(&f.service, Some(ui.path()), "/../Cargo.toml").await;
    assert_ne!(body(traversal).await, std::fs::read("Cargo.toml").unwrap());

    let no_ui = get(&f.service, None, "/").await;
    assert_eq!(no_ui.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn serves_blobs_by_hash() {
    let f = service();
    let hash = f.service.blobs().put(b"Subject: hi\r\n\r\nbody").unwrap();

    let ok = get(&f.service, None, &format!("/api/blob/{}", hash.as_str())).await;
    assert_eq!(ok.status(), StatusCode::OK);
    assert_eq!(ok.headers()[header::CONTENT_TYPE], "message/rfc822");
    assert_eq!(
        ok.headers()[header::CONTENT_SECURITY_POLICY],
        "default-src 'none'; sandbox"
    );
    assert!(
        ok.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .starts_with("attachment")
    );
    assert_eq!(body(ok).await, b"Subject: hi\r\n\r\nbody");

    let bad = get(&f.service, None, "/api/blob/xyz").await;
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    let missing = get(&f.service, None, &format!("/api/blob/{}", "a".repeat(64))).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}
