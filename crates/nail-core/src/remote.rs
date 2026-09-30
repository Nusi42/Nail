//! Requests of wave M2b (ADRs 0036, 0041): the remote UI's settings and
//! status, users, credentials, devices, sessions, pairing codes and audit
//! log, and the remote-content allow-lists; plus the state behind them.
//!
//! [`Service::dispatch`](crate::service::Service) routes exactly these
//! requests here (one match arm). Credential and pairing requests go to
//! [`nail_remote_auth::RemoteAuth::handle_admin`] (hashing, TOTP, codes).
//!
//! **The listener** lives in `nail-server` (which depends on this crate,
//! not the other way round). A host attaches with
//! [`Service::attach_remote_ui`] and receives [`RemoteUiCommand`]s
//! (apply settings, regenerate the certificate); it reports what runs with
//! [`Service::report_remote_ui`]. Without a host (tests, embedders) the
//! settings are stored and the status stays `stopped`.
//!
//! **Remote content**: [`RemoteState`] holds the proxy's [`Signer`] (key
//! in the keychain) and the [`Proxy`] (fetcher + a [`TargetCheck`] that
//! re-reads the allow-lists). `GetMessage` fills
//! `MessageBody::remote_content` and, for allowed messages, rewrites
//! remote references to `nail-proxy:` URLs ([`decorate_remote_content`]).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use nail_api::engine::SecretStore;
use nail_api::remote::{
    RemoteUiSettings, RemoteUiSettingsIssue, RemoteUiState, RemoteUiStatus, SessionEndReason,
    internet_mode_lock,
};
use nail_api::remote_content::{
    ONCE_ALLOW_SECS, RemoteContentAllow, RemoteContentInfo, RemoteContentScope,
    normalize_allow_value, sender_domain,
};
use nail_api::{ApiError, ErrorCode, Event, MessageId, RemoteSessionId, Request, Response};
use nail_proxy::{Check, Fetcher, Proxy, ProxyKey, Signer, TargetCheck};
use nail_remote_auth::{AuthConfig, AuthDeps, AuthPolicy, EventSink, RemoteAuth, SystemClock};
use nail_store::{REMOTE_UI_SETTINGS_KEY, Store};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::engine::unix_now;
use crate::error::Result;
use crate::service::{Service, lock};

/// How long `RegenerateRemoteCertificate` waits for the host.
const REGENERATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// What the core asks of the remote-UI host (`nail-server`).
#[derive(Debug)]
#[non_exhaustive]
pub enum RemoteUiCommand {
    /// Start, restart or stop the listener for these (valid) settings.
    Apply(Box<RemoteUiSettings>),
    /// Make a new self-signed certificate and restart with it; answer
    /// when done.
    RegenerateCertificate(oneshot::Sender<std::result::Result<(), ApiError>>),
}

/// Remote-UI and remote-content state of a [`Service`].
pub(crate) struct RemoteState {
    /// Authentication, sessions, admin operations (ADR 0037).
    pub(crate) auth: RemoteAuth,
    store: Store,
    secrets: Arc<dyn SecretStore>,
    /// The fetcher given in the options (taken when the proxy is built).
    fetcher: Mutex<Option<Fetcher>>,
    /// Proxy URL signing, built on first use (the key lives in the
    /// keychain; `None` when it cannot be read or created).
    signer: OnceLock<Option<Signer>>,
    /// The proxy, built on first use (`None` without a signer or TLS).
    proxy: OnceLock<Option<Arc<Proxy>>>,
    host: Mutex<Option<mpsc::UnboundedSender<RemoteUiCommand>>>,
    /// What the host reported (`None` = nothing runs).
    status: Mutex<Option<RemoteUiStatus>>,
    /// The running remote UI's sandbox origin (mail documents).
    sandbox_origin: Mutex<Option<String>>,
    /// "Load once" allowances: message → expiry (Unix seconds); never
    /// stored.
    pub(crate) once: OnceAllows,
}

/// The "load once" allowances (M2b follow-up), shared with the proxy's
/// check.
#[derive(Debug, Clone, Default)]
pub(crate) struct OnceAllows(Arc<Mutex<HashMap<MessageId, i64>>>);

impl OnceAllows {
    /// Allows `message` until `now + ONCE_ALLOW_SECS`.
    pub(crate) fn allow(&self, message: &MessageId, now: i64) {
        let mut map = lock(&self.0);
        map.retain(|_, expires| *expires > now);
        map.insert(message.clone(), now + ONCE_ALLOW_SECS);
    }

    /// Whether `message` is allowed once at `now`.
    pub(crate) fn allows(&self, message: &MessageId, now: i64) -> bool {
        lock(&self.0)
            .get(message)
            .is_some_and(|expires| *expires > now)
    }

    /// Ends the allowance of `message`; whether there was one.
    pub(crate) fn remove(&self, message: &MessageId) -> bool {
        lock(&self.0).remove(message).is_some()
    }
}

/// Broadcasts `nail-remote-auth`'s events like the core's own.
#[derive(Debug)]
struct BroadcastSink(broadcast::Sender<Event>);

impl EventSink for BroadcastSink {
    fn emit(&self, event: Event) {
        let _ = self.0.send(event);
    }
}

/// The proxy's second opinion: the message's remote content is still
/// allowed (an entry may have been removed since the token was minted, a
/// "load once" may have lapsed).
struct AllowCheck(Store, OnceAllows);

impl TargetCheck for AllowCheck {
    fn allows<'a>(&'a self, message_id: &'a str, _url: &'a url::Url) -> Check<'a> {
        let id = MessageId::from(message_id);
        Box::pin(async move {
            if self.1.allows(&id, unix_now()) {
                return true;
            }
            self.0
                .call(move |s| {
                    let message = s.get_message(&id)?;
                    let sender = message.summary.from.map(|a| a.email);
                    s.remote_content_allowed_by(&id, sender.as_deref())
                })
                .await
                .ok()
                .flatten()
                .is_some()
        })
    }
}

impl RemoteState {
    /// Builds the state. The proxy key is read from (or created in) the
    /// keychain only when remote content is first rewritten or served;
    /// `fetcher` overrides the network fetcher (demos: mock servers only).
    pub(crate) fn new(
        store: Store,
        secrets: Arc<dyn SecretStore>,
        events: broadcast::Sender<Event>,
        fetcher: Option<Fetcher>,
    ) -> Self {
        let auth = RemoteAuth::new(
            AuthDeps {
                store: store.clone(),
                secrets: Arc::clone(&secrets),
                clock: Arc::new(SystemClock),
                events: Arc::new(BroadcastSink(events)),
            },
            AuthConfig::default(),
            AuthPolicy::default(),
        );
        Self {
            auth,
            store,
            secrets,
            fetcher: Mutex::new(fetcher),
            signer: OnceLock::new(),
            proxy: OnceLock::new(),
            host: Mutex::new(None),
            status: Mutex::new(None),
            sandbox_origin: Mutex::new(None),
            once: OnceAllows::default(),
        }
    }

    /// The signer (blocking on first use: keychain).
    pub(crate) fn signer(&self) -> Option<&Signer> {
        self.signer
            .get_or_init(|| match ProxyKey::load_or_create(self.secrets.as_ref()) {
                Ok(key) => Some(Signer::new(key)),
                Err(err) => {
                    tracing::warn!(%err, "no proxy key; remote content stays blocked");
                    None
                }
            })
            .as_ref()
    }

    /// The proxy (blocking on first use: keychain).
    fn proxy(&self) -> Option<Arc<Proxy>> {
        self.proxy
            .get_or_init(|| {
                let signer = self.signer()?.clone();
                let fetcher = lock(&self.fetcher).take().or_else(|| match nail_auth::tls_config() {
                    Ok(tls) => Some(Fetcher::new(Arc::new(tls))),
                    Err(err) => {
                        tracing::warn!(%err, "no TLS configuration for the remote-content proxy");
                        None
                    }
                })?;
                Some(Arc::new(Proxy::new(signer, fetcher).with_check(Arc::new(
                    AllowCheck(self.store.clone(), self.once.clone()),
                ))))
            })
            .clone()
    }
}

impl Service {
    /// Remote-UI authentication (login route, sessions, authorization).
    pub fn remote_auth(&self) -> &RemoteAuth {
        &self.inner.remote.auth
    }

    /// The remote-content proxy (web route and desktop protocol); `None`
    /// when no proxy key could be loaded. The first call reads (or
    /// creates) the key in the keychain.
    pub fn remote_proxy(&self) -> Option<Arc<Proxy>> {
        self.inner.remote.proxy()
    }

    /// The stored remote-UI settings (defaults when none were saved).
    pub fn remote_ui_settings(&self) -> RemoteUiSettings {
        remote_ui_settings(self).unwrap_or_default()
    }

    /// Makes the caller the remote-UI host: it receives every later
    /// [`RemoteUiCommand`] (a previous host is detached). The host starts
    /// from [`Service::remote_ui_settings`] itself.
    pub fn attach_remote_ui(&self) -> mpsc::UnboundedReceiver<RemoteUiCommand> {
        let (tx, rx) = mpsc::unbounded_channel();
        *lock(&self.inner.remote.host) = Some(tx);
        rx
    }

    /// The host reports what runs (`None` = stopped) and the sandbox
    /// origin of mail documents; broadcast as `RemoteUiStatusChanged`.
    pub fn report_remote_ui(&self, status: Option<RemoteUiStatus>, sandbox_origin: Option<String>) {
        *lock(&self.inner.remote.status) = status;
        *lock(&self.inner.remote.sandbox_origin) = sandbox_origin;
        match remote_ui_status(self) {
            Ok(status) => self.broadcast(Event::RemoteUiStatusChanged { status }),
            Err(err) => tracing::warn!(%err, "cannot read the remote-UI status"),
        }
    }

    /// The sandbox origin of the running remote UI (mail documents).
    pub(crate) fn remote_sandbox_origin(&self) -> Option<String> {
        lock(&self.inner.remote.sandbox_origin).clone()
    }

    /// A render token URL for `message` on the sandbox origin, bound to the
    /// remote session `session`, when the remote UI runs with one.
    pub(crate) fn remote_render_url(
        &self,
        message: &MessageId,
        session: &RemoteSessionId,
    ) -> Option<String> {
        let origin = self.remote_sandbox_origin()?;
        let token = self
            .inner
            .remote
            .signer()?
            .sign_render_for(message.as_str(), session.as_str())
            .ok()?;
        Some(format!(
            "{origin}{}{token}",
            nail_api::remote_content::RENDER_PATH
        ))
    }

    fn send_to_host(&self, command: RemoteUiCommand) -> bool {
        let mut host = lock(&self.inner.remote.host);
        match host.as_ref().map(|tx| tx.send(command)) {
            Some(Ok(())) => true,
            Some(Err(_)) => {
                *host = None;
                false
            }
            None => false,
        }
    }
}

/// Fills `remote_content` of a message being read (blocking): whether it
/// has remote resources and why they are allowed. Returns the rewriter
/// for [`nail_mime::parse_message_with`] when they are.
pub(crate) fn remote_rewriter(
    service: &Service,
    id: &MessageId,
    sender: Option<&str>,
) -> (Option<RemoteContentScope>, Option<nail_mime::ProxyRewriter>) {
    let allowed_by = match service.store().remote_content_allowed_by(id, sender) {
        Ok(allowed) => allowed,
        Err(err) => {
            tracing::warn!(%err, "cannot read the remote-content allow-lists");
            None
        }
    }
    .or_else(|| {
        service
            .inner
            .remote
            .once
            .allows(id, unix_now())
            .then_some(RemoteContentScope::Once)
    });
    // The key is only needed (and read) once content is allowed.
    let rewriter = allowed_by
        .and_then(|_| service.inner.remote.signer().cloned())
        .map(|signer| {
            let message = id.as_str().to_owned();
            Arc::new(move |url: &str| signer.proxy_url(&message, url).ok())
                as nail_mime::ProxyRewriter
        });
    (allowed_by, rewriter)
}

/// `MessageBody::remote_content` for a message with `resources` remote
/// references (`None` when it has none).
pub(crate) fn decorate_remote_content(
    resources: u32,
    allowed_by: Option<RemoteContentScope>,
    sender: Option<&str>,
) -> Option<RemoteContentInfo> {
    (resources > 0).then(|| RemoteContentInfo {
        resources,
        allowed_by,
        sender: sender.and_then(|s| normalize_allow_value(RemoteContentScope::Sender, s)),
        domain: sender.and_then(sender_domain),
    })
}

/// Audit entries returned at most per page.
const MAX_AUDIT_PAGE: u32 = 500;

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    let store = service.store();
    let now = unix_now();
    match request {
        Request::GetRemoteUiSettings => remote_ui(service),
        Request::UpdateRemoteUiSettings { settings } => {
            check_settings(&settings)?;
            store.set_setting(REMOTE_UI_SETTINGS_KEY, &settings)?;
            let enabled = settings.enabled;
            if service.send_to_host(RemoteUiCommand::Apply(Box::new(settings.clone()))) && enabled {
                // The host reports `running` (or `failed`) when done.
                let mut starting = remote_ui_status(service)?;
                starting.state = RemoteUiState::Starting;
                starting.error = None;
                starting.message = None;
                *lock(&service.inner.remote.status) = Some(starting);
            }
            let status = remote_ui_status(service)?;
            service.broadcast(Event::RemoteUiStatusChanged {
                status: status.clone(),
            });
            Ok(Response::RemoteUi { settings, status })
        }
        Request::ListRemoteUsers => Ok(Response::RemoteUsers {
            users: store.list_remote_users()?,
        }),
        Request::CreateRemoteUser {
            name,
            display_name,
            grant,
        } => {
            let user = store.create_remote_user(&name, display_name.as_deref(), &grant, now)?;
            service.broadcast(Event::RemoteAccessChanged);
            Ok(Response::RemoteUserSaved {
                user: Box::new(user),
            })
        }
        Request::UpdateRemoteUser { user_id, update } => {
            if update.disabled == Some(true) {
                // End the sessions first so their browsers hear about it.
                let ended = store.end_remote_sessions(
                    Some(&user_id),
                    SessionEndReason::UserChanged,
                    now,
                )?;
                announce_ended(service, ended, SessionEndReason::UserChanged);
            }
            let user = store.update_remote_user(&user_id, &update, now)?;
            service.broadcast(Event::RemoteAccessChanged);
            Ok(Response::RemoteUserSaved {
                user: Box::new(user),
            })
        }
        Request::DeleteRemoteUser { user_id } => {
            let ended =
                store.end_remote_sessions(Some(&user_id), SessionEndReason::UserChanged, now)?;
            if !store.delete_remote_user(&user_id)? {
                return Err(ApiError::not_found(format!("remote user {user_id}")).into());
            }
            announce_ended(service, ended, SessionEndReason::UserChanged);
            service.broadcast(Event::RemoteAccessChanged);
            Ok(Response::Done)
        }
        Request::UnlockRemoteUser { user_id } => {
            let user = store.set_remote_user_lock(&user_id, None)?;
            store.clear_auth_throttle(&format!("user:{}", user.name.to_lowercase()))?;
            service.broadcast(Event::RemoteAccessChanged);
            Ok(Response::RemoteUserSaved {
                user: Box::new(user),
            })
        }
        Request::RenamePasskey { passkey_id, name } => {
            if name.trim().is_empty() {
                return Err(ApiError::invalid_request("a passkey needs a name").into());
            }
            let user = store.rename_passkey(&passkey_id, &name)?;
            service.broadcast(Event::RemoteAccessChanged);
            Ok(Response::RemoteUserSaved {
                user: Box::new(user),
            })
        }
        Request::DeletePasskey { passkey_id } => {
            let user = store.delete_passkey(&passkey_id)?;
            service.broadcast(Event::RemoteAccessChanged);
            Ok(Response::RemoteUserSaved {
                user: Box::new(user),
            })
        }
        Request::ListRemoteDevices { user_id } => Ok(Response::RemoteDevices {
            devices: store.list_remote_devices(user_id.as_ref())?,
        }),
        Request::UpdateRemoteDevice {
            device_id,
            name,
            grant,
        } => {
            let device = store.update_remote_device(&device_id, name.as_deref(), grant.as_ref())?;
            service.broadcast(Event::RemoteAccessChanged);
            Ok(Response::RemoteDeviceSaved {
                device: Box::new(device),
            })
        }
        Request::RevokeRemoteDevice { device_id } => {
            let ended = store.revoke_remote_device(&device_id, now)?;
            announce_ended(service, ended, SessionEndReason::DeviceRevoked);
            service.broadcast(Event::RemoteAccessChanged);
            Ok(Response::Done)
        }
        Request::ListRemoteSessions { filter } => Ok(Response::RemoteSessions {
            sessions: store.list_remote_sessions(&filter, now)?,
        }),
        Request::RevokeRemoteSession { session_id } => {
            if store.end_remote_session(&session_id, SessionEndReason::Revoked, now)? {
                announce_ended(service, vec![session_id], SessionEndReason::Revoked);
                service.broadcast(Event::RemoteAccessChanged);
            }
            Ok(Response::Done)
        }
        Request::RevokeAllRemoteSessions { user_id } => {
            let ended =
                store.end_remote_sessions(user_id.as_ref(), SessionEndReason::Revoked, now)?;
            if !ended.is_empty() {
                announce_ended(service, ended, SessionEndReason::Revoked);
                service.broadcast(Event::RemoteAccessChanged);
            }
            Ok(Response::Done)
        }
        Request::ListPairingCodes => Ok(Response::PairingCodes {
            codes: store.list_pairing_codes(now)?,
        }),
        Request::CancelPairingCode { pairing_id } => {
            if store.cancel_pairing_code(&pairing_id, now)? {
                service.broadcast(Event::RemoteAccessChanged);
            }
            Ok(Response::Done)
        }
        Request::ListRemoteAudit {
            user_id,
            before,
            limit,
        } => Ok(Response::RemoteAudit {
            entries: store.list_remote_audit(
                user_id.as_ref(),
                before,
                limit.min(MAX_AUDIT_PAGE),
            )?,
        }),
        Request::AllowRemoteContent {
            message_id,
            scope: RemoteContentScope::Once,
        } => {
            let message = store.get_message(&message_id)?;
            service.inner.remote.once.allow(&message_id, now);
            service.broadcast(Event::RemoteContentPolicyChanged);
            Ok(Response::RemoteContentAllowed {
                entry: RemoteContentAllow {
                    scope: RemoteContentScope::Once,
                    value: message_id.to_string(),
                    label: Some(message.summary.subject),
                    created_at: crate::rules::rfc3339(now),
                },
            })
        }
        Request::AllowRemoteContent { message_id, scope } => {
            let message = store.get_message(&message_id)?;
            let sender = message.summary.from.as_ref().map(|a| a.email.clone());
            let (value, label) = match scope {
                RemoteContentScope::Message => {
                    (Some(message_id.to_string()), Some(message.summary.subject))
                }
                RemoteContentScope::Sender => (
                    sender.and_then(|s| normalize_allow_value(RemoteContentScope::Sender, &s)),
                    None,
                ),
                RemoteContentScope::Domain => (sender.and_then(|s| sender_domain(&s)), None),
                RemoteContentScope::Once => unreachable!("handled above"),
            };
            let value = value.ok_or_else(|| {
                ApiError::invalid_request(format!("message {message_id} has no usable sender"))
            })?;
            let entry = store.allow_remote_content(scope, &value, label.as_deref(), now)?;
            service.broadcast(Event::RemoteContentPolicyChanged);
            Ok(Response::RemoteContentAllowed { entry })
        }
        Request::ListRemoteContentAllows => Ok(Response::RemoteContentAllows {
            entries: store.list_remote_content_allows()?,
        }),
        Request::RemoveRemoteContentAllow {
            scope: RemoteContentScope::Once,
            value,
        } => {
            if service
                .inner
                .remote
                .once
                .remove(&MessageId::new(value.as_str()))
            {
                service.broadcast(Event::RemoteContentPolicyChanged);
            }
            Ok(Response::Done)
        }
        Request::RemoveRemoteContentAllow { scope, value } => {
            if store.remove_remote_content_allow(scope, &value)? {
                service.broadcast(Event::RemoteContentPolicyChanged);
            }
            Ok(Response::Done)
        }
        Request::RegenerateRemoteCertificate => {
            let (tx, rx) = oneshot::channel();
            if !service.send_to_host(RemoteUiCommand::RegenerateCertificate(tx)) {
                return Err(
                    ApiError::unsupported("the remote UI is not available in this app").into(),
                );
            }
            match tokio::time::timeout(REGENERATE_TIMEOUT, rx).await {
                Ok(Ok(result)) => result?,
                _ => {
                    return Err(ApiError::new(
                        ErrorCode::Unavailable,
                        "the remote UI did not answer",
                    )
                    .into());
                }
            }
            remote_ui(service)
        }
        // Hashing, TOTP and pairing codes (nail-remote-auth); from the
        // desktop, so without a remote actor.
        request @ (Request::SetRemotePassword { .. }
        | Request::StartTotpEnrollment { .. }
        | Request::ConfirmTotpEnrollment { .. }
        | Request::DisableTotp { .. }
        | Request::CreatePairingCode { .. }) => {
            Ok(service.remote_auth().handle_admin(request, None).await?)
        }
        other => Err(ApiError::internal(format!(
            "{} is not a remote-UI request",
            crate::service::request_name(&other)
        ))
        .into()),
    }
}

/// The stored settings (defaults when none were saved).
pub(crate) fn remote_ui_settings(service: &Service) -> Result<RemoteUiSettings> {
    Ok(service
        .store()
        .setting(REMOTE_UI_SETTINGS_KEY)?
        .unwrap_or_default())
}

/// The remote UI's state as the host reported it (`stopped` without one).
pub(crate) fn remote_ui_status(service: &Service) -> Result<RemoteUiStatus> {
    let mut status = lock(&service.inner.remote.status)
        .clone()
        .unwrap_or_else(RemoteUiStatus::stopped);
    status.active_sessions = service.store().count_active_remote_sessions(unix_now())?;
    Ok(status)
}

fn remote_ui(service: &Service) -> Result<Response> {
    Ok(Response::RemoteUi {
        settings: remote_ui_settings(service)?,
        status: remote_ui_status(service)?,
    })
}

/// Refuses invalid settings with the first issue's error code; every
/// issue is named in the message.
fn check_settings(settings: &RemoteUiSettings) -> Result<()> {
    let issues = settings.issues();
    let Some(first) = issues.first() else {
        return Ok(());
    };
    // Internet mode (and ACME, which needs it) outranks the rest.
    let code = if issues
        .iter()
        .any(|i| i.error_code() == ErrorCode::Forbidden)
    {
        ErrorCode::Forbidden
    } else {
        first.error_code()
    };
    let names: Vec<String> = issues.iter().map(issue_name).collect();
    let lock = internet_mode_lock()
        .map(|reason| format!(" (internet mode locked: {})", enum_name(&reason)))
        .unwrap_or_default();
    Err(ApiError::new(
        code,
        format!("invalid remote-UI settings: {}{lock}", names.join(", ")),
    )
    .into())
}

fn issue_name(issue: &RemoteUiSettingsIssue) -> String {
    enum_name(issue)
}

fn enum_name<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn announce_ended(service: &Service, ended: Vec<RemoteSessionId>, reason: SessionEndReason) {
    for session_id in ended {
        service.broadcast(Event::RemoteSessionEnded { session_id, reason });
    }
}
