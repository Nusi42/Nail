//! The shell's [`DesktopHost`] (M4b WP113, ADR 0087): what the core asks
//! the desktop app about itself — installation facts, the updater, the
//! Run key, the `nail plugin dev` endpoint, default apps — passed to the
//! core as `ServiceOptions::desktop`.
//!
//! Methods are quick (registry reads, state snapshots); the updater and
//! the dev endpoint run in tasks on the shell's runtime and report through
//! the core's notifier.
//!
//! M5c (WP173): pop-out windows ([`crate::popout`]) — `open_pop_out` /
//! `close_pop_out` / `focus_pop_out`, reported as `DesktopChange::PopOut`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use nail_api::desktop::{
    DefaultAppsStatus, DesktopChange, DesktopHost, DesktopNotifier, DesktopSettings,
    HostUpdateState, InstallationInfo, PathPick, UpdateSettings, default_apps_settings_uri,
};
use nail_api::engine::BoxFuture;
use nail_api::print::PrintJob;
use nail_api::windows::{PopOutOpen, window_kind};
use nail_api::{ApiError, ErrorCode, LocalizedMessage, PrintJobId};
use nail_core::Service;

use crate::dev_endpoint::DevEndpoint;
use crate::popout::{PopOutManager, PopOutWindows, TauriPopOuts};
use crate::registry::Registry;
use crate::updater::Updater;

/// Opens a Settings URI (the real one: [`crate::win::open_settings_uri`]).
pub type Opener = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// The desktop shell's host.
pub struct ShellHost {
    registry: Arc<dyn Registry>,
    installation: InstallationInfo,
    updater: Updater,
    exe: PathBuf,
    data_dir: PathBuf,
    runtime: tokio::runtime::Handle,
    opener: Opener,
    notify: OnceLock<DesktopNotifier>,
    service: OnceLock<Service>,
    dev: Mutex<Option<DevEndpoint>>,
    close_to_tray: AtomicBool,
    /// Settings → Default apps was opened: re-read when the user comes back.
    awaiting_default_apps: AtomicBool,
    /// M4c: the app (file dialogs' owner, hidden print windows) and the
    /// WebView2 data folder its windows share.
    app: OnceLock<(tauri::AppHandle, Option<PathBuf>)>,
    /// Print views waiting for `PrintViewReady`.
    print_ready: crate::printing::ReadySignals,
    /// M5c: pop-out windows.
    pop_outs: Arc<PopOutManager>,
}

impl std::fmt::Debug for ShellHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellHost")
            .field("installation", &self.installation)
            .field("updater", &self.updater)
            .finish_non_exhaustive()
    }
}

/// What the host is built from.
pub struct HostParts {
    /// The registry (real, test root or memory).
    pub registry: Arc<dyn Registry>,
    /// The installation facts (read once at start).
    pub installation: InstallationInfo,
    /// The updater.
    pub updater: Updater,
    /// The executable the Run key starts.
    pub exe: PathBuf,
    /// Where `ui-endpoint.json` goes.
    pub data_dir: PathBuf,
    /// The shell's async runtime.
    pub runtime: tokio::runtime::Handle,
    /// Opens Settings URIs.
    pub opener: Opener,
}

impl ShellHost {
    /// A new host.
    pub fn new(parts: HostParts) -> Self {
        Self {
            registry: parts.registry,
            installation: parts.installation,
            updater: parts.updater,
            exe: parts.exe,
            data_dir: parts.data_dir,
            pop_outs: PopOutManager::new(parts.runtime.clone()),
            runtime: parts.runtime,
            opener: parts.opener,
            notify: OnceLock::new(),
            service: OnceLock::new(),
            dev: Mutex::new(None),
            close_to_tray: AtomicBool::new(DesktopSettings::default().close_to_tray),
            awaiting_default_apps: AtomicBool::new(false),
            app: OnceLock::new(),
            print_ready: crate::printing::ReadySignals::default(),
        }
    }

    /// The app, once built (`webview_dir` = the WebView2 data folder of
    /// its windows, `None` = the default).
    pub fn bind_app(&self, app: tauri::AppHandle, webview_dir: Option<PathBuf>) {
        self.bind_pop_out_windows(Arc::new(TauriPopOuts::new(
            app.clone(),
            webview_dir.clone(),
        )));
        let _ = self.app.set((app, webview_dir));
    }

    /// The OS windows pop-outs open in (the app's, [`Self::bind_app`]; a
    /// fake in tests).
    pub fn bind_pop_out_windows(&self, windows: Arc<dyn PopOutWindows>) {
        self.pop_outs.bind(windows);
    }

    /// The pop-out windows.
    pub fn pop_outs(&self) -> &PopOutManager {
        &self.pop_outs
    }

    /// The main window closes (to the tray or for good): its pop-outs
    /// close with it.
    pub fn main_window_closing(&self) {
        self.pop_outs.close_all();
    }

    /// The main window's handle (the owner of file dialogs).
    fn main_window_handle(&self) -> Option<isize> {
        #[cfg(windows)]
        {
            use tauri::Manager as _;
            let (app, _) = self.app.get()?;
            let window = app.get_webview_window(crate::shell::MAIN_WINDOW)?;
            window.hwnd().ok().map(|h| h.0 as isize)
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    fn localize(&self, id: &str) -> String {
        match self.service.get() {
            Some(service) => service.render_localized(&LocalizedMessage::new(id)),
            None => id.to_owned(),
        }
    }

    /// Loads the print view in a hidden window and writes it as PDF.
    async fn write_pdf(&self, job: &PrintJob, path: &str) -> Result<u64, ApiError> {
        use crate::printing::{PDF_TIMEOUT, READY_TIMEOUT, pdf_error};
        let target = crate::printing::check_target(path)?;
        let (app, webview_dir) =
            self.app.get().cloned().ok_or_else(|| {
                ApiError::new(ErrorCode::Unavailable, "the app is not running yet")
            })?;
        let label = crate::printing::window_label(&job.job_id);
        let ready = self.print_ready.expect(&job.job_id);
        let url = tauri::WebviewUrl::App(crate::printing::view_url(job).into());
        let mut builder = tauri::WebviewWindowBuilder::new(&app, &label, url)
            .visible(false)
            .focused(false)
            .skip_taskbar(true);
        if let Some(dir) = webview_dir {
            builder = builder.data_directory(dir);
        }
        let window = builder.build().map_err(pdf_error)?;
        let outcome: Result<(), ApiError> = async {
            tokio::time::timeout(READY_TIMEOUT, ready)
                .await
                .map_err(|_| pdf_error("the print view did not finish loading"))?
                .map_err(|_| pdf_error("the print view went away"))?;
            #[cfg(windows)]
            {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let options = job.options.clone();
                let file = target.clone();
                window
                    .with_webview(move |webview| {
                        crate::printing::print_webview_to_pdf(&webview, &file, &options, tx);
                    })
                    .map_err(pdf_error)?;
                tokio::time::timeout(PDF_TIMEOUT, rx)
                    .await
                    .map_err(|_| pdf_error("WebView2 did not finish the PDF"))?
                    .map_err(|_| pdf_error("WebView2 went away"))?
                    .map_err(pdf_error)?;
                Ok(())
            }
            #[cfg(not(windows))]
            {
                Err(pdf_error("PDF files need WebView2 (Windows)"))
            }
        }
        .await;
        self.print_ready.forget(&job.job_id);
        if let Err(err) = window.destroy() {
            tracing::warn!(%err, "cannot close the print window");
        }
        outcome?;
        std::fs::metadata(&target)
            .map(|m| m.len())
            .map_err(pdf_error)
    }

    /// The service, once opened (the dev endpoint serves it).
    pub fn bind_service(&self, service: Service) {
        let _ = self.service.set(service);
    }

    /// The updater.
    pub fn updater(&self) -> &Updater {
        &self.updater
    }

    /// Whether closing the window keeps Nail in the tray.
    pub fn close_to_tray(&self) -> bool {
        self.close_to_tray.load(Ordering::Relaxed)
    }

    /// The main window got the focus: after Settings → Default apps, tell
    /// the UI to re-read the registration.
    pub fn window_focused(&self) {
        if self.awaiting_default_apps.swap(false, Ordering::Relaxed) {
            self.changed(DesktopChange::DefaultApps);
        }
    }

    fn changed(&self, change: DesktopChange) {
        if let Some(notify) = self.notify.get() {
            notify(change);
        }
    }

    fn lock_dev(&self) -> std::sync::MutexGuard<'_, Option<DevEndpoint>> {
        self.dev
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn set_dev_endpoint(&self, on: bool) -> Result<(), ApiError> {
        let mut dev = self.lock_dev();
        match (on, dev.is_some()) {
            (true, false) => {
                let service = self.service.get().cloned().ok_or_else(|| {
                    ApiError::new(ErrorCode::Unavailable, "the service is not open yet")
                })?;
                let endpoint =
                    DevEndpoint::start(service, &self.data_dir, &self.runtime).map_err(|e| {
                        ApiError::internal(format!("cannot start the plug-in dev endpoint: {e}"))
                    })?;
                *dev = Some(endpoint);
            }
            (false, true) => *dev = None,
            _ => {}
        }
        Ok(())
    }
}

impl DesktopHost for ShellHost {
    fn attach(&self, notify: DesktopNotifier) {
        self.updater.attach(Arc::clone(&notify));
        self.pop_outs.attach(Arc::clone(&notify));
        let _ = self.notify.set(notify);
    }

    fn installation(&self) -> InstallationInfo {
        self.installation.clone()
    }

    fn update_state(&self) -> HostUpdateState {
        self.updater.state()
    }

    fn apply_update_settings(&self, settings: &UpdateSettings) {
        self.updater.apply_settings(settings);
    }

    fn check_for_updates(&self) -> Result<(), ApiError> {
        self.updater.check()
    }

    fn download_update(&self) -> Result<(), ApiError> {
        self.updater.download()
    }

    fn install_update(&self) -> Result<(), ApiError> {
        self.updater.install()
    }

    fn start_with_windows(&self) -> Option<bool> {
        crate::startup::is_enabled(&*self.registry)
    }

    fn start_with_windows_managed(&self) -> bool {
        crate::startup::machine_enabled(&*self.registry)
    }

    fn apply_desktop_settings(&self, settings: &DesktopSettings) -> Result<(), ApiError> {
        self.close_to_tray
            .store(settings.close_to_tray, Ordering::Relaxed);
        let run_key =
            crate::startup::set_enabled(&*self.registry, &self.exe, settings.start_with_windows)
                .map_err(|e| {
                    ApiError::internal(format!("cannot change the Run key: {e}"))
                        .with_localized(LocalizedMessage::new("desktop-run-key-failed"))
                });
        let dev = self.set_dev_endpoint(settings.plugin_dev_endpoint);
        run_key.and(dev)
    }

    fn dev_endpoint_url(&self) -> Option<String> {
        self.lock_dev().as_ref().map(|d| d.url().to_owned())
    }

    fn pick_path<'a>(
        &'a self,
        pick: &'a PathPick,
    ) -> BoxFuture<'a, Result<Option<String>, ApiError>> {
        Box::pin(async move {
            let spec = crate::dialog::spec(pick, |id| self.localize(id));
            let owner = self.main_window_handle();
            let (tx, rx) = tokio::sync::oneshot::channel();
            std::thread::Builder::new()
                .name("nail-file-dialog".into())
                .spawn(move || {
                    #[cfg(windows)]
                    let answer = crate::dialog::show(&spec, owner);
                    #[cfg(not(windows))]
                    let answer: Result<Option<String>, String> = {
                        let _ = (spec, owner);
                        Err("file dialogs need Windows".to_owned())
                    };
                    let _ = tx.send(answer);
                })
                .map_err(|e| ApiError::internal(format!("cannot open the file dialog: {e}")))?;
            rx.await
                .map_err(|_| ApiError::internal("the file dialog went away"))?
                .map_err(|e| ApiError::internal(format!("the file dialog failed: {e}")))
        })
    }

    fn print_to_pdf<'a>(
        &'a self,
        job: &'a PrintJob,
        path: &'a str,
    ) -> BoxFuture<'a, Result<u64, ApiError>> {
        Box::pin(self.write_pdf(job, path))
    }

    fn print_view_ready(&self, job_id: &PrintJobId) {
        self.print_ready.ready(job_id);
    }

    fn default_apps(&self) -> DefaultAppsStatus {
        crate::default_apps::status(&*self.registry)
    }

    fn open_default_apps_settings(&self) -> Result<(), ApiError> {
        let uri = default_apps_settings_uri(self.installation.scope);
        (self.opener)(&uri).map_err(|e| {
            ApiError::internal(format!("cannot open {uri}: {e}"))
                .with_localized(LocalizedMessage::new("desktop-open-settings-failed"))
        })?;
        self.awaiting_default_apps.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn open_pop_out(&self, open: &PopOutOpen) -> Result<(), ApiError> {
        // Without the UI's title: the window kind's name (`window-<kind>`).
        let title = open
            .title
            .as_deref()
            .and_then(crate::popout::taskbar_title)
            .unwrap_or_else(|| self.localize(&format!("window-{}", window_kind(&open.window))));
        self.pop_outs.open(open, title)
    }

    fn close_pop_out(&self, window: &str) -> Result<(), ApiError> {
        self.pop_outs.close(window)
    }

    fn focus_pop_out(&self, window: &str) -> Result<(), ApiError> {
        self.pop_outs.focus(window)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{Hive, MemoryRegistry};
    use crate::updater::testing::{FakeBackend, FakeUpdate};
    use nail_api::desktop::{
        InstallKind, InstallScope, UpdateChannel, UpdateDisabledReason, UpdateState, WindowsArch,
        registry as keys,
    };
    use nail_api::{Event, Request, Response};
    use pretty_assertions::assert_eq;

    fn installation(disabled: Option<UpdateDisabledReason>) -> InstallationInfo {
        InstallationInfo {
            version: "1.0.0".into(),
            kind: InstallKind::Nsis,
            scope: Some(InstallScope::PerUser),
            architecture: WindowsArch::X64,
            install_dir: Some(r"C:\Nail".into()),
            updates_disabled: disabled,
            forced_channel: None,
            mapi_installed: false,
            webview_version: None,
        }
    }

    struct Fixture {
        service: Service,
        host: Arc<ShellHost>,
        registry: Arc<MemoryRegistry>,
        backend: Arc<FakeBackend>,
        opened: Arc<Mutex<Vec<String>>>,
        data: tempfile::TempDir,
        _config: tempfile::TempDir,
    }

    fn fixture(disabled: Option<UpdateDisabledReason>) -> Fixture {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let registry = Arc::new(MemoryRegistry::new());
        let backend = Arc::new(FakeBackend::default());
        let opened = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&opened);
        let runtime = tokio::runtime::Handle::current();
        let host = Arc::new(ShellHost::new(HostParts {
            registry: Arc::clone(&registry) as Arc<dyn Registry>,
            installation: installation(disabled),
            updater: Updater::new(
                Arc::clone(&backend) as Arc<dyn crate::updater::UpdateBackend>,
                runtime.clone(),
                disabled.is_none(),
                None,
            ),
            exe: PathBuf::from(r"C:\Nail\Nail.exe"),
            data_dir: data.path().to_path_buf(),
            runtime,
            opener: Arc::new(move |uri| {
                sink.lock().unwrap().push(uri.to_owned());
                Ok(())
            }),
        }));
        let args = crate::AppArgs {
            data_dir: Some(data.path().into()),
            config_dir: Some(config.path().into()),
            seed: Some("demo".into()),
            ..crate::AppArgs::default()
        };
        let mut options = args.service_options().unwrap();
        options.desktop = Some(Arc::clone(&host) as Arc<dyn DesktopHost>);
        let service = Service::open(options).unwrap();
        host.bind_service(service.clone());
        Fixture {
            service,
            host,
            registry,
            backend,
            opened,
            data,
            _config: config,
        }
    }

    async fn status(service: &Service) -> nail_api::desktop::UpdateStatus {
        match service.handle(Request::GetUpdateStatus).await {
            Response::UpdateStatus { status } => *status,
            other => panic!("{other:?}"),
        }
    }

    /// The core serves the desktop requests through this host: update
    /// check → download → ready, with events.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_core_drives_the_updater_through_the_host() {
        let f = fixture(None);
        f.backend
            .answer(Ok(Some(FakeUpdate::new("1.1.0", UpdateChannel::Stable))));
        let mut events = f.service.subscribe();
        let Response::Installation { info } = f.service.handle(Request::GetInstallation).await
        else {
            panic!()
        };
        assert_eq!(info.kind, InstallKind::Nsis);
        let response = f.service.handle(Request::CheckForUpdates).await;
        assert!(
            matches!(response, Response::UpdateStatus { .. }),
            "{response:?}"
        );
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if matches!(
                    status(&f.service).await.state,
                    UpdateState::ReadyToInstall { .. }
                ) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        // The host's changes became events.
        let mut saw = false;
        while let Ok(event) = events.try_recv() {
            saw |= matches!(event, Event::UpdateStatusChanged { .. });
        }
        assert!(saw);
        f.service.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_disabled_installation_is_reported_and_refused() {
        let f = fixture(Some(UpdateDisabledReason::MsiManaged));
        assert_eq!(
            status(&f.service).await.state,
            UpdateState::Disabled {
                reason: UpdateDisabledReason::MsiManaged
            }
        );
        let response = f.service.handle(Request::CheckForUpdates).await;
        assert!(
            matches!(
                response,
                Response::Error {
                    code: ErrorCode::Forbidden,
                    ..
                }
            ),
            "{response:?}"
        );
        assert!(f.backend.asked.lock().unwrap().is_empty());
        f.service.shutdown().await;
    }

    /// Settings: the Run key is written and read back, close to tray is
    /// applied, the dev endpoint starts and stops.
    #[tokio::test(flavor = "multi_thread")]
    async fn desktop_settings_reach_the_run_key_and_the_dev_endpoint() {
        let f = fixture(None);
        let settings = DesktopSettings {
            close_to_tray: false,
            start_hidden: true,
            start_with_windows: true,
            plugin_dev_endpoint: true,
        };
        let response = f
            .service
            .handle(Request::SetDesktopSettings {
                settings: settings.clone(),
            })
            .await;
        let Response::DesktopSettings {
            settings: stored,
            dev_endpoint_url,
            start_with_windows_managed,
        } = response
        else {
            panic!("{response:?}")
        };
        assert_eq!(stored, settings);
        assert!(!start_with_windows_managed);
        assert!(!f.host.close_to_tray());
        assert_eq!(
            f.registry
                .string(Hive::CurrentUser, keys::RUN_KEY, keys::RUN_VALUE)
                .as_deref(),
            Some(r#""C:\Nail\Nail.exe" --autostart"#)
        );
        let url = dev_endpoint_url.unwrap();
        assert!(url.starts_with("http://127.0.0.1:"));
        assert!(
            f.data
                .path()
                .join(nail_api::desktop::UI_ENDPOINT_FILE)
                .exists()
        );

        // The user removed the Run entry elsewhere: the Run key wins.
        f.registry
            .delete_value(Hive::CurrentUser, keys::RUN_KEY, keys::RUN_VALUE)
            .unwrap();
        let Response::DesktopSettings { settings: read, .. } =
            f.service.handle(Request::GetDesktopSettings).await
        else {
            panic!()
        };
        assert!(!read.start_with_windows);

        // The MSI's machine-wide Run key: on, and reported as managed.
        f.registry
            .set_string(
                Hive::LocalMachine,
                keys::RUN_KEY,
                keys::RUN_VALUE,
                r#""C:\Program Files\Nail\Nail.exe" --autostart"#,
            )
            .unwrap();
        let Response::DesktopSettings {
            settings: read,
            start_with_windows_managed,
            ..
        } = f.service.handle(Request::GetDesktopSettings).await
        else {
            panic!()
        };
        assert!(read.start_with_windows);
        assert!(start_with_windows_managed);
        f.registry
            .delete_value(Hive::LocalMachine, keys::RUN_KEY, keys::RUN_VALUE)
            .unwrap();

        let response = f
            .service
            .handle(Request::SetDesktopSettings {
                settings: DesktopSettings::default(),
            })
            .await;
        let Response::DesktopSettings {
            dev_endpoint_url, ..
        } = response
        else {
            panic!("{response:?}")
        };
        assert_eq!(dev_endpoint_url, None);
        assert!(
            !f.data
                .path()
                .join(nail_api::desktop::UI_ENDPOINT_FILE)
                .exists()
        );
        assert!(f.host.close_to_tray());
        f.service.shutdown().await;
    }

    async fn window_states(service: &Service) -> nail_api::windows::WindowStates {
        match service.handle(Request::GetWindowStates).await {
            Response::WindowStates { states } => states,
            other => panic!("{other:?}"),
        }
    }

    fn pop_outs(response: Response) -> Vec<nail_api::windows::PopOut> {
        match response {
            Response::PopOuts { pop_outs } => pop_outs,
            other => panic!("{other:?}"),
        }
    }

    /// Waits until the remembered state of `kind` passes `check`.
    async fn wait_for_state(
        service: &Service,
        kind: &str,
        check: impl Fn(&nail_api::windows::WindowState) -> bool,
    ) -> nail_api::windows::WindowState {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if let Some(state) = window_states(service).await.get(kind)
                    && check(state)
                {
                    return *state;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the state arrives")
    }

    fn pop_out_request(window: &str, title: Option<&str>) -> Request {
        Request::PopOutWindow {
            window: window.into(),
            title: title.map(str::to_owned),
            context: None,
        }
    }

    /// The core drives the pop-outs through this host: opened where the
    /// shell placed them, moves remembered per kind, the OS close button
    /// reported, reopened at the remembered place with the kind's title,
    /// returned to the page, and all closed with the main window.
    #[tokio::test(flavor = "multi_thread")]
    async fn pop_outs_round_trip_through_the_core() {
        use crate::popout::PopOutWindows;
        use crate::popout::testing::FakeWindows;
        use nail_api::windows::{PopOutChange, WindowGeometry, WindowRect};

        let f = fixture(None);
        let fake = FakeWindows::single_monitor();
        f.host
            .bind_pop_out_windows(Arc::clone(&fake) as Arc<dyn PopOutWindows>);
        let mut events = f.service.subscribe();

        let list = pop_outs(
            f.service
                .handle(Request::PopOutWindow {
                    window: "composer:d1".into(),
                    title: Some("Lunch on Friday".into()),
                    context: Some(serde_json::json!({ "draft": "d1" })),
                })
                .await,
        );
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].context, Some(serde_json::json!({ "draft": "d1" })));
        assert_eq!(fake.labels(), ["popout-composer:d1"]);
        assert_eq!(fake.title_of("popout-composer:d1"), "Lunch on Friday");
        // Where it opened is remembered for the kind.
        let placed = fake.geometry_of("popout-composer:d1");
        let state = wait_for_state(&f.service, "composer", |s| s.pop_out.is_some()).await;
        assert_eq!(state.pop_out, Some(placed));
        assert!(state.popped_out);

        // The user drags it elsewhere.
        let moved = WindowRect {
            x: 1000,
            y: 200,
            width: 800,
            height: 700,
        };
        fake.user_moves("popout-composer:d1", moved, false);
        let state = wait_for_state(&f.service, "composer", |s| {
            s.pop_out.is_some_and(|g| g.rect == moved)
        })
        .await;
        assert_eq!(state.pop_out, Some(WindowGeometry::at(moved)));

        // The OS close button: closed, the kind stays popped out.
        fake.user_closes("popout-composer:d1");
        assert!(pop_outs(f.service.handle(Request::ListPopOuts).await).is_empty());
        assert!(window_states(&f.service).await["composer"].popped_out);

        // Reopened (no title from the UI): the remembered place, the
        // kind's name.
        pop_outs(f.service.handle(pop_out_request("composer:d2", None)).await);
        assert_eq!(
            fake.geometry_of("popout-composer:d2"),
            WindowGeometry::at(moved)
        );
        assert_eq!(fake.title_of("popout-composer:d2"), "New message");
        // Focus from the UI.
        pop_outs(
            f.service
                .handle(Request::FocusPopOut {
                    window: "composer:d2".into(),
                })
                .await,
        );

        // Back into the page.
        let list = pop_outs(
            f.service
                .handle(Request::ReturnPopOut {
                    window: "composer:d2".into(),
                })
                .await,
        );
        assert!(list.is_empty());
        assert!(fake.labels().is_empty());
        assert!(!window_states(&f.service).await["composer"].popped_out);

        // The main window closes: every pop-out goes with it.
        for window in ["settings", "rules"] {
            pop_outs(f.service.handle(pop_out_request(window, None)).await);
        }
        assert_eq!(fake.labels(), ["popout-rules", "popout-settings"]);
        f.host.main_window_closing();
        assert!(fake.labels().is_empty());
        assert!(pop_outs(f.service.handle(Request::ListPopOuts).await).is_empty());

        let mut changes = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let Event::PopOutChanged { window, change, .. } = event {
                changes.push((window, change));
            }
        }
        for expected in [
            ("composer:d1".to_owned(), PopOutChange::Opened),
            ("composer:d1".to_owned(), PopOutChange::Moved),
            ("composer:d1".to_owned(), PopOutChange::Closed),
            ("composer:d2".to_owned(), PopOutChange::Focused),
            ("composer:d2".to_owned(), PopOutChange::Returned),
            ("settings".to_owned(), PopOutChange::Closed),
            ("rules".to_owned(), PopOutChange::Closed),
        ] {
            assert!(changes.contains(&expected), "{expected:?} in {changes:?}");
        }
        f.service.shutdown().await;
    }

    /// Without the app (or before it runs) pop-outs are unavailable.
    #[tokio::test(flavor = "multi_thread")]
    async fn pop_outs_need_the_app() {
        let f = fixture(None);
        let response = f.service.handle(pop_out_request("settings", None)).await;
        assert!(
            matches!(
                response,
                Response::Error {
                    code: ErrorCode::Unavailable,
                    ..
                }
            ),
            "{response:?}"
        );
        assert!(pop_outs(f.service.handle(Request::ListPopOuts).await).is_empty());
        f.service.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn default_apps_open_settings_and_refresh_on_return() {
        let f = fixture(None);
        let response = f.service.handle(Request::GetDefaultApps).await;
        let Response::DefaultApps {
            status,
            settings_uri,
        } = response
        else {
            panic!("{response:?}")
        };
        assert!(!status.registered);
        assert_eq!(
            settings_uri,
            "ms-settings:defaultapps?registeredAppUser=Nail"
        );
        let mut events = f.service.subscribe();
        // Coming back without having opened Settings changes nothing.
        f.host.window_focused();
        assert!(matches!(
            f.service.handle(Request::OpenDefaultAppsSettings).await,
            Response::Done
        ));
        assert_eq!(
            *f.opened.lock().unwrap(),
            ["ms-settings:defaultapps?registeredAppUser=Nail"]
        );
        f.host.window_focused();
        f.host.window_focused();
        let mut changes = 0;
        while let Ok(event) = events.try_recv() {
            changes += usize::from(event == Event::DefaultAppsChanged);
        }
        assert_eq!(changes, 1);
        f.service.shutdown().await;
    }
}
