//! The real update backend (M4b WP113, ADR 0087): tauri-plugin-updater
//! reads the channel's signed manifest (`<base>/<channel>/latest.json`,
//! written by the release pipeline, WP112), downloads the package and
//! verifies its minisign signature against the build's public key before
//! anything runs. TLS is rustls with ring (ADR 0014); the installer is the
//! NSIS / MSI package itself.

use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use nail_api::desktop::{UpdateChannel, UpdateFailure, UpdateInfo};
use tauri::{AppHandle, Runtime};
use tauri_plugin_updater::UpdaterExt;

use crate::install::UpdateSource;
use crate::updater::{BackendError, PendingUpdate, UpdateBackend};

/// How long one manifest or package request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

/// Which step an error happened in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Reading the manifest.
    Check,
    /// Downloading and verifying the package.
    Download,
    /// Running the installer.
    Install,
}

/// The [`UpdateFailure`] of an updater error.
pub fn failure_of(err: &tauri_plugin_updater::Error, phase: Phase) -> UpdateFailure {
    use tauri_plugin_updater::Error as E;
    match err {
        E::Minisign(_)
        | E::Base64(_)
        | E::SignatureUtf8(_)
        | E::SignedVersionMismatch { .. }
        | E::MissingSignedVersion => UpdateFailure::Signature,
        E::Serialization(_)
        | E::ReleaseNotFound
        | E::TargetNotFound(_)
        | E::TargetsNotFound(_)
        | E::Semver(_)
        | E::UrlParse(_)
        | E::EmptyEndpoints
        | E::UnsupportedArch
        | E::UnsupportedOs
        | E::InsecureTransportProtocol
        | E::InvalidUpdaterFormat
        | E::FormatDate => UpdateFailure::Manifest,
        _ => match phase {
            Phase::Check => UpdateFailure::Network,
            Phase::Download => UpdateFailure::Download,
            Phase::Install => UpdateFailure::Install,
        },
    }
}

/// The UI's view of a release.
pub fn update_info(update: &tauri_plugin_updater::Update, channel: UpdateChannel) -> UpdateInfo {
    UpdateInfo {
        version: update.version.clone(),
        channel,
        published_at: update
            .raw_json
            .get("pub_date")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        notes: update.body.clone().filter(|n| !n.trim().is_empty()),
        size_bytes: None,
    }
}

/// The backend over the Tauri updater.
pub struct TauriBackend<R: Runtime> {
    app: AppHandle<R>,
    source: UpdateSource,
}

impl<R: Runtime> TauriBackend<R> {
    /// For the app with the given source.
    pub fn new(app: AppHandle<R>, source: UpdateSource) -> Self {
        Self { app, source }
    }
}

impl<R: Runtime> UpdateBackend for TauriBackend<R> {
    fn check(
        &self,
        channel: UpdateChannel,
    ) -> BoxFuture<'static, Result<Option<Arc<dyn PendingUpdate>>, BackendError>> {
        let endpoints = self.source.endpoints_for(channel);
        let built = self
            .app
            .updater_builder()
            .pubkey(self.source.pubkey.clone())
            .timeout(REQUEST_TIMEOUT)
            .endpoints(endpoints)
            .and_then(tauri_plugin_updater::UpdaterBuilder::build);
        Box::pin(async move {
            let updater = built.map_err(|e| (failure_of(&e, Phase::Check), e.to_string()))?;
            let found = updater
                .check()
                .await
                .map_err(|e| (failure_of(&e, Phase::Check), e.to_string()))?;
            Ok(found.map(|update| {
                Arc::new(TauriUpdate {
                    info: update_info(&update, channel),
                    update,
                }) as Arc<dyn PendingUpdate>
            }))
        })
    }
}

struct TauriUpdate {
    info: UpdateInfo,
    update: tauri_plugin_updater::Update,
}

impl PendingUpdate for TauriUpdate {
    fn info(&self) -> &UpdateInfo {
        &self.info
    }

    fn download(
        &self,
        mut progress: Box<dyn FnMut(u64, Option<u64>) + Send>,
    ) -> BoxFuture<'static, Result<Vec<u8>, BackendError>> {
        let update = self.update.clone();
        Box::pin(async move {
            update
                .download(|chunk, total| progress(chunk as u64, total), || {})
                .await
                .map_err(|e| (failure_of(&e, Phase::Download), e.to_string()))
        })
    }

    fn install(&self, bytes: &[u8], restart: bool) -> Result<(), BackendError> {
        self.update
            .clone()
            .restart_after_install(restart)
            .install(bytes)
            .map_err(|e| (failure_of(&e, Phase::Install), e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tauri_plugin_updater::Error as E;

    #[test]
    fn errors_map_to_failures() {
        assert_eq!(
            failure_of(
                &E::SignedVersionMismatch {
                    signed: "1".into(),
                    announced: "2".into()
                },
                Phase::Download
            ),
            UpdateFailure::Signature
        );
        assert_eq!(
            failure_of(&E::MissingSignedVersion, Phase::Check),
            UpdateFailure::Signature
        );
        assert_eq!(
            failure_of(&E::ReleaseNotFound, Phase::Check),
            UpdateFailure::Manifest
        );
        assert_eq!(
            failure_of(
                &E::TargetsNotFound(vec!["windows-x86_64".into()]),
                Phase::Check
            ),
            UpdateFailure::Manifest
        );
        assert_eq!(
            failure_of(&E::Network("503".into()), Phase::Check),
            UpdateFailure::Network
        );
        assert_eq!(
            failure_of(&E::Network("reset".into()), Phase::Download),
            UpdateFailure::Download
        );
        assert_eq!(
            failure_of(&E::Io(std::io::Error::other("denied")), Phase::Install),
            UpdateFailure::Install
        );
    }
}
