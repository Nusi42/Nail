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
//!
//! Windows integration (M2b WP44): [`launch`] parses the `mailto:` /
//! `webcal:` URLs and `.eml` / `.ics` / `.vcf` files Windows starts the app
//! with, [`portable`] keeps everything beside the executable in the portable
//! build, and [`setup_helper`] does the installer's PATH and credential
//! work.

pub mod launch;
pub mod portable;
pub mod setup_helper;

use clap::Parser;
use nail_api::{Event, Request, Response};
use nail_core::{Service, ServiceOptions, paths};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::broadcast;

/// Name of the Tauri event channel carrying `nail_api::Event`s.
pub const EVENT_CHANNEL: &str = "nail://event";

/// Command-line options of the desktop app (development and testing aids,
/// plus what Windows and the installer pass).
#[derive(Debug, Clone, Default, PartialEq, Eq, Parser)]
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
    /// Started by the "Start with Windows" entry: open minimized.
    #[arg(long)]
    pub autostart: bool,
    /// Installer helper: add a directory to the per-user PATH and exit.
    #[arg(long, value_name = "DIR", hide = true)]
    pub setup_path_add: Option<String>,
    /// Installer helper: remove a directory from the per-user PATH and exit.
    #[arg(long, value_name = "DIR", hide = true)]
    pub setup_path_remove: Option<String>,
    /// Installer helper: delete Nail's Credential Manager entries and exit.
    #[arg(long, hide = true)]
    pub setup_purge_credentials: bool,
    /// URLs (`mailto:`, `webcal:`, `webcals:`) or files (`.eml`, `.ics`,
    /// `.vcf`) to open, as passed by Windows.
    #[arg(value_name = "TARGET")]
    pub targets: Vec<String>,
}

impl AppArgs {
    /// The service options these arguments describe, with the platform
    /// defaults for missing directories (a seeded run: `temp`, see
    /// [`Self::demo_dir`]).
    pub fn service_options_in(
        &self,
        temp: Option<&std::path::Path>,
    ) -> Result<ServiceOptions, String> {
        let pick = |given: &Option<std::path::PathBuf>,
                    sub: &str,
                    default: fn() -> Option<std::path::PathBuf>,
                    flag: &str| {
            match (given, temp) {
                (Some(dir), _) => Ok(dir.clone()),
                (None, Some(temp)) => Ok(temp.join(sub)),
                (None, None) => {
                    default().ok_or_else(|| format!("no default {sub} directory; pass {flag}"))
                }
            }
        };
        let data_dir = pick(
            &self.data_dir,
            "data",
            paths::default_data_dir,
            "--data-dir",
        )?;
        let config_dir = pick(
            &self.config_dir,
            "config",
            paths::default_config_dir,
            "--config-dir",
        )?;
        let mut options = ServiceOptions::new(data_dir, config_dir);
        options.locale = self.locale.clone();
        // OAuth2 sign-ins open the system browser.
        options.open_browser = true;
        // "Open file at line" starts the user's editor, except in demo runs.
        options.open_editor = self.seed.is_none();
        if self.seed.is_some() {
            // Demo runs: secrets in memory, no online key lookups.
            options = nail_core::demo::demo_options(options);
        }
        Ok(options)
    }

    /// The installer-helper action these arguments ask for, if any.
    pub fn setup_action(&self) -> Option<setup_helper::SetupAction> {
        use setup_helper::SetupAction;
        if let Some(dir) = &self.setup_path_add {
            Some(SetupAction::PathAdd(dir.clone()))
        } else if let Some(dir) = &self.setup_path_remove {
            Some(SetupAction::PathRemove(dir.clone()))
        } else if self.setup_purge_credentials {
            Some(SetupAction::PurgeCredentials)
        } else {
            None
        }
    }

    /// The launch requests among [`Self::targets`].
    pub fn launch_requests(&self) -> Vec<launch::LaunchRequest> {
        launch::LaunchRequest::from_args(&self.targets)
    }

    /// The directory that replaces the platform defaults: the temporary
    /// directory of a seeded run, else the portable root (when the
    /// executable has a `portable` marker; never for seeded runs).
    pub fn base_dir(
        &self,
        temp: Option<&std::path::Path>,
        portable: Option<std::path::PathBuf>,
    ) -> Option<std::path::PathBuf> {
        match temp {
            Some(temp) => Some(temp.to_path_buf()),
            None if self.seed.is_none() => portable,
            None => None,
        }
    }

    /// Parses the process arguments (without the program name). Arguments
    /// clap rejects (e.g. from a launcher) fall back to the defaults,
    /// keeping any launch targets.
    pub fn from_process_args(raw: Vec<String>) -> Self {
        let argv = std::iter::once("nail-app".to_owned()).chain(raw.iter().cloned());
        Self::try_parse_from(argv).unwrap_or_else(|err| {
            tracing::warn!(%err, "ignoring invalid command-line arguments");
            Self {
                targets: raw,
                ..Self::default()
            }
        })
    }

    /// The service options without a temporary directory.
    pub fn service_options(&self) -> Result<ServiceOptions, String> {
        self.service_options_in(None)
    }

    /// A seeded run without `--data-dir` / `--config-dir` runs in a
    /// temporary directory (removed on exit), never on the real profile,
    /// like `nail serve --seed` (ADR 0021).
    pub fn demo_dir(&self) -> Result<Option<tempfile::TempDir>, String> {
        if self.seed.is_none() || (self.data_dir.is_some() && self.config_dir.is_some()) {
            return Ok(None);
        }
        tempfile::Builder::new()
            .prefix("nail-app-demo-")
            .tempdir()
            .map(Some)
            .map_err(|e| format!("cannot create a temporary directory for the demo: {e}"))
    }
}

/// Things the app keeps alive for its lifetime: the MCP server and the
/// temporary directory of a demo run.
pub struct Running {
    /// The MCP server, if it started.
    pub mcp: Option<nail_core::McpRunning>,
    /// The temporary directory of a seeded run.
    pub temp: Option<tempfile::TempDir>,
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

/// Hands the UI the launch requests (`mailto:` links, opened files) that
/// are still pending; each one once. The integrator (M2b WP45) maps them
/// onto `nail_api` requests.
#[tauri::command]
fn nail_launch_requests(pending: State<'_, launch::PendingLaunch>) -> Vec<launch::LaunchRequest> {
    pending.take()
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

/// Opens the service described by `args` (seeding demo data if asked),
/// with the data and config directories under `temp` when given.
pub fn open_service_in(
    args: &AppArgs,
    temp: Option<&std::path::Path>,
) -> Result<Service, Box<dyn std::error::Error>> {
    let service = Service::open(args.service_options_in(temp)?)?;
    if args.seed.as_deref() == Some("demo") {
        nail_core::demo::seed_demo(service.store(), service.blobs())?;
        service.rebuild_search_index()?;
    }
    Ok(service)
}

/// Opens the service described by `args` (seeding demo data if asked).
pub fn open_service(args: &AppArgs) -> Result<Service, Box<dyn std::error::Error>> {
    open_service_in(args, None)
}

/// Starts the MCP server (ADR 0025) for the service; agents find it through
/// `<data-dir>/mcp-endpoint.json`. A failure is logged, not fatal.
pub async fn start_mcp(
    service: &Service,
    data_dir: &std::path::Path,
) -> Option<nail_core::McpRunning> {
    match service
        .start_mcp_server(0, Some(nail_core::mcp_endpoint_path(data_dir)))
        .await
    {
        Ok(running) => {
            tracing::info!(url = %running.url(), "MCP server running");
            Some(running)
        }
        Err(err) => {
            tracing::warn!(%err, "cannot start the MCP server");
            None
        }
    }
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
    let args = AppArgs::from_process_args(
        std::env::args_os()
            .skip(1)
            .map(|a| a.to_string_lossy().into_owned())
            .collect(),
    );
    if let Some(action) = args.setup_action() {
        std::process::exit(setup_helper::run(&action));
    }
    let launch_requests = args.launch_requests();
    if !launch_requests.is_empty() {
        tracing::info!(count = launch_requests.len(), "launch requests pending");
    }

    tauri::Builder::default()
        .setup(move |app| {
            // Opened within Tauri's runtime, so the account engines, the
            // outbox and the index upkeep start on it (and the accounts.kdl
            // watcher can apply changes).
            let temp = args.demo_dir()?;
            let base = args.base_dir(temp.as_ref().map(tempfile::TempDir::path), portable::root());
            let service =
                tauri::async_runtime::block_on(async { open_service_in(&args, base.as_deref()) })?;
            let data_dir = args.service_options_in(base.as_deref())?.data_dir;
            let mcp = tauri::async_runtime::block_on(start_mcp(&service, &data_dir));
            // The window is created here (not from the config) so that a
            // seeded run keeps its WebView2 profile in the temporary
            // directory too, apart from the real one (and a portable run
            // beside the executable).
            let config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
                .ok_or("tauri.conf.json has no main window")?;
            let mut window = tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?;
            if let Some(base) = &base {
                window = window.data_directory(base.join("webview"));
            }
            let window = window.build()?;
            if args.autostart
                && let Err(err) = window.minimize()
            {
                tracing::warn!(%err, "cannot start minimized");
            }
            app.manage(Running { mcp, temp });
            app.manage(launch::PendingLaunch::new(launch_requests));
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(forward_events(
                service.clone(),
                service.subscribe(),
                move |event| emit(&handle, event),
            ));
            app.manage(service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![nail_call, nail_launch_requests])
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
        assert!(!options.open_editor, "demo runs never open an editor");
        assert!(AppArgs::try_parse_from(["nail-app", "--seed", "x"]).is_err());
        // Both directories given: no temporary directory.
        assert!(args.demo_dir().unwrap().is_none());

        let defaults = AppArgs::try_parse_from(["nail-app"]).unwrap();
        assert_eq!(defaults.seed, None);
        assert!(defaults.demo_dir().unwrap().is_none());
    }

    #[test]
    fn launch_targets_and_flags_parse() {
        let args = AppArgs::from_process_args(vec![
            "--autostart".into(),
            "mailto:a@example.org?subject=Hi".into(),
            r"C:\mail\offer.eml".into(),
        ]);
        assert!(args.autostart);
        assert_eq!(args.setup_action(), None);
        let requests = args.launch_requests();
        assert_eq!(requests.len(), 2);
        assert!(matches!(requests[0], launch::LaunchRequest::Compose { .. }));

        // Windows may add arguments clap does not know: targets survive.
        let args = AppArgs::from_process_args(vec!["--unknown".into(), "webcal://x.org/c".into()]);
        assert_eq!(args.seed, None);
        assert_eq!(args.launch_requests().len(), 1);
    }

    #[test]
    fn setup_actions_parse() {
        use setup_helper::SetupAction;
        let action = |raw: &[&str]| {
            AppArgs::from_process_args(raw.iter().map(|s| (*s).to_owned()).collect()).setup_action()
        };
        assert_eq!(
            action(&["--setup-path-add", r"C:\N\bin"]),
            Some(SetupAction::PathAdd(r"C:\N\bin".into()))
        );
        assert_eq!(
            action(&["--setup-path-remove", r"C:\N\bin"]),
            Some(SetupAction::PathRemove(r"C:\N\bin".into()))
        );
        assert_eq!(
            action(&["--setup-purge-credentials"]),
            Some(SetupAction::PurgeCredentials)
        );
        assert_eq!(action(&[]), None);
    }

    #[test]
    fn base_dir_prefers_the_demo_directory_then_portable_mode() {
        let root = std::path::PathBuf::from(r"E:\Nail");
        let plain = AppArgs::default();
        assert_eq!(plain.base_dir(None, None), None);
        assert_eq!(plain.base_dir(None, Some(root.clone())), Some(root.clone()));
        let temp = std::path::Path::new("tmp");
        assert_eq!(
            plain.base_dir(Some(temp), Some(root.clone())).as_deref(),
            Some(temp)
        );
        // Seeded runs never write into a portable installation.
        let seeded = AppArgs {
            seed: Some("demo".into()),
            ..AppArgs::default()
        };
        assert_eq!(seeded.base_dir(None, Some(root.clone())), None);

        // Portable: data and config beside the executable.
        let options = plain.service_options_in(Some(root.as_path())).unwrap();
        assert_eq!(options.data_dir, root.join("data"));
        assert_eq!(options.config_dir, root.join("config"));
    }

    /// M1a follow-up: `--seed demo` alone never touches the real profile.
    #[test]
    fn a_seeded_run_defaults_to_temporary_directories() {
        let args = AppArgs::try_parse_from(["nail-app", "--seed", "demo"]).unwrap();
        let temp = args.demo_dir().unwrap().expect("a temporary directory");
        let options = args.service_options_in(Some(temp.path())).unwrap();
        assert!(options.data_dir.starts_with(temp.path()));
        assert!(options.config_dir.starts_with(temp.path()));
        let path = temp.path().to_owned();
        drop(temp);
        assert!(!path.exists(), "removed on exit");
    }

    #[tokio::test]
    async fn the_mcp_server_starts_with_the_app() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let args = AppArgs {
            data_dir: Some(data.path().into()),
            config_dir: Some(config.path().into()),
            seed: None,
            ..AppArgs::default()
        };
        let service = open_service(&args).unwrap();
        let running = start_mcp(&service, data.path()).await.unwrap();
        assert!(nail_core::mcp_endpoint_path(data.path()).exists());
        let (response, _) = call(&service, Request::GetMcpServerInfo).await;
        let Response::McpServer { info } = response else {
            panic!("expected server info")
        };
        assert!(info.running);
        assert_eq!(info.url.as_deref(), Some(running.url().as_str()));
        running.shutdown().await;
        service.shutdown().await;
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
            ..AppArgs::default()
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
        // Then every file's error list (empty: all valid).
        let seen: Vec<&Event> = seen
            .iter()
            .filter(|e| !matches!(e, Event::ConfigErrors { errors, .. } if errors.is_empty()))
            .collect();
        assert_eq!(*seen[1], Event::AccountsChanged);
        assert!(
            matches!(seen[2], Event::MailChanged { folder_ids } if !folder_ids.is_empty()),
            "{:?}",
            seen[2]
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
            ..AppArgs::default()
        };
        let service = open_service(&args).unwrap();

        let (response, events) = call(&service, Request::ListAccounts).await;
        let Response::Accounts { accounts } = response else {
            panic!("expected accounts");
        };
        // "All accounts" and the demo account.
        assert_eq!(accounts.len(), 2);
        assert!(events.is_empty());

        let (response, events) = call(&service, Request::GetConfig).await;
        assert!(matches!(response, Response::Config { .. }));
        // The M1a first error, then every error per file (M1b).
        assert!(
            matches!(events.first(), Some(Event::ConfigError { file, .. }) if file == "theme.kdl"),
            "{events:?}"
        );
        assert!(events.iter().any(
            |e| matches!(e, Event::ConfigErrors { file, errors } if file == "theme.kdl" && !errors.is_empty())
        ));

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
