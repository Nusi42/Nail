//! The admin requests that need hashing, the keychain or code generation
//! (ADR 0036: answered `unsupported` by the core until WP45 routes them
//! here): `SetRemotePassword`, `StartTotpEnrollment`,
//! `ConfirmTotpEnrollment`, `DisableTotp` and `CreatePairingCode`.
//!
//! `actor` is the remote session sending the request (`None` on the
//! desktop). Credential changes end the user's other sessions (reason
//! `user-changed`), keeping the actor's own; every change is audited.

use nail_api::remote::{
    RemoteAuditKind, RemoteAuditOutcome, RemoteUser, SessionEndReason, TotpEnrollment,
};
use nail_api::{ApiError, Event, RemoteSessionId, RemoteUserId, Request, Response};

use crate::error::{AuthError, Result};
use crate::service::RemoteAuth;
use crate::session::{ClientMeta, RemoteSessionCtx};
use crate::{pairing, totp};

impl RemoteAuth {
    /// Handles one of the admin requests listed in the module docs; any
    /// other request is `unsupported` (the core serves it).
    pub async fn handle_admin(
        &self,
        request: Request,
        actor: Option<&RemoteSessionCtx>,
    ) -> Result<Response, ApiError> {
        let result = match request {
            Request::SetRemotePassword { user_id, password } => {
                let password = password.map(zeroize::Zeroizing::new);
                self.set_password(&user_id, password.as_deref().map(String::as_str), actor)
                    .await
                    .map(|user| Response::RemoteUserSaved {
                        user: Box::new(user),
                    })
            }
            Request::StartTotpEnrollment { user_id } => self
                .start_totp_enrollment(&user_id)
                .map(|enrollment| Response::TotpEnrollment { enrollment }),
            Request::ConfirmTotpEnrollment { user_id, code } => self
                .confirm_totp_enrollment(&user_id, &code, actor)
                .map(|user| Response::RemoteUserSaved {
                    user: Box::new(user),
                }),
            Request::DisableTotp { user_id } => {
                self.disable_totp(&user_id, actor)
                    .map(|user| Response::RemoteUserSaved {
                        user: Box::new(user),
                    })
            }
            Request::CreatePairingCode {
                user_id,
                device_name,
                grant,
                ttl_minutes,
            } => self.create_pairing_code(
                &user_id,
                device_name.as_deref(),
                grant.as_ref(),
                ttl_minutes,
                actor,
            ),
            _ => {
                return Err(ApiError::unsupported(
                    "not a nail-remote-auth admin request",
                ));
            }
        };
        result.map_err(ApiError::from)
    }

    fn actor_client(actor: Option<&RemoteSessionCtx>) -> ClientMeta {
        actor.map(|a| a.client.clone()).unwrap_or_default()
    }

    fn audit_admin(
        &self,
        kind: RemoteAuditKind,
        user: &RemoteUser,
        actor: Option<&RemoteSessionCtx>,
        detail: &str,
    ) {
        self.audit(
            kind,
            RemoteAuditOutcome::Success,
            &Self::actor_client(actor),
            |r| {
                r.user_id = Some(user.id.clone());
                r.user_name = Some(user.name.clone());
                if let Some(actor) = actor {
                    r.session_id = Some(actor.session.id.clone());
                    r.device_id = Some(actor.session.device_id.clone());
                }
                r.detail = Some(detail.to_owned());
            },
        );
    }

    /// Ends every session of `user` except `keep`.
    fn end_other_sessions(
        &self,
        user: &RemoteUserId,
        keep: Option<&RemoteSessionId>,
    ) -> Result<()> {
        let now = self.now();
        let sessions = self.store().list_remote_sessions(
            &nail_api::remote::RemoteSessionFilter {
                user_id: Some(user.clone()),
                ..Default::default()
            },
            now,
        )?;
        let mut ended = Vec::new();
        for session in sessions {
            if Some(&session.id) == keep {
                continue;
            }
            if self
                .store()
                .end_remote_session(&session.id, SessionEndReason::UserChanged, now)?
            {
                ended.push(session.id);
            }
        }
        self.announce_ended(&ended, SessionEndReason::UserChanged);
        Ok(())
    }

    /// Sets (or with `None` removes) a user's password. The password must
    /// meet the policy; it is hashed with Argon2id and only the hash is
    /// stored. Ends the user's other sessions.
    pub async fn set_password(
        &self,
        user: &RemoteUserId,
        password: Option<&str>,
        actor: Option<&RemoteSessionCtx>,
    ) -> Result<RemoteUser> {
        self.store().get_remote_user(user)?;
        let hash = match password {
            None => None,
            Some(password) => {
                let policy = self.policy().password;
                let password = zeroize::Zeroizing::new(password.to_owned());
                let hash = tokio::task::spawn_blocking(move || policy.hash(&password))
                    .await
                    .map_err(|e| AuthError::Internal(format!("hashing task: {e}")))??;
                Some(hash)
            }
        };
        let saved = self
            .store()
            .set_remote_password_hash(user, hash.as_deref(), self.now())?;
        self.end_other_sessions(user, actor.map(|a| &a.session.id))?;
        self.audit_admin(
            RemoteAuditKind::PasswordChanged,
            &saved,
            actor,
            if hash.is_some() {
                "password set"
            } else {
                "password removed"
            },
        );
        self.emit(Event::RemoteAccessChanged);
        Ok(saved)
    }

    /// Starts a TOTP enrollment: a new secret waits in the keychain until
    /// [`RemoteAuth::confirm_totp_enrollment`]; the answer (shown once)
    /// holds the secret and the `otpauth://` URI.
    pub fn start_totp_enrollment(&self, user: &RemoteUserId) -> Result<TotpEnrollment> {
        let found = self.store().get_remote_user(user)?;
        totp::start_enrollment(self.inner.secrets.as_ref(), user, &found.name)
    }

    /// Confirms a TOTP enrollment with a code from the authenticator app;
    /// from then on the password needs the code. The confirming code
    /// cannot be used again to log in.
    pub fn confirm_totp_enrollment(
        &self,
        user: &RemoteUserId,
        code: &str,
        actor: Option<&RemoteSessionCtx>,
    ) -> Result<RemoteUser> {
        self.store().get_remote_user(user)?;
        let (reference, step) =
            totp::confirm_enrollment(self.inner.secrets.as_ref(), user, code, self.now())?;
        let saved = self
            .store()
            .set_remote_totp(user, Some(reference.as_str()), self.now())?;
        self.store().accept_totp_step(user, step)?;
        self.audit_admin(RemoteAuditKind::TotpChanged, &saved, actor, "TOTP enabled");
        self.emit(Event::RemoteAccessChanged);
        Ok(saved)
    }

    /// Turns TOTP off for a user and deletes the secret. Ends the user's
    /// other sessions.
    pub fn disable_totp(
        &self,
        user: &RemoteUserId,
        actor: Option<&RemoteSessionCtx>,
    ) -> Result<RemoteUser> {
        self.store().get_remote_user(user)?;
        let saved = self.store().set_remote_totp(user, None, self.now())?;
        totp::delete_secrets(self.inner.secrets.as_ref(), user)?;
        self.end_other_sessions(user, actor.map(|a| &a.session.id))?;
        self.audit_admin(RemoteAuditKind::TotpChanged, &saved, actor, "TOTP disabled");
        self.emit(Event::RemoteAccessChanged);
        Ok(saved)
    }

    /// Creates a pairing code (the desktop's approval of the device that
    /// enters it). The answer carries the code (shown once), the address
    /// to open and the certificate fingerprint to compare.
    pub fn create_pairing_code(
        &self,
        user: &RemoteUserId,
        device_name: Option<&str>,
        grant: Option<&nail_api::remote::RemoteGrant>,
        ttl_minutes: Option<u32>,
        actor: Option<&RemoteSessionCtx>,
    ) -> Result<Response> {
        let created = pairing::create(
            self.store(),
            user,
            device_name,
            grant,
            ttl_minutes,
            self.now(),
        )?;
        let owner = self.store().get_remote_user(user)?;
        self.audit_admin(
            RemoteAuditKind::PairingCreated,
            &owner,
            actor,
            &format!("pairing code {} created", created.pairing.id),
        );
        self.emit(Event::RemoteAccessChanged);
        let config = self.config();
        Ok(Response::PairingCodeCreated {
            pairing: created.pairing,
            code: created.code.to_string(),
            url: config.origins.first().map(|o| format!("{o}/")),
            certificate_fingerprint: config.certificate_fingerprint,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_errors_are_invalid_requests() {
        assert_eq!(
            ApiError::from(AuthError::Invalid("x".into())).code,
            nail_api::ErrorCode::InvalidRequest
        );
    }
}
