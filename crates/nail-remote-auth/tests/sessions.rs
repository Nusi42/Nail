//! Sessions: device-bound cookies, tampering, rotation with a grace
//! period, idle and absolute expiry, revocation, logout.

#![cfg(test)]

#[expect(
    dead_code,
    reason = "the shared test helpers serve several test crates; each uses a subset"
)]
mod common;

use common::*;
use nail_api::Event;
use nail_api::remote::{
    AuthRequest, AuthResponse, RemoteAuditKind, RemoteAuditOutcome, SessionEndReason,
};
use nail_remote_auth::NoSession;
use nail_remote_auth::secret::random_token;

#[tokio::test]
async fn a_session_needs_its_own_device_cookie() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let bob = h.user("bob", false).await;
    let phone = h.paired(&ada, "192.0.2.10").await;
    let laptop = h.paired(&ada, "192.0.2.11").await;
    let bobs = h.paired(&bob, "192.0.2.12").await;
    assert!(phone.clone().resolve(&h).is_some());

    // The session cookie alone (stolen) is useless.
    let mut stolen = phone.clone();
    stolen.device = None;
    assert_eq!(
        h.auth
            .resolve(&stolen.cookies(), &stolen.client)
            .unwrap_err(),
        NoSession::Invalid
    );
    // With another device's cookie (same user or another user) too.
    for other in [&laptop, &bobs] {
        let mut mixed = phone.clone();
        mixed.device = other.device.clone();
        assert_eq!(
            h.auth.resolve(&mixed.cookies(), &mixed.client).unwrap_err(),
            NoSession::Invalid
        );
    }
    // A made-up or altered token.
    let mut forged = phone.clone();
    forged.session = Some(random_token().to_string());
    assert!(h.auth.resolve(&forged.cookies(), &forged.client).is_err());
    let mut altered = phone.clone();
    let mut token = altered.session.clone().unwrap();
    let last = token.pop().unwrap();
    token.push(if last == 'A' { 'B' } else { 'A' });
    altered.session = Some(token);
    assert!(h.auth.resolve(&altered.cookies(), &altered.client).is_err());
    // Not token-shaped at all.
    altered.session = Some("x'; DROP TABLE remote_sessions; --".into());
    assert_eq!(
        h.auth
            .resolve(&altered.cookies(), &altered.client)
            .unwrap_err(),
        NoSession::Invalid
    );
    let mut none = phone.clone();
    none.session = None;
    assert_eq!(
        h.auth.resolve(&none.cookies(), &none.client).unwrap_err(),
        NoSession::Missing
    );
    // The genuine pair still works.
    assert!(phone.clone().resolve(&h).is_some());

    // Status of a browser with a bad session cookie clears it.
    let status = forged.status(&h).await;
    assert!(!status.authenticated && status.paired);
    assert!(forged.session.is_none());
}

#[tokio::test]
async fn tokens_rotate_and_the_old_one_has_a_grace_period() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    let original = browser.clone();
    let policy = h.auth.policy().clone();

    // Before the rotation interval: no new cookie.
    h.clock.advance(policy.rotate_after_secs - 1);
    let resolved = h.auth.resolve(&browser.cookies(), &browser.client).unwrap();
    assert!(resolved.set_cookie.is_none());

    h.clock.advance(1);
    let resolved = h.auth.resolve(&browser.cookies(), &browser.client).unwrap();
    let cookie = resolved.set_cookie.expect("rotated");
    for part in [
        "__Host-nail-session=",
        "HttpOnly",
        "Secure",
        "SameSite=Strict",
        "Path=/",
    ] {
        assert!(cookie.contains(part), "{cookie}");
    }
    browser.apply(&[cookie]);
    assert_ne!(browser.session, original.session);
    let id = resolved.ctx.session.id.clone();

    // A request in flight with the old token still works for a moment
    // (and does not rotate again)...
    let old = h
        .auth
        .resolve(&original.cookies(), &original.client)
        .unwrap();
    assert_eq!(old.ctx.session.id, id);
    assert!(old.set_cookie.is_none());
    // ...but not after the grace period.
    h.clock.advance(policy.rotation_grace_secs);
    assert!(
        h.auth
            .resolve(&original.cookies(), &original.client)
            .is_err()
    );
    assert_eq!(browser.resolve(&h).unwrap().session.id, id);
}

#[tokio::test]
async fn idle_sessions_end() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    let settings = h.auth.settings();
    let idle = i64::from(settings.session_idle_minutes) * 60;
    let id = browser.resolve(&h).unwrap().session.id;
    h.events();

    // Activity keeps it alive.
    for _ in 0..4 {
        h.clock.advance(idle - 60);
        assert!(browser.resolve(&h).is_some());
    }
    h.clock.advance(idle);
    assert!(browser.resolve(&h).is_none());
    let session = h.store.get_remote_session(&id).unwrap();
    assert_eq!(session.end_reason, Some(SessionEndReason::Expired));
    assert!(h.events().contains(&Event::RemoteSessionEnded {
        session_id: id,
        reason: SessionEndReason::Expired
    }));
}

#[tokio::test]
async fn sessions_have_an_absolute_limit() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    let max = i64::from(h.auth.settings().session_max_hours) * 3600;
    let mut elapsed = 0;
    while elapsed + 600 < max {
        h.clock.advance(600);
        elapsed += 600;
        assert!(browser.resolve(&h).is_some(), "ended early at {elapsed}");
    }
    h.clock.advance(max - elapsed);
    assert!(browser.resolve(&h).is_none());
    // The device stays paired: the password logs in again.
    logged_in(&browser.password(&h, "ada", PASSWORD, None).await);
}

#[tokio::test]
async fn sweep_ends_idle_sessions_and_announces_them() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    let id = browser.resolve(&h).unwrap().session.id;
    h.events();
    assert!(h.auth.sweep().is_empty());
    h.clock
        .advance(i64::from(h.auth.settings().session_idle_minutes) * 60);
    assert_eq!(h.auth.sweep(), std::slice::from_ref(&id));
    let events = h.events();
    assert!(events.contains(&Event::RemoteSessionEnded {
        session_id: id,
        reason: SessionEndReason::Expired
    }));
    assert!(events.contains(&Event::RemoteAccessChanged));
}

#[tokio::test]
async fn revoking_a_device_or_session_logs_the_browser_out() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut phone = h.paired(&ada, "192.0.2.10").await;
    let mut laptop = h.paired(&ada, "192.0.2.11").await;
    let phone_ctx = phone.resolve(&h).unwrap();
    let laptop_ctx = laptop.resolve(&h).unwrap();

    h.store
        .end_remote_session(&laptop_ctx.session.id, SessionEndReason::Revoked, h.now())
        .unwrap();
    assert!(laptop.resolve(&h).is_none());
    assert!(h.auth.refresh(&laptop_ctx).is_none());

    h.store
        .revoke_remote_device(&phone_ctx.device.id, h.now())
        .unwrap();
    assert!(phone.resolve(&h).is_none());
    // Its CSRF token was bound to the device, so the stale one fails...
    let reply = phone.password(&h, "ada", PASSWORD, None).await;
    assert_eq!(failure(&reply).0, nail_api::remote::AuthFailure::BadRequest);
    // ...and with a fresh one the revoked device cannot log in with the
    // password either: it must pair again.
    let status = phone.status(&h).await;
    assert!(!status.paired);
    let reply = phone.password(&h, "ada", PASSWORD, None).await;
    assert_eq!(failure(&reply).0, nail_api::remote::AuthFailure::NotPaired);
}

#[tokio::test]
async fn logout_ends_the_session_and_clears_the_cookie() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    let before = browser.clone();
    let id = browser.resolve(&h).unwrap().session.id;
    h.events();
    let reply = browser.send(&h, AuthRequest::Logout).await;
    assert_eq!(reply.response, AuthResponse::LoggedOut);
    assert!(browser.session.is_none());
    assert!(browser.device.is_some(), "the device stays paired");
    // The old cookie is dead even if the browser kept it.
    assert!(h.auth.resolve(&before.cookies(), &before.client).is_err());
    assert_eq!(
        h.store.get_remote_session(&id).unwrap().end_reason,
        Some(SessionEndReason::Logout)
    );
    assert!(h.events().contains(&Event::RemoteSessionEnded {
        session_id: id,
        reason: SessionEndReason::Logout
    }));
    assert!(
        h.audit_kinds()
            .contains(&(RemoteAuditKind::Logout, RemoteAuditOutcome::Success))
    );
    // Logging out twice is harmless.
    let reply = browser.send(&h, AuthRequest::Logout).await;
    assert_eq!(reply.response, AuthResponse::LoggedOut);
}

#[tokio::test]
async fn refresh_keeps_a_websocket_session_alive_and_notices_its_end() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    let ctx = browser.resolve(&h).unwrap();
    let idle = i64::from(h.auth.settings().session_idle_minutes) * 60;
    let mut ctx = ctx;
    for _ in 0..3 {
        h.clock.advance(idle - 10);
        ctx = h.auth.refresh(&ctx).expect("still valid");
    }
    // Disabling the user ends it.
    h.store
        .update_remote_user(
            &ada.id,
            &nail_api::remote::RemoteUserUpdate {
                disabled: Some(true),
                ..Default::default()
            },
            h.now(),
        )
        .unwrap();
    assert!(h.auth.refresh(&ctx).is_none());
    assert!(browser.resolve(&h).is_none());
}

#[tokio::test]
async fn sessions_record_activity_and_the_client() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    h.clock.advance(120);
    browser.client.ip = Some("192.0.2.99".parse().unwrap());
    let ctx = browser.resolve(&h).unwrap();
    assert_eq!(ctx.session.ip.as_deref(), Some("192.0.2.99"));
    let device = h.store.get_remote_device(&ctx.device.id).unwrap();
    assert_eq!(device.last_ip.as_deref(), Some("192.0.2.99"));
    assert_eq!(device.user_agent.as_deref(), Some("TestBrowser/1.0"));
    let stored = h.store.get_remote_session(&ctx.session.id).unwrap();
    assert_eq!(stored.last_seen_at, ctx.session.last_seen_at);
}

/// M2b follow-up: the sandbox origin checks which session a render token
/// belongs to without touching or rotating it.
#[tokio::test]
async fn the_session_of_cookies_is_found_without_touching_it() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let phone = h.paired(&ada, "192.0.2.10").await;
    let laptop = h.paired(&ada, "192.0.2.11").await;
    let ctx = phone.clone().resolve(&h).unwrap();
    assert_eq!(h.auth.session_id_of(&phone.cookies()), Some(ctx.session.id));
    // Another device's cookie with this session, or none: no session.
    let mut mixed = phone.clone();
    mixed.device = laptop.device.clone();
    assert_eq!(h.auth.session_id_of(&mixed.cookies()), None);
    let mut stolen = phone.clone();
    stolen.device = None;
    assert_eq!(h.auth.session_id_of(&stolen.cookies()), None);
    assert_eq!(
        h.auth.session_id_of(&Browser::new("192.0.2.1").cookies()),
        None
    );
}
