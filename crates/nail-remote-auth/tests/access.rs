//! Authorization of remote sessions: scopes, admin with re-authentication,
//! self-service, desktop-only requests, audit; and the exhaustiveness of
//! `required_access`.

mod common;

use common::*;
use nail_api::remote::{
    RemoteAccessPreset, RemoteAuditKind, RemoteAuditOutcome, RemoteGrant, RemoteSessionFilter,
};
use nail_api::{ErrorCode, FolderId, McpGrant, MessageId, Request};
use nail_remote_auth::access::{account_allowed, folder_allowed};
use nail_remote_auth::{Access, required_access};

fn list_messages() -> Request {
    Request::ListMessages {
        folder_id: FolderId::from("f1"),
        offset: 0,
        limit: 50,
    }
}

fn delete(permanent: bool) -> Request {
    Request::DeleteMessages {
        message_ids: vec![MessageId::from("m1")],
        permanent,
    }
}

#[tokio::test]
async fn scopes_of_the_session_decide() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let code = h.code(&ada, Some(&read_only()));
    let mut browser = Browser::new("192.0.2.10");
    logged_in(&browser.pair(&h, &code, "Tablet").await);
    let ctx = browser.resolve(&h).unwrap();

    assert!(h.auth.authorize(&ctx, &Request::Ping).is_ok());
    assert!(h.auth.authorize(&ctx, &list_messages()).is_ok());
    let error = h.auth.authorize(&ctx, &delete(false)).unwrap_err();
    assert_eq!(error.code, ErrorCode::Forbidden);
    assert!(error.message.contains("organize"), "{}", error.message);
    let send = Request::SendDraft {
        draft_id: "d1".into(),
        delay_secs: None,
        send_at: None,
        crypto: None,
        expected_revision: None,
    };
    assert_eq!(
        h.auth.authorize(&ctx, &send).unwrap_err().code,
        ErrorCode::Forbidden
    );

    // Denials are audited; allowed reads are not.
    let audit = h.store.list_remote_audit(None, None, 10).unwrap();
    let requests: Vec<_> = audit
        .iter()
        .filter(|e| e.kind == RemoteAuditKind::Request)
        .collect();
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|e| e.outcome == RemoteAuditOutcome::Denied)
    );
    assert_eq!(requests[1].name.as_deref(), Some("DeleteMessages"));
    assert_eq!(requests[1].session_id.as_ref(), Some(&ctx.session.id));
    assert_eq!(requests[1].error, Some(ErrorCode::Forbidden));
}

#[tokio::test]
async fn allowed_writes_are_audited() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    let ctx = browser.resolve(&h).unwrap();
    assert!(h.auth.authorize(&ctx, &delete(false)).is_ok());
    assert!(h.auth.authorize(&ctx, &delete(true)).is_ok());
    let audit = h.store.list_remote_audit(None, None, 2).unwrap();
    assert!(audit.iter().all(|e| e.kind == RemoteAuditKind::Request
        && e.outcome == RemoteAuditOutcome::Success
        && e.name.as_deref() == Some("DeleteMessages")));
}

#[tokio::test]
async fn an_empty_grant_reads_nothing() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let nowhere = RemoteGrant {
        mail: McpGrant {
            scopes: vec![nail_api::McpScope::Read],
            all_accounts: false,
            ..McpGrant::default()
        },
        admin: false,
    };
    let code = h.code(&ada, Some(&nowhere));
    let mut browser = Browser::new("192.0.2.10");
    logged_in(&browser.pair(&h, &code, "Kiosk").await);
    let ctx = browser.resolve(&h).unwrap();
    assert_eq!(
        h.auth.authorize(&ctx, &list_messages()).unwrap_err().code,
        ErrorCode::Forbidden
    );
    assert!(h.auth.authorize(&ctx, &Request::Ping).is_ok());
}

#[tokio::test]
async fn admin_needs_the_grant_and_a_fresh_authentication() {
    let h = Harness::new();
    let root = h.user("root", true).await;
    let ada = h.user("ada", false).await;
    let mut admin = Browser::new("192.0.2.10");
    let mut grant = RemoteAccessPreset::Full.grant();
    grant.admin = true;
    let code = h.code(&root, Some(&grant));
    logged_in(&admin.pair(&h, &code, "Admin laptop").await);
    let mut user = h.paired(&ada, "192.0.2.11").await;

    let admin_ctx = admin.resolve(&h).unwrap();
    assert!(admin_ctx.session.grant.admin);
    assert!(
        h.auth
            .authorize(&admin_ctx, &Request::ListRemoteUsers)
            .is_ok()
    );
    let user_ctx = user.resolve(&h).unwrap();
    assert_eq!(
        h.auth
            .authorize(&user_ctx, &Request::ListRemoteUsers)
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );

    // After the re-authentication window: reauth_required.
    h.clock
        .advance(i64::from(h.auth.settings().reauth_minutes) * 60 + 1);
    let admin_ctx = admin.resolve(&h).unwrap();
    let error = h
        .auth
        .authorize(
            &admin_ctx,
            &Request::UpdateRemoteUiSettings {
                settings: Default::default(),
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ReauthRequired);
    // Its own sessions stay visible without re-authenticating.
    let own = Request::ListRemoteSessions {
        filter: RemoteSessionFilter {
            user_id: Some(root.id.clone()),
            ..Default::default()
        },
    };
    assert!(h.auth.authorize(&admin_ctx, &own).is_ok());
    let everyone = Request::ListRemoteSessions {
        filter: RemoteSessionFilter::default(),
    };
    assert_eq!(
        h.auth.authorize(&admin_ctx, &everyone).unwrap_err().code,
        ErrorCode::ReauthRequired
    );
    // Sensitive non-admin requests too.
    let unlock = Request::UnlockKey {
        key_id: "k1".into(),
        passphrase: "x".into(),
        remember_minutes: None,
    };
    assert_eq!(
        required_access(&unlock).access,
        Access::Sensitive(nail_api::McpScope::Encrypted)
    );

    admin.reauth_password(&h, "root", PASSWORD).await;
    let admin_ctx = admin.resolve(&h).unwrap();
    assert!(
        h.auth
            .authorize(
                &admin_ctx,
                &Request::UpdateRemoteUiSettings {
                    settings: Default::default(),
                }
            )
            .is_ok()
    );
    assert!(
        h.audit_kinds()
            .contains(&(RemoteAuditKind::Request, RemoteAuditOutcome::Success))
    );
}

#[tokio::test]
async fn self_service_for_own_sessions_and_devices() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let bob = h.user("bob", false).await;
    let mut phone = h.paired(&ada, "192.0.2.10").await;
    let mut laptop = h.paired(&ada, "192.0.2.11").await;
    let mut bobs = h.paired(&bob, "192.0.2.12").await;
    let ctx = phone.resolve(&h).unwrap();
    let laptop_ctx = laptop.resolve(&h).unwrap();
    let bob_ctx = bobs.resolve(&h).unwrap();

    let own_list = Request::ListRemoteSessions {
        filter: RemoteSessionFilter {
            user_id: Some(ada.id.clone()),
            ..Default::default()
        },
    };
    assert!(h.auth.authorize(&ctx, &own_list).is_ok());
    let all = Request::ListRemoteSessions {
        filter: RemoteSessionFilter::default(),
    };
    assert_eq!(
        h.auth.authorize(&ctx, &all).unwrap_err().code,
        ErrorCode::Forbidden
    );
    let revoke_own = Request::RevokeRemoteSession {
        session_id: laptop_ctx.session.id.clone(),
    };
    assert!(h.auth.authorize(&ctx, &revoke_own).is_ok());
    let revoke_bobs = Request::RevokeRemoteSession {
        session_id: bob_ctx.session.id.clone(),
    };
    assert_eq!(
        h.auth.authorize(&ctx, &revoke_bobs).unwrap_err().code,
        ErrorCode::Forbidden
    );
    assert!(
        h.auth
            .authorize(
                &ctx,
                &Request::RevokeRemoteDevice {
                    device_id: laptop_ctx.device.id.clone(),
                }
            )
            .is_ok()
    );
    assert!(
        h.auth
            .authorize(
                &ctx,
                &Request::RevokeRemoteDevice {
                    device_id: bob_ctx.device.id.clone(),
                }
            )
            .is_err()
    );
    assert!(
        h.auth
            .authorize(
                &ctx,
                &Request::ListRemoteDevices {
                    user_id: Some(ada.id.clone()),
                }
            )
            .is_ok()
    );
}

#[tokio::test]
async fn desktop_only_requests_are_refused_even_for_admins() {
    let h = Harness::new();
    let root = h.user("root", true).await;
    let mut grant = RemoteAccessPreset::Full.grant();
    grant.admin = true;
    let code = h.code(&root, Some(&grant));
    let mut admin = Browser::new("192.0.2.10");
    logged_in(&admin.pair(&h, &code, "Admin").await);
    let ctx = admin.resolve(&h).unwrap();
    for request in [
        Request::OpenConfigFile {
            file: nail_api::ConfigFile::Theme,
            line: 1,
            column: 1,
        },
        Request::StartOAuth {
            provider: "google".into(),
            login_hint: None,
            protocol: None,
        },
    ] {
        let error = h.auth.authorize(&ctx, &request).unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden, "{request:?}");
    }
}

#[test]
fn account_and_folder_filters() {
    let grant = read_only();
    assert!(account_allowed(&grant, &"work".into()));
    let limited = RemoteGrant {
        mail: McpGrant {
            scopes: vec![nail_api::McpScope::Read],
            accounts: vec!["work".into()],
            excluded_folders: vec!["f-hr".into()],
            ..McpGrant::default()
        },
        admin: false,
    };
    assert!(account_allowed(&limited, &"work".into()));
    assert!(!account_allowed(&limited, &"home".into()));
    assert!(!folder_allowed(&limited, &"f-hr".into()));
    assert!(folder_allowed(&limited, &"f-inbox".into()));
}

/// Every `Request` variant has an arm in `required_access` whose name is
/// its wire name (the match itself is exhaustive; this guards the names).
#[test]
fn every_request_has_its_wire_name() {
    let protocol = include_str!("../../nail-api/src/protocol.rs");
    let access = include_str!("../src/access.rs");
    let start = protocol.find("pub enum Request {").unwrap();
    let end = protocol[start..].find("\n}\n").unwrap() + start;
    let mut variants = Vec::new();
    for line in protocol[start..end].lines().skip(1) {
        let Some(rest) = line.strip_prefix("    ") else {
            continue;
        };
        if rest.starts_with(' ') || rest.starts_with('/') || rest.starts_with('#') {
            continue;
        }
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            variants.push(name);
        }
    }
    assert!(variants.len() > 100, "{}", variants.len());
    for name in &variants {
        let arm = format!("Request::{name} ");
        let arm_unit = format!("Request::{name} =>");
        assert!(
            access.contains(&arm) || access.contains(&arm_unit),
            "no arm for {name}"
        );
        assert!(
            access.contains(&format!("(\"{name}\"")) || access.contains(&format!("\"{name}\",")),
            "no name for {name}"
        );
    }
}

#[tokio::test]
async fn sensitive_requests_need_the_scope_and_a_fresh_login() {
    let h = Harness::new();
    let ada = h.user("ada", false).await;
    let mut grant = RemoteAccessPreset::Full.grant();
    grant.mail.scopes.push(nail_api::McpScope::Encrypted);
    h.store
        .update_remote_user(
            &ada.id,
            &nail_api::remote::RemoteUserUpdate {
                grant: Some(grant.clone()),
                ..Default::default()
            },
            h.now(),
        )
        .unwrap();
    let unlock = Request::UnlockKey {
        key_id: "k1".into(),
        passphrase: "x".into(),
        remember_minutes: None,
    };
    // Without the scope: forbidden.
    let code = h.code(&ada, Some(&RemoteAccessPreset::Full.grant()));
    let mut plain = Browser::new("192.0.2.10");
    logged_in(&plain.pair(&h, &code, "Plain").await);
    let ctx = plain.resolve(&h).unwrap();
    assert_eq!(
        h.auth.authorize(&ctx, &unlock).unwrap_err().code,
        ErrorCode::Forbidden
    );
    // With it: fine while fresh, then reauth_required.
    let code = h.code(&ada, Some(&grant));
    let mut trusted = Browser::new("192.0.2.11");
    logged_in(&trusted.pair(&h, &code, "Trusted").await);
    let ctx = trusted.resolve(&h).unwrap();
    assert!(h.auth.authorize(&ctx, &unlock).is_ok());
    h.clock
        .advance(i64::from(h.auth.settings().reauth_minutes) * 60);
    let ctx = trusted.resolve(&h).unwrap();
    assert_eq!(
        h.auth.authorize(&ctx, &unlock).unwrap_err().code,
        ErrorCode::ReauthRequired
    );
}
