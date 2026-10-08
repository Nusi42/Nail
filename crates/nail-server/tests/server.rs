//! The server over real sockets (WebSocket round trips) and through the
//! router (static files, blobs, headers).

#![cfg(test)]

use std::net::SocketAddr;
use std::path::Path;

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

use nail_testkit::TEST_TIMEOUT as TIMEOUT;

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
            test_control: None,
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
    // "All accounts" and the demo account.
    assert_eq!(accounts.len(), 2);

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
    // An accent override changes the appearance first (M5a, ADR 0102).
    let mut message = receive(&mut socket).await;
    if matches!(
        message,
        ServerMessage::Event {
            event: Event::ThemeChanged { .. }
        }
    ) {
        message = receive(&mut socket).await;
    }
    match message {
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

/// DNS rebinding: the attacker's name resolves to 127.0.0.1, so `Host` and
/// `Origin` agree with each other but not with the server's loopback names.
#[tokio::test]
async fn dns_rebinding_is_refused_on_the_websocket() {
    let f = service();
    let server = start(&f.service).await;
    let port = server.local_addr().port();
    let url = format!("ws://{}/api/ws", server.local_addr());

    let mut request = url.clone().into_client_request().unwrap();
    let evil = format!("evil.example:{port}");
    request.headers_mut().insert("Host", evil.parse().unwrap());
    request
        .headers_mut()
        .insert("Origin", format!("http://{evil}").parse().unwrap());
    match tokio_tungstenite::connect_async(request).await.unwrap_err() {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        other => panic!("unexpected {other:?}"),
    }

    // `localhost` with the right port is fine.
    let mut request = url.into_client_request().unwrap();
    let local = format!("localhost:{port}");
    request.headers_mut().insert("Host", local.parse().unwrap());
    request
        .headers_mut()
        .insert("Origin", format!("http://{local}").parse().unwrap());
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
            test_control: None,
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

/// The port the router under test believes it is bound to.
const PORT: u16 = 4780;

async fn get(service: &Service, ui: Option<&Path>, uri: &str) -> axum::response::Response {
    get_with(service, ui, uri, &[("Host", "127.0.0.1:4780")]).await
}

async fn get_with(
    service: &Service,
    ui: Option<&Path>,
    uri: &str,
    headers: &[(&str, &str)],
) -> axum::response::Response {
    let mut request = HttpRequest::get(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    router(service.clone(), ui.map(Path::to_path_buf), PORT)
        .oneshot(request.body(Body::empty()).unwrap())
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

    // Static files are guarded too, and so is a request without `Host`.
    let rebound = get_with(
        &f.service,
        Some(ui.path()),
        "/",
        &[("Host", "evil.example:4780")],
    )
    .await;
    assert_eq!(rebound.status(), StatusCode::FORBIDDEN);
    assert_eq!(rebound.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    let no_host = get_with(&f.service, Some(ui.path()), "/", &[]).await;
    assert_eq!(no_host.status(), StatusCode::FORBIDDEN);
    let wrong_port = get_with(
        &f.service,
        Some(ui.path()),
        "/",
        &[("Host", "127.0.0.1:4781")],
    )
    .await;
    assert_eq!(wrong_port.status(), StatusCode::FORBIDDEN);

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

    // DNS rebinding: the raw mail must not be readable from the attacker's page.
    let rebound = get_with(
        &f.service,
        None,
        &format!("/api/blob/{}", hash.as_str()),
        &[
            ("Host", "evil.example:4780"),
            ("Origin", "http://evil.example:4780"),
        ],
    )
    .await;
    assert_eq!(rebound.status(), StatusCode::FORBIDDEN);
    assert_eq!(body(rebound).await.len(), 0);

    let bad = get(&f.service, None, "/api/blob/xyz").await;
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    let missing = get(&f.service, None, &format!("/api/blob/{}", "a".repeat(64))).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

/// Regression: a client that fell behind the event channel only got a log
/// line, and its lists and config stayed stale. Now it gets the current
/// state (config first, then a change notice for every folder).
#[tokio::test]
async fn a_lagging_client_is_resynced() {
    let f = service();
    let server = start(&f.service).await;
    let mut socket = connect(&server).await;
    send(&mut socket, 1, Request::Ping).await;
    response(&mut socket, 1).await;

    // The test runtime is single-threaded: nothing is forwarded while this
    // loop runs, so the connection's receiver overflows.
    for _ in 0..nail_core::EVENT_CAPACITY * 2 {
        f.service.broadcast(Event::MailChanged {
            folder_ids: Vec::new(),
        });
    }
    let mut saw_config = false;
    loop {
        match receive(&mut socket).await {
            ServerMessage::Event {
                event: Event::ConfigChanged { .. },
            } => saw_config = true,
            ServerMessage::Event {
                event: Event::MailChanged { folder_ids },
            } if saw_config && !folder_ids.is_empty() => break,
            _ => {}
        }
    }
}

struct Echo;

impl nail_server::TestControl for Echo {
    fn control(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        match command {
            "echo" => Ok(serde_json::json!({ "echo": args })),
            other => Err(format!("unknown {other}")),
        }
    }
}

async fn post_control(
    service: &Service,
    control: Option<std::sync::Arc<dyn nail_server::TestControl>>,
    uri: &str,
    host: &str,
    body: &str,
) -> axum::response::Response {
    let request = HttpRequest::post(uri)
        .header("Host", host)
        .body(Body::from(body.to_owned()))
        .unwrap();
    nail_server::router_with_control(service.clone(), None, PORT, control)
        .oneshot(request)
        .await
        .unwrap()
}

#[tokio::test]
async fn test_control_exists_only_when_given() {
    let f = service();
    let host = "127.0.0.1:4780";
    let absent = post_control(&f.service, None, "/api/test/echo", host, "{}").await;
    assert_eq!(absent.status(), StatusCode::NOT_FOUND);

    let control: Option<std::sync::Arc<dyn nail_server::TestControl>> =
        Some(std::sync::Arc::new(Echo));
    let ok = post_control(
        &f.service,
        control.clone(),
        "/api/test/echo",
        host,
        r#"{"a":1}"#,
    )
    .await;
    assert_eq!(ok.status(), StatusCode::OK);
    let value: serde_json::Value = serde_json::from_slice(&body(ok).await).unwrap();
    assert_eq!(value, serde_json::json!({"echo": {"a": 1}}));

    let unknown = post_control(&f.service, control.clone(), "/api/test/nope", host, "").await;
    assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);
    let bad_json = post_control(&f.service, control.clone(), "/api/test/echo", host, "{").await;
    assert_eq!(bad_json.status(), StatusCode::BAD_REQUEST);
    // The host guard applies as everywhere.
    let foreign = post_control(
        &f.service,
        control,
        "/api/test/echo",
        "evil.example:4780",
        "{}",
    )
    .await;
    assert_eq!(foreign.status(), StatusCode::FORBIDDEN);
}
