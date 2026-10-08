//! M4b desktop requests (ADR 0087): installation facts, the updater's
//! settings and state, tray / startup / dev-endpoint settings, default
//! apps and the first run — store-backed settings plus the shell's
//! [`DesktopHost`] (`ServiceOptions::desktop`). Without a host (`nail
//! serve`, tests) everything but the first-run state answers `unsupported`
//! (`desktop-unavailable`).
//!
//! Also Simple MAPI ([`Service::submit_mapi`]): a program's "send mail"
//! becomes a saved draft the UI opens
//! ([`LaunchRequest::ComposeDraft`]); nothing is sent without the user.

use std::sync::Arc;

use base64::Engine as _;
use nail_api::compose::ComposerMode;
use nail_api::desktop::{
    DesktopChange, DesktopHost, DesktopSettings, FirstRunState, UpdateSettings, UpdateStatus,
    default_apps_settings_uri,
};
use nail_api::launch::LaunchRequest;
use nail_api::mapi::{MapiMessage, MapiRecipientClass, MapiResult};
use nail_api::{Address, ApiError, ErrorCode, Event, LocalizedMessage, Request, Response};

use crate::error::Result;
use crate::service::Service;

/// `app_settings` key of [`UpdateSettings`].
pub(crate) const UPDATE_SETTINGS_KEY: &str = "desktop.updates";
/// `app_settings` key of [`DesktopSettings`].
pub(crate) const DESKTOP_SETTINGS_KEY: &str = "desktop.settings";
/// `app_settings` key of [`FirstRunState`].
pub(crate) const FIRST_RUN_KEY: &str = "desktop.first-run";

/// Hands the host a notifier that turns its changes into events (called
/// once from `Service::open`), and the stored settings to start from.
pub(crate) fn attach(service: &Service) {
    let Some(host) = service.inner.desktop.clone() else {
        return;
    };
    let weak = Arc::downgrade(&service.inner);
    host.attach(Arc::new(move |change| {
        if let Some(inner) = weak.upgrade() {
            let service = Service { inner };
            service.desktop_changed(change);
        }
    }));
    host.apply_update_settings(&service.stored_update_settings_sync());
}

impl Service {
    /// Broadcasts the event for a change the desktop host reported.
    pub fn desktop_changed(&self, change: DesktopChange) {
        match change {
            DesktopChange::Update => {
                if let Some(host) = &self.inner.desktop {
                    let stored = self.stored_update_settings_sync();
                    let status =
                        UpdateStatus::compose(stored, &host.installation(), host.update_state());
                    self.broadcast(Event::UpdateStatusChanged {
                        status: Box::new(status),
                    });
                }
            }
            DesktopChange::Settings => self.broadcast(Event::DesktopSettingsChanged),
            DesktopChange::DefaultApps => self.broadcast(Event::DefaultAppsChanged),
            DesktopChange::PopOut(report) => crate::windows::pop_out_reported(self, report),
        }
    }

    /// The stored desktop settings (defaults when never saved), with
    /// `start_with_windows` as the host's Run key says. For the shell at
    /// start (start hidden, close to tray, dev endpoint).
    pub async fn desktop_settings(&self) -> Result<DesktopSettings> {
        let mut settings: DesktopSettings = self
            .store()
            .call(|s| s.setting(DESKTOP_SETTINGS_KEY))
            .await?
            .unwrap_or_default();
        if let Some(run) = self
            .inner
            .desktop
            .as_ref()
            .and_then(|h| h.start_with_windows())
        {
            settings.start_with_windows = run;
        }
        Ok(settings)
    }

    fn stored_update_settings_sync(&self) -> UpdateSettings {
        self.inner
            .store
            .setting(UPDATE_SETTINGS_KEY)
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    /// A program's Simple MAPI "send mail" (from the shell's pipe server):
    /// reads the attachments now (callers delete temporary files once the
    /// call returns), saves a draft from the default identity and asks the
    /// UI to open it ([`LaunchRequest::ComposeDraft`]). Requests without
    /// UI are treated the same: Nail never sends for another program.
    /// Returns what `MAPISendMail` should return.
    pub async fn submit_mapi(&self, message: MapiMessage) -> MapiResult {
        match self.mapi_draft(&message).await {
            Ok(draft_id) => {
                self.push_launch_requests(vec![LaunchRequest::ComposeDraft {
                    draft_id,
                    program: message.program.clone(),
                }]);
                MapiResult::Success
            }
            Err(result) => result,
        }
    }

    async fn mapi_draft(
        &self,
        message: &MapiMessage,
    ) -> std::result::Result<nail_api::DraftId, MapiResult> {
        message.check()?;
        // Files first: nothing is saved when one cannot be read.
        let paths: Vec<_> = message
            .attachments
            .iter()
            .map(|a| (a.path.clone(), a.display_name()))
            .collect();
        let files = tokio::task::spawn_blocking(move || read_attachments(&paths))
            .await
            .map_err(|_| MapiResult::Failure)??;

        let draft = match self
            .handle(Request::NewDraft {
                identity_id: None,
                account_id: None,
                mode: Some(ComposerMode::Plain),
                context: None,
            })
            .await
        {
            Response::Draft { draft } => draft,
            Response::Error {
                code: ErrorCode::Internal,
                ..
            } => {
                return Err(MapiResult::Failure);
            }
            // No identity / account to write from.
            _ => return Err(MapiResult::LoginFailure),
        };
        let mut draft = *draft;
        let address = |class| {
            message
                .recipients
                .iter()
                .filter(|r| r.class == class)
                .filter_map(|r| {
                    r.email().map(|email| Address {
                        name: r.display_name(),
                        email,
                    })
                })
                .collect::<Vec<_>>()
        };
        draft.to = address(MapiRecipientClass::To);
        draft.cc = address(MapiRecipientClass::Cc);
        draft.bcc = address(MapiRecipientClass::Bcc);
        draft.subject = message.subject.clone().unwrap_or_default();
        if let Some(body) = message.body.as_deref().filter(|b| !b.is_empty()) {
            // The signature NewDraft inserted stays below the program's text.
            draft.text = if draft.text.is_empty() {
                body.to_owned()
            } else {
                format!("{body}\n\n{}", draft.text)
            };
        }
        for (name, bytes) in files {
            let response = self
                .handle(Request::UploadAttachment {
                    filename: name,
                    mime: "application/octet-stream".to_owned(),
                    data: base64::engine::general_purpose::STANDARD.encode(&bytes),
                    content_id: None,
                })
                .await;
            match response {
                Response::AttachmentUploaded { attachment } => draft.attachments.push(attachment),
                Response::Error {
                    code: ErrorCode::TooLarge,
                    ..
                } => {
                    return Err(MapiResult::AttachmentTooLarge);
                }
                _ => return Err(MapiResult::Failure),
            }
        }
        match self.handle(Request::SaveDraft { draft }).await {
            Response::DraftSaved { draft_id } => Ok(draft_id),
            Response::Error { code, message, .. } => {
                tracing::warn!(?code, %message, "cannot save the Simple MAPI draft");
                Err(MapiResult::from_api_error(&ApiError::new(code, message)))
            }
            _ => Err(MapiResult::Failure),
        }
    }
}

/// Reads every attachment (blocking), checking existence and the size
/// limit before reading.
fn read_attachments(
    paths: &[(String, String)],
) -> std::result::Result<Vec<(String, Vec<u8>)>, MapiResult> {
    let mut out = Vec::with_capacity(paths.len());
    for (path, name) in paths {
        let meta = std::fs::metadata(path).map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => MapiResult::AttachmentNotFound,
            _ => MapiResult::AttachmentOpenFailure,
        })?;
        if !meta.is_file() {
            return Err(MapiResult::AttachmentOpenFailure);
        }
        if meta.len() > nail_api::compose::MAX_ATTACHMENT_BYTES {
            return Err(MapiResult::AttachmentTooLarge);
        }
        let bytes = std::fs::read(path).map_err(|_| MapiResult::AttachmentOpenFailure)?;
        out.push((name.clone(), bytes));
    }
    Ok(out)
}

fn unavailable() -> crate::error::CoreError {
    ApiError::unsupported("only the desktop app has this")
        .with_localized(LocalizedMessage::new("desktop-unavailable"))
        .into()
}

fn host(service: &Service) -> Result<&Arc<dyn DesktopHost>> {
    service.inner.desktop.as_ref().ok_or_else(unavailable)
}

fn forbidden_disabled(reason: nail_api::desktop::UpdateDisabledReason) -> crate::error::CoreError {
    ApiError::new(ErrorCode::Forbidden, "updates are disabled")
        .with_localized(LocalizedMessage::new(reason.message_id()))
        .into()
}

async fn update_status(service: &Service, host: &Arc<dyn DesktopHost>) -> Result<UpdateStatus> {
    let stored: UpdateSettings = service
        .store()
        .call(|s| s.setting(UPDATE_SETTINGS_KEY))
        .await?
        .unwrap_or_default();
    Ok(UpdateStatus::compose(
        stored,
        &host.installation(),
        host.update_state(),
    ))
}

fn status_response(status: UpdateStatus) -> Response {
    Response::UpdateStatus {
        status: Box::new(status),
    }
}

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    match request {
        Request::GetFirstRun => {
            let state: FirstRunState = service
                .store()
                .call(|s| s.setting(FIRST_RUN_KEY))
                .await?
                .unwrap_or_default();
            Ok(Response::FirstRun { state })
        }
        Request::CompleteFirstRun => {
            let state = FirstRunState {
                completed: true,
                completed_at: Some(crate::rules::rfc3339(crate::engine::unix_now())),
                completed_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            };
            let saved = state.clone();
            service
                .store()
                .call(move |s| s.set_setting(FIRST_RUN_KEY, &saved))
                .await?;
            Ok(Response::FirstRun { state })
        }
        Request::GetInstallation => Ok(Response::Installation {
            info: Box::new(host(service)?.installation()),
        }),
        Request::GetUpdateStatus => {
            let host = host(service)?;
            Ok(status_response(update_status(service, host).await?))
        }
        Request::SetUpdateSettings { settings } => {
            let host = host(service)?;
            let installation = host.installation();
            if let Some(forced) = installation.forced_channel
                && forced != settings.channel
            {
                return Err(
                    ApiError::new(ErrorCode::Forbidden, "the channel is set by policy")
                        .with_localized(LocalizedMessage::new("update-channel-locked"))
                        .into(),
                );
            }
            let saved = settings.clone();
            service
                .store()
                .call(move |s| s.set_setting(UPDATE_SETTINGS_KEY, &saved))
                .await?;
            host.apply_update_settings(&settings);
            let status = update_status(service, host).await?;
            service.broadcast(Event::UpdateStatusChanged {
                status: Box::new(status.clone()),
            });
            Ok(status_response(status))
        }
        request @ (Request::CheckForUpdates | Request::DownloadUpdate | Request::InstallUpdate) => {
            let host = host(service)?;
            if let Some(reason) = host.installation().updates_disabled {
                return Err(forbidden_disabled(reason));
            }
            let state = host.update_state().state;
            let allowed = match request {
                Request::CheckForUpdates => state.can_check(),
                Request::DownloadUpdate => state.can_download(),
                _ => state.can_install(),
            };
            if !allowed {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    format!("{} does not apply now", request.wire_type()),
                )
                .into());
            }
            match request {
                Request::CheckForUpdates => host.check_for_updates()?,
                Request::DownloadUpdate => host.download_update()?,
                _ => {
                    host.install_update()?;
                    return Ok(Response::Done);
                }
            }
            Ok(status_response(update_status(service, host).await?))
        }
        Request::GetDesktopSettings => {
            let host = host(service)?;
            Ok(Response::DesktopSettings {
                settings: service.desktop_settings().await?,
                dev_endpoint_url: host.dev_endpoint_url(),
                start_with_windows_managed: host.start_with_windows_managed(),
            })
        }
        Request::SetDesktopSettings { settings } => {
            let host = host(service)?;
            let saved = settings.clone();
            service
                .store()
                .call(move |s| s.set_setting(DESKTOP_SETTINGS_KEY, &saved))
                .await?;
            host.apply_desktop_settings(&settings)?;
            service.broadcast(Event::DesktopSettingsChanged);
            Ok(Response::DesktopSettings {
                settings: service.desktop_settings().await?,
                dev_endpoint_url: host.dev_endpoint_url(),
                start_with_windows_managed: host.start_with_windows_managed(),
            })
        }
        Request::GetDefaultApps => {
            let host = host(service)?;
            Ok(Response::DefaultApps {
                status: Box::new(host.default_apps()),
                settings_uri: default_apps_settings_uri(host.installation().scope),
            })
        }
        Request::OpenDefaultAppsSettings => {
            host(service)?.open_default_apps_settings()?;
            Ok(Response::Done)
        }
        other => Err(
            ApiError::internal(format!("{} is not a desktop request", other.wire_type())).into(),
        ),
    }
}
