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
//!
//! M2b integration (ADR 0041): launch requests go to the core
//! ([`Service::push_launch_requests`]; a second start forwards its own to
//! this instance), the `nail-proxy:` protocol serves allowed remote
//! content through the core's proxy ([`PROXY_PROTOCOL`]), and the remote
//! UI (TLS + login) runs when enabled in the settings
//! ([`nail_server::RemoteUiHost`]).
//!
//! M4b (WP113, ADR 0087): the shell implements the core's
//! [`nail_api::DesktopHost`] ([`host`]): installation facts from the
//! registry ([`install`], [`registry`]), the updater ([`updater`] over
//! tauri-plugin-updater, [`updater_tauri`]), "Start with Windows"
//! ([`startup`]), default apps ([`default_apps`]) and the `nail plugin dev`
//! endpoint ([`dev_endpoint`]). It shows a tray icon ([`tray`]) with close
//! to tray and start hidden, forwards a second start's arguments to the
//! running app, and serves the Simple MAPI pipe ([`mapi_server`]).
//!
//! M5c (WP173, ADR 0116): windows pop out into their own OS windows
//! ([`popout`]): a webview window per pop-out (`popout-<id>`) loads the same
//! UI at `#/window/<id>` and talks to the same core through [`nail_call`];
//! every core event reaches every webview, so each window keeps itself
//! current. Closing the main window closes its pop-outs.

pub mod default_apps;
pub mod dev_endpoint;
pub mod dialog;
pub mod host;
pub mod install;
pub mod launch;
pub mod mapi_server;
pub mod popout;
pub mod portable;
pub mod printing;
pub mod registry;
pub mod setup_helper;
pub mod shell;
pub mod startup;
pub mod tray;
pub mod updater;
pub mod updater_tauri;
pub mod win;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use clap::Parser;
use nail_api::{Event, Request, Response};
use nail_core::{Service, ServiceOptions, paths};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::broadcast;

use crate::shell::{MAIN_WINDOW, ShellState};

/// Name of the Tauri event channel carrying `nail_api::Event`s.
pub const EVENT_CHANNEL: &str = "nail://event";

/// The custom protocol the reader loads proxied remote content from
/// (`nail-proxy:<token>` in `sanitized_html`; WebView2 serves it as
/// `http://nail-proxy.localhost/<token>`).
pub const PROXY_PROTOCOL: &str = "nail-proxy";

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
        launch::from_args(&self.targets)
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

/// The app's MCP server, restarted when its settings change (M2c: a fixed
/// port in the settings).
pub type McpSlot = Arc<tokio::sync::Mutex<Option<nail_core::McpRunning>>>;

/// Things the app keeps alive for its lifetime: the MCP server, the lock on
/// the data directory and the temporary directory of a demo run.
pub struct Running {
    /// The MCP server, if it started.
    pub mcp: McpSlot,
    /// The single-instance lock of the data directory (a headless
    /// `nail mcp` or `nail serve` cannot open the same data meanwhile).
    pub _lock: nail_core::DataDirLock,
    /// The temporary directory of a seeded run.
    pub temp: Option<tempfile::TempDir>,
    /// The remote UI (stopped when the app exits).
    pub _remote_ui: nail_server::RemoteUiHost,
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

/// Answers a `nail-proxy` protocol request (`/<token>`) through the
/// core's proxy: only tokens the core signed for allowed messages, public
/// addresses, size and type limits (ADR 0038). Errors carry no body.
pub async fn proxy_response(
    proxy: Option<&nail_proxy::Proxy>,
    path: &str,
) -> tauri::http::Response<Vec<u8>> {
    let token = path.trim_start_matches('/');
    let token = token.split(['?', '#']).next().unwrap_or_default();
    let mut builder = tauri::http::Response::builder();
    for (name, value) in nail_proxy::RESPONSE_HEADERS {
        builder = builder.header(*name, *value);
    }
    let result = match proxy {
        Some(proxy) => proxy.serve(token).await.map_err(|err| err.status()),
        None => Err(404),
    };
    let response = match result {
        Ok(resource) => builder
            .status(200)
            .header("content-type", resource.content_type)
            .header("cache-control", resource.cache_control)
            .body(resource.body),
        Err(status) => builder
            .status(status)
            .header("cache-control", "no-store")
            .body(Vec::new()),
    };
    response.unwrap_or_else(|_| {
        let mut fallback = tauri::http::Response::new(Vec::new());
        *fallback.status_mut() = tauri::http::StatusCode::INTERNAL_SERVER_ERROR;
        fallback
    })
}

/// The UI files the remote UI serves: `NAIL_UI_DIR`, the bundled `ui`
/// resource, `ui` beside the executable (the portable zip; the installer's
/// resource directory is the same place), or the repository's `ui/dist`
/// (development). Candidates without an `index.html` are skipped.
fn remote_ui_dir(
    env: Option<std::path::PathBuf>,
    resource_dir: Option<std::path::PathBuf>,
    exe: Option<&std::path::Path>,
) -> Option<std::path::PathBuf> {
    env.into_iter()
        .chain(resource_dir.map(|d| d.join("ui")))
        .chain(exe.and_then(std::path::Path::parent).map(|d| d.join("ui")))
        .chain(std::iter::once(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist"),
        ))
        .find(|dir| dir.join("index.html").is_file())
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

/// Test control of seeded runs only (the desktop smoke test; see
/// [`shell::test_control`]): a tray menu entry, or whether the window shows.
/// Registered only with `--seed`, never for a real profile.
#[tauri::command]
fn nail_test_shell(app: AppHandle, op: String) -> Result<serde_json::Value, String> {
    shell::test_control(&app, &op)
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
    open_service_with(args, args.service_options_in(temp)?)
}

/// Opens the service with prepared options (M4b: the desktop host set),
/// seeding demo data if `args` ask.
pub fn open_service_with(
    args: &AppArgs,
    options: ServiceOptions,
) -> Result<Service, Box<dyn std::error::Error>> {
    let service = Service::open(options)?;
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

/// Starts the MCP server (ADR 0025) for the service on the port of the
/// MCP settings (any free port when none is set); agents find it through
/// `<data-dir>/mcp-endpoint.json`. A failure is logged, not fatal.
pub async fn start_mcp(
    service: &Service,
    data_dir: &std::path::Path,
) -> Option<nail_core::McpRunning> {
    let port = service.mcp_port().await;
    match service
        .start_mcp_server(port, Some(nail_core::mcp_endpoint_path(data_dir)))
        .await
    {
        Ok(running) => {
            tracing::info!(url = %running.url(), "MCP server running");
            Some(running)
        }
        Err(err) => {
            tracing::warn!(%err, port, "cannot start the MCP server");
            None
        }
    }
}

/// Restarts the MCP server when its settings change (`McpSettingsChanged`).
async fn restart_mcp_on_change(service: Service, slot: McpSlot, data_dir: std::path::PathBuf) {
    let mut events = service.subscribe();
    loop {
        match events.recv().await {
            Ok(Event::McpSettingsChanged) => {
                let mut running = slot.lock().await;
                if let Some(old) = running.take() {
                    old.shutdown().await;
                }
                *running = start_mcp(&service, &data_dir).await;
            }
            Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// The texts of an OS notification for a core `NotificationRequested`
/// (M2c): title and body rendered in the app's language.
pub fn notification_texts(
    service: &Service,
    notification: &nail_api::triage::MailNotification,
) -> (String, Option<String>) {
    (
        service.render_localized(&notification.title),
        notification
            .body
            .as_ref()
            .map(|b| service.render_localized(b)),
    )
}

fn show_plugin_notification(
    app: &AppHandle,
    service: &Service,
    notification: &nail_api::plugins::PluginNotification,
) {
    use tauri_plugin_notification::NotificationExt;
    let (title, body) = service.plugin_notification_texts(notification);
    if let Err(err) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!(%err, "cannot show a plug-in notification");
    }
}

fn show_notification(
    app: &AppHandle,
    service: &Service,
    notification: &nail_api::triage::MailNotification,
) {
    use tauri_plugin_notification::NotificationExt;
    let (title, body) = notification_texts(service, notification);
    let mut builder = app.notification().builder().title(title);
    if let Some(body) = body {
        builder = builder.body(body);
    }
    if notification.sound {
        builder = builder.sound("Default");
    }
    if let Err(err) = builder.show() {
        tracing::warn!(%err, "cannot show a notification");
    }
}

/// Shows the core's notification requests as OS notifications (the web
/// content gets no notification permission).
async fn show_notifications(app: AppHandle, service: Service) {
    let mut events = service.subscribe();
    loop {
        match events.recv().await {
            Ok(Event::NotificationRequested { notification }) => {
                show_notification(&app, &service, &notification);
            }
            // M2d: plug-in notifications that ask for the OS (the core
            // already applied the notification switch and quiet hours).
            Ok(Event::PluginNotification { notification }) if notification.spec.system => {
                show_plugin_notification(&app, &service, &notification);
            }
            Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// Tells the user why the app does not start (another Nail process holds
/// the data directory).
#[allow(unsafe_code)] // One MessageBoxW call.
fn refuse_start(message: &str) {
    tracing::error!("{message}");
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        use windows::core::HSTRING;
        // SAFETY: a modal message box without an owner window; both
        // strings live until it returns.
        unsafe {
            MessageBoxW(
                None,
                &HSTRING::from(message),
                &HSTRING::from("Nail"),
                MB_OK | MB_ICONERROR,
            );
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
        // Recent lines for the diagnostics bundle (M4c).
        .with_writer(|| nail_core::logs::TeeWriter)
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
    let seeded = args.seed.is_some();
    let launch_requests = args.launch_requests();
    if !launch_requests.is_empty() {
        tracing::info!(count = launch_requests.len(), "launch requests pending");
    }

    let mut builder = tauri::Builder::default();
    // One Nail per profile: a second start (a mailto: click, an opened
    // file, the Run key) hands its targets to this one, which comes to the
    // front (also from the tray). Seeded runs (tests, demos) have their own
    // temporary profile and stay independent.
    if args.seed.is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let (requests, show) = shell::second_start(&argv);
            tracing::info!(
                count = requests.len(),
                "another start forwarded its targets"
            );
            if let Some(service) = app.try_state::<Service>() {
                service.push_launch_requests(requests);
            }
            if show {
                shell::show_main_window(app);
            }
        }));
    }
    // OS notifications (M2c, ADR 0043): the shell shows the core's
    // `NotificationRequested` events from Rust (wired by WP55); the web
    // content gets no notification permission.
    builder = builder.plugin(tauri_plugin_notification::init());
    // M4b: the updater (signed manifests; driven from Rust only — the web
    // content gets no updater permission).
    builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    let builder = builder
        .register_asynchronous_uri_scheme_protocol(PROXY_PROTOCOL, |ctx, request, responder| {
            let proxy = ctx
                .app_handle()
                .try_state::<Service>()
                .and_then(|service| service.remote_proxy());
            let path = request.uri().path().to_owned();
            tauri::async_runtime::spawn(async move {
                responder.respond(proxy_response(proxy.as_deref(), &path).await);
            });
        })
        .setup(move |app| {
            // Opened within Tauri's runtime, so the account engines, the
            // outbox and the index upkeep start on it (and the accounts.kdl
            // watcher can apply changes).
            let temp = args.demo_dir()?;
            let base = args.base_dir(temp.as_ref().map(tempfile::TempDir::path), portable::root());
            let mut options = args.service_options_in(base.as_deref())?;
            let data_dir = options.data_dir.clone();
            // One Nail per data directory (M2c): a headless `nail mcp` or a
            // `nail serve` on the same data refuses to start while the app
            // runs, and the app while one of them runs.
            let lock = match paths::lock_data_dir(&data_dir, "desktop", args.locale.as_deref()) {
                Ok(lock) => lock,
                Err(message) => {
                    refuse_start(&message);
                    std::process::exit(1);
                }
            };
            // M4b: the desktop host (installation facts, updater, Run key,
            // dev endpoint, default apps) for the core's desktop requests.
            let host = shell::desktop_host(app.handle(), &args, &data_dir);
            options.desktop = Some(Arc::clone(&host) as Arc<dyn nail_api::DesktopHost>);
            let service =
                tauri::async_runtime::block_on(async { open_service_with(&args, options) })?;
            host.bind_service(service.clone());
            let desktop = tauri::async_runtime::block_on(service.desktop_settings())
                .unwrap_or_else(|err| {
                    tracing::warn!(%err, "cannot read the desktop settings");
                    nail_api::desktop::DesktopSettings::default()
                });
            if let Err(err) = nail_api::DesktopHost::apply_desktop_settings(&*host, &desktop) {
                tracing::warn!(%err, "cannot apply the desktop settings");
            }
            host.updater().spawn_schedule(updater::Schedule::default());
            let mcp: McpSlot = Arc::new(tokio::sync::Mutex::new(tauri::async_runtime::block_on(
                start_mcp(&service, &data_dir),
            )));
            tauri::async_runtime::spawn(restart_mcp_on_change(
                service.clone(),
                Arc::clone(&mcp),
                data_dir.clone(),
            ));
            let tray = match shell::build_tray(app.handle(), &service) {
                Ok(tray) => Some(tray),
                Err(err) => {
                    tracing::warn!(%err, "cannot create the tray icon");
                    None
                }
            };
            // The window is created here (not from the config) so that a
            // seeded run keeps its WebView2 profile in the temporary
            // directory too, apart from the real one (and a portable run
            // beside the executable).
            let config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == MAIN_WINDOW)
                .cloned()
                .ok_or("tauri.conf.json has no main window")?;
            // Started with Windows and "start hidden": only the tray icon.
            let hidden =
                startup::starts_hidden(args.autostart, desktop.start_hidden, tray.is_some());
            let mut window =
                tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?.visible(!hidden);
            if let Some(base) = &base {
                window = window.data_directory(base.join("webview"));
            }
            let window = window.build()?;
            // M4c: file dialogs (owned by this window) and hidden print
            // windows (sharing its WebView2 data folder).
            host.bind_app(
                app.handle().clone(),
                base.as_ref().map(|b| b.join("webview")),
            );
            if args.autostart
                && !hidden
                && let Err(err) = window.minimize()
            {
                tracing::warn!(%err, "cannot start minimized");
            }
            service.push_launch_requests(launch_requests.clone());
            // Simple MAPI ("Send to → Mail recipient"): the per-user pipe.
            let mapi = shell::mapi_pipe(&args, shell::test_env(nail_api::mapi::MAPI_PIPE_ENV))
                .and_then(|name| shell::start_mapi(app.handle(), &service, &name));
            let ui_dir = remote_ui_dir(
                std::env::var_os("NAIL_UI_DIR").map(std::path::PathBuf::from),
                app.path().resource_dir().ok(),
                std::env::current_exe().ok().as_deref(),
            );
            let remote_ui = tauri::async_runtime::block_on(async {
                nail_server::RemoteUiHost::start(
                    service.clone(),
                    nail_server::RemoteUiOptions::new(ui_dir),
                )
            });
            app.manage(Running {
                mcp,
                _lock: lock,
                temp,
                _remote_ui: remote_ui,
            });
            app.manage(ShellState {
                host,
                quitting: AtomicBool::new(false),
                tray,
                mapi,
            });
            tauri::async_runtime::spawn(show_notifications(app.handle().clone(), service.clone()));
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(forward_events(
                service.clone(),
                service.subscribe(),
                move |event| emit(&handle, event),
            ));
            app.manage(service);
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != MAIN_WINDOW {
                return;
            }
            let Some(shell) = window.try_state::<ShellState>() else {
                return;
            };
            match event {
                // Close to tray: Nail keeps syncing; "Quit" in the tray ends it.
                // Either way the pop-outs close with the main window.
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    shell.host.main_window_closing();
                    if shell.hides_on_close() {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
                tauri::WindowEvent::Destroyed => shell.host.main_window_closing(),
                tauri::WindowEvent::Focused(true) => shell.host.window_focused(),
                _ => {}
            }
        });
    // Seeded runs (demos, the desktop smoke test) also get the test control.
    let builder = if seeded {
        builder.invoke_handler(tauri::generate_handler![nail_call, nail_test_shell])
    } else {
        builder.invoke_handler(tauri::generate_handler![nail_call])
    };
    let app = builder
        .build(tauri::generate_context!())
        .expect("failed to build the Tauri application");
    app.run(|app, event| {
        // Quitting with a downloaded update installs it (no restart).
        if let tauri::RunEvent::Exit = event
            && let Some(shell) = app.try_state::<ShellState>()
            && shell.host.updater().install_on_exit()
        {
            tracing::info!("installing the downloaded update");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn event_channel_name() {
        assert_eq!(EVENT_CHANNEL, "nail://event");
    }

    /// The portable zip's `Nail\ui` beside `Nail\Nail.exe` is found without
    /// the build machine's `ui/dist`; candidates need an `index.html`.
    #[test]
    fn the_remote_ui_files_are_found_beside_the_app() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("Nail.exe");
        let empty = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("ui")).unwrap();
        std::fs::write(root.path().join("ui").join("index.html"), "x").unwrap();
        assert_eq!(
            remote_ui_dir(
                Some(empty.path().to_path_buf()),
                Some(empty.path().to_path_buf()),
                Some(&exe)
            ),
            Some(root.path().join("ui"))
        );
        std::fs::write(empty.path().join("index.html"), "x").unwrap();
        assert_eq!(
            remote_ui_dir(Some(empty.path().to_path_buf()), None, Some(&exe)),
            Some(empty.path().to_path_buf())
        );
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

    /// M2c: the MCP server uses the port of the settings and restarts on
    /// the new one when they change.
    #[tokio::test]
    async fn the_mcp_server_follows_its_port_setting() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let args = AppArgs {
            data_dir: Some(data.path().into()),
            config_dir: Some(config.path().into()),
            seed: Some("demo".into()),
            ..AppArgs::default()
        };
        let service = open_service(&args).unwrap();
        let free = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let (response, _) = call(
            &service,
            Request::UpdateMcpSettings {
                settings: nail_api::McpSettings { port: Some(free) },
            },
        )
        .await;
        assert!(
            matches!(response, Response::McpSettings { .. }),
            "{response:?}"
        );
        let slot: McpSlot = Arc::new(tokio::sync::Mutex::new(None));
        let task = tokio::spawn(restart_mcp_on_change(
            service.clone(),
            Arc::clone(&slot),
            data.path().to_path_buf(),
        ));
        tokio::task::yield_now().await;
        service.broadcast(Event::McpSettingsChanged);
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if slot.lock().await.is_some() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let url = slot.lock().await.as_ref().unwrap().url();
        assert!(url.contains(&format!(":{free}")), "{url}");
        task.abort();
        if let Some(running) = slot.lock().await.take() {
            running.shutdown().await;
        }
        service.shutdown().await;
    }

    /// M2c: OS notifications show the core's texts in the app's language.
    #[test]
    fn notification_texts_are_rendered() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let args = AppArgs {
            data_dir: Some(data.path().into()),
            config_dir: Some(config.path().into()),
            seed: Some("demo".into()),
            ..AppArgs::default()
        };
        let service = open_service(&args).unwrap();
        let notification = nail_api::triage::MailNotification {
            id: "n".into(),
            tag: "t".into(),
            kind: nail_api::triage::NotificationKind::NewMail,
            account_id: None,
            folder_id: None,
            message_ids: Vec::new(),
            title: nail_api::LocalizedMessage::new("notification-new-mail-title")
                .arg("count", 2u64)
                .arg("account", "Work"),
            body: Some(
                nail_api::LocalizedMessage::new("notification-new-mail-body")
                    .arg("sender", "Bob")
                    .arg("subject", "Lunch"),
            ),
            sound: false,
        };
        let (title, body) = notification_texts(&service, &notification);
        assert!(!title.contains("notification-"), "{title}");
        assert!(title.contains("Work"), "{title}");
        let body = body.unwrap();
        assert!(body.contains("Bob") && body.contains("Lunch"), "{body}");
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
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
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
        // The theme list and selection first (M5a), then the configuration.
        assert!(
            matches!(seen[0], Event::ThemeChanged { .. }),
            "{:?}",
            seen[0]
        );
        let seen: Vec<Event> = seen[1..].to_vec();
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
    async fn the_proxy_protocol_serves_only_signed_tokens() {
        // No proxy (no key): nothing is served.
        let none = proxy_response(None, "/abc.def").await;
        assert_eq!(none.status(), 404);
        assert!(none.body().is_empty());
        assert_eq!(none.headers()["x-content-type-options"], "nosniff");

        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let args = AppArgs {
            data_dir: Some(data.path().into()),
            config_dir: Some(config.path().into()),
            seed: Some("demo".into()),
            ..AppArgs::default()
        };
        let service = open_service(&args).unwrap();
        let proxy = service.remote_proxy().unwrap();
        // A forged token: 404, inert headers, no body.
        let forged = proxy_response(Some(&proxy), "/forged.token").await;
        assert_eq!(forged.status(), 404);
        assert!(
            forged.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("sandbox")
        );
        // A signed token for a message that is not allowed: the core says no.
        let token = proxy
            .signer()
            .sign_resource("m-1", "https://images.example.org/a.png")
            .unwrap();
        let denied = proxy_response(Some(&proxy), &format!("/{token}")).await;
        assert_eq!(denied.status(), 403);
        service.shutdown().await;
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
