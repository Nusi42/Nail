//! The shell's [`DesktopHost`] (M4b WP113, ADR 0087): what the core asks
//! the desktop app about itself — installation facts, the updater, the
//! Run key, the `nail plugin dev` endpoint, default apps — passed to the
//! core as `ServiceOptions::desktop`.
//!
//! Methods are quick (registry reads, state snapshots); the updater and
//! the dev endpoint run in tasks on the shell's runtime and report through
//! the core's notifier.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use nail_api::desktop::{
    DefaultAppsStatus, DesktopChange, DesktopHost, DesktopNotifier, DesktopSettings,
    HostUpdateState, InstallationInfo, UpdateSettings, default_apps_settings_uri,
};
use nail_api::{ApiError, ErrorCode, LocalizedMessage};
use nail_core::Service;

use crate::dev_endpoint::DevEndpoint;
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
            runtime: parts.runtime,
            opener: parts.opener,
            notify: OnceLock::new(),
            service: OnceLock::new(),
            dev: Mutex::new(None),
            close_to_tray: AtomicBool::new(DesktopSettings::default().close_to_tray),
            awaiting_default_apps: AtomicBool::new(false),
        }
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
