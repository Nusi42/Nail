//! Password (+ TOTP) logins: paired devices only, brute force backs off
//! and locks out, TOTP codes work once and tolerate one step of skew.

#![cfg(test)]

#[expect(
    dead_code,
    reason = "the shared test helpers serve several test crates; each uses a subset"
)]
mod common;

use common::*;
use nail_api::Event;
use nail_api::remote::{
    AuthFailure, AuthResponse, RemoteAlert, RemoteAuditKind, RemoteAuditOutcome, RemoteAuthMethod,
    RemoteUserUpdate,
};
use nail_remote_auth::totp;
use totp_rs::Secret;

async fn logged_out_paired(h: &Harness, user: &nail_api::remote::RemoteUser) -> Browser {
    let mut browser = h.paired(user, "192.0.2.10").await;
    let reply = browser.send(h, nail_api::remote::AuthRequest::Logout).await;
    assert_eq!(reply.response, AuthResponse::LoggedOut);
    assert!(browser.session.is_none() && browser.device.is_some());
    browser
}

#[tokio::test]
async fn password_login_on_a_paired_device() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = logged_out_paired(&h, &ada).await;
    let session = logged_in(&browser.password(&h, "Ada", PASSWORD, None).await);
    assert_eq!(session.auth_method, RemoteAuthMethod::Password);
    assert_eq!(session.user_name, "ada");
    assert!(browser.resolve(&h).is_some());
    assert!(
        h.store
            .get_remote_user(&ada.id)
            .unwrap()
            .last_login_at
            .is_some()
    );
    assert!(
        h.audit_kinds()
            .contains(&(RemoteAuditKind::Login, RemoteAuditOutcome::Success))
    );
}

#[tokio::test]
async fn a_new_browser_must_pair_first() {
    let h = Harness::new();
    h.user("ada", false).await;
    let mut stranger = Browser::new("192.0.2.99");
    let reply = stranger.password(&h, "ada", PASSWORD, None).await;
    assert_eq!(failure(&reply).0, AuthFailure::NotPaired);
    assert!(stranger.session.is_none());
}

#[tokio::test]
async fn wrong_name_and_wrong_password_look_the_same() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = logged_out_paired(&h, &ada).await;
    let wrong_password = failure(&browser.password(&h, "ada", "not the password!", None).await);
    let wrong_name = failure(&browser.password(&h, "nobody", PASSWORD, None).await);
    assert_eq!(wrong_password.0, AuthFailure::InvalidCredentials);
    assert_eq!(wrong_name.0, AuthFailure::InvalidCredentials);
    let audit = h.store.list_remote_audit(None, None, 10).unwrap();
    assert_eq!(audit[0].user_name.as_deref(), Some("nobody"));
    assert_eq!(audit[0].outcome, RemoteAuditOutcome::Failure);
    assert_eq!(audit[1].user_id.as_ref(), Some(&ada.id));
}

#[tokio::test]
async fn a_device_of_another_user_cannot_log_in_as_someone_else() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    h.user("bob", true).await;
    let mut adas_phone = logged_out_paired(&h, &ada).await;
    // Bob's right password on Ada's device: refused like a wrong one.
    let reply = adas_phone.password(&h, "bob", PASSWORD, None).await;
    assert_eq!(failure(&reply).0, AuthFailure::InvalidCredentials);
    assert!(adas_phone.session.is_none());
}

#[tokio::test]
async fn brute_force_backs_off_then_locks_the_account_and_notifies() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut attacker = logged_out_paired(&h, &ada).await;
    h.events();
    let mut evaluated = 0;
    let mut throttled = 0;
    let lock = loop {
        let reply = attacker
            .password(&h, "ada", "guess-guess-guess", None)
            .await;
        match failure(&reply) {
            (AuthFailure::InvalidCredentials, retry) => {
                evaluated += 1;
                if let Some(retry) = retry {
                    // An immediate retry is refused without evaluation.
                    let early = attacker.password(&h, "ada", PASSWORD, None).await;
                    assert_eq!(failure(&early), (AuthFailure::RateLimited, Some(retry)));
                    throttled += 1;
                    h.clock.advance(i64::from(retry));
                }
            }
            (AuthFailure::LockedOut, retry) => break retry.unwrap(),
            other => panic!("{other:?}"),
        }
        assert!(evaluated < 50, "never locked out");
    };
    // 10 failures per account: 9 answered as wrong, the 10th locks.
    assert_eq!(evaluated, 9);
    assert!(throttled >= 5, "{throttled}");
    assert_eq!(lock, 15 * 60);

    // The desktop is told, the user shows as locked, the audit has it.
    let events = h.events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::RemoteSecurityAlert { alert: RemoteAlert::Lockout { user_name: Some(name), .. } }
                if name == "ada"
        )),
        "{events:?}"
    );
    assert!(
        h.store
            .get_remote_user(&ada.id)
            .unwrap()
            .locked_until
            .is_some()
    );
    assert!(
        h.audit_kinds()
            .contains(&(RemoteAuditKind::Lockout, RemoteAuditOutcome::Denied))
    );

    // During the lockout even the right password is refused.
    h.clock.advance(60);
    let reply = attacker.password(&h, "ada", PASSWORD, None).await;
    assert_eq!(failure(&reply).0, AuthFailure::LockedOut);

    // Unlocking on the desktop (what the core does) lets the user in.
    h.store.set_remote_user_lock(&ada.id, None).unwrap();
    h.store.clear_auth_throttle("user:ada").unwrap();
    h.clock.advance(5 * 60);
    logged_in(&attacker.password(&h, "ada", PASSWORD, None).await);
}

#[tokio::test]
async fn lockout_ends_by_itself() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = logged_out_paired(&h, &ada).await;
    loop {
        let (reason, retry) =
            failure(&browser.password(&h, "ada", "wrong-wrong-wrong", None).await);
        match reason {
            AuthFailure::LockedOut => break,
            _ => h.clock.advance(i64::from(retry.unwrap_or(0))),
        }
    }
    h.clock.advance(15 * 60);
    logged_in(&browser.password(&h, "ada", PASSWORD, None).await);
    // A success clears the counters.
    assert!(h.store.auth_throttle("user:ada").unwrap().is_none());
}

#[tokio::test]
async fn disabled_users_cannot_log_in() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = logged_out_paired(&h, &ada).await;
    h.store
        .update_remote_user(
            &ada.id,
            &RemoteUserUpdate {
                disabled: Some(true),
                ..Default::default()
            },
            h.now(),
        )
        .unwrap();
    // A wrong password stays a wrong password (no hint the user exists).
    let reply = browser.password(&h, "ada", "wrong-wrong-wrong", None).await;
    assert_eq!(failure(&reply).0, AuthFailure::InvalidCredentials);
    let reply = browser.password(&h, "ada", PASSWORD, None).await;
    assert_eq!(failure(&reply).0, AuthFailure::Disabled);
}

fn enroll_totp(h: &Harness, user: &nail_api::remote::RemoteUser) -> Vec<u8> {
    let enrollment = h.auth.start_totp_enrollment(&user.id).unwrap();
    let secret = Secret::try_from_base32(&enrollment.secret)
        .unwrap()
        .as_bytes()
        .to_vec();
    // Confirm with the code of a step long before the tests log in.
    let code = totp::code_at(&secret, h.now() - 600).unwrap();
    let confirmed = h.auth.confirm_totp_enrollment(&user.id, &code, None);
    assert!(confirmed.is_err(), "an old code must not confirm");
    let code = totp::code_at(&secret, h.now()).unwrap();
    let user = h
        .auth
        .confirm_totp_enrollment(&user.id, &code, None)
        .unwrap();
    assert!(user.totp_enabled);
    // The confirming step is used up: move on.
    h.clock.advance(30);
    secret
}

#[tokio::test]
async fn totp_is_required_and_each_code_works_once() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let secret = enroll_totp(&h, &ada);
    let mut browser = logged_out_paired(&h, &ada).await;

    let reply = browser.password(&h, "ada", PASSWORD, None).await;
    assert_eq!(reply.response, AuthResponse::TotpRequired);
    assert!(browser.session.is_none());

    let code = totp::code_at(&secret, h.now()).unwrap();
    let session = logged_in(&browser.password(&h, "ada", PASSWORD, Some(&code)).await);
    assert_eq!(session.auth_method, RemoteAuthMethod::PasswordTotp);

    // Replaying the same code (same step) fails, even from the same
    // browser a second later.
    browser
        .send(&h, nail_api::remote::AuthRequest::Logout)
        .await;
    h.clock.advance(1);
    let reply = browser.password(&h, "ada", PASSWORD, Some(&code)).await;
    assert_eq!(failure(&reply).0, AuthFailure::InvalidCredentials);
    let audit = h.store.list_remote_audit(None, None, 1).unwrap();
    assert_eq!(audit[0].detail.as_deref(), Some("TOTP code used before"));

    // A code of an older step than the last accepted one fails too.
    let old = totp::code_at(&secret, h.now() - 31).unwrap();
    let reply = browser.password(&h, "ada", PASSWORD, Some(&old)).await;
    assert_eq!(failure(&reply).0, AuthFailure::InvalidCredentials);

    // The next step's code works.
    h.clock.advance(30);
    let code = totp::code_at(&secret, h.now()).unwrap();
    logged_in(&browser.password(&h, "ada", PASSWORD, Some(&code)).await);

    // A wrong code counts as a failure.
    let reply = browser.password(&h, "ada", PASSWORD, Some("000000")).await;
    let _ = failure(&reply);
    assert_eq!(
        h.store.auth_throttle("user:ada").unwrap().unwrap().failures,
        1
    );
}

#[tokio::test]
async fn totp_tolerates_one_step_of_clock_skew() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let secret = enroll_totp(&h, &ada);
    let mut browser = logged_out_paired(&h, &ada).await;

    // The phone runs 30 s ahead: its code is for the next step.
    h.clock.advance(60);
    let ahead = totp::code_at(&secret, h.now() + 30).unwrap();
    logged_in(&browser.password(&h, "ada", PASSWORD, Some(&ahead)).await);
    browser
        .send(&h, nail_api::remote::AuthRequest::Logout)
        .await;

    // The phone runs 30 s behind (two steps later on the server).
    h.clock.advance(90);
    let behind = totp::code_at(&secret, h.now() - 30).unwrap();
    logged_in(&browser.password(&h, "ada", PASSWORD, Some(&behind)).await);
    browser
        .send(&h, nail_api::remote::AuthRequest::Logout)
        .await;

    // Two steps off is refused, either way.
    h.clock.advance(120);
    let far_ahead = totp::code_at(&secret, h.now() + 60).unwrap();
    let reply = browser
        .password(&h, "ada", PASSWORD, Some(&far_ahead))
        .await;
    assert_eq!(failure(&reply).0, AuthFailure::InvalidCredentials);
    h.clock.advance(120);
    let far_behind = totp::code_at(&secret, h.now() - 60).unwrap();
    let reply = browser
        .password(&h, "ada", PASSWORD, Some(&far_behind))
        .await;
    assert_eq!(failure(&reply).0, AuthFailure::InvalidCredentials);
}

#[tokio::test]
async fn reauthentication_with_the_password() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    let settings = h.auth.settings();
    let ctx = browser.resolve(&h).unwrap();
    assert!(ctx.is_fresh(h.now()));
    h.clock.advance(i64::from(settings.reauth_minutes) * 60 + 1);
    let ctx = browser.resolve(&h).unwrap();
    assert!(!ctx.is_fresh(h.now()));

    // Wrong user name for this session: refused.
    let reply = browser.reauth_password(&h, "bob", PASSWORD).await;
    assert_eq!(failure(&reply).0, AuthFailure::InvalidCredentials);
    let reply = browser.reauth_password(&h, "ada", PASSWORD).await;
    assert!(
        matches!(reply.response, AuthResponse::Reauthenticated { .. }),
        "{reply:?}"
    );
    let ctx = browser.resolve(&h).unwrap();
    assert!(ctx.is_fresh(h.now()));
    assert!(ctx.session.reauth_at.is_some());
    assert!(
        h.audit_kinds()
            .contains(&(RemoteAuditKind::Reauth, RemoteAuditOutcome::Success))
    );

    // Without a session there is nothing to re-authenticate.
    let mut stranger = Browser::new("192.0.2.77");
    let reply = stranger.reauth_password(&h, "ada", PASSWORD).await;
    assert_eq!(failure(&reply).0, AuthFailure::NotLoggedIn);
}
