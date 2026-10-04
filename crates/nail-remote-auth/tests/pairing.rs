//! Pairing: a code approved on the desktop pairs one browser once;
//! guessing codes backs off and locks out.

#![cfg(test)]

#[expect(
    dead_code,
    reason = "the shared test helpers serve several test crates; each uses a subset"
)]
mod common;

use common::*;
use nail_api::Event;
use nail_api::remote::{
    AuthFailure, AuthRequest, PairingState, RemoteAlert, RemoteAuditKind, RemoteAuditOutcome,
    RemoteAuthMethod,
};
use nail_remote_auth::throttle::ThrottlePolicy;
use nail_remote_auth::{AuthPolicy, Clock};

#[tokio::test]
async fn a_code_pairs_one_browser_once() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    h.events();
    let code = h.code(&ada, Some(&read_only()));

    let mut phone = Browser::new("192.0.2.10");
    let status = phone.status(&h).await;
    assert!(!status.paired && !status.authenticated);
    assert_eq!(status.certificate_fingerprint, Some("AB".repeat(32)));
    assert!(status.methods.passkey && status.methods.password);

    // Case and separators do not matter.
    let reply = phone
        .pair(&h, &code.to_lowercase().replace('-', " "), "Phone")
        .await;
    let session = logged_in(&reply);
    assert_eq!(session.auth_method, RemoteAuthMethod::Pairing);
    assert_eq!(session.user_id, ada.id);
    assert_eq!(session.device_name, "Phone");
    // The device got the code's grant (narrowed to the user's).
    assert_eq!(session.grant, read_only());
    assert!(phone.session.is_some() && phone.device.is_some());
    let status = phone.status(&h).await;
    assert!(status.paired && status.authenticated);
    assert!(
        status.reauth_until.is_some(),
        "the pairing counts as a fresh login"
    );

    // The desktop hears about the new device.
    let events = h.events();
    assert!(events.iter().any(|e| matches!(
        e,
        Event::RemoteSecurityAlert { alert: RemoteAlert::NewDevice { device_name, user_name, ip, .. } }
            if device_name == "Phone" && user_name == "ada" && ip.as_deref() == Some("192.0.2.10")
    )), "{events:?}");

    let codes = h.store.list_pairing_codes(h.now()).unwrap();
    assert_eq!(codes[0].state, PairingState::Used);
    assert_eq!(codes[0].device_id.as_ref(), Some(&session.device_id));

    // Replaying the code from another browser fails.
    let mut thief = Browser::new("192.0.2.66");
    let reply = thief.pair(&h, &code, "Laptop").await;
    assert_eq!(failure(&reply).0, AuthFailure::InvalidCode);
    assert!(thief.device.is_none() && thief.session.is_none());

    let audit = h.audit_kinds();
    assert!(audit.contains(&(RemoteAuditKind::PairingCreated, RemoteAuditOutcome::Success)));
    assert!(audit.contains(&(RemoteAuditKind::Pairing, RemoteAuditOutcome::Success)));
    assert!(audit.contains(&(RemoteAuditKind::Pairing, RemoteAuditOutcome::Failure)));
}

#[tokio::test]
async fn expired_and_cancelled_codes_do_not_pair() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let code = h.code(&ada, None);
    h.clock.advance(10 * 60);
    let mut browser = Browser::new("192.0.2.10");
    assert_eq!(
        failure(&browser.pair(&h, &code, "Phone").await).0,
        AuthFailure::InvalidCode
    );

    let code = h.code(&ada, None);
    let pending = h.store.list_pairing_codes(h.now()).unwrap();
    let pending = pending
        .iter()
        .find(|p| p.state == PairingState::Pending)
        .unwrap();
    assert!(h.store.cancel_pairing_code(&pending.id, h.now()).unwrap());
    assert_eq!(
        failure(&browser.pair(&h, &code, "Phone").await).0,
        AuthFailure::InvalidCode
    );

    // A disabled user's code does not pair either.
    let code = h.code(&ada, None);
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
    assert_eq!(
        failure(&browser.pair(&h, &code, "Phone").await).0,
        AuthFailure::InvalidCode
    );
}

#[tokio::test]
async fn a_bad_device_name_does_not_spend_the_code() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let code = h.code(&ada, None);
    let mut browser = Browser::new("192.0.2.10");
    let reply = browser.pair(&h, &code, "   ").await;
    assert_eq!(failure(&reply).0, AuthFailure::BadRequest);
    logged_in(&browser.pair(&h, &code, "Phone").await);
}

#[tokio::test]
async fn pairing_needs_the_csrf_token() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let code = h.code(&ada, None);
    let browser = Browser::new("192.0.2.10");
    for csrf in [None, Some("forged")] {
        let reply = h
            .auth
            .handle_auth(
                AuthRequest::Pair {
                    code: code.clone(),
                    device_name: "Phone".into(),
                },
                &browser.cookies(),
                &browser.client,
                csrf,
            )
            .await;
        assert_eq!(failure(&reply).0, AuthFailure::BadRequest);
    }
    // The code is still good.
    let mut browser = browser;
    logged_in(&browser.pair(&h, &code, "Phone").await);
}

#[tokio::test]
async fn guessing_codes_backs_off_then_locks_the_address_out() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let policy = ThrottlePolicy::default();
    let mut attacker = Browser::new("198.51.100.7");
    h.events();

    let mut guesses = 0;
    let mut saw_backoff = false;
    let mut locked = None;
    for n in 0..200 {
        let guess = format!("AAAA-{:04}", 2222 + n);
        let reply = attacker.pair(&h, &guess, "Evil").await;
        let (reason, retry) = failure(&reply);
        match reason {
            AuthFailure::InvalidCode => guesses += 1,
            AuthFailure::RateLimited => {
                saw_backoff = true;
                // Wait the backoff out, like a patient attacker.
                h.clock.advance(i64::from(retry.unwrap()));
            }
            AuthFailure::LockedOut => {
                locked = retry;
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(saw_backoff);
    // The 30th wrong guess locks the address out for 15 minutes.
    assert_eq!(guesses, policy.ip_lockout_failures);
    assert_eq!(locked, Some(15 * 60));
    let events = h.events();
    assert!(events.iter().any(|e| matches!(
        e,
        Event::RemoteSecurityAlert { alert: RemoteAlert::Lockout { ip: Some(ip), user_name: None, .. } }
            if ip == "198.51.100.7"
    )), "{events:?}");
    assert!(
        h.audit_kinds()
            .contains(&(RemoteAuditKind::Lockout, RemoteAuditOutcome::Denied))
    );

    // Even a right code is refused from the locked address...
    let code = h.code(&ada, None);
    let reply = attacker.pair(&h, &code, "Evil").await;
    assert_eq!(failure(&reply).0, AuthFailure::LockedOut);
    // ...and was not spent: the real device still pairs from elsewhere.
    let mut phone = Browser::new("192.0.2.10");
    logged_in(&phone.pair(&h, &code, "Phone").await);
}

#[tokio::test]
async fn guessing_from_many_addresses_pauses_pairing() {
    let h = Harness::with_policy(AuthPolicy {
        throttle: ThrottlePolicy {
            global_lockout_failures: 5,
            ..ThrottlePolicy::default()
        },
        ..fast_policy()
    });
    let ada = h.user("ada", false).await;
    for n in 0..5 {
        let mut browser = Browser::new(&format!("203.0.113.{n}"));
        let reply = browser.pair(&h, "BBBB-CCCC", "Evil").await;
        assert_ne!(failure(&reply).0, AuthFailure::RateLimited);
    }
    let code = h.code(&ada, None);
    let mut phone = Browser::new("192.0.2.10");
    let (reason, retry) = failure(&phone.pair(&h, &code, "Phone").await);
    assert_eq!(reason, AuthFailure::LockedOut);
    assert_eq!(retry, Some(60));
    h.clock.advance(60);
    logged_in(&phone.pair(&h, &code, "Phone").await);
    assert!(h.clock.now() > T0);
    // The successful pairing cleared the global count: one more wrong code
    // does not pause pairing again at once.
    assert_eq!(
        h.store
            .auth_throttle(nail_remote_auth::throttle::GLOBAL_PAIRING_KEY)
            .unwrap(),
        None
    );
    let mut evil = Browser::new("203.0.113.99");
    evil.pair(&h, "BBBB-CCCC", "Evil").await;
    let code = h.code(&ada, None);
    let mut tablet = Browser::new("192.0.2.11");
    logged_in(&tablet.pair(&h, &code, "Tablet").await);
}

#[tokio::test]
async fn pairing_again_replaces_the_browsers_session() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    let first = browser.resolve(&h).unwrap().session.id;
    let code = h.code(&ada, None);
    let second = logged_in(&browser.pair(&h, &code, "Phone 2").await);
    assert_ne!(first, second.id);
    let ended = h.store.get_remote_session(&first).unwrap();
    assert!(ended.ended_at.is_some());
    assert_eq!(h.store.list_remote_devices(Some(&ada.id)).unwrap().len(), 2);
}
