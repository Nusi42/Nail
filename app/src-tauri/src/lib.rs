//! The Nail desktop shell.
//!
//! Hosts a [`nail_core::Service`] in-process and exposes it to the webview
//! exactly like `nail serve` exposes it over the WebSocket (ADR 0002):
//!
//! * one command, [`nail_call`]: a `nail_api::Request` in, a
//!   `nail_api::Response` out (errors are `Response::Error`, never a
//!   rejected promise);
//! * every `nail_api::Event` the core broadcasts is emitted to the webview
//!   on [`EVENT_CHANNEL`].
//!
//! The UI is the same bundle as in the browser (`ui/dist`).

use clap::Parser;
use nail_api::{Event, Request, Response};
use nail_core::{Service, ServiceOptions, paths};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::broadcast;

/// Name of the Tauri event channel carrying `nail_api::Event`s.
pub const EVENT_CHANNEL: &str = "nail://event";

/// Command-line options of the desktop app (development and testing aids).
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
#[command(name = "nail-app", version, about = "Nail desktop app")]
pub struct AppArgs {
    /// Data directory (store and message blobs). Default: %LOCALAPPDATA%\Nail.
    #[arg(long, value_name = "DIR")]
    pub data_dir: Option<std::path::PathBuf>,
    /// Config directory (theme.kdl, layout.kdl). Default: %APPDATA%\Nail.
    #[arg(long, value_name = "DIR")]
    pub config_dir: Option<std::path::PathBuf>,
    /// Load the demo mail corpus as a local account before starting.
    #[arg(long, value_parser = ["demo"])]
    pub seed: Option<String>,
    /// UI language override (BCP 47, e.g. "de").
    #[arg(long, value_name = "TAG")]
    pub locale: Option<String>,
}

impl AppArgs {
    /// The service options these arguments describe, with the platform
    /// defaults for missing directories.
    pub fn service_options(&self) -> Result<ServiceOptions, String> {
        let data_dir = self
            .data_dir
            .clone()
            .or_else(paths::default_data_dir)
            .ok_or("no default data directory; pass --data-dir")?;
        let config_dir = self
            .config_dir
            .clone()
            .or_else(paths::default_config_dir)
            .ok_or("no default config directory; pass --config-dir")?;
        let mut options = ServiceOptions::new(data_dir, config_dir);
        options.locale = self.locale.clone();
        Ok(options)
    }
}

/// Answers one request. Config errors found before the UI asked for its
/// configuration (e.g. at start-up) are re-sent as events after
/// `GetConfig`, like the WebSocket server does on connect.
pub async fn call(service: &Service, request: Request) -> (Response, Vec<Event>) {
    let wants_config = matches!(request, Request::GetConfig);
    let response = service.handle(request).await;
    let events = if wants_config {
        service.config_error_events()
    } else {
        Vec::new()
    };
    (response, events)
}

/// The single IPC command: one `Request` in, one `Response` out.
#[tauri::command]
async fn nail_call(
    app: AppHandle,
    service: State<'_, Service>,
    request: Request,
) -> Result<Response, String> {
    let (response, events) = call(&service, request).await;
    for event in events {
        emit(&app, &event);
    }
    Ok(response)
}

fn emit(app: &AppHandle, event: &Event) {
    if let Err(err) = app.emit(EVENT_CHANNEL, event) {
        tracing::warn!(%err, "cannot emit an event to the webview");
    }
}

/// Forwards every core event to `emit` (the webview) until the service goes
/// away. When the receiver lagged and events were lost, the current state
/// is sent instead ([`Service::resync_events`]), so the UI refetches.
async fn forward_events(
    service: Service,
    mut events: broadcast::Receiver<Event>,
    emit: impl Fn(&Event),
) {
    loop {
        match events.recv().await {
            Ok(event) => emit(&event),
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                tracing::warn!(missed, "the webview missed events; resyncing it");
                for event in service.resync_events().await {
                    emit(&event);
                }
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// Opens the service described by `args` (seeding demo data if asked).
pub fn open_service(args: &AppArgs) -> Result<Service, Box<dyn std::error::Error>> {
    let service = Service::open(args.service_options()?)?;
    if args.seed.as_deref() == Some("demo") {
        nail_core::demo::seed_demo(service.store(), service.blobs())?;
    }
    Ok(service)
}

/// Builds and runs the Tauri application.
///
/// # Panics
///
/// Panics when the Tauri runtime fails to start (e.g. no WebView2 runtime).
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("NAIL_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
    // Unknown arguments (e.g. from a launcher) fall back to the defaults.
    let args = AppArgs::try_parse().unwrap_or_else(|err| {
        tracing::warn!(%err, "ignoring invalid command-line arguments");
        AppArgs::parse_from(["nail-app"])
    });

    tauri::Builder::default()
        .setup(move |app| {
            let service = open_service(&args)?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(forward_events(
                service.clone(),
                service.subscribe(),
                move |event| emit(&handle, event),
            ));
            app.manage(service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![nail_call])
        .run(tauri::generate_context!())
        .expect("failed to run the Tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn event_channel_name() {
        assert_eq!(EVENT_CHANNEL, "nail://event");
    }

    #[test]
    fn args_parse() {
        let args = AppArgs::try_parse_from([
            "nail-app",
            "--data-dir",
            "d",
            "--config-dir",
            "c",
            "--seed",
            "demo",
            "--locale",
            "de",
        ])
        .unwrap();
        let options = args.service_options().unwrap();
        assert_eq!(options.data_dir, std::path::PathBuf::from("d"));
        assert_eq!(options.config_dir, std::path::PathBuf::from("c"));
        assert_eq!(options.locale.as_deref(), Some("de"));
        assert!(options.watch_config);
        assert!(AppArgs::try_parse_from(["nail-app", "--seed", "x"]).is_err());

        let defaults = AppArgs::try_parse_from(["nail-app"]).unwrap();
        assert_eq!(defaults.seed, None);
    }

    /// Regression: a lagging forwarder only logged the loss, so the UI
    /// never learned about missed mail or config changes.
    #[tokio::test]
    async fn a_lagging_forwarder_resyncs_the_webview() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let args = AppArgs {
            data_dir: Some(data.path().into()),
            config_dir: Some(config.path().into()),
            seed: Some("demo".into()),
            locale: None,
        };
        let service = open_service(&args).unwrap();
        let events = service.subscribe();
        // Overflow the receiver before the forwarder reads anything.
        for _ in 0..nail_core::EVENT_CAPACITY + 10 {
            service.broadcast(Event::MailChanged {
                folder_ids: Vec::new(),
            });
        }
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&seen);
        let task = tokio::spawn(forward_events(service.clone(), events, move |event| {
            sink.lock().unwrap().push(event.clone());
        }));
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while seen.lock().unwrap().is_empty() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        task.abort();
        let seen = seen.lock().unwrap();
        assert!(
            matches!(seen[0], Event::ConfigChanged { .. }),
            "{:?}",
            seen[0]
        );
        assert!(
            matches!(&seen[1], Event::MailChanged { folder_ids } if !folder_ids.is_empty()),
            "{:?}",
            seen[1]
        );
    }

    #[tokio::test]
    async fn call_answers_and_resends_config_errors() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        std::fs::write(config.path().join("theme.kdl"), "theme \"x\" {\n").unwrap();
        let args = AppArgs {
            data_dir: Some(data.path().into()),
            config_dir: Some(config.path().into()),
            seed: Some("demo".into()),
            locale: None,
        };
        let service = open_service(&args).unwrap();

        let (response, events) = call(&service, Request::ListAccounts).await;
        let Response::Accounts { accounts } = response else {
            panic!("expected accounts");
        };
        assert_eq!(accounts.len(), 1);
        assert!(events.is_empty());

        let (response, events) = call(&service, Request::GetConfig).await;
        assert!(matches!(response, Response::Config { .. }));
        assert!(
            matches!(events.as_slice(), [Event::ConfigError { file, .. }] if file == "theme.kdl")
        );

        // Errors are responses, not rejected promises.
        let (response, _) = call(
            &service,
            Request::GetMessage {
                id: "missing".into(),
            },
        )
        .await;
        assert!(matches!(response, Response::Error { .. }));
    }
}
