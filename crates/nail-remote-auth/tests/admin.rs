//! The admin requests the core routes here: passwords, TOTP, pairing
//! codes.

#![cfg(test)]

#[expect(
    dead_code,
    reason = "the shared test helpers serve several test crates; each uses a subset"
)]
mod common;

use common::*;
use nail_api::engine::SecretStore as _;
use nail_api::remote::{
    AuthRequest, AuthResponse, PairingState, RemoteAuditKind, SessionEndReason,
};
use nail_api::{ErrorCode, Event, Request, Response};
use nail_remote_auth::totp;
use totp_rs::Secret;

#[tokio::test]
async fn set_password_hashes_and_ends_other_sessions() {
    let h = Harness::new();
    let ada = h.user("ada", true).await;
    let hash = h
        .store
        .remote_credentials(&ada.id)
        .unwrap()
        .password_hash
        .unwrap();
    assert!(hash.starts_with("$argon2id$"));
    assert!(!hash.contains(PASSWORD));

    let mut phone = h.paired(&ada, "192.0.2.10").await;
    let mut laptop = h.paired(&ada, "192.0.2.11").await;
    let phone_ctx = phone.resolve(&h).unwrap();
    let laptop_ctx = laptop.resolve(&h).unwrap();
    h.events();

    // Changed from the phone: the laptop is logged out, the phone stays.
    let response = h
        .auth
        .handle_admin(
            Request::SetRemotePassword {
                user_id: ada.id.clone(),
                password: Some("a brand new passphrase".into()),
            },
            Some(&phone_ctx),
        )
        .await
        .unwrap();
    assert!(matches!(response, Response::RemoteUserSaved { .. }));
    assert!(phone.resolve(&h).is_some());
    assert!(laptop.resolve(&h).is_none());
    assert_eq!(
        h.store
            .get_remote_session(&laptop_ctx.session.id)
            .unwrap()
            .end_reason,
        Some(SessionEndReason::UserChanged)
    );
    let events = h.events();
    assert!(events.contains(&Event::RemoteSessionEnded {
        session_id: laptop_ctx.session.id.clone(),
        reason: SessionEndReason::UserChanged
    }));
    let audit = h.store.list_remote_audit(Some(&ada.id), None, 5).unwrap();
    assert!(
        audit
            .iter()
            .any(|e| e.kind == RemoteAuditKind::PasswordChanged
                && e.session_id.as_ref() == Some(&phone_ctx.session.id))
    );

    // The old password no longer works, the new one does.
    phone.send(&h, AuthRequest::Logout).await;
    let reply = phone.password(&h, "ada", PASSWORD, None).await;
    let _ = failure(&reply);
    logged_in(
        &phone
            .password(&h, "ada", "a brand new passphrase", None)
            .await,
    );

    // Weak passwords are refused; removing the password works.
    let error = h
        .auth
        .handle_admin(
            Request::SetRemotePassword {
                user_id: ada.id.clone(),
                password: Some("short".into()),
            },
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    let user = h.auth.set_password(&ada.id, None, None).await.unwrap();
    assert!(!user.has_password);
    // Unknown user.
    let error = h
        .auth
        .handle_admin(
            Request::SetRemotePassword {
                user_id: "ruser-99".into(),
                password: Some("long enough password".into()),
            },
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[tokio::test]
async fn totp_enrollment_through_admin_requests() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let Response::TotpEnrollment { enrollment } = h
        .auth
        .handle_admin(
            Request::StartTotpEnrollment {
                user_id: ada.id.clone(),
            },
            None,
        )
        .await
        .unwrap()
    else {
        panic!("no enrollment")
    };
    // Nothing enabled before the confirmation.
    assert!(!h.store.get_remote_user(&ada.id).unwrap().totp_enabled);
    let secret = Secret::try_from_base32(&enrollment.secret)
        .unwrap()
        .as_bytes()
        .to_vec();
    let wrong = h
        .auth
        .handle_admin(
            Request::ConfirmTotpEnrollment {
                user_id: ada.id.clone(),
                code: "12345".into(),
            },
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(wrong.code, ErrorCode::InvalidRequest);
    let code = totp::code_at(&secret, h.now()).unwrap();
    let Response::RemoteUserSaved { user } = h
        .auth
        .handle_admin(
            Request::ConfirmTotpEnrollment {
                user_id: ada.id.clone(),
                code: code.clone(),
            },
            None,
        )
        .await
        .unwrap()
    else {
        panic!("not saved")
    };
    assert!(user.totp_enabled);
    // Only a keychain reference is stored; the secret is in the keychain.
    let credentials = h.store.remote_credentials(&ada.id).unwrap();
    assert_eq!(
        credentials.totp_ref.as_deref(),
        Some(totp::secret_ref(&ada.id).as_str())
    );

    // The confirming code cannot log in.
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    browser.send(&h, AuthRequest::Logout).await;
    let reply = browser.password(&h, "ada", PASSWORD, Some(&code)).await;
    let _ = failure(&reply);

    // Disabling removes the secret; the password alone works again.
    let Response::RemoteUserSaved { user } = h
        .auth
        .handle_admin(
            Request::DisableTotp {
                user_id: ada.id.clone(),
            },
            None,
        )
        .await
        .unwrap()
    else {
        panic!("not saved")
    };
    assert!(!user.totp_enabled);
    assert!(h.secrets.get(&totp::secret_ref(&ada.id)).unwrap().is_none());
    h.clock.advance(60);
    let reply = browser.password(&h, "ada", PASSWORD, None).await;
    assert!(
        matches!(reply.response, AuthResponse::LoggedIn { .. }),
        "{reply:?}"
    );
    let changes = h
        .audit_kinds()
        .iter()
        .filter(|(k, _)| *k == RemoteAuditKind::TotpChanged)
        .count();
    assert_eq!(changes, 2);
}

#[tokio::test]
async fn pairing_codes_through_admin_requests() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let Response::PairingCodeCreated {
        pairing,
        code,
        url,
        certificate_fingerprint,
    } = h
        .auth
        .handle_admin(
            Request::CreatePairingCode {
                user_id: ada.id.clone(),
                device_name: Some("Tablet".into()),
                grant: Some(read_only()),
                ttl_minutes: Some(2),
            },
            None,
        )
        .await
        .unwrap()
    else {
        panic!("no code")
    };
    assert_eq!(pairing.state, PairingState::Pending);
    assert_eq!(pairing.device_name.as_deref(), Some("Tablet"));
    assert_eq!(url.as_deref(), Some("https://nail.local:4780/"));
    assert_eq!(certificate_fingerprint, Some("AB".repeat(32)));
    assert_eq!(code.len(), 9);
    // The listing never shows the code.
    let listed = format!("{:?}", h.store.list_pairing_codes(h.now()).unwrap());
    assert!(!listed.contains(&code));
    assert!(!listed.contains(&code.replace('-', "")));

    // The browser's own name is ignored when the code names the device.
    let mut browser = Browser::new("192.0.2.10");
    let session = logged_in(&browser.pair(&h, &code, "Something else").await);
    assert_eq!(session.device_name, "Tablet");

    for bad in [Some(0), Some(61)] {
        let error = h
            .auth
            .handle_admin(
                Request::CreatePairingCode {
                    user_id: ada.id.clone(),
                    device_name: None,
                    grant: None,
                    ttl_minutes: bad,
                },
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
    }
    // Other requests are not handled here.
    let error = h
        .auth
        .handle_admin(Request::ListRemoteUsers, None)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert!(
        h.audit_kinds()
            .iter()
            .any(|(k, _)| *k == RemoteAuditKind::PairingCreated)
    );
}
