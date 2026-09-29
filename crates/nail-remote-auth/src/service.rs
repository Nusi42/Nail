//! [`RemoteAuth`]: the shared state behind the login route, the session
//! middleware and the admin operations, and session resolution.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use nail_api::engine::SecretStore;
use nail_api::remote::{
    RemoteAlert, RemoteAuditKind, RemoteAuditOutcome, RemoteUiSettings, SessionEndReason,
};
use nail_api::{Event, RemoteSessionId};
use nail_store::{REMOTE_UI_SETTINGS_KEY, RemoteAuditRecord, Store, hash_remote_secret};

use crate::clock::{Clock, SystemClock, parse_rfc3339};
use crate::events::{EventSink, NoEvents};
use crate::passkey::{LoginCeremony, RegistrationCeremony, RelyingParty};
use crate::password::PasswordPolicy;
use crate::secret::{CsrfKey, is_token_shaped, random_token};
use crate::session::{ClientMeta, Cookies, RemoteSessionCtx, session_cookie};
use crate::throttle::{Lockout, ThrottlePolicy};

/// Tunables of the authentication (defaults are the production values).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthPolicy {
    /// Brute-force limits.
    pub throttle: ThrottlePolicy,
    /// Password rules and hashing cost.
    pub password: PasswordPolicy,
    /// A session token is replaced after this many seconds of use.
    pub rotate_after_secs: i64,
    /// The replaced token stays valid this long (requests in flight).
    pub rotation_grace_secs: i64,
    /// A WebAuthn challenge is valid this long.
    pub ceremony_ttl_secs: i64,
    /// Open WebAuthn ceremonies at most (older ones are dropped first).
    pub max_ceremonies: usize,
    /// A session's `last_seen_at` is written at most this often.
    pub touch_interval_secs: i64,
    /// Life of the device cookie (browsers cap it at 400 days).
    pub device_cookie_secs: i64,
    /// Largest login request body.
    pub max_body_bytes: usize,
}

impl Default for AuthPolicy {
    fn default() -> Self {
        Self {
            throttle: ThrottlePolicy::default(),
            password: PasswordPolicy::default(),
            rotate_after_secs: 5 * 60,
            rotation_grace_secs: 60,
            ceremony_ttl_secs: 5 * 60,
            max_ceremonies: 256,
            touch_interval_secs: 15,
            device_cookie_secs: 400 * 24 * 60 * 60,
            max_body_bytes: 64 * 1024,
        }
    }
}

/// Deployment facts the server knows (and may change while running).
#[derive(Debug, Clone, Default)]
pub struct AuthConfig {
    /// The WebAuthn relying party (needs a host name); `None` = passkeys
    /// are not offered.
    pub relying_party: Option<RelyingParty>,
    /// Origins the browser may send (`Origin` header of the login route,
    /// e.g. `https://nail.local:4780`). Empty = the request's own `Host`.
    pub origins: Vec<String>,
    /// Fingerprint of the served certificate (shown at pairing).
    pub certificate_fingerprint: Option<String>,
}

/// What [`RemoteAuth`] needs from its host.
#[derive(Debug, Clone)]
pub struct AuthDeps {
    /// The store (schema v6).
    pub store: Store,
    /// The OS keychain (TOTP secrets).
    pub secrets: Arc<dyn SecretStore>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Event delivery (the core's broadcast).
    pub events: Arc<dyn EventSink>,
}

impl AuthDeps {
    /// Dependencies with the system clock and no event delivery.
    pub fn new(store: Store, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            store,
            secrets,
            clock: Arc::new(SystemClock),
            events: Arc::new(NoEvents),
        }
    }
}

/// What a WebAuthn ceremony was started for.
#[derive(Debug)]
pub(crate) enum CeremonyKind {
    /// A login or re-authentication.
    Login {
        ceremony: LoginCeremony,
        purpose: nail_api::remote::AuthPurpose,
        user_name: Option<String>,
    },
    /// Registering a passkey for the session's user.
    Register {
        ceremony: RegistrationCeremony,
        name: String,
    },
}

#[derive(Debug)]
pub(crate) struct Ceremony {
    pub(crate) kind: CeremonyKind,
    pub(crate) expires_at: i64,
    /// The session that started it (re-authentication, registration).
    pub(crate) session: Option<RemoteSessionId>,
    /// The device cookie's device when it started.
    pub(crate) device: Option<nail_api::RemoteDeviceId>,
}

#[derive(Debug)]
pub(crate) struct Inner {
    pub(crate) store: Store,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) events: Arc<dyn EventSink>,
    pub(crate) policy: AuthPolicy,
    pub(crate) config: RwLock<AuthConfig>,
    pub(crate) csrf: CsrfKey,
    pub(crate) ceremonies: Mutex<HashMap<String, Ceremony>>,
    /// Last token rotation per session (in memory: after a restart every
    /// session rotates on its next request).
    pub(crate) rotations: Mutex<HashMap<RemoteSessionId, i64>>,
}

/// Remote-UI authentication: login protocol, session resolution,
/// authorization, admin operations. Cheap to clone (shared state).
#[derive(Debug, Clone)]
pub struct RemoteAuth {
    pub(crate) inner: Arc<Inner>,
}

/// Why a request has no usable session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoSession {
    /// No session cookie was sent.
    Missing,
    /// A session cookie was sent but is not (or no longer) valid: wrong,
    /// expired, ended, revoked, or not sent with its device's cookie. The
    /// cookie should be cleared.
    Invalid,
}

/// A request's valid session, plus the rotated cookie to send back.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// The session.
    pub ctx: RemoteSessionCtx,
    /// `Set-Cookie` with a new session token, when it was rotated.
    pub set_cookie: Option<String>,
}

impl RemoteAuth {
    /// Remote authentication over `deps` with `config` and `policy`.
    pub fn new(deps: AuthDeps, config: AuthConfig, policy: AuthPolicy) -> Self {
        Self {
            inner: Arc::new(Inner {
                store: deps.store,
                secrets: deps.secrets,
                clock: deps.clock,
                events: deps.events,
                policy,
                config: RwLock::new(config),
                csrf: CsrfKey::random(),
                ceremonies: Mutex::new(HashMap::new()),
                rotations: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Replaces the deployment facts (new certificate, host name, port).
    pub fn set_config(&self, config: AuthConfig) {
        *self
            .inner
            .config
            .write()
            .unwrap_or_else(PoisonError::into_inner) = config;
    }

    /// The deployment facts.
    pub fn config(&self) -> AuthConfig {
        self.inner
            .config
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The policy.
    pub fn policy(&self) -> &AuthPolicy {
        &self.inner.policy
    }

    /// The store.
    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    /// The current time (Unix seconds).
    pub fn now(&self) -> i64 {
        self.inner.clock.now()
    }

    /// The remote-UI settings (defaults when none are stored).
    pub fn settings(&self) -> RemoteUiSettings {
        match self.inner.store.setting(REMOTE_UI_SETTINGS_KEY) {
            Ok(Some(settings)) => settings,
            Ok(None) => RemoteUiSettings::default(),
            Err(e) => {
                tracing::warn!("remote-ui settings unreadable, using defaults: {e}");
                RemoteUiSettings::default()
            }
        }
    }

    pub(crate) fn emit(&self, event: Event) {
        self.inner.events.emit(event);
    }

    /// Writes an audit entry; a failure is logged, never fatal.
    pub(crate) fn audit(
        &self,
        kind: RemoteAuditKind,
        outcome: RemoteAuditOutcome,
        client: &ClientMeta,
        fill: impl FnOnce(&mut RemoteAuditRecord),
    ) {
        let mut record = RemoteAuditRecord::new(self.now(), kind, outcome);
        record.ip = client.ip_string();
        record.user_agent = client.user_agent();
        fill(&mut record);
        if let Err(e) = self.inner.store.append_remote_audit(&record) {
            tracing::warn!("remote audit entry lost: {e}");
        }
    }

    /// Announces ended sessions (their browsers show the login page).
    pub(crate) fn announce_ended(&self, ended: &[RemoteSessionId], reason: SessionEndReason) {
        for session_id in ended {
            self.forget_rotation(session_id);
            self.emit(Event::RemoteSessionEnded {
                session_id: session_id.clone(),
                reason,
            });
        }
        if !ended.is_empty() {
            self.emit(Event::RemoteAccessChanged);
        }
    }

    fn forget_rotation(&self, session: &RemoteSessionId) {
        self.inner
            .rotations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(session);
    }

    /// Raises alerts, audit entries and user locks for new lockouts.
    pub(crate) fn report_lockouts(
        &self,
        lockouts: &[Lockout],
        user: Option<&nail_api::remote::RemoteUser>,
        tried_name: Option<&str>,
        client: &ClientMeta,
    ) {
        for lockout in lockouts {
            let until = crate::clock::rfc3339(lockout.until);
            let is_user = lockout.key.starts_with("user:");
            if is_user
                && let Some(user) = user
                && let Err(e) = self
                    .inner
                    .store
                    .set_remote_user_lock(&user.id, Some(lockout.until))
            {
                tracing::warn!("could not record the lockout of {}: {e}", user.id);
            }
            let alert = RemoteAlert::Lockout {
                user_name: is_user.then(|| tried_name.unwrap_or_default().to_owned()),
                ip: lockout
                    .key
                    .starts_with("ip:")
                    .then(|| client.ip_string())
                    .flatten(),
                until,
            };
            self.audit(
                RemoteAuditKind::Lockout,
                RemoteAuditOutcome::Denied,
                client,
                |r| {
                    r.user_id = user.filter(|_| is_user).map(|u| u.id.clone());
                    r.user_name = tried_name.map(str::to_owned);
                    r.error = Some(nail_api::ErrorCode::RateLimited);
                    r.detail = Some(format!("locked out: {}", lockout.key));
                },
            );
            self.emit(Event::RemoteSecurityAlert { alert });
            if is_user {
                self.emit(Event::RemoteAccessChanged);
            }
        }
    }

    /// The CSRF binding of a browser: its device when paired, else
    /// anonymous. Stable across login, logout and token rotation.
    pub(crate) fn csrf_binding(&self, cookies: &Cookies) -> String {
        match self.device_of(cookies) {
            Some(device) => format!("device:{}", device.id),
            None => "anon".to_owned(),
        }
    }

    /// The CSRF token for a browser with these cookies.
    pub fn csrf_token(&self, cookies: &Cookies) -> String {
        self.inner.csrf.token(&self.csrf_binding(cookies))
    }

    /// Whether `presented` is the CSRF token of a browser with these
    /// cookies (constant-time comparison).
    pub fn check_csrf(&self, cookies: &Cookies, presented: Option<&str>) -> bool {
        presented.is_some_and(|token| self.inner.csrf.verify(&self.csrf_binding(cookies), token))
    }

    /// The active device of the device cookie.
    pub(crate) fn device_of(&self, cookies: &Cookies) -> Option<nail_api::remote::RemoteDevice> {
        let secret = cookies.device.as_deref().filter(|s| is_token_shaped(s))?;
        self.inner
            .store
            .remote_device_by_secret(&hash_remote_secret(secret))
            .ok()
            .flatten()
    }

    /// Until when a session counts as freshly authenticated.
    pub(crate) fn reauth_until(
        session: &nail_api::remote::RemoteSession,
        settings: &RemoteUiSettings,
    ) -> i64 {
        let login = parse_rfc3339(&session.created_at).unwrap_or_default();
        let reauth = session
            .reauth_at
            .as_deref()
            .and_then(parse_rfc3339)
            .unwrap_or_default();
        login.max(reauth) + i64::from(settings.reauth_minutes) * 60
    }

    /// Ends a session because of its idle or maximum age, if due at `now`.
    fn expire_if_due(
        &self,
        session: &nail_api::remote::RemoteSession,
        settings: &RemoteUiSettings,
        now: i64,
    ) -> bool {
        let last_seen = parse_rfc3339(&session.last_seen_at).unwrap_or_default();
        let created = parse_rfc3339(&session.created_at).unwrap_or_default();
        let idle = last_seen + i64::from(settings.session_idle_minutes) * 60 <= now;
        let too_old = created + i64::from(settings.session_max_hours) * 3600 <= now;
        if !(idle || too_old) {
            return false;
        }
        if let Ok(true) =
            self.inner
                .store
                .end_remote_session(&session.id, SessionEndReason::Expired, now)
        {
            self.announce_ended(std::slice::from_ref(&session.id), SessionEndReason::Expired);
        }
        true
    }

    /// Resolves the session of a request: the session cookie must name a
    /// valid session, sent together with the cookie of the device the
    /// session belongs to; idle and absolute limits are applied; the
    /// session is touched and its token rotated when due.
    pub fn resolve(&self, cookies: &Cookies, client: &ClientMeta) -> Result<Resolved, NoSession> {
        let Some(token) = cookies.session.as_deref() else {
            return Err(NoSession::Missing);
        };
        if !is_token_shaped(token) {
            return Err(NoSession::Invalid);
        }
        let now = self.now();
        let store = &self.inner.store;
        let session = store
            .remote_session_by_token(&hash_remote_secret(token), now)
            .ok()
            .flatten()
            .ok_or(NoSession::Invalid)?;
        let device = self.device_of(cookies).ok_or(NoSession::Invalid)?;
        if device.id != session.device_id || device.user_id != session.user_id {
            return Err(NoSession::Invalid);
        }
        let settings = self.settings();
        if self.expire_if_due(&session, &settings, now) {
            return Err(NoSession::Invalid);
        }
        let mut session = session;
        let last_seen = parse_rfc3339(&session.last_seen_at).unwrap_or_default();
        if now - last_seen >= self.inner.policy.touch_interval_secs {
            let ip = client.ip_string();
            let touched = store
                .touch_remote_session(&session.id, ip.as_deref(), now)
                .and_then(|()| {
                    store.touch_remote_device(
                        &device.id,
                        ip.as_deref(),
                        client.user_agent().as_deref(),
                        now,
                    )
                });
            if let Err(e) = touched {
                tracing::warn!("could not record remote activity: {e}");
            }
            session.last_seen_at = crate::clock::rfc3339(now);
            if ip.is_some() {
                session.ip = ip;
            }
        }
        let set_cookie = self.rotate_if_due(&session, now);
        let reauth_until = Self::reauth_until(&session, &settings);
        Ok(Resolved {
            ctx: RemoteSessionCtx {
                session,
                device,
                client: client.clone(),
                reauth_until,
            },
            set_cookie,
        })
    }

    /// Rotates the session token when the last rotation (or the login) is
    /// older than the policy allows; returns the new cookie.
    fn rotate_if_due(&self, session: &nail_api::remote::RemoteSession, now: i64) -> Option<String> {
        let policy = &self.inner.policy;
        {
            let mut rotations = self
                .inner
                .rotations
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let last = rotations
                .get(&session.id)
                .copied()
                .unwrap_or_else(|| parse_rfc3339(&session.created_at).unwrap_or_default());
            if now - last < policy.rotate_after_secs {
                return None;
            }
            // Claim the rotation before releasing the lock so concurrent
            // requests do not rotate twice.
            rotations.insert(session.id.clone(), now);
        }
        let token = random_token();
        let rotated = self.inner.store.rotate_remote_session(
            &session.id,
            &hash_remote_secret(&token),
            now + policy.rotation_grace_secs,
            now,
        );
        match rotated {
            Ok(()) => {
                let expires = parse_rfc3339(&session.expires_at).unwrap_or(now);
                Some(session_cookie(&token, expires - now))
            }
            Err(e) => {
                tracing::warn!("session rotation failed: {e}");
                None
            }
        }
    }

    /// Re-reads a session that a long-lived connection (the WebSocket)
    /// holds: `None` once it ended, expired, went idle or lost its device.
    /// Counts as activity (keeps the session from going idle).
    pub fn refresh(&self, ctx: &RemoteSessionCtx) -> Option<RemoteSessionCtx> {
        let now = self.now();
        let store = &self.inner.store;
        let session = store.get_remote_session(&ctx.session.id).ok()?;
        if session.ended_at.is_some()
            || parse_rfc3339(&session.expires_at).is_none_or(|end| end <= now)
        {
            return None;
        }
        let device = store.get_remote_device(&session.device_id).ok()?;
        let user = store.get_remote_user(&session.user_id).ok()?;
        if device.revoked_at.is_some() || user.disabled {
            return None;
        }
        let settings = self.settings();
        if self.expire_if_due(&session, &settings, now) {
            return None;
        }
        let mut session = session;
        let last_seen = parse_rfc3339(&session.last_seen_at).unwrap_or_default();
        if now - last_seen >= self.inner.policy.touch_interval_secs {
            let ip = ctx.client.ip_string();
            if let Err(e) = store.touch_remote_session(&session.id, ip.as_deref(), now) {
                tracing::warn!("could not record remote activity: {e}");
            }
            session.last_seen_at = crate::clock::rfc3339(now);
        }
        let reauth_until = Self::reauth_until(&session, &settings);
        Some(RemoteSessionCtx {
            session,
            device,
            client: ctx.client.clone(),
            reauth_until,
        })
    }

    /// Housekeeping, to call every minute or so: ends idle and expired
    /// sessions (announcing them) and drops expired WebAuthn ceremonies.
    /// Returns the ended sessions.
    pub fn sweep(&self) -> Vec<RemoteSessionId> {
        let now = self.now();
        let settings = self.settings();
        let idle = i64::from(settings.session_idle_minutes) * 60;
        let ended = match self.inner.store.expire_remote_sessions(now, idle) {
            Ok(ended) => ended,
            Err(e) => {
                tracing::warn!("expiring remote sessions failed: {e}");
                Vec::new()
            }
        };
        self.announce_ended(&ended, SessionEndReason::Expired);
        self.inner
            .ceremonies
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|_, c| c.expires_at > now);
        ended
    }

    /// Runs [`RemoteAuth::sweep`] every `every` on the current Tokio
    /// runtime until the returned task is aborted.
    pub fn spawn_sweeper(&self, every: std::time::Duration) -> tokio::task::JoinHandle<()> {
        let auth = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(every);
            loop {
                interval.tick().await;
                auth.sweep();
            }
        })
    }

    /// Keeps a new WebAuthn ceremony; `None` when too many are open.
    pub(crate) fn put_ceremony(&self, ceremony: Ceremony) -> Option<String> {
        let now = self.now();
        let mut ceremonies = self
            .inner
            .ceremonies
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        ceremonies.retain(|_, c| c.expires_at > now);
        if ceremonies.len() >= self.inner.policy.max_ceremonies {
            return None;
        }
        let id = crate::secret::random_id();
        ceremonies.insert(id.clone(), ceremony);
        Some(id)
    }

    /// Takes (and so ends) a ceremony; `None` when unknown or expired.
    pub(crate) fn take_ceremony(&self, id: &str) -> Option<Ceremony> {
        let now = self.now();
        self.inner
            .ceremonies
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id)
            .filter(|c| c.expires_at > now)
    }
}
