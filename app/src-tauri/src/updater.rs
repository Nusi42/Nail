//! The in-app updater's state machine (M4b WP113, ADR 0087).
//!
//! [`Updater`] runs the states of [`UpdateState`]: a check (on start, then
//! every [`UPDATE_CHECK_INTERVAL_SECS`] when `auto_check` is on, or on
//! request) finds a release → it downloads in the background when
//! `auto_download` is on → ready to install, which happens when the user
//! picks "Restart to install" or when Nail quits. Failures land in
//! `failed` and the next check starts over — except that a downloaded
//! release stays ready: a later check that fails, finds nothing or finds
//! the same version returns to `ready-to-install` and never downloads it
//! again; only a different version replaces it.
//!
//! Changes are announced one at a time (`Shared::notify_order`): the
//! core's notifier reads the state and then broadcasts it, so two
//! announcements racing on different threads could otherwise send an
//! older state after a newer one and leave the UI on it.
//!
//! The network side is an [`UpdateBackend`] — the real one is
//! [`crate::updater_tauri`] (tauri-plugin-updater: signed manifests and
//! packages, minisign, rustls + ring) — so the machine is tested with a
//! scripted backend. A disabled updater (MSI-managed, policy, portable,
//! development, not configured) never checks; the core reports it as
//! `disabled` whatever this machine says (`UpdateStatus::compose`).

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures_util::future::BoxFuture;
use nail_api::desktop::{
    DesktopChange, DesktopNotifier, HostUpdateState, UPDATE_CHECK_INTERVAL_SECS, UpdateChannel,
    UpdateFailure, UpdateInfo, UpdateSettings, UpdateState,
};
use nail_api::{ApiError, ErrorCode};

/// A failed backend step: what failed and an English detail for logs.
pub type BackendError = (UpdateFailure, String);

/// A release the backend found, ready to download and install.
pub trait PendingUpdate: Send + Sync {
    /// What the UI shows.
    fn info(&self) -> &UpdateInfo;
    /// Downloads and verifies the package; `progress(downloaded, total)`.
    fn download(
        &self,
        progress: Box<dyn FnMut(u64, Option<u64>) + Send>,
    ) -> BoxFuture<'static, Result<Vec<u8>, BackendError>>;
    /// Runs the installer (on Windows this ends the process; `restart`
    /// starts Nail again afterwards).
    fn install(&self, bytes: &[u8], restart: bool) -> Result<(), BackendError>;
}

/// Where updates come from.
pub trait UpdateBackend: Send + Sync {
    /// Asks the channel's manifest for a newer release (`None` = up to date).
    fn check(
        &self,
        channel: UpdateChannel,
    ) -> BoxFuture<'static, Result<Option<Arc<dyn PendingUpdate>>, BackendError>>;
}

/// Timing of the automatic checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    /// First check after start.
    pub initial_delay: Duration,
    /// Between checks.
    pub interval: Duration,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(20),
            interval: Duration::from_secs(UPDATE_CHECK_INTERVAL_SECS),
        }
    }
}

/// A downloaded release and its package.
type Ready = (Arc<dyn PendingUpdate>, Arc<Vec<u8>>);

/// How long `InstallUpdate` waits before the installer ends the process,
/// so the request's answer reaches the UI.
const INSTALL_DELAY: Duration = Duration::from_millis(400);

struct State {
    host: HostUpdateState,
    settings: UpdateSettings,
    pending: Option<Arc<dyn PendingUpdate>>,
    bytes: Option<Arc<Vec<u8>>>,
    /// Bumped when the channel changes: results of older work are dropped.
    generation: u64,
}

struct Shared {
    backend: Arc<dyn UpdateBackend>,
    runtime: tokio::runtime::Handle,
    enabled: bool,
    forced_channel: Option<UpdateChannel>,
    state: Mutex<State>,
    notify: OnceLock<DesktopNotifier>,
    /// Held while announcing a change, so announcements never overtake
    /// each other: the last one always carries the latest state.
    notify_order: Mutex<()>,
}

/// The updater (cheap to clone).
#[derive(Clone)]
pub struct Updater {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Updater {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Updater")
            .field("enabled", &self.shared.enabled)
            .field("state", &self.lock().host.state)
            .finish_non_exhaustive()
    }
}

fn conflict(what: &str) -> ApiError {
    ApiError::new(ErrorCode::Conflict, format!("{what} does not apply now"))
}

impl Updater {
    /// A new updater. `enabled` = the installation allows updates (else it
    /// never checks); `forced_channel` = the policy's channel.
    pub fn new(
        backend: Arc<dyn UpdateBackend>,
        runtime: tokio::runtime::Handle,
        enabled: bool,
        forced_channel: Option<UpdateChannel>,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                backend,
                runtime,
                enabled,
                forced_channel,
                state: Mutex::new(State {
                    host: HostUpdateState::default(),
                    settings: UpdateSettings::default(),
                    pending: None,
                    bytes: None,
                    generation: 0,
                }),
                notify: OnceLock::new(),
                notify_order: Mutex::new(()),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Keeps the core's notifier.
    pub fn attach(&self, notify: DesktopNotifier) {
        drop(self.shared.notify.set(notify));
    }

    fn changed(&self) {
        if let Some(notify) = self.shared.notify.get() {
            // The notifier reads the state itself (after a store read that
            // may be slow): one at a time, so a newer state announced on
            // another thread is never followed by an older one.
            let _order = self
                .shared
                .notify_order
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            notify(DesktopChange::Update);
        }
    }

    /// The current state.
    pub fn state(&self) -> HostUpdateState {
        self.lock().host.clone()
    }

    /// Whether the installation allows updates.
    pub fn enabled(&self) -> bool {
        self.shared.enabled
    }

    /// The channel in effect (the policy's, else the user's).
    pub fn channel(&self) -> UpdateChannel {
        self.shared
            .forced_channel
            .unwrap_or(self.lock().settings.channel)
    }

    /// New settings from the core. A changed channel drops what the old
    /// one found and checks again (when automatic checks are on).
    pub fn apply_settings(&self, settings: &UpdateSettings) {
        let recheck = {
            let mut state = self.lock();
            let before = self.shared.forced_channel.unwrap_or(state.settings.channel);
            state.settings = settings.clone();
            let after = self.shared.forced_channel.unwrap_or(settings.channel);
            let busy = matches!(
                state.host.state,
                UpdateState::Checking
                    | UpdateState::Downloading { .. }
                    | UpdateState::Installing { .. }
            );
            if before != after && !busy {
                state.generation += 1;
                state.pending = None;
                state.bytes = None;
                state.host.state = UpdateState::Idle;
                true
            } else {
                false
            }
        };
        if recheck {
            self.changed();
            if settings.auto_check && self.shared.enabled {
                drop(self.check());
            }
        }
    }

    /// Starts a check (`conflict` while one runs or when disabled).
    pub fn check(&self) -> Result<(), ApiError> {
        if !self.shared.enabled {
            return Err(conflict("CheckForUpdates"));
        }
        let (channel, generation) = {
            let mut state = self.lock();
            if !state.host.state.can_check() {
                return Err(conflict("CheckForUpdates"));
            }
            state.host.state = UpdateState::Checking;
            (
                self.shared.forced_channel.unwrap_or(state.settings.channel),
                state.generation,
            )
        };
        self.changed();
        let this = self.clone();
        let check = self.shared.backend.check(channel);
        self.shared.runtime.spawn(async move {
            let result = check.await;
            this.check_finished(generation, result);
        });
        Ok(())
    }

    fn check_finished(
        &self,
        generation: u64,
        result: Result<Option<Arc<dyn PendingUpdate>>, BackendError>,
    ) {
        let download = {
            let mut state = self.lock();
            if state.generation != generation {
                return;
            }
            state.host.last_checked_at = Some(rfc3339_now());
            // A release already downloaded stays ready unless the check
            // found a different version: never download it again.
            let ready = state
                .pending
                .as_ref()
                .filter(|_| state.bytes.is_some())
                .map(|p| p.info().clone());
            match (result, ready) {
                (Ok(None), Some(update)) => {
                    state.host.state = UpdateState::ReadyToInstall { update };
                    false
                }
                (Err((failure, detail)), Some(update)) => {
                    tracing::warn!(?failure, %detail, "update check failed; the downloaded release stays ready");
                    state.host.state = UpdateState::ReadyToInstall { update };
                    false
                }
                (Ok(None), None) => {
                    state.pending = None;
                    state.bytes = None;
                    state.host.state = UpdateState::UpToDate;
                    false
                }
                (Ok(Some(found)), ready) => {
                    let info = found.info().clone();
                    if let Some(update) = ready.filter(|r| r.version == info.version) {
                        state.host.state = UpdateState::ReadyToInstall { update };
                        false
                    } else {
                        state.pending = Some(found);
                        state.bytes = None;
                        state.host.state = UpdateState::Available { update: info };
                        state.settings.auto_download
                    }
                }
                (Err((failure, detail)), None) => {
                    tracing::warn!(?failure, %detail, "update check failed");
                    state.pending = None;
                    state.bytes = None;
                    state.host.state = UpdateState::Failed {
                        failure,
                        detail: Some(detail),
                        update: None,
                    };
                    false
                }
            }
        };
        self.changed();
        if download {
            drop(self.download());
        }
    }

    /// Starts downloading the available release.
    pub fn download(&self) -> Result<(), ApiError> {
        let (pending, generation) = {
            let mut state = self.lock();
            let (UpdateState::Available { update }, Some(pending)) =
                (&state.host.state, state.pending.clone())
            else {
                return Err(conflict("DownloadUpdate"));
            };
            state.host.state = UpdateState::Downloading {
                update: update.clone(),
                downloaded_bytes: 0,
                total_bytes: update.size_bytes,
            };
            (pending, state.generation)
        };
        self.changed();
        let this = self.clone();
        let progress_target = self.clone();
        let mut downloaded = 0u64;
        let mut reported = 0u64;
        let download = pending.download(Box::new(move |chunk, total| {
            downloaded += chunk;
            // Report every 1 % (or 256 KiB without a total).
            let step = total.map_or(256 * 1024, |t| (t / 100).max(1));
            if downloaded - reported >= step || total == Some(downloaded) {
                reported = downloaded;
                progress_target.progress(generation, downloaded, total);
            }
        }));
        self.shared.runtime.spawn(async move {
            let result = download.await;
            this.download_finished(generation, &pending, result);
        });
        Ok(())
    }

    fn progress(&self, generation: u64, downloaded: u64, total: Option<u64>) {
        {
            let mut state = self.lock();
            if state.generation != generation {
                return;
            }
            let UpdateState::Downloading { update, .. } = &state.host.state else {
                return;
            };
            state.host.state = UpdateState::Downloading {
                update: update.clone(),
                downloaded_bytes: downloaded,
                total_bytes: total,
            };
        }
        self.changed();
    }

    fn download_finished(
        &self,
        generation: u64,
        pending: &Arc<dyn PendingUpdate>,
        result: Result<Vec<u8>, BackendError>,
    ) {
        {
            let mut state = self.lock();
            if state.generation != generation {
                return;
            }
            let update = pending.info().clone();
            match result {
                Ok(bytes) => {
                    state.bytes = Some(Arc::new(bytes));
                    state.host.state = UpdateState::ReadyToInstall { update };
                }
                Err((failure, detail)) => {
                    tracing::warn!(?failure, %detail, "update download failed");
                    state.bytes = None;
                    state.host.state = UpdateState::Failed {
                        failure,
                        detail: Some(detail),
                        update: Some(update),
                    };
                }
            }
        }
        self.changed();
    }

    /// "Restart to install": runs the installer shortly (after the request
    /// was answered); Nail restarts when it is done.
    pub fn install(&self) -> Result<(), ApiError> {
        let (pending, bytes) = self.take_ready().ok_or_else(|| conflict("InstallUpdate"))?;
        self.changed();
        let this = self.clone();
        self.shared.runtime.spawn(async move {
            tokio::time::sleep(INSTALL_DELAY).await;
            let runner = Arc::clone(&pending);
            let result = tokio::task::spawn_blocking(move || runner.install(&bytes, true))
                .await
                .unwrap_or_else(|e| Err((UpdateFailure::Install, e.to_string())));
            if let Err((failure, detail)) = result {
                this.install_failed(&pending, failure, detail);
            }
        });
        Ok(())
    }

    /// Nail quits with a downloaded release: install it now, without
    /// restarting. Returns whether an installer ran.
    pub fn install_on_exit(&self) -> bool {
        let Some((pending, bytes)) = self.take_ready() else {
            return false;
        };
        match pending.install(&bytes, false) {
            Ok(()) => true,
            Err((failure, detail)) => {
                self.install_failed(&pending, failure, detail);
                false
            }
        }
    }

    fn take_ready(&self) -> Option<Ready> {
        let mut state = self.lock();
        let (UpdateState::ReadyToInstall { update }, Some(pending), Some(bytes)) = (
            state.host.state.clone(),
            state.pending.clone(),
            state.bytes.clone(),
        ) else {
            return None;
        };
        state.host.state = UpdateState::Installing { update };
        Some((pending, bytes))
    }

    fn install_failed(
        &self,
        pending: &Arc<dyn PendingUpdate>,
        failure: UpdateFailure,
        detail: String,
    ) {
        tracing::warn!(?failure, %detail, "the update could not be installed");
        {
            let mut state = self.lock();
            state.bytes = None;
            state.host.state = UpdateState::Failed {
                failure,
                detail: Some(detail),
                update: Some(pending.info().clone()),
            };
        }
        self.changed();
    }

    /// Checks shortly after start and then every interval while automatic
    /// checks are on (a disabled updater never checks).
    pub fn spawn_schedule(&self, schedule: Schedule) -> Option<tokio::task::JoinHandle<()>> {
        if !self.shared.enabled {
            return None;
        }
        let this = self.clone();
        Some(self.shared.runtime.spawn(async move {
            tokio::time::sleep(schedule.initial_delay).await;
            loop {
                if this.lock().settings.auto_check {
                    drop(this.check());
                }
                tokio::time::sleep(schedule.interval).await;
            }
        }))
    }
}

/// Now as RFC 3339 (UTC, seconds).
pub fn rfc3339_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    rfc3339(secs)
}

/// Unix seconds as RFC 3339 UTC (`1970-01-01T00:00:00Z`).
pub fn rfc3339(secs: u64) -> String {
    nail_util::time::rfc3339_utc(i64::try_from(secs).unwrap_or(i64::MAX))
}

#[cfg(test)]
pub(crate) mod testing {
    //! A scripted backend.
    use super::*;
    use std::collections::VecDeque;

    /// What a fake release does.
    pub(crate) struct FakeUpdate {
        pub info: UpdateInfo,
        pub download: Mutex<Option<Result<Vec<u8>, BackendError>>>,
        pub install_result: Mutex<Option<BackendError>>,
        pub installs: Mutex<Vec<(usize, bool)>>,
        /// Holds the download until notified.
        pub gate: Option<Arc<tokio::sync::Notify>>,
    }

    impl FakeUpdate {
        pub(crate) fn new(version: &str, channel: UpdateChannel) -> Arc<Self> {
            Arc::new(Self {
                info: UpdateInfo {
                    version: version.into(),
                    channel,
                    published_at: None,
                    notes: Some("Fixes".into()),
                    size_bytes: Some(1000),
                },
                download: Mutex::new(Some(Ok(vec![7; 1000]))),
                install_result: Mutex::new(None),
                installs: Mutex::new(Vec::new()),
                gate: None,
            })
        }
    }

    impl PendingUpdate for FakeUpdate {
        fn info(&self) -> &UpdateInfo {
            &self.info
        }
        fn download(
            &self,
            mut progress: Box<dyn FnMut(u64, Option<u64>) + Send>,
        ) -> BoxFuture<'static, Result<Vec<u8>, BackendError>> {
            let result = self
                .download
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| Err((UpdateFailure::Download, "used".into())));
            let gate = self.gate.clone();
            Box::pin(async move {
                if let Some(gate) = gate {
                    progress(400, Some(1000));
                    gate.notified().await;
                }
                if let Ok(bytes) = &result {
                    progress(bytes.len() as u64, Some(1000));
                }
                result
            })
        }
        fn install(&self, bytes: &[u8], restart: bool) -> Result<(), BackendError> {
            self.installs.lock().unwrap().push((bytes.len(), restart));
            match self.install_result.lock().unwrap().clone() {
                Some(err) => Err(err),
                None => Ok(()),
            }
        }
    }

    /// Answers checks from a script; records the channels asked.
    #[derive(Default)]
    pub(crate) struct FakeBackend {
        pub answers: Mutex<VecDeque<Result<Option<Arc<FakeUpdate>>, BackendError>>>,
        pub asked: Mutex<Vec<UpdateChannel>>,
    }

    impl FakeBackend {
        pub(crate) fn answer(&self, answer: Result<Option<Arc<FakeUpdate>>, BackendError>) {
            self.answers.lock().unwrap().push_back(answer);
        }
    }

    impl UpdateBackend for FakeBackend {
        fn check(
            &self,
            channel: UpdateChannel,
        ) -> BoxFuture<'static, Result<Option<Arc<dyn PendingUpdate>>, BackendError>> {
            self.asked.lock().unwrap().push(channel);
            let answer = self
                .answers
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(None))
                .map(|found| found.map(|f| -> Arc<dyn PendingUpdate> { f }));
            Box::pin(async move { answer })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{FakeBackend, FakeUpdate};
    use super::*;
    use pretty_assertions::assert_eq;

    fn updater(backend: &Arc<FakeBackend>, enabled: bool) -> (Updater, Arc<Mutex<u32>>) {
        let updater = Updater::new(
            backend.clone(),
            tokio::runtime::Handle::current(),
            enabled,
            None,
        );
        let changes = Arc::new(Mutex::new(0u32));
        let counter = Arc::clone(&changes);
        updater.attach(Arc::new(move |change| {
            assert_eq!(change, DesktopChange::Update);
            *counter.lock().unwrap() += 1;
        }));
        (updater, changes)
    }

    async fn wait_for(updater: &Updater, done: impl Fn(&UpdateState) -> bool) -> UpdateState {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let state = updater.state().state;
                if done(&state) {
                    return state;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the updater reached the state")
    }

    #[tokio::test]
    async fn check_download_and_install_on_restart() {
        let backend = Arc::new(FakeBackend::default());
        let release = FakeUpdate::new("1.1.0", UpdateChannel::Stable);
        backend.answer(Ok(Some(Arc::clone(&release))));
        let (updater, changes) = updater(&backend, true);
        updater.apply_settings(&UpdateSettings::default());
        assert_eq!(updater.state().state, UpdateState::Idle);
        updater.check().unwrap();
        // Automatic download: straight on to ready.
        let state = wait_for(&updater, |s| {
            matches!(s, UpdateState::ReadyToInstall { .. })
        })
        .await;
        assert_eq!(state.update().unwrap().version, "1.1.0");
        assert!(updater.state().last_checked_at.is_some());
        assert!(
            *changes.lock().unwrap() >= 4,
            "checking, available, downloading, ready"
        );
        assert!(updater.download().is_err(), "nothing left to download");

        updater.install().unwrap();
        assert!(matches!(
            updater.state().state,
            UpdateState::Installing { .. }
        ));
        tokio::time::timeout(Duration::from_secs(5), async {
            while release.installs.lock().unwrap().is_empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(*release.installs.lock().unwrap(), [(1000, true)]);
        assert!(updater.install().is_err(), "installing once");
    }

    #[tokio::test]
    async fn without_auto_download_the_user_starts_it() {
        let backend = Arc::new(FakeBackend::default());
        let mut release = FakeUpdate::new("1.1.0", UpdateChannel::Beta);
        let gate = Arc::new(tokio::sync::Notify::new());
        Arc::get_mut(&mut release).unwrap().gate = Some(Arc::clone(&gate));
        backend.answer(Ok(Some(Arc::clone(&release))));
        let (updater, _) = updater(&backend, true);
        updater.apply_settings(&UpdateSettings {
            channel: UpdateChannel::Beta,
            auto_check: false,
            auto_download: false,
        });
        updater.check().unwrap();
        wait_for(&updater, |s| matches!(s, UpdateState::Available { .. })).await;
        assert_eq!(*backend.asked.lock().unwrap(), [UpdateChannel::Beta]);
        updater.download().unwrap();
        // Progress while downloading.
        let state = wait_for(&updater, |s| {
            matches!(s, UpdateState::Downloading { downloaded_bytes, .. } if *downloaded_bytes > 0)
        })
        .await;
        let UpdateState::Downloading {
            downloaded_bytes,
            total_bytes,
            ..
        } = state
        else {
            unreachable!()
        };
        assert_eq!((downloaded_bytes, total_bytes), (400, Some(1000)));
        assert!(updater.check().is_err(), "no check while downloading");
        gate.notify_one();
        wait_for(&updater, |s| {
            matches!(s, UpdateState::ReadyToInstall { .. })
        })
        .await;

        // Quitting installs it without a restart.
        assert!(updater.install_on_exit());
        assert_eq!(*release.installs.lock().unwrap(), [(1000, false)]);
        assert!(!updater.install_on_exit());
    }

    #[tokio::test]
    async fn failures_are_reported_and_the_next_check_starts_over() {
        let backend = Arc::new(FakeBackend::default());
        backend.answer(Err((UpdateFailure::Network, "offline".into())));
        let broken = FakeUpdate::new("1.1.0", UpdateChannel::Stable);
        *broken.download.lock().unwrap() = Some(Err((UpdateFailure::Signature, "bad sig".into())));
        backend.answer(Ok(Some(Arc::clone(&broken))));
        backend.answer(Ok(None));
        let (updater, _) = updater(&backend, true);
        updater.apply_settings(&UpdateSettings::default());

        updater.check().unwrap();
        let state = wait_for(&updater, |s| matches!(s, UpdateState::Failed { .. })).await;
        assert_eq!(
            state,
            UpdateState::Failed {
                failure: UpdateFailure::Network,
                detail: Some("offline".into()),
                update: None
            }
        );
        updater.check().unwrap();
        let state = wait_for(&updater, |s| {
            matches!(
                s,
                UpdateState::Failed {
                    failure: UpdateFailure::Signature,
                    ..
                }
            )
        })
        .await;
        assert_eq!(
            state.update().unwrap().version,
            "1.1.0",
            "names the release"
        );
        assert!(
            broken.installs.lock().unwrap().is_empty(),
            "nothing installed"
        );
        updater.check().unwrap();
        wait_for(&updater, |s| *s == UpdateState::UpToDate).await;
    }

    #[tokio::test]
    async fn a_ready_release_stays_ready_and_is_never_downloaded_again() {
        let backend = Arc::new(FakeBackend::default());
        let release = FakeUpdate::new("1.1.0", UpdateChannel::Stable);
        backend.answer(Ok(Some(Arc::clone(&release))));
        // Later checks: the same version, a failure, nothing found.
        backend.answer(Ok(Some(Arc::clone(&release))));
        backend.answer(Err((UpdateFailure::Network, "offline".into())));
        backend.answer(Ok(None));
        let (updater, _) = updater(&backend, true);
        updater.apply_settings(&UpdateSettings::default());
        updater.check().unwrap();
        wait_for(&updater, |s| {
            matches!(s, UpdateState::ReadyToInstall { .. })
        })
        .await;
        // A second download would fail: none may start.
        *release.download.lock().unwrap() = None;
        for _ in 0..3 {
            updater.check().unwrap();
            let state = wait_for(&updater, |s| *s != UpdateState::Checking).await;
            assert_eq!(state.update().unwrap().version, "1.1.0");
            assert!(
                matches!(state, UpdateState::ReadyToInstall { .. }),
                "still ready, not {state:?}"
            );
        }
        assert_eq!(backend.asked.lock().unwrap().len(), 4);
        assert!(updater.install_on_exit());
        assert_eq!(*release.installs.lock().unwrap(), [(1000, false)]);
    }

    #[tokio::test]
    async fn a_different_version_replaces_the_ready_one() {
        let backend = Arc::new(FakeBackend::default());
        let first = FakeUpdate::new("1.1.0", UpdateChannel::Stable);
        let second = FakeUpdate::new("1.2.0", UpdateChannel::Stable);
        backend.answer(Ok(Some(Arc::clone(&first))));
        backend.answer(Ok(Some(Arc::clone(&second))));
        let (updater, _) = updater(&backend, true);
        updater.apply_settings(&UpdateSettings::default());
        updater.check().unwrap();
        wait_for(&updater, |s| {
            matches!(s, UpdateState::ReadyToInstall { .. })
        })
        .await;
        updater.check().unwrap();
        let state = wait_for(
            &updater,
            |s| matches!(s, UpdateState::ReadyToInstall { update } if update.version == "1.2.0"),
        )
        .await;
        assert_eq!(state.update().unwrap().version, "1.2.0");
        assert!(updater.install_on_exit());
        assert!(first.installs.lock().unwrap().is_empty());
        assert_eq!(*second.installs.lock().unwrap(), [(1000, false)]);
    }

    /// The core's notifier reads the state, then broadcasts it (the store
    /// read before can be slow). A change announced on another thread
    /// meanwhile must not be followed by the older state: the desktop e2e
    /// once ended on "downloading 100 %" after "ready to install".
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn announcements_never_overtake_each_other() {
        use std::sync::mpsc;
        let backend = Arc::new(FakeBackend::default());
        let updater = Updater::new(backend, tokio::runtime::Handle::current(), true, None);
        let broadcasts = Arc::new(Mutex::new(Vec::<UpdateState>::new()));
        let (entered_tx, entered_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let slow = Mutex::new(Some((entered_tx, release_rx)));
        let reader = updater.clone();
        let record = Arc::clone(&broadcasts);
        updater.attach(Arc::new(move |_| {
            let state = reader.state().state;
            // The first announcement reads, then stalls before sending.
            let stall = slow.lock().unwrap().take();
            if let Some((entered, release)) = stall {
                entered.send(()).unwrap();
                release.recv().unwrap();
            }
            record.lock().unwrap().push(state);
        }));
        let update = FakeUpdate::new("1.1.0", UpdateChannel::Stable).info.clone();
        let announce = |state: UpdateState| {
            updater.lock().host.state = state;
            updater.changed();
        };
        std::thread::scope(|scope| {
            let downloading = UpdateState::Downloading {
                update: update.clone(),
                downloaded_bytes: 1000,
                total_bytes: Some(1000),
            };
            let older = scope.spawn(|| announce(downloading));
            entered_rx.recv().unwrap();
            let newer = scope.spawn(|| {
                announce(UpdateState::ReadyToInstall {
                    update: update.clone(),
                });
            });
            // Give the newer announcement time to overtake (it must wait).
            std::thread::sleep(Duration::from_millis(100));
            release_tx.send(()).unwrap();
            older.join().unwrap();
            newer.join().unwrap();
        });
        let broadcasts = broadcasts.lock().unwrap();
        assert!(
            matches!(broadcasts.last(), Some(UpdateState::ReadyToInstall { .. })),
            "the last announcement is the latest state: {broadcasts:?}"
        );
    }

    #[tokio::test]
    async fn a_failed_installer_is_reported() {
        let backend = Arc::new(FakeBackend::default());
        let release = FakeUpdate::new("1.1.0", UpdateChannel::Stable);
        *release.install_result.lock().unwrap() =
            Some((UpdateFailure::Install, "cancelled".into()));
        backend.answer(Ok(Some(Arc::clone(&release))));
        let (updater, _) = updater(&backend, true);
        updater.apply_settings(&UpdateSettings::default());
        updater.check().unwrap();
        wait_for(&updater, |s| {
            matches!(s, UpdateState::ReadyToInstall { .. })
        })
        .await;
        updater.install().unwrap();
        let state = wait_for(&updater, |s| matches!(s, UpdateState::Failed { .. })).await;
        assert!(matches!(
            state,
            UpdateState::Failed {
                failure: UpdateFailure::Install,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn a_disabled_updater_never_checks() {
        let backend = Arc::new(FakeBackend::default());
        let (updater, _) = updater(&backend, false);
        updater.apply_settings(&UpdateSettings::default());
        assert!(updater.check().is_err());
        assert!(updater.spawn_schedule(Schedule::default()).is_none());
        assert!(backend.asked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_policy_channel_wins_and_a_channel_change_rechecks() {
        let backend = Arc::new(FakeBackend::default());
        let forced = Updater::new(
            backend.clone(),
            tokio::runtime::Handle::current(),
            true,
            Some(UpdateChannel::Stable),
        );
        forced.apply_settings(&UpdateSettings {
            channel: UpdateChannel::Beta,
            ..UpdateSettings::default()
        });
        assert_eq!(forced.channel(), UpdateChannel::Stable);
        forced.check().unwrap();
        wait_for(&forced, |s| *s == UpdateState::UpToDate).await;
        assert_eq!(*backend.asked.lock().unwrap(), [UpdateChannel::Stable]);

        let backend = Arc::new(FakeBackend::default());
        let (free, _) = updater(&backend, true);
        free.apply_settings(&UpdateSettings::default());
        free.check().unwrap();
        wait_for(&free, |s| *s == UpdateState::UpToDate).await;
        free.apply_settings(&UpdateSettings {
            channel: UpdateChannel::Beta,
            ..UpdateSettings::default()
        });
        wait_for(&free, |s| *s == UpdateState::UpToDate).await;
        assert_eq!(
            *backend.asked.lock().unwrap(),
            [UpdateChannel::Stable, UpdateChannel::Beta]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn checks_shortly_after_start_and_then_daily() {
        let backend = Arc::new(FakeBackend::default());
        let (updater, _) = updater(&backend, true);
        updater.apply_settings(&UpdateSettings::default());
        let task = updater.spawn_schedule(Schedule::default()).unwrap();
        let asked = || backend.asked.lock().unwrap().len();
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(asked(), 0);
        tokio::time::sleep(Duration::from_secs(20)).await;
        assert_eq!(asked(), 1, "shortly after start");
        tokio::time::sleep(Duration::from_secs(UPDATE_CHECK_INTERVAL_SECS - 60)).await;
        assert_eq!(asked(), 1);
        tokio::time::sleep(Duration::from_mins(2)).await;
        assert_eq!(asked(), 2, "a day later");
        // Automatic checks off: the schedule asks no more.
        updater.apply_settings(&UpdateSettings {
            auto_check: false,
            ..UpdateSettings::default()
        });
        tokio::time::sleep(Duration::from_secs(UPDATE_CHECK_INTERVAL_SECS * 2)).await;
        assert_eq!(asked(), 2);
        task.abort();
    }

    #[test]
    fn rfc3339_dates() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_791_000_000), "2026-10-03T04:00:00Z");
        assert!(rfc3339_now().ends_with('Z'));
    }
}
