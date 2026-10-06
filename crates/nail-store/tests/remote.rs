//! Schema v6 (ADR 0036): remote-UI users and credentials, passkeys,
//! devices, sessions, pairing codes, login throttling, the remote audit
//! log and the remote-content allow-lists.
#![cfg(test)]

use nail_api::mcp::McpScope;
use nail_api::remote::{
    PairingState, RemoteAccessPreset, RemoteAuditKind, RemoteAuditOutcome, RemoteAuthMethod,
    RemoteGrant, RemoteSessionFilter, RemoteUiSettings, RemoteUserUpdate, SessionEndReason,
};
use nail_api::remote_content::RemoteContentScope;
use nail_api::{ErrorCode, MessageId, RemoteUserId};
use nail_store::{
    Error, NewPairingCode, NewPasskey, NewRemoteDevice, NewRemoteSession, REMOTE_UI_SETTINGS_KEY,
    RemoteAuditRecord, Store, hash_remote_secret,
};
use pretty_assertions::assert_eq;

const T0: i64 = 1_790_000_000;
const PHC: &str = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaA";

fn store() -> Store {
    Store::open_in_memory().unwrap()
}

fn full() -> RemoteGrant {
    RemoteAccessPreset::Full.grant()
}

fn device(
    store: &Store,
    user: &RemoteUserId,
    secret: &str,
    grant: RemoteGrant,
) -> nail_api::RemoteDevice {
    store
        .create_remote_device(
            &NewRemoteDevice {
                user_id: user.clone(),
                name: "Phone".into(),
                secret_hash: hash_remote_secret(secret),
                grant,
                user_agent: Some("Firefox".into()),
                ip: Some("192.168.1.20".into()),
            },
            T0,
        )
        .unwrap()
}

fn session(
    store: &Store,
    user: &RemoteUserId,
    device: &nail_api::RemoteDeviceId,
    token: &str,
    grant: RemoteGrant,
) -> nail_api::RemoteSession {
    store
        .create_remote_session(
            &NewRemoteSession {
                token_hash: hash_remote_secret(token),
                user_id: user.clone(),
                device_id: device.clone(),
                auth_method: RemoteAuthMethod::Passkey,
                grant,
                expires_at: T0 + 3_600,
                ip: Some("192.168.1.20".into()),
                user_agent: None,
            },
            T0,
        )
        .unwrap()
}

#[test]
fn users_have_unique_names_and_only_hashed_credentials() {
    let store = store();
    let ada = store
        .create_remote_user("ada", Some("Ada"), &full(), T0)
        .unwrap();
    assert_eq!(ada.id.as_str(), "ruser-1");
    assert_eq!(ada.created_at, "2026-09-21T14:13:20Z");
    assert!(!ada.has_password && !ada.totp_enabled && ada.passkeys.is_empty());
    assert!(matches!(
        store.create_remote_user("ADA", None, &full(), T0),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        store.create_remote_user("  ", None, &full(), T0),
        Err(Error::InvalidData(_))
    ));
    assert_eq!(
        store.remote_user_by_name("Ada").unwrap().unwrap().id,
        ada.id
    );

    // Plain text is refused, by the accessor and by the schema.
    assert!(matches!(
        store.set_remote_password_hash(&ada.id, Some("hunter2"), T0),
        Err(Error::InvalidData(_))
    ));
    assert!(matches!(
        store.set_remote_totp(&ada.id, Some("JBSWY3DPEHPK3PXP"), T0),
        Err(Error::InvalidData(_))
    ));
    let user = store
        .set_remote_password_hash(&ada.id, Some(PHC), T0)
        .unwrap();
    assert!(user.has_password);
    let user = store
        .set_remote_totp(&ada.id, Some("keychain:nail/remote-totp/ruser-1"), T0)
        .unwrap();
    assert!(user.totp_enabled);
    let creds = store.remote_credentials(&ada.id).unwrap();
    assert_eq!(creds.password_hash.as_deref(), Some(PHC));
    assert_eq!(
        creds.totp_ref.as_deref(),
        Some("keychain:nail/remote-totp/ruser-1")
    );

    // A TOTP step is accepted once, and never an older one.
    assert!(store.accept_totp_step(&ada.id, 100).unwrap());
    assert!(!store.accept_totp_step(&ada.id, 100).unwrap());
    assert!(!store.accept_totp_step(&ada.id, 99).unwrap());
    assert!(store.accept_totp_step(&ada.id, 101).unwrap());
    // Re-enrolling resets the guard.
    store
        .set_remote_totp(&ada.id, Some("keychain:nail/remote-totp/ruser-1"), T0)
        .unwrap();
    assert!(store.accept_totp_step(&ada.id, 5).unwrap());

    let user = store.set_remote_user_lock(&ada.id, Some(T0 + 600)).unwrap();
    assert_eq!(user.locked_until.as_deref(), Some("2026-09-21T14:23:20Z"));
    store.record_remote_login(&ada.id, T0 + 700).unwrap();
    let user = store.get_remote_user(&ada.id).unwrap();
    assert_eq!(user.locked_until, None);
    assert!(user.last_login_at.is_some());

    assert!(
        store
            .get_remote_user(&"ruser-99".into())
            .unwrap_err()
            .is_not_found()
    );
    assert!(
        store
            .get_remote_user(&"bogus".into())
            .unwrap_err()
            .is_not_found()
    );
}

#[test]
fn passkeys_belong_to_users() {
    let store = store();
    let ada = store.create_remote_user("ada", None, &full(), T0).unwrap();
    let phone = device(&store, &ada.id, "device-secret", full());
    let key = NewPasskey {
        user_id: ada.id.clone(),
        credential_id: vec![1, 2, 3],
        name: " Phone ".into(),
        credential: r#"{"counter":0}"#.into(),
        device_id: Some(phone.id.clone()),
    };
    let stored = store.add_passkey(&key, T0).unwrap();
    assert_eq!(stored.passkey.name, "Phone");
    assert_eq!(stored.passkey.device_id, Some(phone.id.clone()));
    assert!(matches!(
        store.add_passkey(&key, T0),
        Err(Error::Conflict(_))
    ));

    let found = store.passkey_by_credential_id(&[1, 2, 3]).unwrap().unwrap();
    assert_eq!(found.user_id, ada.id);
    store
        .update_passkey_credential(&found.passkey.id, r#"{"counter":7}"#, T0 + 5)
        .unwrap();
    let keys = store.passkeys_of_user(&ada.id).unwrap();
    assert_eq!(keys[0].credential, r#"{"counter":7}"#);
    assert!(keys[0].passkey.last_used_at.is_some());

    let user = store.rename_passkey(&found.passkey.id, "Laptop").unwrap();
    assert_eq!(user.passkeys[0].name, "Laptop");
    let user = store.delete_passkey(&found.passkey.id).unwrap();
    assert!(user.passkeys.is_empty(), "{:?}", user.passkeys);
    assert!(
        store
            .delete_passkey(&found.passkey.id)
            .unwrap_err()
            .is_not_found()
    );
}

#[test]
fn grants_narrow_from_user_to_device_to_session() {
    let store = store();
    let ada = store
        .create_remote_user("ada", None, &RemoteAccessPreset::NoSend.grant(), T0)
        .unwrap();
    // The device asks for more than the user has: narrowed.
    let phone = device(&store, &ada.id, "s1", full());
    assert!(!phone.grant.mail.has(McpScope::Send));
    assert!(phone.grant.mail.has(McpScope::Organize));
    let sess = session(&store, &ada.id, &phone.id, "t1", full());
    assert_eq!(sess.grant, phone.grant);

    // Narrowing the user narrows the device and its valid sessions.
    store
        .update_remote_user(
            &ada.id,
            &RemoteUserUpdate {
                grant: Some(RemoteAccessPreset::ReadOnly.grant()),
                ..RemoteUserUpdate::default()
            },
            T0 + 1,
        )
        .unwrap();
    let phone = store.get_remote_device(&phone.id).unwrap();
    assert!(!phone.grant.mail.has(McpScope::Organize));
    let sess = store.get_remote_session(&sess.id).unwrap();
    assert!(!sess.grant.mail.has(McpScope::Organize));
    assert!(sess.grant.mail.has(McpScope::Read));

    // Widening the device again stays within the user.
    let phone = store
        .update_remote_device(&phone.id, Some("Tablet"), Some(&full()))
        .unwrap();
    assert_eq!(phone.name, "Tablet");
    assert_eq!(phone.grant, RemoteAccessPreset::ReadOnly.grant());
}

#[test]
fn sessions_rotate_expire_and_end() {
    let store = store();
    let ada = store.create_remote_user("ada", None, &full(), T0).unwrap();
    let phone = device(&store, &ada.id, "s1", full());
    let sess = session(&store, &ada.id, &phone.id, "t1", full());
    assert_eq!(sess.id.as_str(), "rsess-1");
    assert_eq!(sess.user_name, "ada");
    assert_eq!(sess.device_name, "Phone");
    let t1 = hash_remote_secret("t1");
    assert_eq!(
        store
            .remote_session_by_token(&t1, T0 + 1)
            .unwrap()
            .unwrap()
            .id,
        sess.id
    );
    assert!(
        store
            .remote_session_by_token(&t1, T0 + 3_600)
            .unwrap()
            .is_none()
    );

    // Rotation: the old token works during the grace period only.
    let t2 = hash_remote_secret("t2");
    store
        .rotate_remote_session(&sess.id, &t2, T0 + 30, T0 + 10)
        .unwrap();
    assert!(
        store
            .remote_session_by_token(&t2, T0 + 11)
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .remote_session_by_token(&t1, T0 + 29)
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .remote_session_by_token(&t1, T0 + 30)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        store.rotate_remote_session(&sess.id, "not-a-hash", T0, T0),
        Err(Error::InvalidData(_))
    ));

    store
        .touch_remote_session(&sess.id, Some("10.0.0.5"), T0 + 20)
        .unwrap();
    let reauth = store.set_remote_reauth(&sess.id, T0 + 21).unwrap();
    assert_eq!(reauth.ip.as_deref(), Some("10.0.0.5"));
    assert!(reauth.reauth_at.is_some());

    assert_eq!(store.count_active_remote_sessions(T0 + 22).unwrap(), 1);
    let listed = store
        .list_remote_sessions(&RemoteSessionFilter::default(), T0 + 22)
        .unwrap();
    assert_eq!(listed.len(), 1);

    // Idle expiry.
    let other = session(&store, &ada.id, &phone.id, "t3", full());
    let expired = store.expire_remote_sessions(T0 + 1_000, 900).unwrap();
    assert_eq!(expired, [sess.id.clone(), other.id.clone()]);
    let ended = store.get_remote_session(&sess.id).unwrap();
    assert_eq!(ended.end_reason, Some(SessionEndReason::Expired));
    assert!(
        store
            .remote_session_by_token(&t2, T0 + 1_001)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .list_remote_sessions(&RemoteSessionFilter::default(), T0 + 1_001)
            .unwrap()
            .len(),
        0
    );
    let all = store
        .list_remote_sessions(
            &RemoteSessionFilter {
                include_ended: true,
                ..RemoteSessionFilter::default()
            },
            T0 + 1_001,
        )
        .unwrap();
    assert_eq!(all.len(), 2);

    let third = session(&store, &ada.id, &phone.id, "t4", full());
    assert!(
        store
            .end_remote_session(&third.id, SessionEndReason::Logout, T0 + 5)
            .unwrap()
    );
    assert!(
        !store
            .end_remote_session(&third.id, SessionEndReason::Logout, T0 + 6)
            .unwrap()
    );
}

#[test]
fn revoking_and_disabling_end_sessions() {
    let store = store();
    let ada = store.create_remote_user("ada", None, &full(), T0).unwrap();
    let phone = device(&store, &ada.id, "s1", full());
    let laptop = device(&store, &ada.id, "s2", full());
    let a = session(&store, &ada.id, &phone.id, "a", full());
    let b = session(&store, &ada.id, &laptop.id, "b", full());
    assert_eq!(
        store
            .remote_device_by_secret(&hash_remote_secret("s1"))
            .unwrap()
            .unwrap()
            .id,
        phone.id
    );

    let ended = store.revoke_remote_device(&phone.id, T0 + 1).unwrap();
    assert_eq!(ended, std::slice::from_ref(&a.id));
    assert!(
        store
            .revoke_remote_device(&phone.id, T0 + 2)
            .unwrap()
            .is_empty(),
        "{:?}",
        store.revoke_remote_device(&phone.id, T0 + 2).unwrap()
    );
    assert!(
        store
            .remote_device_by_secret(&hash_remote_secret("s1"))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.get_remote_session(&a.id).unwrap().end_reason,
        Some(SessionEndReason::DeviceRevoked)
    );
    // A revoked device cannot log in again.
    assert!(matches!(
        store.create_remote_session(
            &NewRemoteSession {
                token_hash: hash_remote_secret("c"),
                user_id: ada.id.clone(),
                device_id: phone.id.clone(),
                auth_method: RemoteAuthMethod::Password,
                grant: full(),
                expires_at: T0 + 100,
                ip: None,
                user_agent: None,
            },
            T0 + 3
        ),
        Err(Error::Conflict(_))
    ));

    let user = store
        .update_remote_user(
            &ada.id,
            &RemoteUserUpdate {
                disabled: Some(true),
                ..RemoteUserUpdate::default()
            },
            T0 + 4,
        )
        .unwrap();
    assert!(user.disabled);
    assert_eq!(
        store.get_remote_session(&b.id).unwrap().end_reason,
        Some(SessionEndReason::UserChanged)
    );

    let bob = store.create_remote_user("bob", None, &full(), T0).unwrap();
    let bob_phone = device(&store, &bob.id, "s3", full());
    let c = session(&store, &bob.id, &bob_phone.id, "c", full());
    assert_eq!(
        store
            .end_remote_sessions(None, SessionEndReason::Revoked, T0 + 5)
            .unwrap(),
        [c.id]
    );

    // Deleting a user removes devices, sessions and passkeys.
    assert!(store.delete_remote_user(&ada.id).unwrap());
    assert!(!store.delete_remote_user(&ada.id).unwrap());
    assert_eq!(store.list_remote_devices(Some(&ada.id)).unwrap().len(), 0);
    assert_eq!(store.list_remote_devices(None).unwrap().len(), 1);
}

#[test]
fn pairing_codes_are_single_use_and_short_lived() {
    let store = store();
    let ada = store
        .create_remote_user("ada", None, &RemoteAccessPreset::ReadOnly.grant(), T0)
        .unwrap();
    let new = |code: &str, expires_at| NewPairingCode {
        user_id: ada.id.clone(),
        code_hash: hash_remote_secret(code),
        device_name: Some("Phone".into()),
        grant: full(),
        expires_at,
    };
    let code = store
        .create_pairing_code(&new("K7QM2WXR", T0 + 600), T0)
        .unwrap();
    assert_eq!(code.id.as_str(), "pair-1");
    assert_eq!(code.state, PairingState::Pending);
    assert_eq!(code.grant, RemoteAccessPreset::ReadOnly.grant());
    let expired = store
        .create_pairing_code(&new("OLDCODE1", T0 + 60), T0)
        .unwrap();
    let cancelled = store
        .create_pairing_code(&new("CANCEL01", T0 + 600), T0)
        .unwrap();
    assert!(store.cancel_pairing_code(&cancelled.id, T0 + 1).unwrap());
    assert!(!store.cancel_pairing_code(&cancelled.id, T0 + 2).unwrap());

    let listed = store.list_pairing_codes(T0 + 100).unwrap();
    assert_eq!(listed[0].id, code.id);
    let state = |id| listed.iter().find(|c| c.id == id).unwrap().state;
    assert_eq!(state(expired.id.clone()), PairingState::Expired);
    assert_eq!(state(cancelled.id.clone()), PairingState::Cancelled);

    let wrong = hash_remote_secret("WRONG000");
    assert!(
        store
            .redeem_pairing_code(&wrong, T0 + 100)
            .unwrap_err()
            .is_not_found()
    );
    assert!(
        store
            .redeem_pairing_code(&hash_remote_secret("OLDCODE1"), T0 + 100)
            .unwrap_err()
            .is_not_found()
    );
    assert!(
        store
            .redeem_pairing_code(&hash_remote_secret("CANCEL01"), T0 + 100)
            .unwrap_err()
            .is_not_found()
    );
    let used = store
        .redeem_pairing_code(&hash_remote_secret("K7QM2WXR"), T0 + 100)
        .unwrap();
    assert_eq!(used.state, PairingState::Used);
    // A replay of the same code fails.
    assert!(
        store
            .redeem_pairing_code(&hash_remote_secret("K7QM2WXR"), T0 + 101)
            .unwrap_err()
            .is_not_found()
    );
    let phone = device(&store, &ada.id, "s1", used.grant.clone());
    store.set_pairing_device(&used.id, &phone.id).unwrap();
    let listed = store.list_pairing_codes(T0 + 200).unwrap();
    let used = listed.iter().find(|c| c.id == used.id).unwrap();
    assert_eq!(used.device_id, Some(phone.id));
}

#[test]
fn throttle_counts_failures_per_window() {
    let store = store();
    assert!(store.auth_throttle("ip:10.0.0.9").unwrap().is_none());
    for i in 0..3 {
        let rec = store
            .record_auth_failure("ip:10.0.0.9", T0 + i, 600)
            .unwrap();
        assert_eq!(rec.failures, u32::try_from(i + 1).unwrap());
        assert_eq!(rec.first_failure_at, T0);
    }
    store.lock_auth_throttle("ip:10.0.0.9", T0 + 900).unwrap();
    let rec = store.auth_throttle("ip:10.0.0.9").unwrap().unwrap();
    assert_eq!(rec.locked_until, Some(T0 + 900));
    // A failure after the window starts a new one.
    let rec = store
        .record_auth_failure("ip:10.0.0.9", T0 + 700, 600)
        .unwrap();
    assert_eq!((rec.failures, rec.first_failure_at), (1, T0 + 700));
    store.clear_auth_throttle("ip:10.0.0.9").unwrap();
    assert!(store.auth_throttle("ip:10.0.0.9").unwrap().is_none());
}

#[test]
fn audit_log_pages_newest_first_and_survives_deletion() {
    let store = store();
    let ada = store.create_remote_user("ada", None, &full(), T0).unwrap();
    let mut failed =
        RemoteAuditRecord::new(T0, RemoteAuditKind::Login, RemoteAuditOutcome::Failure);
    failed.user_name = Some("ada".into());
    failed.ip = Some("10.0.0.9".into());
    failed.method = Some(RemoteAuthMethod::Password);
    failed.error = Some(ErrorCode::AuthFailed);
    store.append_remote_audit(&failed).unwrap();
    let mut ok =
        RemoteAuditRecord::new(T0 + 1, RemoteAuditKind::Login, RemoteAuditOutcome::Success);
    ok.user_id = Some(ada.id.clone());
    ok.session_id = Some("rsess-42".into()); // gone: recorded without it
    let id = store.append_remote_audit(&ok).unwrap();
    let denied =
        RemoteAuditRecord::new(T0 + 2, RemoteAuditKind::Request, RemoteAuditOutcome::Denied);
    store.append_remote_audit(&denied).unwrap();

    let page = store.list_remote_audit(None, None, 2).unwrap();
    assert_eq!(page.len(), 2);
    assert_eq!(page[0].kind, RemoteAuditKind::Request);
    let older = store.list_remote_audit(None, Some(page[1].id), 10).unwrap();
    assert_eq!(older.len(), 1);
    assert_eq!(older[0].error, Some(ErrorCode::AuthFailed));
    assert_eq!(older[0].method, Some(RemoteAuthMethod::Password));
    let mine = store.list_remote_audit(Some(&ada.id), None, 10).unwrap();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].id, id);
    assert_eq!(mine[0].session_id, None);

    store.delete_remote_user(&ada.id).unwrap();
    assert_eq!(store.list_remote_audit(None, None, 10).unwrap().len(), 3);
    assert_eq!(store.purge_remote_audit(T0 + 2).unwrap(), 2);
}

#[test]
fn remote_content_allow_lists() {
    let store = store();
    let m1 = MessageId::from("m-1");
    assert_eq!(
        store
            .remote_content_allowed_by(&m1, Some("News@Shop.example"))
            .unwrap(),
        None
    );
    store
        .allow_remote_content(RemoteContentScope::Domain, "Shop.Example", None, T0)
        .unwrap();
    assert_eq!(
        store
            .remote_content_allowed_by(&m1, Some("news@shop.example"))
            .unwrap(),
        Some(RemoteContentScope::Domain)
    );
    let entry = store
        .allow_remote_content(
            RemoteContentScope::Sender,
            " News@Shop.example ",
            None,
            T0 + 1,
        )
        .unwrap();
    assert_eq!(entry.value, "news@shop.example");
    store
        .allow_remote_content(RemoteContentScope::Message, "m-1", Some("Sale"), T0 + 2)
        .unwrap();
    // The most specific entry wins.
    assert_eq!(
        store
            .remote_content_allowed_by(&m1, Some("news@shop.example"))
            .unwrap(),
        Some(RemoteContentScope::Message)
    );
    assert_eq!(
        store
            .remote_content_allowed_by(&"m-2".into(), Some("NEWS@shop.example"))
            .unwrap(),
        Some(RemoteContentScope::Sender)
    );
    assert_eq!(
        store
            .remote_content_allowed_by(&"m-2".into(), Some("other@sub.shop.example"))
            .unwrap(),
        None
    );
    assert_eq!(
        store
            .remote_content_allowed_by(&"m-2".into(), None)
            .unwrap(),
        None
    );
    assert!(matches!(
        store.allow_remote_content(RemoteContentScope::Sender, "nobody", None, T0),
        Err(Error::InvalidData(_))
    ));

    let all = store.list_remote_content_allows().unwrap();
    let scopes: Vec<_> = all.iter().map(|e| e.scope).collect();
    assert_eq!(
        scopes,
        [
            RemoteContentScope::Message,
            RemoteContentScope::Sender,
            RemoteContentScope::Domain
        ]
    );
    assert_eq!(all[0].label.as_deref(), Some("Sale"));
    assert!(
        store
            .remove_remote_content_allow(RemoteContentScope::Domain, "SHOP.example")
            .unwrap()
    );
    assert!(
        !store
            .remove_remote_content_allow(RemoteContentScope::Domain, "shop.example")
            .unwrap()
    );
}

#[test]
fn remote_ui_settings_are_a_json_setting() {
    let store = store();
    assert_eq!(
        store
            .setting::<RemoteUiSettings>(REMOTE_UI_SETTINGS_KEY)
            .unwrap(),
        None
    );
    let settings = RemoteUiSettings {
        enabled: true,
        bind_address: "0.0.0.0".into(),
        ..RemoteUiSettings::default()
    };
    store
        .set_setting(REMOTE_UI_SETTINGS_KEY, &settings)
        .unwrap();
    assert_eq!(
        store
            .setting::<RemoteUiSettings>(REMOTE_UI_SETTINGS_KEY)
            .unwrap(),
        Some(settings)
    );
}

#[test]
fn stale_message_allows_are_purged() {
    let store = store();
    store
        .allow_remote_content(RemoteContentScope::Message, "gone-1", None, T0)
        .unwrap();
    store
        .allow_remote_content(RemoteContentScope::Sender, "news@shop.example", None, T0)
        .unwrap();
    // No message "gone-1" exists (deleted or expunged): its entry goes,
    // sender and domain entries never do.
    assert_eq!(store.purge_stale_remote_content_allows().unwrap(), 1);
    let left = store.list_remote_content_allows().unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].scope, RemoteContentScope::Sender);
    assert_eq!(store.purge_stale_remote_content_allows().unwrap(), 0);
}
