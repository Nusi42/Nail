//! Tauri wiring of the M4b desktop shell (WP113, ADR 0087): building the
//! [`ShellHost`], the tray icon and its menu, showing / hiding the main
//! window, a second start's arguments, and the Simple MAPI pipe.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nail_api::Request;
use nail_api::launch::LaunchRequest;
use nail_api::mapi::MapiResult;
use nail_core::Service;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

use crate::AppArgs;
use crate::host::{HostParts, ShellHost};
use crate::install::{LocalFacts, UPDATE_ENDPOINT_ENV, UPDATE_PUBKEY_ENV, UpdateSource};
use crate::mapi_server::{MapiHandler, MapiServer};
use crate::registry::{MemoryRegistry, REGISTRY_ROOT_ENV, Registry, WindowsRegistry};
use crate::tray::TrayAction;
use crate::updater::Updater;
use crate::updater_tauri::TauriBackend;

/// The label of the main window.
pub const MAIN_WINDOW: &str = "main";

/// What the shell keeps for its lifetime (managed by Tauri).
pub struct ShellState {
    /// The desktop host (also the core's).
    pub host: Arc<ShellHost>,
    /// Set by "Quit": closing no longer goes to the tray.
    pub quitting: AtomicBool,
    /// The tray icon (none when it could not be created).
    pub tray: Option<TrayIcon>,
    /// The Simple MAPI pipe server, when running.
    pub mapi: Option<MapiServer>,
}

impl ShellState {
    /// Whether closing the main window should hide it instead.
    pub fn hides_on_close(&self) -> bool {
        closes_to_tray(
            self.host.close_to_tray(),
            self.tray.is_some(),
            self.quitting.load(Ordering::Relaxed),
        )
    }
}

/// Close to tray: the setting is on, there is a tray icon to come back
/// from, and the user did not pick "Quit".
pub fn closes_to_tray(setting: bool, has_tray: bool, quitting: bool) -> bool {
    setting && has_tray && !quitting
}

/// Whether this build honours the test-only environment overrides
/// (`NAIL_REGISTRY_ROOT`, `NAIL_UPDATE_ENDPOINT` / `NAIL_UPDATE_PUBKEY`,
/// `NAIL_MAPI_PIPE`): only with the `test-overrides` feature, which the e2e
/// desktop build enables and installer builds never do.
pub const TEST_OVERRIDES: bool = cfg!(feature = "test-overrides");

/// A test-only environment variable: its value in `test-overrides` builds,
/// else `None` (a set variable is logged and ignored).
pub fn test_env(name: &str) -> Option<String> {
    test_env_with(TEST_OVERRIDES, name, |n| std::env::var(n).ok())
}

fn test_env_with(
    allowed: bool,
    name: &str,
    read: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    let value = read(name)?;
    if allowed {
        Some(value)
    } else {
        tracing::warn!("ignoring {name}: test overrides are not part of this build");
        None
    }
}

/// Which registry the shell uses: a test root when `NAIL_REGISTRY_ROOT`
/// is set (`test-overrides` builds), memory for seeded runs (a demo never
/// writes the real Run key), else the real one.
pub fn registry_for(root: Option<String>, seeded: bool) -> Arc<dyn Registry> {
    match root.filter(|r| !r.trim().is_empty()) {
        Some(root) => Arc::new(WindowsRegistry::below(root)),
        None if seeded => Arc::new(MemoryRegistry::new()),
        None => Arc::new(WindowsRegistry::new()),
    }
}

/// Builds the desktop host for this app.
pub fn desktop_host<R: Runtime>(
    app: &AppHandle<R>,
    args: &AppArgs,
    data_dir: &Path,
) -> Arc<ShellHost> {
    let registry = registry_for(test_env(REGISTRY_ROOT_ENV), args.seed.is_some());
    let source = UpdateSource::from_plugin_config(app.config().plugins.0.get("updater"))
        .with_test_override(test_env(UPDATE_ENDPOINT_ENV), test_env(UPDATE_PUBKEY_ENV));
    let exe = std::env::current_exe().ok();
    let local = LocalFacts {
        version: app.package_info().version.to_string(),
        exe: exe.clone(),
        portable: args.seed.is_none() && crate::portable::root().is_some(),
        webview_version: tauri::webview_version().ok(),
    };
    let installation = crate::install::installation(&*registry, &local, &source);
    tracing::info!(
        kind = installation.kind.as_str(),
        updates_disabled = ?installation.updates_disabled,
        "installation"
    );
    let runtime = tauri::async_runtime::handle().inner().clone();
    let updater = Updater::new(
        Arc::new(TauriBackend::new(app.clone(), source)),
        runtime.clone(),
        installation.updates_disabled.is_none(),
        installation.forced_channel,
    );
    Arc::new(ShellHost::new(HostParts {
        registry,
        installation,
        updater,
        exe: exe.unwrap_or_else(|| nail_api::mapi::APP_EXE_FILE.into()),
        data_dir: data_dir.to_path_buf(),
        runtime,
        opener: Arc::new(crate::win::open_settings_uri),
    }))
}

/// Shows, restores and focuses the main window.
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// What a second start hands the running app: its launch requests and
/// whether to bring the window up (not for a second `--autostart` without
/// targets, e.g. the Run key while Nail already runs).
pub fn second_start(argv: &[String]) -> (Vec<LaunchRequest>, bool) {
    let args = AppArgs::from_process_args(argv.iter().skip(1).cloned().collect());
    let requests = args.launch_requests();
    let show = !args.autostart || !requests.is_empty();
    (requests, show)
}

/// Runs a tray menu entry.
pub fn tray_action<R: Runtime>(app: &AppHandle<R>, action: TrayAction) {
    let service = app.try_state::<Service>().map(|s| s.inner().clone());
    match action {
        TrayAction::Quit => {
            if let Some(shell) = app.try_state::<ShellState>() {
                shell.quitting.store(true, Ordering::Relaxed);
            }
            app.exit(0);
            return;
        }
        TrayAction::CheckMail => {
            if let Some(service) = service.clone() {
                tauri::async_runtime::spawn(async move {
                    service.handle(Request::SyncNow { account_id: None }).await;
                });
            }
        }
        TrayAction::Open | TrayAction::NewMessage => {}
    }
    if let (Some(request), Some(service)) = (action.launch_request(), service) {
        service.push_launch_requests(vec![request]);
    }
    if action.shows_window() {
        show_main_window(app);
    }
}

/// A test control of seeded runs (WP115 desktop smoke; never registered
/// for a real profile): what the e2e tests cannot reach through the web
/// content — the tray menu and whether the window is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestControl {
    /// Runs a tray menu entry as if it was clicked.
    Tray(TrayAction),
    /// Answers whether the main window is visible.
    WindowVisible,
    /// Lists the pop-out OS windows (M5c): `[{label, title, visible}]`.
    PopOuts,
}

impl TestControl {
    /// `"window-visible"`, `"pop-outs"` or a tray menu item id
    /// (`"tray-new-message"`, …).
    pub fn parse(op: &str) -> Option<Self> {
        match op {
            "window-visible" => Some(Self::WindowVisible),
            "pop-outs" => Some(Self::PopOuts),
            other => TrayAction::from_id(other).map(Self::Tray),
        }
    }
}

/// Runs a [`TestControl`]; the answer is JSON (`null` or a boolean).
pub fn test_control<R: Runtime>(app: &AppHandle<R>, op: &str) -> Result<serde_json::Value, String> {
    match TestControl::parse(op).ok_or_else(|| format!("unknown test control {op}"))? {
        TestControl::Tray(action) => {
            tray_action(app, action);
            Ok(serde_json::Value::Null)
        }
        TestControl::WindowVisible => {
            let window = app
                .get_webview_window(MAIN_WINDOW)
                .ok_or("no main window")?;
            window
                .is_visible()
                .map(serde_json::Value::Bool)
                .map_err(|e| e.to_string())
        }
        TestControl::PopOuts => {
            let mut windows: Vec<serde_json::Value> = app
                .webview_windows()
                .into_iter()
                .filter(|(label, _)| crate::popout::is_pop_out_label(label))
                .map(|(label, window)| {
                    serde_json::json!({
                        "label": label,
                        "title": window.title().unwrap_or_default(),
                        "visible": window.is_visible().unwrap_or(false),
                    })
                })
                .collect();
            windows.sort_by(|a, b| a["label"].as_str().cmp(&b["label"].as_str()));
            Ok(serde_json::Value::Array(windows))
        }
    }
}

/// Creates the tray icon with its menu in the core's language.
pub fn build_tray<R: Runtime>(app: &AppHandle<R>, service: &Service) -> tauri::Result<TrayIcon<R>> {
    let items = TrayAction::ALL
        .iter()
        .map(|action| {
            MenuItem::with_id(
                app,
                action.id(),
                service.render_localized(&action.label()),
                true,
                None::<&str>,
            )
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[&items[0], &items[1], &items[2], &separator, &items[3]],
    )?;
    let mut builder = TrayIconBuilder::with_id(MAIN_WINDOW)
        .tooltip(service.render_localized(&crate::tray::tooltip()))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            if let Some(action) = TrayAction::from_id(event.id().as_ref()) {
                tray_action(app, action);
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)
}

/// The Simple MAPI pipe for this run: `NAIL_MAPI_PIPE` (`test-overrides`
/// builds, read by the caller through [`test_env`]), else the user's —
/// never for seeded runs without the variable (they must not take the real
/// Nail's pipe).
pub fn mapi_pipe(args: &AppArgs, env: Option<String>) -> Option<String> {
    let sid = crate::win::current_user_sid()
        .map_err(|err| tracing::warn!(%err, "no user SID for the Simple MAPI pipe"))
        .ok();
    match env {
        Some(env) => crate::mapi_server::pipe_name(Some(env), sid.as_deref()),
        None if args.seed.is_some() => None,
        None => crate::mapi_server::pipe_name(None, sid.as_deref()),
    }
}

/// Starts the Simple MAPI pipe server: each message becomes a draft the
/// composer opens (the window comes up).
pub fn start_mapi<R: Runtime>(
    app: &AppHandle<R>,
    service: &Service,
    name: &str,
) -> Option<MapiServer> {
    let sid = match crate::win::current_user_sid() {
        Ok(sid) => sid,
        Err(err) => {
            tracing::warn!(%err, "Simple MAPI is off: no user SID");
            return None;
        }
    };
    let service = service.clone();
    let app = app.clone();
    let handler: MapiHandler = Arc::new(move |message| {
        let service = service.clone();
        let app = app.clone();
        Box::pin(async move {
            let result = service.submit_mapi(message).await;
            if result == MapiResult::Success {
                show_main_window(&app);
            }
            result
        })
    });
    let runtime = tauri::async_runtime::handle().inner().clone();
    match MapiServer::start(name, &crate::win::server_pipe_sddl(&sid), handler, &runtime) {
        Ok(server) => {
            tracing::info!(pipe = name, "Simple MAPI pipe ready");
            Some(server)
        }
        Err(err) => {
            tracing::warn!(%err, "Simple MAPI is off");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn closing_goes_to_the_tray_only_with_a_tray_and_not_when_quitting() {
        assert!(closes_to_tray(true, true, false));
        assert!(!closes_to_tray(false, true, false));
        assert!(
            !closes_to_tray(true, false, false),
            "no tray: closing quits"
        );
        assert!(!closes_to_tray(true, true, true), "Quit quits");
    }

    #[test]
    fn a_second_start_forwards_its_targets() {
        let argv = |rest: &[&str]| {
            std::iter::once("Nail.exe")
                .chain(rest.iter().copied())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        // What the registered open commands pass: `"Nail.exe" -- "%1"`.
        let (requests, show) = second_start(&argv(&["--", "mailto:ada@example.org?subject=Hi"]));
        assert_eq!(requests.len(), 1);
        assert!(
            matches!(&requests[0], LaunchRequest::Compose { mailto } if mailto.to == ["ada@example.org"])
        );
        assert!(show);
        let (requests, _) = second_start(&argv(&[
            "--",
            r"C:\mail\a.eml",
            r"C:\cal\b.ics",
            "webcal://x.example/c.ics",
        ]));
        assert_eq!(requests.len(), 3);
        // A plain second start brings the window up.
        assert_eq!(second_start(&argv(&[])), (vec![], true));
        // The Run key while Nail runs: nothing to show.
        assert_eq!(second_start(&argv(&["--autostart"])), (vec![], false));
        // Flags are never targets.
        let (requests, _) = second_start(&argv(&["--locale", "de"]));
        assert!(requests.is_empty());
    }

    #[test]
    fn test_controls_parse() {
        assert_eq!(
            TestControl::parse("tray-new-message"),
            Some(TestControl::Tray(TrayAction::NewMessage))
        );
        assert_eq!(
            TestControl::parse("window-visible"),
            Some(TestControl::WindowVisible)
        );
        assert_eq!(TestControl::parse("pop-outs"), Some(TestControl::PopOuts));
        assert_eq!(TestControl::parse("tray-format-disk"), None);
    }

    #[test]
    fn test_overrides_are_ignored_unless_built_in() {
        let read = |name: &str| (name == "NAIL_REGISTRY_ROOT").then(|| r"Software\X".to_owned());
        assert_eq!(test_env_with(false, "NAIL_REGISTRY_ROOT", read), None);
        assert_eq!(
            test_env_with(true, "NAIL_REGISTRY_ROOT", read).as_deref(),
            Some(r"Software\X")
        );
        assert_eq!(test_env_with(true, "NAIL_UPDATE_PUBKEY", read), None);
        // `cargo test --workspace` builds without the feature: the real
        // environment is ignored too.
        #[cfg(not(feature = "test-overrides"))]
        {
            const { assert!(!TEST_OVERRIDES) };
            // PATH is always set, and still ignored.
            assert!(std::env::var_os("PATH").is_some());
            assert_eq!(test_env("PATH"), None);
        }
    }

    #[test]
    fn seeded_runs_never_touch_the_real_registry_or_pipe() {
        let seeded = AppArgs {
            seed: Some("demo".into()),
            ..AppArgs::default()
        };
        assert_eq!(mapi_pipe(&seeded, None), None);
        assert_eq!(
            mapi_pipe(&seeded, Some(r"\\.\pipe\nail-mapi-e2e".into())).as_deref(),
            Some(r"\\.\pipe\nail-mapi-e2e")
        );
        #[cfg(windows)]
        assert!(
            mapi_pipe(&AppArgs::default(), None)
                .is_some_and(|name| name.starts_with(r"\\.\pipe\nail-mapi-S-1-"))
        );
        // Memory registry: a demo writes no Run key.
        let reg = registry_for(None, true);
        crate::startup::set_enabled(&*reg, Path::new(r"C:\x\Nail.exe"), true).unwrap();
        assert!(format!("{reg:?}").contains("MemoryRegistry"));
        let reg = registry_for(Some(r"Software\NailTests\x".into()), true);
        assert!(format!("{reg:?}").contains("NailTests"));
    }
}
