//! The remote UI's listener (ADR 0041): started, restarted and stopped
//! from the core's settings.
//!
//! [`RemoteUiHost::start`] attaches to a [`Service`]
//! ([`Service::attach_remote_ui`]), applies the stored settings and then
//! every [`RemoteUiCommand`]: `UpdateRemoteUiSettings` from the desktop
//! restarts the listener, `RegenerateRemoteCertificate` makes a new
//! self-signed certificate. The listener is a [`Server::bind_with`] with
//! TLS (a self-signed certificate from [`crate::cert`], kept across
//! restarts), the remote UI's host names, the allowed networks, the
//! proxy, a sandbox origin for mail HTML, the strict header profile and
//! **authentication** ([`crate::TransportOptions::auth`]). The desktop's
//! own UI (loopback `nail serve`, the Tauri window) is not affected.
//!
//! Internet mode and ACME stay refused (locked in this build; the core
//! refuses the settings already).

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use nail_api::remote::{
    RemoteTlsMode, RemoteUiSettings, RemoteUiState, RemoteUiStatus, internet_mode_lock,
};
use nail_api::{ApiError, ErrorCode};
use nail_core::{RemoteUiCommand, Service};
use nail_remote_auth::{AuthConfig, RelyingParty};
use tokio::task::JoinHandle;

use crate::{SandboxOptions, Server, ServerOptions, TransportOptions, cert};

/// Where the remote UI's files come from and what the machine is called.
#[derive(Debug, Clone, Default)]
pub struct RemoteUiOptions {
    /// The built UI (`ui/dist`); `None` serves the API only.
    pub ui_dir: Option<PathBuf>,
    /// The computer's name, used when no host name is configured.
    pub computer_name: Option<String>,
}

impl RemoteUiOptions {
    /// Options with `ui_dir` and this computer's name (`COMPUTERNAME`).
    pub fn new(ui_dir: Option<PathBuf>) -> Self {
        Self {
            ui_dir,
            computer_name: std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .ok()
                .map(|n| n.to_ascii_lowercase()),
        }
    }
}

/// Runs the remote UI for a service. Stops (listener included) when
/// dropped.
#[derive(Debug)]
pub struct RemoteUiHost {
    task: JoinHandle<()>,
}

impl Drop for RemoteUiHost {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl RemoteUiHost {
    /// Attaches to `service` and applies its remote-UI settings (see the
    /// module docs). Needs a tokio runtime.
    pub fn start(service: Service, options: RemoteUiOptions) -> Self {
        let mut commands = service.attach_remote_ui();
        let task = tokio::spawn(async move {
            let mut running = Running::default();
            let settings = service.remote_ui_settings();
            let _ = running.apply(&service, &options, &settings, false).await;
            while let Some(command) = commands.recv().await {
                match command {
                    RemoteUiCommand::Apply(settings) => {
                        let _ = running.apply(&service, &options, &settings, false).await;
                    }
                    RemoteUiCommand::RegenerateCertificate(reply) => {
                        let settings = service.remote_ui_settings();
                        let result = running.apply(&service, &options, &settings, true).await;
                        let _ = reply.send(result);
                    }
                    other => tracing::debug!(?other, "unknown remote-UI command"),
                }
            }
            running.stop(&service).await;
        });
        Self { task }
    }
}

#[derive(Default)]
struct Running {
    server: Option<Server>,
    sweeper: Option<JoinHandle<()>>,
}

impl Running {
    async fn stop(&mut self, service: &Service) {
        if let Some(sweeper) = self.sweeper.take() {
            sweeper.abort();
        }
        if let Some(server) = self.server.take()
            && let Err(err) = server.shutdown().await
        {
            tracing::warn!(%err, "stopping the remote UI failed");
        }
        service.remote_auth().set_config(AuthConfig::default());
    }

    /// Stops what runs and starts `settings` (if enabled); reports the
    /// outcome to the core.
    async fn apply(
        &mut self,
        service: &Service,
        options: &RemoteUiOptions,
        settings: &RemoteUiSettings,
        regenerate: bool,
    ) -> Result<(), ApiError> {
        self.stop(service).await;
        if !settings.enabled {
            if regenerate {
                // A certificate for the next start.
                make_identity(service, options, settings, true).await?;
            }
            service.report_remote_ui(None, None);
            return Ok(());
        }
        match self.start(service, options, settings, regenerate).await {
            Ok(()) => Ok(()),
            Err(err) => {
                tracing::warn!(%err, "the remote UI did not start");
                let mut status = RemoteUiStatus::stopped();
                status.state = RemoteUiState::Failed;
                status.error = Some(err.code);
                status.message = Some(err.message.clone());
                service.report_remote_ui(Some(status), None);
                Err(err)
            }
        }
    }

    async fn start(
        &mut self,
        service: &Service,
        options: &RemoteUiOptions,
        settings: &RemoteUiSettings,
        regenerate: bool,
    ) -> Result<(), ApiError> {
        if settings.internet_mode || settings.tls == RemoteTlsMode::Acme {
            let lock = internet_mode_lock()
                .map(|r| format!("{r:?}"))
                .unwrap_or_default();
            return Err(ApiError::new(
                ErrorCode::Forbidden,
                format!("internet mode is locked ({lock})"),
            ));
        }
        let ip = settings
            .bind_ip()
            .ok_or_else(|| ApiError::invalid_request("invalid bind address"))?;
        let identity = match settings.tls {
            RemoteTlsMode::Off => None,
            _ => Some(make_identity(service, options, settings, regenerate).await?),
        };
        let tls = match &identity {
            Some(identity) => Some(identity.tls_config().map_err(internal)?),
            None => None,
        };
        let host = primary_host(settings, options);
        let mut host_names: Vec<String> = host.iter().cloned().collect();
        if ip.is_unspecified() {
            // Only here, for a running listener on every address: read from
            // the network interfaces, no socket (ADR 0057).
            if let Some(lan) = crate::lan::lan_address(ip) {
                host_names.push(lan.to_string());
            }
        } else if !ip.is_loopback() {
            host_names.push(ip.to_string());
        }
        let allowed_networks = settings
            .allowed_networks
            .iter()
            .filter_map(|n| n.trim().parse().ok())
            .collect();
        let proxy = service.remote_proxy();
        let transport = TransportOptions {
            tls,
            host_names: host_names.clone(),
            allowed_networks,
            // Internet mode is refused above: local peers only.
            public_peers: false,
            sandbox: proxy.as_ref().map(|_| SandboxOptions {
                port: 0,
                source: None,
            }),
            proxy,
            headers: crate::HeaderProfile::Strict,
            acme_challenges: None,
            auth: Some(service.remote_auth().clone()),
        };
        let server = Server::bind_with(
            service.clone(),
            ServerOptions {
                addr: SocketAddr::new(ip, settings.port),
                ui_dir: options.ui_dir.clone(),
                test_control: None,
            },
            transport,
        )
        .await
        .map_err(|err| ApiError::new(ErrorCode::Unavailable, err.to_string()))?;

        let scheme = if identity.is_some() { "https" } else { "http" };
        let port = server.local_addr().port();
        let mut names = host_names;
        if ip.is_loopback() || ip.is_unspecified() {
            names.push("localhost".to_owned());
        }
        let authority = |name: &str| match name.parse::<IpAddr>() {
            Ok(IpAddr::V6(v6)) => format!("[{v6}]"),
            _ => name.to_owned(),
        };
        let origins: Vec<String> = names
            .iter()
            .map(|n| format!("{scheme}://{}:{port}", authority(n)))
            .collect();
        let sandbox_origin = server.sandbox_addr().map(|addr| {
            let name = names
                .first()
                .map_or_else(|| addr.ip().to_string(), |n| authority(n));
            format!("{scheme}://{name}:{}", addr.port())
        });
        let relying_party = host
            .as_deref()
            .filter(|_| identity.is_some())
            .and_then(|h| RelyingParty::for_port(h, port).ok());
        let fingerprint = identity.as_ref().map(|i| i.fingerprint().to_owned());
        service.remote_auth().set_config(AuthConfig {
            relying_party,
            origins: origins.clone(),
            certificate_fingerprint: fingerprint,
        });

        let mut status = RemoteUiStatus::stopped();
        status.state = RemoteUiState::Running;
        status.urls = origins.iter().map(|o| format!("{o}/")).collect();
        status.certificate = identity.as_ref().map(|i| i.info().clone());
        service.report_remote_ui(Some(status), sandbox_origin);
        self.sweeper = Some(service.remote_auth().spawn_sweeper(Duration::from_secs(60)));
        self.server = Some(server);
        tracing::info!(urls = ?origins, "remote UI running");
        Ok(())
    }
}

fn internal(err: impl std::fmt::Display) -> ApiError {
    ApiError::internal(err.to_string())
}

/// The name browsers use: the configured host name, else the computer's
/// name when it is a usable DNS label.
fn primary_host(settings: &RemoteUiSettings, options: &RemoteUiOptions) -> Option<String> {
    settings
        .host_name
        .as_deref()
        .map(|h| h.trim_end_matches('.').to_ascii_lowercase())
        .or_else(|| {
            options
                .computer_name
                .clone()
                .filter(|n| nail_remote_auth::passkey::check_rp_host(n).is_ok())
        })
}

/// The self-signed certificate for `settings` (stored one reused, or a new
/// one when `regenerate`). Blocking work runs off the async threads.
async fn make_identity(
    service: &Service,
    options: &RemoteUiOptions,
    settings: &RemoteUiSettings,
    regenerate: bool,
) -> Result<cert::ServerIdentity, ApiError> {
    let names = cert::subject_names(settings, options.computer_name.as_deref());
    let service = service.clone();
    tokio::task::spawn_blocking(move || {
        let now = chrono::Utc::now();
        if regenerate {
            cert::regenerate_self_signed(&service, &names, now)
        } else {
            cert::load_or_create_self_signed(&service, &names, now)
        }
    })
    .await
    .map_err(internal)?
    .map_err(internal)
}
