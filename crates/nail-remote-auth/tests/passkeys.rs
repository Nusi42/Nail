//! Passkeys with a software authenticator: registration needs a fresh
//! login, login and re-authentication work, replayed or foreign answers
//! and cloned authenticators are refused.

#![cfg(test)]

#[expect(
    dead_code,
    reason = "the shared test helpers serve several test crates; each uses a subset"
)]
mod common;

use common::*;
use nail_api::remote::{
    AuthFailure, AuthPurpose, AuthRequest, AuthResponse, RemoteAuditKind, RemoteAuditOutcome,
    RemoteAuthMethod,
};
use nail_remote_auth::passkey::StoredCredential;
use nail_remote_auth::{AuthConfig, Clock};

async fn register(
    h: &Harness,
    browser: &mut Browser,
    key: &mut Authenticator,
    name: &str,
) -> AuthResponse {
    let reply = browser
        .send(
            h,
            AuthRequest::PasskeyRegisterStart {
                name: name.to_owned(),
            },
        )
        .await;
    let (challenge_id, options) = challenge(&reply);
    let credential = key.create(&options);
    browser
        .send(
            h,
            AuthRequest::PasskeyRegisterFinish {
                challenge_id,
                credential,
            },
        )
        .await
        .response
}

async fn passkey_login(
    h: &Harness,
    browser: &mut Browser,
    key: &mut Authenticator,
    purpose: AuthPurpose,
) -> nail_remote_auth::AuthReply {
    let reply = browser
        .send(
            h,
            AuthRequest::PasskeyStart {
                user_name: None,
                purpose,
            },
        )
        .await;
    let (challenge_id, options) = challenge(&reply);
    assert_eq!(options["rpId"], HOST);
    assert_eq!(options["userVerification"], "required");
    let credential = key.get(&options);
    browser
        .send(
            h,
            AuthRequest::PasskeyFinish {
                challenge_id,
                credential,
            },
        )
        .await
}

async fn logout(h: &Harness, browser: &mut Browser) {
    browser.send(h, AuthRequest::Logout).await;
}

#[tokio::test]
async fn register_then_log_in_with_a_passkey() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    let mut key = Authenticator::new(1);

    let registered = register(&h, &mut browser, &mut key, " Phone passkey ").await;
    let AuthResponse::PasskeyRegistered { passkey } = registered else {
        panic!("{registered:?}");
    };
    assert_eq!(passkey.name, "Phone passkey");
    let user = h.store.get_remote_user(&ada.id).unwrap();
    assert_eq!(user.passkeys.len(), 1);
    // Only public data is stored.
    let stored = h.store.passkeys_of_user(&ada.id).unwrap();
    let credential = StoredCredential::parse(&stored[0].credential).unwrap();
    assert_eq!(credential.user_handle, key.user_handle.clone().unwrap());
    assert_eq!(credential.sign_count(), Some(0));

    logout(&h, &mut browser).await;
    let reply = passkey_login(&h, &mut browser, &mut key, AuthPurpose::Login).await;
    let session = logged_in(&reply);
    assert_eq!(session.auth_method, RemoteAuthMethod::Passkey);
    assert_eq!(session.user_id, ada.id);
    // The counter moved and the last use is recorded.
    let stored = h.store.passkeys_of_user(&ada.id).unwrap();
    let credential = StoredCredential::parse(&stored[0].credential).unwrap();
    assert_eq!(credential.sign_count(), Some(1));
    assert!(stored[0].passkey.last_used_at.is_some());
    let audit = h.audit_kinds();
    assert!(audit.contains(&(RemoteAuditKind::PasskeyChanged, RemoteAuditOutcome::Success)));
    assert!(audit.contains(&(RemoteAuditKind::Login, RemoteAuditOutcome::Success)));

    // A second passkey reuses the user handle and excludes the first.
    let mut second = Authenticator::new(2);
    let reply = browser
        .send(&h, AuthRequest::PasskeyRegisterStart { name: "Key".into() })
        .await;
    let (_, options) = challenge(&reply);
    assert_eq!(options["user"]["id"], key.user_handle.clone().unwrap());
    assert_eq!(options["excludeCredentials"].as_array().unwrap().len(), 1);
    let _ = &mut second;
}

#[tokio::test]
async fn registering_needs_a_fresh_login() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    h.clock
        .advance(i64::from(h.auth.settings().reauth_minutes) * 60 + 1);
    let reply = browser
        .send(
            &h,
            AuthRequest::PasskeyRegisterStart {
                name: "Phone".into(),
            },
        )
        .await;
    assert_eq!(failure(&reply).0, AuthFailure::ReauthRequired);
    let reply = browser.reauth_password(&h, "ada", PASSWORD).await;
    assert!(matches!(
        reply.response,
        AuthResponse::Reauthenticated { .. }
    ));
    let mut key = Authenticator::new(3);
    assert!(matches!(
        register(&h, &mut browser, &mut key, "Phone").await,
        AuthResponse::PasskeyRegistered { .. }
    ));

    // Without a session: not logged in.
    let mut stranger = Browser::new("192.0.2.50");
    let reply = stranger
        .send(
            &h,
            AuthRequest::PasskeyRegisterStart {
                name: "Phone".into(),
            },
        )
        .await;
    assert_eq!(failure(&reply).0, AuthFailure::NotLoggedIn);
}

#[tokio::test]
async fn replayed_answers_and_challenges_are_refused() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    let mut key = Authenticator::new(4);
    register(&h, &mut browser, &mut key, "Phone").await;
    logout(&h, &mut browser).await;

    let reply = browser
        .send(
            &h,
            AuthRequest::PasskeyStart {
                user_name: Some("ada".into()),
                purpose: AuthPurpose::Login,
            },
        )
        .await;
    let (challenge_id, options) = challenge(&reply);
    let credential = key.get(&options);
    let finish = AuthRequest::PasskeyFinish {
        challenge_id: challenge_id.clone(),
        credential: credential.clone(),
    };
    logged_in(&browser.send(&h, finish.clone()).await);
    logout(&h, &mut browser).await;

    // The same answer again: the challenge is gone.
    let reply = browser.send(&h, finish).await;
    assert_eq!(failure(&reply).0, AuthFailure::ChallengeExpired);

    // The old answer against a new challenge: signed over the wrong one.
    let reply = browser
        .send(
            &h,
            AuthRequest::PasskeyStart {
                user_name: None,
                purpose: AuthPurpose::Login,
            },
        )
        .await;
    let (new_id, _) = challenge(&reply);
    let reply = browser
        .send(
            &h,
            AuthRequest::PasskeyFinish {
                challenge_id: new_id,
                credential,
            },
        )
        .await;
    assert_eq!(failure(&reply).0, AuthFailure::InvalidCredentials);
    assert!(browser.session.is_none());

    // A made-up challenge id.
    let reply = browser
        .send(
            &h,
            AuthRequest::PasskeyFinish {
                challenge_id: "not-a-challenge".into(),
                credential: serde_json::json!({}),
            },
        )
        .await;
    assert_eq!(failure(&reply).0, AuthFailure::ChallengeExpired);
}

#[tokio::test]
async fn challenges_expire() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    let mut key = Authenticator::new(5);
    register(&h, &mut browser, &mut key, "Phone").await;
    logout(&h, &mut browser).await;
    let reply = browser
        .send(
            &h,
            AuthRequest::PasskeyStart {
                user_name: None,
                purpose: AuthPurpose::Login,
            },
        )
        .await;
    let (challenge_id, options) = challenge(&reply);
    h.clock.advance(h.auth.policy().ceremony_ttl_secs);
    let credential = key.get(&options);
    let reply = browser
        .send(
            &h,
            AuthRequest::PasskeyFinish {
                challenge_id,
                credential,
            },
        )
        .await;
    assert_eq!(failure(&reply).0, AuthFailure::ChallengeExpired);
}

#[tokio::test]
async fn a_challenge_belongs_to_the_browser_that_asked() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut phone = h.paired(&ada, "192.0.2.10").await;
    let mut laptop = h.paired(&ada, "192.0.2.11").await;
    let mut key = Authenticator::new(6);
    register(&h, &mut phone, &mut key, "Phone").await;
    logout(&h, &mut phone).await;
    logout(&h, &mut laptop).await;
    let reply = phone
        .send(
            &h,
            AuthRequest::PasskeyStart {
                user_name: None,
                purpose: AuthPurpose::Login,
            },
        )
        .await;
    let (challenge_id, options) = challenge(&reply);
    let credential = key.get(&options);
    let reply = laptop
        .send(
            &h,
            AuthRequest::PasskeyFinish {
                challenge_id,
                credential,
            },
        )
        .await;
    assert_eq!(failure(&reply).0, AuthFailure::BadRequest);
    assert!(laptop.session.is_none());
}

#[tokio::test]
async fn a_cloned_authenticator_is_refused() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    let mut key = Authenticator::new(7);
    register(&h, &mut browser, &mut key, "Phone").await;
    logout(&h, &mut browser).await;
    // The genuine key logs in a few times.
    for _ in 0..3 {
        logged_in(&passkey_login(&h, &mut browser, &mut key, AuthPurpose::Login).await);
        logout(&h, &mut browser).await;
    }
    // A clone still at counter 2 answers.
    let reply = browser
        .send(
            &h,
            AuthRequest::PasskeyStart {
                user_name: None,
                purpose: AuthPurpose::Login,
            },
        )
        .await;
    let (challenge_id, options) = challenge(&reply);
    let credential = key.assert_with(&options, 2);
    let reply = browser
        .send(
            &h,
            AuthRequest::PasskeyFinish {
                challenge_id,
                credential,
            },
        )
        .await;
    assert_eq!(failure(&reply).0, AuthFailure::InvalidCredentials);
    let audit = h.store.list_remote_audit(None, None, 1).unwrap();
    assert_eq!(
        audit[0].detail.as_deref(),
        Some("passkey verification failed")
    );
    assert_eq!(audit[0].user_id.as_ref(), Some(&ada.id));
}

#[tokio::test]
async fn answers_for_another_origin_or_key_are_refused() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    let mut key = Authenticator::new(8);
    register(&h, &mut browser, &mut key, "Phone").await;
    logout(&h, &mut browser).await;

    // A phishing page on another origin relays the challenge.
    key.origin = "https://nail-login.example".into();
    let reply = passkey_login(&h, &mut browser, &mut key, AuthPurpose::Login).await;
    assert_eq!(failure(&reply).0, AuthFailure::InvalidCredentials);
    key.origin = ORIGIN.into();

    // An unregistered authenticator.
    let mut stranger = Authenticator::new(9);
    stranger.user_handle = key.user_handle.clone();
    let reply = passkey_login(&h, &mut browser, &mut stranger, AuthPurpose::Login).await;
    assert_eq!(failure(&reply).0, AuthFailure::InvalidCredentials);

    // The real one still works.
    logged_in(&passkey_login(&h, &mut browser, &mut key, AuthPurpose::Login).await);
}

#[tokio::test]
async fn a_passkey_logs_in_only_on_its_users_devices() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let bob = h.user("bob", false).await;
    let mut adas = h.paired(&ada, "192.0.2.10").await;
    let mut bobs = h.paired(&bob, "192.0.2.11").await;
    let mut key = Authenticator::new(10);
    register(&h, &mut adas, &mut key, "Phone").await;
    logout(&h, &mut bobs).await;
    let reply = passkey_login(&h, &mut bobs, &mut key, AuthPurpose::Login).await;
    assert_eq!(failure(&reply).0, AuthFailure::InvalidCredentials);
    assert!(bobs.session.is_none());

    // Naming another user than the passkey's is refused too.
    logout(&h, &mut adas).await;
    let reply = adas
        .send(
            &h,
            AuthRequest::PasskeyStart {
                user_name: Some("bob".into()),
                purpose: AuthPurpose::Login,
            },
        )
        .await;
    let (challenge_id, options) = challenge(&reply);
    let credential = key.get(&options);
    let reply = adas
        .send(
            &h,
            AuthRequest::PasskeyFinish {
                challenge_id,
                credential,
            },
        )
        .await;
    assert_eq!(failure(&reply).0, AuthFailure::InvalidCredentials);
}

#[tokio::test]
async fn reauthentication_with_a_passkey() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    let mut key = Authenticator::new(11);
    register(&h, &mut browser, &mut key, "Phone").await;
    h.clock
        .advance(i64::from(h.auth.settings().reauth_minutes) * 60 + 1);
    assert!(!browser.resolve(&h).unwrap().is_fresh(h.now()));
    let reply = passkey_login(&h, &mut browser, &mut key, AuthPurpose::Reauth).await;
    assert!(
        matches!(reply.response, AuthResponse::Reauthenticated { .. }),
        "{reply:?}"
    );
    assert!(browser.resolve(&h).unwrap().is_fresh(h.clock.now()));
}

#[tokio::test]
async fn without_a_host_name_passkeys_are_not_offered() {
    let h = Harness::new();
    h.auth.set_config(AuthConfig {
        relying_party: None,
        ..config()
    });
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    let status = browser.status(&h).await;
    assert!(!status.methods.passkey);
    let reply = browser
        .send(
            &h,
            AuthRequest::PasskeyStart {
                user_name: None,
                purpose: AuthPurpose::Login,
            },
        )
        .await;
    assert_eq!(failure(&reply).0, AuthFailure::BadRequest);
}
