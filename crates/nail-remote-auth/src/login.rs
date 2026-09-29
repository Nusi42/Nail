//! The browser's login protocol (`POST /api/auth`, [`AuthRequest`] →
//! [`AuthResponse`]), independent of the HTTP framework: the caller passes
//! the cookies, the client and the CSRF header, and sends back the
//! response with the returned `Set-Cookie` values.
//!
//! Rules every step follows:
//!
//! * Every step but `Status` needs the CSRF token of the browser.
//! * Logging in needs a paired device (the device cookie): a new browser
//!   pairs first with a code approved on the desktop.
//! * Attempts are refused without being evaluated while the address or
//!   the account is backing off or locked out; every failure counts.
//! * Failures are coarse (`invalid-credentials` for a wrong name, password,
//!   TOTP code, passkey or a device of another user).
//! * Every login, failed login, pairing, re-authentication, logout and
//!   passkey registration is audited.

use nail_api::remote::{
    AuthFailure, AuthPurpose, AuthRequest, AuthResponse, AuthStatus, LoginMethods, RemoteAlert,
    RemoteAuditKind, RemoteAuditOutcome, RemoteAuthMethod, RemoteDevice, RemoteSession, RemoteUser,
    SESSION_COOKIE, SessionEndReason,
};
use nail_api::{ErrorCode, Event};
use nail_store::{NewPasskey, NewRemoteDevice, NewRemoteSession, hash_remote_secret};
use zeroize::Zeroizing;

use crate::clock::{parse_rfc3339, rfc3339};
use crate::passkey::{self, StoredCredential, StoredCredentialRef};
use crate::service::{Ceremony, CeremonyKind, NoSession, RemoteAuth, Resolved};
use crate::session::{
    ClientMeta, Cookies, clean_label, clear_cookie, device_cookie, session_cookie,
};
use crate::throttle::{GLOBAL_PAIRING_KEY, Gate, ip_key, user_key};
use crate::{pairing, password, totp};

/// The answer to one login-protocol request.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthReply {
    /// The JSON answer (HTTP 200).
    pub response: AuthResponse,
    /// `Set-Cookie` header values to send with it.
    pub cookies: Vec<String>,
    /// A new CSRF token when the step changed the browser's binding (a
    /// pairing binds it to the new device); sent as the [`CSRF_HEADER`]
    /// response header so the browser need not ask `Status` again.
    ///
    /// [`CSRF_HEADER`]: nail_api::remote::CSRF_HEADER
    pub csrf_token: Option<String>,
}

impl AuthReply {
    fn new(response: AuthResponse) -> Self {
        Self {
            response,
            cookies: Vec::new(),
            csrf_token: None,
        }
    }

    fn failed(reason: AuthFailure) -> Self {
        Self::new(AuthResponse::Failed {
            reason,
            retry_after_secs: None,
        })
    }

    fn with_cookie(mut self, cookie: Option<String>) -> Self {
        self.cookies.extend(cookie);
        self
    }
}

/// `Failed` for a closed gate.
fn gate_reply(gate: Gate) -> Option<AuthReply> {
    match gate {
        Gate::Open => None,
        Gate::Wait {
            retry_after_secs,
            locked,
        } => Some(AuthReply::new(AuthResponse::Failed {
            reason: if locked {
                AuthFailure::LockedOut
            } else {
                AuthFailure::RateLimited
            },
            retry_after_secs: Some(retry_after_secs),
        })),
    }
}

fn audit_kind(purpose: AuthPurpose) -> RemoteAuditKind {
    match purpose {
        AuthPurpose::Login => RemoteAuditKind::Login,
        AuthPurpose::Reauth => RemoteAuditKind::Reauth,
    }
}

/// Who is asking, established before the credentials are checked.
enum Asker {
    /// Logging in from this paired device.
    Device(Box<RemoteDevice>),
    /// Re-authenticating this session.
    Session(Box<Resolved>),
}

impl Asker {
    fn user_id(&self) -> &nail_api::RemoteUserId {
        match self {
            Asker::Device(device) => &device.user_id,
            Asker::Session(resolved) => &resolved.ctx.session.user_id,
        }
    }
}

/// A failed passkey check (the user when the credential is known).
fn passkey_failure<'a>(
    detail: &'static str,
    purpose: AuthPurpose,
    user: Option<&'a RemoteUser>,
    client: &ClientMeta,
) -> Failure<'a> {
    let ip = ip_key(client.ip);
    Failure {
        keys: match user {
            Some(user) => vec![ip, user_key(&user.name)],
            None => vec![ip],
        },
        user,
        tried_name: user.map(|u| u.name.as_str()),
        method: RemoteAuthMethod::Passkey,
        purpose,
        detail,
    }
}

/// A credential check that failed, for [`RemoteAuth::credential_failure`].
struct Failure<'a> {
    keys: Vec<String>,
    user: Option<&'a RemoteUser>,
    tried_name: Option<&'a str>,
    method: RemoteAuthMethod,
    purpose: AuthPurpose,
    detail: &'static str,
}

impl RemoteAuth {
    /// Whether the `Origin` of a login request is acceptable: one of the
    /// configured origins, or (none configured) the request's own `Host`.
    /// A missing `Origin` is refused (browsers send it on every `POST`).
    pub fn origin_allowed(&self, origin: Option<&str>, host: Option<&str>) -> bool {
        let Some(origin) = origin.map(|o| o.trim().trim_end_matches('/')) else {
            return false;
        };
        let config = self.config();
        if !config.origins.is_empty() {
            return config
                .origins
                .iter()
                .any(|allowed| allowed.trim_end_matches('/').eq_ignore_ascii_case(origin));
        }
        let Some((scheme, authority)) = origin.split_once("://") else {
            return false;
        };
        matches!(scheme, "https" | "http")
            && host.is_some_and(|host| host.trim().eq_ignore_ascii_case(authority))
    }

    /// Handles one login-protocol request.
    pub async fn handle_auth(
        &self,
        request: AuthRequest,
        cookies: &Cookies,
        client: &ClientMeta,
        csrf: Option<&str>,
    ) -> AuthReply {
        if !matches!(request, AuthRequest::Status) && !self.check_csrf(cookies, csrf) {
            return AuthReply::failed(AuthFailure::BadRequest);
        }
        match request {
            AuthRequest::Status => self.status(cookies, client),
            AuthRequest::Pair { code, device_name } => {
                let code = Zeroizing::new(code);
                self.pair(&code, &device_name, cookies, client)
            }
            AuthRequest::Password {
                user_name,
                password,
                totp_code,
                purpose,
            } => {
                let password = Zeroizing::new(password);
                let totp_code = totp_code.map(Zeroizing::new);
                self.password(
                    &user_name,
                    password,
                    totp_code.as_deref().map(String::as_str),
                    purpose,
                    cookies,
                    client,
                )
                .await
            }
            AuthRequest::PasskeyStart { user_name, purpose } => {
                self.passkey_start(user_name, purpose, cookies, client)
            }
            AuthRequest::PasskeyFinish {
                challenge_id,
                credential,
            } => self.passkey_finish(&challenge_id, &credential, cookies, client),
            AuthRequest::PasskeyRegisterStart { name } => {
                self.passkey_register_start(&name, cookies, client)
            }
            AuthRequest::PasskeyRegisterFinish {
                challenge_id,
                credential,
            } => self.passkey_register_finish(&challenge_id, &credential, cookies, client),
            AuthRequest::Logout => self.logout(cookies, client),
        }
    }

    // --- Status ---

    fn status(&self, cookies: &Cookies, client: &ClientMeta) -> AuthReply {
        let now = self.now();
        let mut out = Vec::new();
        let (session, reauth_until) = match self.resolve(cookies, client) {
            Ok(resolved) => {
                out.extend(resolved.set_cookie);
                let until = resolved.ctx.reauth_until;
                (
                    Some(resolved.ctx.session),
                    (until > now).then(|| rfc3339(until)),
                )
            }
            Err(NoSession::Invalid) => {
                out.push(clear_cookie(SESSION_COOKIE));
                (None, None)
            }
            Err(NoSession::Missing) => (None, None),
        };
        let config = self.config();
        AuthReply {
            response: AuthResponse::Status {
                status: AuthStatus {
                    authenticated: session.is_some(),
                    paired: self.device_of(cookies).is_some(),
                    session,
                    reauth_until,
                    csrf_token: self.csrf_token(cookies),
                    methods: LoginMethods {
                        passkey: config.relying_party.is_some(),
                        password: true,
                    },
                    certificate_fingerprint: config.certificate_fingerprint,
                },
            },
            cookies: out,
            csrf_token: None,
        }
    }

    // --- Sessions ---

    /// Starts a session for `user` on `device`.
    fn start_session(
        &self,
        user: &RemoteUser,
        device: &RemoteDevice,
        method: RemoteAuthMethod,
        client: &ClientMeta,
    ) -> crate::Result<(RemoteSession, String)> {
        let now = self.now();
        let settings = self.settings();
        let token = crate::secret::random_token();
        let expires_at = now + i64::from(settings.session_max_hours) * 3600;
        let session = self.store().create_remote_session(
            &NewRemoteSession {
                token_hash: hash_remote_secret(&token),
                user_id: user.id.clone(),
                device_id: device.id.clone(),
                auth_method: method,
                grant: device.grant.clone(),
                expires_at,
                ip: client.ip_string(),
                user_agent: client.user_agent(),
            },
            now,
        )?;
        self.store().record_remote_login(&user.id, now)?;
        self.inner
            .rotations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session.id.clone(), now);
        Ok((session, session_cookie(&token, expires_at - now)))
    }

    /// Ends the session this browser still holds (a new login replaces it).
    fn end_current(&self, cookies: &Cookies, client: &ClientMeta) {
        if let Ok(resolved) = self.resolve(cookies, client) {
            let id = resolved.ctx.session.id;
            if let Ok(true) =
                self.store()
                    .end_remote_session(&id, SessionEndReason::Logout, self.now())
            {
                self.announce_ended(&[id], SessionEndReason::Logout);
            }
        }
    }

    fn finish_login(
        &self,
        user: &RemoteUser,
        device: &RemoteDevice,
        method: RemoteAuthMethod,
        cookies: &Cookies,
        client: &ClientMeta,
    ) -> AuthReply {
        self.end_current(cookies, client);
        match self.start_session(user, device, method, client) {
            Ok((session, cookie)) => {
                self.audit(
                    RemoteAuditKind::Login,
                    RemoteAuditOutcome::Success,
                    client,
                    |r| {
                        r.user_id = Some(user.id.clone());
                        r.user_name = Some(user.name.clone());
                        r.device_id = Some(device.id.clone());
                        r.session_id = Some(session.id.clone());
                        r.method = Some(method);
                    },
                );
                self.emit(Event::RemoteAccessChanged);
                AuthReply::new(AuthResponse::LoggedIn { session }).with_cookie(Some(cookie))
            }
            Err(e) => {
                tracing::warn!("could not start a remote session: {e}");
                AuthReply::failed(AuthFailure::BadRequest)
            }
        }
    }

    fn finish_reauth(
        &self,
        resolved: &Resolved,
        method: RemoteAuthMethod,
        client: &ClientMeta,
    ) -> AuthReply {
        let now = self.now();
        let session = &resolved.ctx.session;
        if let Err(e) = self.store().set_remote_reauth(&session.id, now) {
            tracing::warn!("could not record a re-authentication: {e}");
            return AuthReply::failed(AuthFailure::NotLoggedIn);
        }
        self.audit(
            RemoteAuditKind::Reauth,
            RemoteAuditOutcome::Success,
            client,
            |r| {
                r.user_id = Some(session.user_id.clone());
                r.user_name = Some(session.user_name.clone());
                r.device_id = Some(session.device_id.clone());
                r.session_id = Some(session.id.clone());
                r.method = Some(method);
            },
        );
        let until = now + i64::from(self.settings().reauth_minutes) * 60;
        AuthReply::new(AuthResponse::Reauthenticated {
            reauth_until: rfc3339(until),
        })
        .with_cookie(resolved.set_cookie.clone())
    }

    /// Establishes who asks: the session (re-authentication) or the
    /// paired device (login).
    fn asker(
        &self,
        purpose: AuthPurpose,
        cookies: &Cookies,
        client: &ClientMeta,
    ) -> Result<Asker, AuthFailure> {
        match purpose {
            AuthPurpose::Reauth => self
                .resolve(cookies, client)
                .map(|r| Asker::Session(Box::new(r)))
                .map_err(|_| AuthFailure::NotLoggedIn),
            AuthPurpose::Login => self
                .device_of(cookies)
                .map(|d| Asker::Device(Box::new(d)))
                .ok_or(AuthFailure::NotPaired),
        }
    }

    fn user_locked(&self, user: &RemoteUser) -> Option<AuthReply> {
        let now = self.now();
        let until = user.locked_until.as_deref().and_then(parse_rfc3339)?;
        (until > now).then(|| {
            AuthReply::new(AuthResponse::Failed {
                reason: AuthFailure::LockedOut,
                retry_after_secs: Some(u32::try_from(until - now).unwrap_or(u32::MAX)),
            })
        })
    }

    /// Counts a failed credential check, audits it, reports lockouts and
    /// builds the answer.
    fn credential_failure(&self, failure: Failure<'_>, client: &ClientMeta) -> AuthReply {
        let now = self.now();
        let throttle = &self.policy().throttle;
        let lockouts = throttle
            .record_failure(self.store(), &failure.keys, now)
            .unwrap_or_else(|e| {
                tracing::warn!("could not count a failed login: {e}");
                Vec::new()
            });
        self.audit(
            audit_kind(failure.purpose),
            RemoteAuditOutcome::Failure,
            client,
            |r| {
                r.user_id = failure.user.map(|u| u.id.clone());
                r.user_name = failure.tried_name.map(str::to_owned);
                r.method = Some(failure.method);
                r.error = Some(ErrorCode::AuthFailed);
                r.detail = Some(failure.detail.to_owned());
            },
        );
        self.report_lockouts(&lockouts, failure.user, failure.tried_name, client);
        if let Some(until) = lockouts.iter().map(|l| l.until).max() {
            return AuthReply::new(AuthResponse::Failed {
                reason: AuthFailure::LockedOut,
                retry_after_secs: Some(u32::try_from(until - now).unwrap_or(u32::MAX)),
            });
        }
        let retry_after_secs = match throttle.check(self.store(), &failure.keys, now) {
            Ok(Gate::Wait {
                retry_after_secs, ..
            }) => Some(retry_after_secs),
            _ => None,
        };
        AuthReply::new(AuthResponse::Failed {
            reason: AuthFailure::InvalidCredentials,
            retry_after_secs,
        })
    }

    // --- Pairing ---

    fn pair(
        &self,
        code: &str,
        device_name: &str,
        cookies: &Cookies,
        client: &ClientMeta,
    ) -> AuthReply {
        let now = self.now();
        let keys = vec![ip_key(client.ip), GLOBAL_PAIRING_KEY.to_owned()];
        match self.policy().throttle.check(self.store(), &keys, now) {
            Ok(gate) => {
                if let Some(reply) = gate_reply(gate) {
                    return reply;
                }
            }
            Err(e) => {
                tracing::warn!("throttle unreadable: {e}");
                return AuthReply::failed(AuthFailure::RateLimited);
            }
        }
        // Check the name before the code is spent.
        let Ok(browser_name) = clean_label(device_name) else {
            return AuthReply::failed(AuthFailure::BadRequest);
        };
        let redeemed = pairing::normalize(code).and_then(|normalized| {
            self.store()
                .redeem_pairing_code(&pairing::code_hash(&normalized), now)
                .ok()
        });
        let Some(code) = redeemed else {
            let lockouts = self
                .policy()
                .throttle
                .record_failure(self.store(), &keys, now)
                .unwrap_or_default();
            self.audit(
                RemoteAuditKind::Pairing,
                RemoteAuditOutcome::Failure,
                client,
                |r| {
                    r.method = Some(RemoteAuthMethod::Pairing);
                    r.error = Some(ErrorCode::AuthFailed);
                    r.detail = Some("wrong, used or expired pairing code".into());
                },
            );
            self.report_lockouts(&lockouts, None, None, client);
            let retry_after_secs = match self.policy().throttle.check(self.store(), &keys, now) {
                Ok(Gate::Wait {
                    retry_after_secs, ..
                }) => Some(retry_after_secs),
                _ => None,
            };
            return AuthReply::new(AuthResponse::Failed {
                reason: AuthFailure::InvalidCode,
                retry_after_secs,
            });
        };
        let result = (|| -> crate::Result<AuthReply> {
            let store = self.store();
            let user = store.get_remote_user(&code.user_id)?;
            let secret = crate::secret::random_token();
            let device = store.create_remote_device(
                &NewRemoteDevice {
                    user_id: user.id.clone(),
                    name: code.device_name.clone().unwrap_or(browser_name),
                    secret_hash: hash_remote_secret(&secret),
                    grant: code.grant.clone(),
                    user_agent: client.user_agent(),
                    ip: client.ip_string(),
                },
                now,
            )?;
            store.set_pairing_device(&code.id, &device.id)?;
            self.end_current(cookies, client);
            let (session, session_cookie) =
                self.start_session(&user, &device, RemoteAuthMethod::Pairing, client)?;
            // The address's count and the global pairing count: a pairing
            // went through, so a pause must not outlive it.
            if let Err(e) = self.policy().throttle.clear(store, &keys) {
                tracing::warn!("could not clear the throttle: {e}");
            }
            self.audit(
                RemoteAuditKind::Pairing,
                RemoteAuditOutcome::Success,
                client,
                |r| {
                    r.user_id = Some(user.id.clone());
                    r.user_name = Some(user.name.clone());
                    r.device_id = Some(device.id.clone());
                    r.session_id = Some(session.id.clone());
                    r.method = Some(RemoteAuthMethod::Pairing);
                    r.name = Some(device.name.clone());
                },
            );
            self.emit(Event::RemoteSecurityAlert {
                alert: RemoteAlert::NewDevice {
                    device_id: device.id.clone(),
                    device_name: device.name.clone(),
                    user_name: user.name.clone(),
                    ip: client.ip_string(),
                },
            });
            self.emit(Event::RemoteAccessChanged);
            Ok(AuthReply {
                response: AuthResponse::LoggedIn { session },
                cookies: vec![
                    device_cookie(&secret, self.policy().device_cookie_secs),
                    session_cookie,
                ],
                csrf_token: Some(self.inner.csrf.token(&format!("device:{}", device.id))),
            })
        })();
        result.unwrap_or_else(|e| {
            tracing::warn!("pairing failed after the code was accepted: {e}");
            AuthReply::failed(AuthFailure::InvalidCode)
        })
    }

    // --- Password (+ TOTP) ---

    async fn password(
        &self,
        user_name: &str,
        password: Zeroizing<String>,
        totp_code: Option<&str>,
        purpose: AuthPurpose,
        cookies: &Cookies,
        client: &ClientMeta,
    ) -> AuthReply {
        let now = self.now();
        let tried = user_name.trim();
        let keys = vec![ip_key(client.ip), user_key(tried)];
        let asker = match self.asker(purpose, cookies, client) {
            Ok(asker) => asker,
            Err(reason) => return AuthReply::failed(reason),
        };
        match self.policy().throttle.check(self.store(), &keys, now) {
            Ok(gate) => {
                if let Some(reply) = gate_reply(gate) {
                    return reply;
                }
            }
            Err(e) => {
                tracing::warn!("throttle unreadable: {e}");
                return AuthReply::failed(AuthFailure::RateLimited);
            }
        }
        let user = self.store().remote_user_by_name(tried).ok().flatten();
        if let Some(reply) = user.as_ref().and_then(|u| self.user_locked(u)) {
            return reply;
        }
        let credentials = user
            .as_ref()
            .and_then(|u| self.store().remote_credentials(&u.id).ok())
            .unwrap_or_default();
        let hash = credentials.password_hash.clone();
        let cost = self.policy().password.cost;
        let verified =
            tokio::task::spawn_blocking(move || password::verify(&password, hash.as_deref(), cost))
                .await
                .unwrap_or(false);
        let failure = |detail| Failure {
            keys: keys.clone(),
            user: user.as_ref(),
            tried_name: Some(tried),
            method: RemoteAuthMethod::Password,
            purpose,
            detail,
        };
        let Some(found) = user.as_ref().filter(|_| verified) else {
            return self.credential_failure(failure("wrong name or password"), client);
        };
        if found.id != *asker.user_id() {
            // A device (or session) of another user: the same answer as a
            // wrong password.
            return self.credential_failure(failure("device of another user"), client);
        }
        if found.disabled {
            self.audit(
                audit_kind(purpose),
                RemoteAuditOutcome::Denied,
                client,
                |r| {
                    r.user_id = Some(found.id.clone());
                    r.user_name = Some(found.name.clone());
                    r.error = Some(ErrorCode::Forbidden);
                    r.detail = Some("user disabled".into());
                },
            );
            return AuthReply::failed(AuthFailure::Disabled);
        }
        let method = match credentials.totp_ref.as_deref() {
            None => RemoteAuthMethod::Password,
            Some(reference) => {
                let Some(code) = totp_code else {
                    return AuthReply::new(AuthResponse::TotpRequired);
                };
                let secret = match totp::load_secret(self.inner.secrets.as_ref(), reference) {
                    Ok(Some(secret)) => secret,
                    Ok(None) | Err(_) => {
                        tracing::warn!("the TOTP secret of {} is missing", found.id);
                        return self.credential_failure(failure("TOTP secret unavailable"), client);
                    }
                };
                let Some(step) = totp::matching_step(&secret, code, now) else {
                    return self.credential_failure(
                        Failure {
                            method: RemoteAuthMethod::PasswordTotp,
                            ..failure("wrong TOTP code")
                        },
                        client,
                    );
                };
                if !self
                    .store()
                    .accept_totp_step(&found.id, step)
                    .unwrap_or(false)
                {
                    return self.credential_failure(
                        Failure {
                            method: RemoteAuthMethod::PasswordTotp,
                            ..failure("TOTP code used before")
                        },
                        client,
                    );
                }
                RemoteAuthMethod::PasswordTotp
            }
        };
        if let Err(e) = self.policy().throttle.clear(self.store(), &keys) {
            tracing::warn!("could not clear the throttle: {e}");
        }
        match asker {
            Asker::Device(device) => self.finish_login(found, &device, method, cookies, client),
            Asker::Session(resolved) => self.finish_reauth(&resolved, method, client),
        }
    }

    // --- Passkeys ---

    fn passkey_start(
        &self,
        user_name: Option<String>,
        purpose: AuthPurpose,
        cookies: &Cookies,
        client: &ClientMeta,
    ) -> AuthReply {
        let Some(rp) = self.config().relying_party else {
            return AuthReply::failed(AuthFailure::BadRequest);
        };
        let now = self.now();
        let keys = [ip_key(client.ip)];
        if let Some(reply) = self
            .policy()
            .throttle
            .check(self.store(), &keys, now)
            .ok()
            .and_then(gate_reply)
        {
            return reply;
        }
        let asker = match self.asker(purpose, cookies, client) {
            Ok(asker) => asker,
            Err(reason) => return AuthReply::failed(reason),
        };
        let Ok((ceremony, options)) = passkey::start_login(&rp) else {
            return AuthReply::failed(AuthFailure::BadRequest);
        };
        let (session, device) = match &asker {
            Asker::Device(device) => (None, Some(device.id.clone())),
            Asker::Session(resolved) => (
                Some(resolved.ctx.session.id.clone()),
                Some(resolved.ctx.device.id.clone()),
            ),
        };
        let stored = self.put_ceremony(Ceremony {
            kind: CeremonyKind::Login {
                ceremony,
                purpose,
                user_name: user_name
                    .map(|n| n.trim().to_owned())
                    .filter(|n| !n.is_empty()),
            },
            expires_at: now + self.policy().ceremony_ttl_secs,
            session,
            device,
        });
        match stored {
            Some(challenge_id) => AuthReply::new(AuthResponse::PasskeyChallenge {
                challenge_id,
                options,
            }),
            None => AuthReply::new(AuthResponse::Failed {
                reason: AuthFailure::RateLimited,
                retry_after_secs: Some(30),
            }),
        }
    }

    fn passkey_finish(
        &self,
        challenge_id: &str,
        credential: &serde_json::Value,
        cookies: &Cookies,
        client: &ClientMeta,
    ) -> AuthReply {
        let now = self.now();
        let ip_keys = vec![ip_key(client.ip)];
        if let Some(reply) = self
            .policy()
            .throttle
            .check(self.store(), &ip_keys, now)
            .ok()
            .and_then(gate_reply)
        {
            return reply;
        }
        let failure = passkey_failure;
        let Some(ceremony) = self.take_ceremony(challenge_id) else {
            let reply = self.credential_failure(
                failure(
                    "unknown or expired challenge",
                    AuthPurpose::Login,
                    None,
                    client,
                ),
                client,
            );
            return match reply.response {
                AuthResponse::Failed {
                    reason: AuthFailure::InvalidCredentials,
                    retry_after_secs,
                } => AuthReply::new(AuthResponse::Failed {
                    reason: AuthFailure::ChallengeExpired,
                    retry_after_secs,
                }),
                _ => reply,
            };
        };
        let CeremonyKind::Login {
            ceremony: login,
            purpose,
            user_name,
        } = ceremony.kind
        else {
            return AuthReply::failed(AuthFailure::BadRequest);
        };
        let Some(rp) = self.config().relying_party else {
            return AuthReply::failed(AuthFailure::BadRequest);
        };
        // The browser finishing must be the one that started.
        let asker = match self.asker(purpose, cookies, client) {
            Ok(asker) => asker,
            Err(reason) => return AuthReply::failed(reason),
        };
        let same = match &asker {
            Asker::Device(device) => ceremony.device.as_ref() == Some(&device.id),
            Asker::Session(resolved) => ceremony.session.as_ref() == Some(&resolved.ctx.session.id),
        };
        if !same {
            return AuthReply::failed(AuthFailure::BadRequest);
        }
        let Ok(credential_id) = passkey::login_credential_id(credential) else {
            return self.credential_failure(
                failure("malformed assertion", purpose, None, client),
                client,
            );
        };
        let stored_passkey = self
            .store()
            .passkey_by_credential_id(&credential_id)
            .ok()
            .flatten();
        let Some(stored_passkey) = stored_passkey else {
            return self
                .credential_failure(failure("unknown passkey", purpose, None, client), client);
        };
        let Ok(user) = self.store().get_remote_user(&stored_passkey.user_id) else {
            return self
                .credential_failure(failure("unknown passkey", purpose, None, client), client);
        };
        let verified = StoredCredential::parse(&stored_passkey.credential).and_then(|stored| {
            passkey::finish_login(&rp, login, credential, &credential_id, &stored)
        });
        let updated = match verified {
            Ok(updated) => updated,
            Err(e) => {
                tracing::info!("passkey login refused: {e}");
                return self.credential_failure(
                    failure("passkey verification failed", purpose, Some(&user), client),
                    client,
                );
            }
        };
        if let Err(e) = self.store().update_passkey_credential(
            &stored_passkey.passkey.id,
            &updated.to_json(),
            now,
        ) {
            tracing::warn!("could not store the passkey counter: {e}");
            return AuthReply::failed(AuthFailure::BadRequest);
        }
        let named_other = user_name
            .as_deref()
            .is_some_and(|name| !name.eq_ignore_ascii_case(&user.name));
        if named_other || user.id != *asker.user_id() {
            return self.credential_failure(
                failure("passkey of another user", purpose, Some(&user), client),
                client,
            );
        }
        if let Some(reply) = self.user_locked(&user) {
            return reply;
        }
        if user.disabled {
            return AuthReply::failed(AuthFailure::Disabled);
        }
        let keys = [ip_keys[0].clone(), user_key(&user.name)];
        if let Err(e) = self.policy().throttle.clear(self.store(), &keys) {
            tracing::warn!("could not clear the throttle: {e}");
        }
        match asker {
            Asker::Device(device) => {
                self.finish_login(&user, &device, RemoteAuthMethod::Passkey, cookies, client)
            }
            Asker::Session(resolved) => {
                self.finish_reauth(&resolved, RemoteAuthMethod::Passkey, client)
            }
        }
    }

    fn passkey_register_start(
        &self,
        name: &str,
        cookies: &Cookies,
        client: &ClientMeta,
    ) -> AuthReply {
        let Ok(resolved) = self.resolve(cookies, client) else {
            return AuthReply::failed(AuthFailure::NotLoggedIn);
        };
        let now = self.now();
        if !resolved.ctx.is_fresh(now) {
            return AuthReply::failed(AuthFailure::ReauthRequired).with_cookie(resolved.set_cookie);
        }
        let Some(rp) = self.config().relying_party else {
            return AuthReply::failed(AuthFailure::BadRequest);
        };
        let Ok(name) = clean_label(name) else {
            return AuthReply::failed(AuthFailure::BadRequest);
        };
        let session = &resolved.ctx.session;
        let started = (|| -> crate::Result<_> {
            let user = self.store().get_remote_user(&session.user_id)?;
            let existing = self.store().passkeys_of_user(&user.id)?;
            let parsed: Vec<(Vec<u8>, StoredCredential)> = existing
                .into_iter()
                .filter_map(|p| {
                    StoredCredential::parse(&p.credential)
                        .ok()
                        .map(|s| (p.credential_id, s))
                })
                .collect();
            let handle = parsed
                .first()
                .map(|(_, s)| s.user_handle.clone())
                .unwrap_or_else(passkey::new_user_handle);
            let refs: Vec<StoredCredentialRef<'_>> = parsed
                .iter()
                .map(|(id, stored)| StoredCredentialRef {
                    credential_id: id,
                    stored,
                })
                .collect();
            passkey::start_registration(
                &rp,
                &handle,
                &user.name,
                user.display_name.as_deref(),
                &refs,
            )
        })();
        let Ok((ceremony, options)) = started else {
            return AuthReply::failed(AuthFailure::BadRequest);
        };
        let stored = self.put_ceremony(Ceremony {
            kind: CeremonyKind::Register { ceremony, name },
            expires_at: now + self.policy().ceremony_ttl_secs,
            session: Some(session.id.clone()),
            device: Some(resolved.ctx.device.id.clone()),
        });
        let reply = match stored {
            Some(challenge_id) => AuthReply::new(AuthResponse::PasskeyChallenge {
                challenge_id,
                options,
            }),
            None => AuthReply::new(AuthResponse::Failed {
                reason: AuthFailure::RateLimited,
                retry_after_secs: Some(30),
            }),
        };
        reply.with_cookie(resolved.set_cookie)
    }

    fn passkey_register_finish(
        &self,
        challenge_id: &str,
        credential: &serde_json::Value,
        cookies: &Cookies,
        client: &ClientMeta,
    ) -> AuthReply {
        let Ok(resolved) = self.resolve(cookies, client) else {
            return AuthReply::failed(AuthFailure::NotLoggedIn);
        };
        let Some(ceremony) = self.take_ceremony(challenge_id) else {
            return AuthReply::failed(AuthFailure::ChallengeExpired);
        };
        let session = &resolved.ctx.session;
        let CeremonyKind::Register {
            ceremony: registration,
            name,
        } = ceremony.kind
        else {
            return AuthReply::failed(AuthFailure::BadRequest);
        };
        if ceremony.session.as_ref() != Some(&session.id) {
            return AuthReply::failed(AuthFailure::BadRequest);
        }
        let Some(rp) = self.config().relying_party else {
            return AuthReply::failed(AuthFailure::BadRequest);
        };
        let audit_failure = |detail: String| {
            self.audit(
                RemoteAuditKind::PasskeyChanged,
                RemoteAuditOutcome::Failure,
                client,
                |r| {
                    r.user_id = Some(session.user_id.clone());
                    r.user_name = Some(session.user_name.clone());
                    r.device_id = Some(session.device_id.clone());
                    r.session_id = Some(session.id.clone());
                    r.name = Some(name.clone());
                    r.error = Some(ErrorCode::AuthFailed);
                    r.detail = Some(detail);
                },
            );
        };
        let new = match passkey::finish_registration(&rp, registration, credential) {
            Ok(new) => new,
            Err(e) => {
                audit_failure(e.to_string());
                return AuthReply::failed(AuthFailure::InvalidCredentials);
            }
        };
        let added = self.store().add_passkey(
            &NewPasskey {
                user_id: session.user_id.clone(),
                credential_id: new.credential_id,
                name: name.clone(),
                credential: new.stored.to_json(),
                device_id: Some(session.device_id.clone()),
            },
            self.now(),
        );
        match added {
            Ok(stored) => {
                self.audit(
                    RemoteAuditKind::PasskeyChanged,
                    RemoteAuditOutcome::Success,
                    client,
                    |r| {
                        r.user_id = Some(session.user_id.clone());
                        r.user_name = Some(session.user_name.clone());
                        r.device_id = Some(session.device_id.clone());
                        r.session_id = Some(session.id.clone());
                        r.name = Some(name.clone());
                        r.detail = Some(format!("registered {}", stored.passkey.id));
                    },
                );
                self.emit(Event::RemoteAccessChanged);
                AuthReply::new(AuthResponse::PasskeyRegistered {
                    passkey: stored.passkey,
                })
                .with_cookie(resolved.set_cookie)
            }
            Err(e) => {
                audit_failure(e.to_string());
                AuthReply::failed(AuthFailure::InvalidCredentials)
            }
        }
    }

    // --- Logout ---

    fn logout(&self, cookies: &Cookies, client: &ClientMeta) -> AuthReply {
        if let Ok(resolved) = self.resolve(cookies, client) {
            let session = resolved.ctx.session;
            if let Ok(true) =
                self.store()
                    .end_remote_session(&session.id, SessionEndReason::Logout, self.now())
            {
                self.announce_ended(std::slice::from_ref(&session.id), SessionEndReason::Logout);
            }
            self.audit(
                RemoteAuditKind::Logout,
                RemoteAuditOutcome::Success,
                client,
                |r| {
                    r.user_id = Some(session.user_id.clone());
                    r.user_name = Some(session.user_name.clone());
                    r.device_id = Some(session.device_id.clone());
                    r.session_id = Some(session.id.clone());
                },
            );
        }
        AuthReply {
            response: AuthResponse::LoggedOut,
            cookies: vec![clear_cookie(SESSION_COOKIE)],
            csrf_token: None,
        }
    }
}
