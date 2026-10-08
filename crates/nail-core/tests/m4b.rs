//! Wave M4b contract (ADR 0087): the desktop requests are served from the
//! store plus the shell's `DesktopHost` (here a fake): installation facts,
//! the updater (policy wins, state checks, settings stored and applied,
//! host changes broadcast), tray / startup settings with the Run key,
//! default apps, the first run; `unsupported` without a host. Simple MAPI:
//! a program's message becomes a saved draft with its files that the UI
//! opens; nothing is sent; errors map to MAPI codes.

#![cfg(test)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use nail_api::desktop::{
    DefaultAppState, DefaultAppsStatus, DesktopChange, DesktopHost, DesktopNotifier,
    DesktopSettings, HostUpdateState, InstallKind, InstallScope, InstallationInfo, UpdateChannel,
    UpdateDisabledReason, UpdateInfo, UpdateSettings, UpdateState, WindowsArch,
};
use nail_api::mapi::{
    MapiAttachment, MapiMessage, MapiRecipient, MapiRecipientClass, MapiResult, MapiUi,
};
use nail_api::{ApiError, ErrorCode, Event, Request, Response};
use nail_core::demo::start_demo_multi;
use nail_core::{Service, ServiceOptions};
use pretty_assertions::assert_eq;

#[derive(Debug, Default)]
struct FakeState {
    installation: Option<InstallationInfo>,
    update: HostUpdateState,
    applied_updates: Vec<UpdateSettings>,
    applied_desktop: Vec<DesktopSettings>,
    run_key: Option<bool>,
    run_key_managed: bool,
    calls: Vec<&'static str>,
}

#[derive(Default)]
struct FakeHost {
    state: Mutex<FakeState>,
    notify: Mutex<Option<DesktopNotifier>>,
}

impl std::fmt::Debug for FakeHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FakeHost")
    }
}

impl FakeHost {
    fn with(installation: InstallationInfo) -> Arc<Self> {
        let host = Self::default();
        host.state.lock().unwrap().installation = Some(installation);
        Arc::new(host)
    }
    fn set_update(&self, state: UpdateState) {
        self.state.lock().unwrap().update.state = state;
    }
    fn fire(&self, change: DesktopChange) {
        let notify = self.notify.lock().unwrap().clone().expect("attached");
        notify(change);
    }
    fn calls(&self) -> Vec<&'static str> {
        self.state.lock().unwrap().calls.clone()
    }
}

impl DesktopHost for FakeHost {
    fn attach(&self, notify: DesktopNotifier) {
        *self.notify.lock().unwrap() = Some(notify);
    }
    fn installation(&self) -> InstallationInfo {
        self.state.lock().unwrap().installation.clone().unwrap()
    }
    fn update_state(&self) -> HostUpdateState {
        self.state.lock().unwrap().update.clone()
    }
    fn apply_update_settings(&self, settings: &UpdateSettings) {
        self.state
            .lock()
            .unwrap()
            .applied_updates
            .push(settings.clone());
    }
    fn check_for_updates(&self) -> Result<(), ApiError> {
        let mut state = self.state.lock().unwrap();
        state.calls.push("check");
        state.update.state = UpdateState::Checking;
        Ok(())
    }
    fn download_update(&self) -> Result<(), ApiError> {
        self.state.lock().unwrap().calls.push("download");
        Ok(())
    }
    fn install_update(&self) -> Result<(), ApiError> {
        self.state.lock().unwrap().calls.push("install");
        Ok(())
    }
    fn start_with_windows(&self) -> Option<bool> {
        self.state.lock().unwrap().run_key
    }
    fn start_with_windows_managed(&self) -> bool {
        self.state.lock().unwrap().run_key_managed
    }
    fn apply_desktop_settings(&self, settings: &DesktopSettings) -> Result<(), ApiError> {
        let mut state = self.state.lock().unwrap();
        state.run_key = Some(settings.start_with_windows);
        state.applied_desktop.push(settings.clone());
        Ok(())
    }
    fn dev_endpoint_url(&self) -> Option<String> {
        let state = self.state.lock().unwrap();
        state
            .applied_desktop
            .last()
            .filter(|s| s.plugin_dev_endpoint)
            .map(|_| "http://127.0.0.1:5123/".to_owned())
    }
    fn default_apps(&self) -> DefaultAppsStatus {
        let mut status = DefaultAppsStatus::unregistered();
        status.registered = true;
        status.associations[0].registered = true;
        status.associations[0].state = DefaultAppState::Default;
        status
    }
    fn open_default_apps_settings(&self) -> Result<(), ApiError> {
        self.state.lock().unwrap().calls.push("open-settings");
        Ok(())
    }
}

fn nsis() -> InstallationInfo {
    InstallationInfo {
        version: "1.0.0".into(),
        kind: InstallKind::Nsis,
        scope: Some(InstallScope::PerUser),
        architecture: WindowsArch::X64,
        install_dir: None,
        updates_disabled: None,
        forced_channel: None,
        mapi_installed: true,
        webview_version: None,
    }
}

fn msi() -> InstallationInfo {
    InstallationInfo {
        kind: InstallKind::Msi,
        scope: Some(InstallScope::PerMachine),
        updates_disabled: Some(UpdateDisabledReason::MsiManaged),
        forced_channel: Some(UpdateChannel::Stable),
        ..nsis()
    }
}

fn update() -> UpdateInfo {
    UpdateInfo {
        version: "1.1.0".into(),
        channel: UpdateChannel::Stable,
        published_at: None,
        notes: Some("Faster search".into()),
        size_bytes: None,
    }
}

fn open(host: Option<Arc<FakeHost>>) -> (Service, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    options.start_engines = false;
    options.desktop = host.map(|h| -> Arc<dyn DesktopHost> { h });
    let service = Service::open(nail_core::demo::demo_options(options)).unwrap();
    (service, data, config)
}

fn error_of(response: Response) -> (ErrorCode, Option<String>) {
    match response {
        Response::Error {
            code, localized, ..
        } => (code, localized.map(|l| l.id)),
        other => panic!("error expected, got {other:?}"),
    }
}

fn status_of(response: Response) -> nail_api::UpdateStatus {
    match response {
        Response::UpdateStatus { status } => *status,
        other => panic!("status expected, got {other:?}"),
    }
}

async fn next_event(events: &mut tokio::sync::broadcast::Receiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn without_the_desktop_only_the_first_run_is_served() {
    let (service, _d, _c) = open(None);
    for request in [
        Request::GetInstallation,
        Request::GetUpdateStatus,
        Request::CheckForUpdates,
        Request::GetDesktopSettings,
        Request::GetDefaultApps,
        Request::OpenDefaultAppsSettings,
    ] {
        assert_eq!(
            error_of(service.handle(request).await),
            (ErrorCode::Unsupported, Some("desktop-unavailable".into()))
        );
    }
    let Response::FirstRun { state } = service.handle(Request::GetFirstRun).await else {
        panic!()
    };
    assert!(!state.completed);
    let Response::FirstRun { state } = service.handle(Request::CompleteFirstRun).await else {
        panic!()
    };
    assert!(state.completed && state.completed_at.is_some());
    let Response::FirstRun { state: again } = service.handle(Request::GetFirstRun).await else {
        panic!()
    };
    assert_eq!(again, state);
}

#[tokio::test]
async fn the_updater_follows_settings_and_states() {
    let host = FakeHost::with(nsis());
    let (service, _d, _c) = open(Some(host.clone()));
    // The stored (default) settings reach the host when the core opens.
    assert_eq!(
        host.state.lock().unwrap().applied_updates,
        vec![UpdateSettings::default()]
    );
    let Response::Installation { info } = service.handle(Request::GetInstallation).await else {
        panic!()
    };
    assert_eq!(*info, nsis());

    let status = status_of(service.handle(Request::GetUpdateStatus).await);
    assert_eq!(status.state, UpdateState::Idle);
    assert_eq!(status.current_version, "1.0.0");

    let mut events = service.subscribe();
    let beta = UpdateSettings {
        channel: UpdateChannel::Beta,
        auto_download: false,
        ..UpdateSettings::default()
    };
    let status = status_of(
        service
            .handle(Request::SetUpdateSettings {
                settings: beta.clone(),
            })
            .await,
    );
    assert_eq!(status.settings, beta);
    assert!(matches!(
        next_event(&mut events).await,
        Event::UpdateStatusChanged { .. }
    ));
    assert_eq!(
        host.state.lock().unwrap().applied_updates.last(),
        Some(&beta)
    );

    // Nothing to download or install yet.
    assert_eq!(
        error_of(service.handle(Request::DownloadUpdate).await).0,
        ErrorCode::Conflict
    );
    assert_eq!(
        error_of(service.handle(Request::InstallUpdate).await).0,
        ErrorCode::Conflict
    );
    let status = status_of(service.handle(Request::CheckForUpdates).await);
    assert_eq!(status.state, UpdateState::Checking);
    assert_eq!(
        error_of(service.handle(Request::CheckForUpdates).await).0,
        ErrorCode::Conflict
    );

    // The host found one and reports it.
    host.set_update(UpdateState::Available { update: update() });
    host.fire(DesktopChange::Update);
    let Event::UpdateStatusChanged { status } = next_event(&mut events).await else {
        panic!()
    };
    assert_eq!(status.state, UpdateState::Available { update: update() });
    assert_eq!(status.settings.channel, UpdateChannel::Beta);
    status_of(service.handle(Request::DownloadUpdate).await);
    host.set_update(UpdateState::ReadyToInstall { update: update() });
    assert_eq!(service.handle(Request::InstallUpdate).await, Response::Done);
    assert_eq!(host.calls(), ["check", "download", "install"]);
}

#[tokio::test]
async fn policy_disables_the_updater_and_locks_the_channel() {
    let host = FakeHost::with(msi());
    host.set_update(UpdateState::Available { update: update() });
    let (service, _d, _c) = open(Some(host.clone()));
    let status = status_of(service.handle(Request::GetUpdateStatus).await);
    assert_eq!(
        status.state,
        UpdateState::Disabled {
            reason: UpdateDisabledReason::MsiManaged
        }
    );
    assert_eq!(status.forced_channel, Some(UpdateChannel::Stable));
    for request in [
        Request::CheckForUpdates,
        Request::DownloadUpdate,
        Request::InstallUpdate,
    ] {
        assert_eq!(
            error_of(service.handle(request).await),
            (
                ErrorCode::Forbidden,
                Some("update-disabled-msi-managed".into())
            )
        );
    }
    assert_eq!(
        error_of(
            service
                .handle(Request::SetUpdateSettings {
                    settings: UpdateSettings {
                        channel: UpdateChannel::Beta,
                        ..UpdateSettings::default()
                    }
                })
                .await
        ),
        (ErrorCode::Forbidden, Some("update-channel-locked".into()))
    );
    assert_eq!(host.calls().len(), 0);
}

#[tokio::test]
async fn desktop_settings_are_stored_and_applied() {
    let host = FakeHost::with(nsis());
    let (service, _d, _c) = open(Some(host.clone()));
    let Response::DesktopSettings {
        settings,
        dev_endpoint_url,
        start_with_windows_managed,
    } = service.handle(Request::GetDesktopSettings).await
    else {
        panic!()
    };
    assert_eq!(settings, DesktopSettings::default());
    assert_eq!(dev_endpoint_url, None);
    assert!(!start_with_windows_managed);
    // The MSI's machine-wide Run key: the UI learns why the switch stays on.
    host.state.lock().unwrap().run_key_managed = true;
    let Response::DesktopSettings {
        start_with_windows_managed,
        ..
    } = service.handle(Request::GetDesktopSettings).await
    else {
        panic!()
    };
    assert!(start_with_windows_managed);
    host.state.lock().unwrap().run_key_managed = false;

    // The installer's Run key wins over the stored value.
    host.state.lock().unwrap().run_key = Some(true);
    assert!(service.desktop_settings().await.unwrap().start_with_windows);

    let mut events = service.subscribe();
    let wanted = DesktopSettings {
        close_to_tray: false,
        start_hidden: false,
        start_with_windows: false,
        plugin_dev_endpoint: true,
    };
    let Response::DesktopSettings {
        settings,
        dev_endpoint_url,
        ..
    } = service
        .handle(Request::SetDesktopSettings {
            settings: wanted.clone(),
        })
        .await
    else {
        panic!()
    };
    assert_eq!(settings, wanted);
    assert_eq!(dev_endpoint_url.as_deref(), Some("http://127.0.0.1:5123/"));
    assert_eq!(next_event(&mut events).await, Event::DesktopSettingsChanged);
    assert_eq!(host.state.lock().unwrap().applied_desktop, vec![wanted]);

    host.fire(DesktopChange::DefaultApps);
    assert_eq!(next_event(&mut events).await, Event::DefaultAppsChanged);
    host.fire(DesktopChange::Settings);
    assert_eq!(next_event(&mut events).await, Event::DesktopSettingsChanged);
}

#[tokio::test]
async fn default_apps_come_from_the_host() {
    let host = FakeHost::with(msi());
    let (service, _d, _c) = open(Some(host.clone()));
    let Response::DefaultApps {
        status,
        settings_uri,
    } = service.handle(Request::GetDefaultApps).await
    else {
        panic!()
    };
    assert!(status.registered && status.is_default_mail_app());
    assert_eq!(
        settings_uri,
        "ms-settings:defaultapps?registeredAppMachine=Nail"
    );
    assert_eq!(
        service.handle(Request::OpenDefaultAppsSettings).await,
        Response::Done
    );
    assert_eq!(host.calls(), ["open-settings"]);
}

fn mapi_message(attachments: Vec<MapiAttachment>) -> MapiMessage {
    MapiMessage {
        subject: Some("Quarterly report".into()),
        body: Some("Please find the report attached.".into()),
        recipients: vec![
            MapiRecipient {
                class: MapiRecipientClass::To,
                name: Some("Ada".into()),
                address: Some("SMTP:ada@example.org".into()),
            },
            MapiRecipient {
                class: MapiRecipientClass::Bcc,
                name: None,
                address: Some("boss@example.org".into()),
            },
        ],
        attachments,
        ui: MapiUi::None,
        program: Some("EXCEL.EXE".into()),
    }
}

#[tokio::test]
async fn a_mapi_message_becomes_a_draft_the_ui_opens() {
    let demo = start_demo_multi().await.unwrap();
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    demo.write_config(config.path()).unwrap();
    let mut options = demo.service_options(ServiceOptions::new(data.path(), config.path()));
    options.start_engines = false;
    let service = Service::open(options).unwrap();

    let files = tempfile::tempdir().unwrap();
    let path = files.path().join("~report.tmp");
    std::fs::write(&path, b"col1,col2\n1,2\n").unwrap();
    let message = mapi_message(vec![MapiAttachment {
        path: path.display().to_string(),
        file_name: Some("Report.csv".into()),
    }]);
    let mut events = service.subscribe();
    assert_eq!(service.submit_mapi(message).await, MapiResult::Success);
    // The caller may delete its file now: the draft has the content.
    std::fs::remove_file(&path).unwrap();
    assert_eq!(next_event(&mut events).await, Event::LaunchRequested);
    let (draft, program) = nail_core::demo::take_program_draft(&service).await.unwrap();
    assert_eq!(program.as_deref(), Some("EXCEL.EXE"));
    assert_eq!(draft.subject, "Quarterly report");
    assert_eq!(draft.to[0].email, "ada@example.org");
    assert_eq!(draft.to[0].name.as_deref(), Some("Ada"));
    assert_eq!(draft.bcc[0].email, "boss@example.org");
    assert!(draft.text.starts_with("Please find the report attached."));
    assert_eq!(draft.attachments.len(), 1);
    assert_eq!(draft.attachments[0].filename, "Report.csv");
    assert_ne!(draft.identity_id.as_str().len(), 0);
    // Nothing was queued for sending.
    let Response::Outbox { entries } = service.handle(Request::ListOutbox).await else {
        panic!()
    };
    assert!(entries.is_empty(), "{entries:?}");
}

#[tokio::test]
async fn mapi_errors_map_to_mapi_codes() {
    let (service, _d, _c) = open(None);
    let files = tempfile::tempdir().unwrap();
    let missing = files.path().join("gone.pdf");
    assert_eq!(
        service
            .submit_mapi(mapi_message(vec![MapiAttachment {
                path: missing.display().to_string(),
                file_name: None,
            }]))
            .await,
        MapiResult::AttachmentNotFound
    );
    assert_eq!(
        service
            .submit_mapi(mapi_message(vec![MapiAttachment {
                path: files.path().display().to_string(),
                file_name: None,
            }]))
            .await,
        MapiResult::AttachmentOpenFailure
    );
    let mut bad = mapi_message(vec![]);
    bad.recipients[0].address = Some("not an address".into());
    bad.recipients[0].name = None;
    assert_eq!(service.submit_mapi(bad).await, MapiResult::InvalidRecips);
    // No account to write from.
    assert_eq!(
        service.submit_mapi(mapi_message(vec![])).await,
        MapiResult::LoginFailure
    );
    assert_eq!(
        service.handle(Request::TakeLaunchRequests).await,
        Response::LaunchRequests { requests: vec![] }
    );
}
