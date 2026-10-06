//! The remote UI as the desktop starts it (ADR 0041): settings from the
//! core start a TLS listener with login, the pairing answer carries its
//! address and fingerprint, protected routes need a session, disabling
//! stops it.

#![cfg(test)]

use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use nail_api::engine::MemorySecretStore;
use nail_api::remote::{
    AuthResponse, RemoteAccessPreset, RemoteUiSettings, RemoteUiState, RemoteUiStatus,
};
use nail_api::{Request, Response};
use nail_core::demo::{demo_options, seed_demo};
use nail_core::{Service, ServiceOptions};
use nail_server::{RemoteUiHost, RemoteUiOptions, cert};
use pretty_assertions::assert_eq;
use rustls::pki_types::ServerName;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct Fixture {
    service: Service,
    _host: RemoteUiHost,
    _data: TempDir,
    _config: TempDir,
}

fn start() -> Fixture {
    start_with(|_| {})
}

fn start_with(before: impl FnOnce(&Service)) -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    options.start_engines = false;
    options.secrets = Some(Arc::new(MemorySecretStore::new()));
    let service = Service::open(demo_options(options)).unwrap();
    seed_demo(service.store(), service.blobs()).unwrap();
    before(&service);
    let host = RemoteUiHost::start(
        service.clone(),
        RemoteUiOptions {
            ui_dir: None,
            computer_name: Some("testpc".into()),
        },
    );
    Fixture {
        service,
        _host: host,
        _data: data,
        _config: config,
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn status(service: &Service) -> RemoteUiStatus {
    match service.handle(Request::GetRemoteUiSettings).await {
        Response::RemoteUi { status, .. } => status,
        other => panic!("{other:?}"),
    }
}

async fn wait_for(service: &Service, state: RemoteUiState) -> RemoteUiStatus {
    for _ in 0..200 {
        let now = status(service).await;
        if now.state == state {
            return now;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "remote UI never reached {state:?}: {:?}",
        status(service).await
    );
}

struct Http {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Http {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// One HTTPS request to `127.0.0.1:port` pinned to the stored certificate.
async fn https(
    service: &Service,
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Http {
    let identity = cert::load(service).unwrap().unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(identity.chain()[0].clone()).unwrap();
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let tcp = TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
        .unwrap();
    let mut tls = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(ServerName::try_from("localhost").unwrap(), tcp)
        .await
        .unwrap();
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: localhost:{port}\r\n");
    for (name, value) in headers {
        let _ = write!(request, "{name}: {value}\r\n");
    }
    let _ = write!(
        request,
        "Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    tls.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    drop(tokio::time::timeout(Duration::from_secs(10), tls.read_to_end(&mut raw)).await);
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    Http {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enabling_starts_a_tls_listener_with_login_and_disabling_stops_it() {
    let f = start();
    assert_eq!(status(&f.service).await.state, RemoteUiState::Stopped);
    let port = free_port();
    let settings = RemoteUiSettings {
        enabled: true,
        port,
        host_name: Some("localhost".into()),
        ..RemoteUiSettings::default()
    };
    let saved = f
        .service
        .handle(Request::UpdateRemoteUiSettings {
            settings: settings.clone(),
        })
        .await;
    assert!(matches!(saved, Response::RemoteUi { .. }), "{saved:?}");
    let running = wait_for(&f.service, RemoteUiState::Running).await;
    assert_eq!(running.urls[0], format!("https://localhost:{port}/"));
    let fingerprint = running.certificate.clone().unwrap().fingerprint_sha256;

    // Strict headers: Trusted Types enforced, HSTS.
    let home = https(&f.service, port, "GET", "/", &[], "").await;
    let csp = home.header("content-security-policy").unwrap();
    assert!(csp.contains("require-trusted-types-for 'script'"), "{csp}");
    assert!(home.header("strict-transport-security").is_some());

    // The protocol, raw mail and the proxy need a session.
    for path in ["/api/ws", "/api/blob/00", "/api/proxy/abc"] {
        let answer = https(&f.service, port, "GET", path, &[], "").await;
        assert_eq!(answer.status, 401, "{path}");
    }

    // The login route answers.
    let origin = format!("https://localhost:{port}");
    let auth = https(
        &f.service,
        port,
        "POST",
        "/api/auth",
        &[("Content-Type", "application/json"), ("Origin", &origin)],
        r#"{"type":"Status"}"#,
    )
    .await;
    assert_eq!(auth.status, 200);
    match serde_json::from_slice::<AuthResponse>(&auth.body).unwrap() {
        AuthResponse::Status { status } => {
            assert!(!status.authenticated);
            assert_eq!(
                status.certificate_fingerprint.as_deref(),
                Some(fingerprint.as_str())
            );
            assert!(
                status.methods.passkey,
                "localhost is a usable relying party"
            );
        }
        other => panic!("{other:?}"),
    }

    // A pairing code made on the desktop names this address.
    let user = match f
        .service
        .handle(Request::CreateRemoteUser {
            name: "ada".into(),
            display_name: None,
            grant: RemoteAccessPreset::ReadOnly.grant(),
        })
        .await
    {
        Response::RemoteUserSaved { user } => user,
        other => panic!("{other:?}"),
    };
    match f
        .service
        .handle(Request::CreatePairingCode {
            user_id: user.id,
            device_name: None,
            grant: None,
            ttl_minutes: None,
        })
        .await
    {
        Response::PairingCodeCreated {
            url,
            certificate_fingerprint,
            ..
        } => {
            assert_eq!(url, Some(format!("https://localhost:{port}/")));
            assert_eq!(certificate_fingerprint, Some(fingerprint.clone()));
        }
        other => panic!("{other:?}"),
    }

    // A new certificate: a new fingerprint, still running.
    match f.service.handle(Request::RegenerateRemoteCertificate).await {
        Response::RemoteUi { status, .. } => {
            assert_eq!(status.state, RemoteUiState::Running);
            assert_ne!(status.certificate.unwrap().fingerprint_sha256, fingerprint);
        }
        other => panic!("{other:?}"),
    }

    // Disabling stops the listener.
    f.service
        .handle(Request::UpdateRemoteUiSettings {
            settings: RemoteUiSettings {
                enabled: false,
                ..settings
            },
        })
        .await;
    wait_for(&f.service, RemoteUiState::Stopped).await;
    let mut refused = false;
    for _ in 0..50 {
        if TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port)))
            .await
            .is_err()
        {
            refused = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(refused, "the port is closed after disabling");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_busy_port_is_reported_as_failed() {
    let f = start();
    let busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = busy.local_addr().unwrap().port();
    f.service
        .handle(Request::UpdateRemoteUiSettings {
            settings: RemoteUiSettings {
                enabled: true,
                port,
                ..RemoteUiSettings::default()
            },
        })
        .await;
    let failed = wait_for(&f.service, RemoteUiState::Failed).await;
    assert!(failed.message.unwrap().contains(&port.to_string()));
    drop(busy);
}

/// Stored settings that the setter would refuse (a restored or damaged
/// value) do not start a plain-HTTP listener beyond loopback.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stored_settings_are_checked_again_before_the_listener_starts() {
    let f = start_with(|service| {
        let settings = RemoteUiSettings {
            enabled: true,
            // A documentation address: never one of this machine's.
            bind_address: "192.0.2.1".into(),
            port: free_port(),
            tls: nail_api::remote::RemoteTlsMode::Off,
            ..RemoteUiSettings::default()
        };
        assert_ne!(settings.issues().len(), 0);
        service
            .store()
            .set_setting(nail_store::REMOTE_UI_SETTINGS_KEY, &settings)
            .unwrap();
    });
    let failed = wait_for(&f.service, RemoteUiState::Failed).await;
    let message = failed.message.unwrap();
    assert!(message.contains("TlsRequired"), "{message}");
}
