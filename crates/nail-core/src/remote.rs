//! Requests of wave M2b (ADR 0036): the remote UI's settings and status,
//! users, devices, sessions, pairing codes and audit log, and the
//! remote-content allow-lists.
//!
//! [`Service::dispatch`](crate::service::Service) routes exactly these
//! requests here (one match arm). What needs only the store is served now;
//! what needs `nail-remote-auth` (password hashing, TOTP, pairing-code
//! generation) or `nail-server` (the listener, certificates) answers
//! `unsupported` until WP45 wires those crates in. Filling
//! `MessageBody::remote_content` and rewriting remote URLs to proxy URLs
//! is WP45's as well.
//!
//! The remote UI never runs before WP45, so the status is always
//! `stopped`, and settings are stored but take effect only then.
//! Invalid settings and internet mode are refused already.

use nail_api::remote::{
    RemoteUiSettings, RemoteUiSettingsIssue, RemoteUiStatus, SessionEndReason, internet_mode_lock,
};
use nail_api::remote_content::{RemoteContentScope, normalize_allow_value, sender_domain};
use nail_api::{ApiError, ErrorCode, Event, RemoteSessionId, Request, Response};
use nail_store::REMOTE_UI_SETTINGS_KEY;

use crate::engine::unix_now;
use crate::error::Result;
use crate::service::Service;

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
        Request::RemoveRemoteContentAllow { scope, value } => {
            if store.remove_remote_content_allow(scope, &value)? {
                service.broadcast(Event::RemoteContentPolicyChanged);
            }
            Ok(Response::Done)
        }
        // Wired by WP45: nail-server (certificates) and nail-remote-auth
        // (hashing, TOTP, pairing codes).
        other @ (Request::RegenerateRemoteCertificate
        | Request::SetRemotePassword { .. }
        | Request::StartTotpEnrollment { .. }
        | Request::ConfirmTotpEnrollment { .. }
        | Request::DisableTotp { .. }
        | Request::CreatePairingCode { .. }) => Err(ApiError::unsupported(format!(
            "{} is not available yet (remote UI, M2b)",
            crate::service::request_name(&other)
        ))
        .into()),
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

/// The remote UI's state: not running until WP45 wires the server.
pub(crate) fn remote_ui_status(service: &Service) -> Result<RemoteUiStatus> {
    let mut status = RemoteUiStatus::stopped();
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
