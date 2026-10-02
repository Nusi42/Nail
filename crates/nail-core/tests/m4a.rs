//! Wave M4a contract (ADR 0079): the confirmation, policy, rate-limit and
//! plug-in activity requests are served from the store; the plug-in host
//! requests go to the host (WP105): no annotations without open hooks,
//! previews, actions and manual job runs of a plug-in that is not running
//! fail, an unknown veto has nothing to "Send anyway".

use nail_api::agents::{
    ActionActor, ActionAttribution, ActionKind, ActionPolicyMode, ActionTarget, ActionTargetKind,
    ConfirmationDecision, ConfirmationFilter, ConfirmationState, NewConfirmation,
};
use nail_api::mcp::{McpGrant, McpRateLimits};
use nail_api::plugins::{PluginActionTarget, PluginManifest, PluginPackageSource};
use nail_api::{ErrorCode, Event, LocalizedMessage, Request, Response};
use nail_core::{Service, ServiceOptions};
use nail_store::PluginInstall;
use serde_json::json;

fn setup() -> (Service, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    options.start_engines = false;
    let service = Service::open(nail_core::demo::demo_options(options)).unwrap();
    (service, data, config)
}

async fn create_client(service: &Service) -> nail_api::McpClientId {
    let Response::McpClientCreated { client, .. } = service
        .handle(Request::CreateMcpClient {
            name: "Agent".into(),
            grant: McpGrant::default(),
        })
        .await
    else {
        panic!("client expected");
    };
    client.id
}

fn error_of(response: Response) -> (ErrorCode, Option<String>) {
    match response {
        Response::Error {
            code, localized, ..
        } => (code, localized.map(|l| l.id)),
        other => panic!("error expected, got {other:?}"),
    }
}

#[tokio::test]
async fn confirmations_are_listed_and_decided() {
    let (service, _data, _config) = setup();
    let client = create_client(&service).await;
    let actor = ActionActor::McpClient {
        client_id: client.clone(),
    };
    let mut events = service.subscribe();

    let empty = service
        .handle(Request::ListConfirmations {
            filter: ConfirmationFilter::Pending,
            actor: None,
            before: None,
            limit: 50,
        })
        .await;
    assert_eq!(
        empty,
        Response::Confirmations {
            confirmations: vec![]
        }
    );

    // A requester (WP101 / WP102) stores a pending confirmation.
    let now = chrono::Utc::now().timestamp();
    let new = |kind: ActionKind| NewConfirmation {
        requested_by: ActionAttribution {
            actor: actor.clone(),
            name: "Agent".into(),
            on_behalf_of: None,
        },
        kind,
        operation: "send_message".into(),
        summary: LocalizedMessage::new("confirmation-summary-send")
            .arg("actor", "Agent")
            .arg("subject", "Hi")
            .arg("count", 1),
        targets: vec![ActionTarget {
            kind: ActionTargetKind::Draft,
            id: "d1".into(),
            label: "Hi".into(),
        }],
        changes: vec![],
        account_ids: vec!["work".into()],
        payload: json!({"draft_id": "d1"}),
        idempotency_key: None,
        timeout_secs: 300,
    };
    let send = service
        .store()
        .insert_confirmation(new(ActionKind::Send), now, 20)
        .unwrap()
        .confirmation()
        .clone();
    let trash = service
        .store()
        .insert_confirmation(new(ActionKind::Trash), now, 20)
        .unwrap()
        .confirmation()
        .clone();

    let Response::Confirmations { confirmations } = service
        .handle(Request::ListConfirmations {
            filter: ConfirmationFilter::Pending,
            actor: Some(actor.clone()),
            before: None,
            limit: 50,
        })
        .await
    else {
        panic!("confirmations expected");
    };
    assert_eq!(confirmations, [send.clone(), trash.clone()]);

    // "Always allow" is refused for a destructive kind, with its text.
    let refused = service
        .handle(Request::DecideConfirmation {
            confirmation_id: trash.id.clone(),
            decision: ConfirmationDecision::Approve,
            always_allow: true,
        })
        .await;
    assert_eq!(
        error_of(refused),
        (
            ErrorCode::InvalidRequest,
            Some("action-policy-issue-destructive".into())
        )
    );

    // Approve the send with "always allow": the policy changes too.
    let Response::ConfirmationDecided { confirmation } = service
        .handle(Request::DecideConfirmation {
            confirmation_id: send.id.clone(),
            decision: ConfirmationDecision::Approve,
            always_allow: true,
        })
        .await
    else {
        panic!("decided expected");
    };
    assert_eq!(confirmation.state, ConfirmationState::Approved);
    let mut saw_policy = false;
    let mut saw_resolved = false;
    while let Ok(event) = events.try_recv() {
        match event {
            Event::ActionPoliciesChanged { actor: a } => saw_policy = a == actor,
            Event::ConfirmationResolved { confirmation } => {
                saw_resolved = confirmation.id == send.id;
            }
            _ => {}
        }
    }
    assert!(saw_policy && saw_resolved);
    let Response::ActionPolicies { policies, .. } = service
        .handle(Request::ListActionPolicies {
            actor: actor.clone(),
        })
        .await
    else {
        panic!("policies expected");
    };
    let send_policy = policies
        .iter()
        .find(|p| p.kind == ActionKind::Send)
        .unwrap();
    assert_eq!(send_policy.mode, ActionPolicyMode::Allow);

    // Deciding twice conflicts; deny the other.
    let again = service
        .handle(Request::DecideConfirmation {
            confirmation_id: send.id.clone(),
            decision: ConfirmationDecision::Deny,
            always_allow: false,
        })
        .await;
    assert_eq!(error_of(again).0, ErrorCode::Conflict);
    let Response::ConfirmationDecided { confirmation } = service
        .handle(Request::DecideConfirmation {
            confirmation_id: trash.id.clone(),
            decision: ConfirmationDecision::Deny,
            always_allow: false,
        })
        .await
    else {
        panic!("decided expected");
    };
    assert_eq!(confirmation.state, ConfirmationState::Denied);
    let Response::Confirmations { confirmations } = service
        .handle(Request::ListConfirmations {
            filter: ConfirmationFilter::Decided,
            actor: None,
            before: None,
            limit: 50,
        })
        .await
    else {
        panic!("confirmations expected");
    };
    assert_eq!(confirmations.len(), 2);
    let missing = service
        .handle(Request::DecideConfirmation {
            confirmation_id: "999".into(),
            decision: ConfirmationDecision::Deny,
            always_allow: false,
        })
        .await;
    assert_eq!(error_of(missing).0, ErrorCode::NotFound);
}

#[tokio::test]
async fn policies_and_rate_limits() {
    let (service, _data, _config) = setup();
    let client = create_client(&service).await;
    let actor = ActionActor::McpClient {
        client_id: client.clone(),
    };
    let refused = service
        .handle(Request::SetActionPolicy {
            actor: actor.clone(),
            kind: ActionKind::PermanentDelete,
            mode: Some(ActionPolicyMode::Allow),
        })
        .await;
    assert_eq!(
        error_of(refused),
        (
            ErrorCode::InvalidRequest,
            Some("action-policy-issue-destructive".into())
        )
    );
    let Response::ActionPolicies { policies, .. } = service
        .handle(Request::SetActionPolicy {
            actor: actor.clone(),
            kind: ActionKind::Draft,
            mode: Some(ActionPolicyMode::Ask),
        })
        .await
    else {
        panic!("policies expected");
    };
    let draft = policies
        .iter()
        .find(|p| p.kind == ActionKind::Draft)
        .unwrap();
    assert!(draft.customized && draft.mode == ActionPolicyMode::Ask);
    let unknown = service
        .handle(Request::ListActionPolicies {
            actor: ActionActor::plugin("ghost"),
        })
        .await;
    assert_eq!(error_of(unknown).0, ErrorCode::NotFound);

    let Response::McpClientSaved { client: saved } = service
        .handle(Request::UpdateMcpRateLimits {
            client_id: client.clone(),
            limits: McpRateLimits {
                sends_per_hour: Some(1),
                ..McpRateLimits::default()
            },
        })
        .await
    else {
        panic!("saved expected");
    };
    assert_eq!(saved.rate_limits.sends_per_hour, Some(1));
    let bad = service
        .handle(Request::UpdateMcpRateLimits {
            client_id: client,
            limits: McpRateLimits {
                writes_per_minute: Some(100_000),
                ..McpRateLimits::default()
            },
        })
        .await;
    assert_eq!(
        error_of(bad),
        (
            ErrorCode::InvalidRequest,
            Some("mcp-rate-limit-issue-too-high".into())
        )
    );
}

#[tokio::test]
async fn plugin_activity_and_host_requests() {
    let (service, _data, _config) = setup();
    let manifest = PluginManifest::parse(
        json!({
            "id": "digest", "name": "Digest", "version": "1.0.0", "apiVersion": "1",
            "main": "index.js", "permissions": ["background"],
            "contributes": {"jobs": [{"id": "daily", "schedule": "0 8 * * *"}]}
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();
    service
        .store()
        .install_plugin(&PluginInstall {
            manifest: &manifest,
            source: &PluginPackageSource::Dev {
                folder: "/dev/digest".into(),
            },
            package_sha256: None,
            grants: &[],
            enabled: false,
        })
        .unwrap();
    let Response::PluginActivity { activity } = service
        .handle(Request::GetPluginActivity {
            plugin_id: "digest".into(),
        })
        .await
    else {
        panic!("activity expected");
    };
    assert_eq!(activity.jobs.len(), 1);
    assert_eq!(activity.jobs[0].schedule.as_deref(), Some("0 8 * * *"));
    assert_eq!(activity.jobs[0].run_count, 0);
    assert!(activity.hooks.is_empty());
    let missing = service
        .handle(Request::GetPluginActivity {
            plugin_id: "ghost".into(),
        })
        .await;
    assert_eq!(error_of(missing).0, ErrorCode::NotFound);

    let annotations = service
        .handle(Request::GetMessagePluginAnnotations {
            message_id: "m1".into(),
            client_id: None,
        })
        .await;
    assert_eq!(
        annotations,
        Response::MessagePluginAnnotations {
            message_id: "m1".into(),
            banners: vec![],
            badges: vec![],
        }
    );
    for request in [
        Request::RunPluginJob {
            plugin_id: "digest".into(),
            job: "daily".into(),
        },
        Request::RenderAttachmentPreview {
            message_id: "m1".into(),
            index: 0,
            renderer: "digest.csv".into(),
            client_id: None,
        },
        Request::RunPluginAction {
            action: "digest.save".into(),
            target: PluginActionTarget::Attachment {
                message_id: "m1".into(),
                index: 0,
            },
            context: Default::default(),
            client_id: None,
        },
    ] {
        // Served by the host since WP105: a plug-in that is not running
        // has no jobs, renderers or actions to offer.
        let code = error_of(service.handle(request).await).0;
        assert!(
            matches!(code, ErrorCode::NotFound | ErrorCode::Unavailable),
            "{code:?}"
        );
    }
    let anyway = service
        .handle(Request::OverrideSendVeto {
            veto_id: "v1".into(),
        })
        .await;
    assert_eq!(error_of(anyway).0, ErrorCode::NotFound);
}

fn pending(actor: &ActionActor) -> NewConfirmation {
    NewConfirmation {
        requested_by: ActionAttribution {
            actor: actor.clone(),
            name: "Someone".into(),
            on_behalf_of: None,
        },
        kind: ActionKind::Send,
        operation: "send_message".into(),
        summary: LocalizedMessage::new("confirmation-summary-send"),
        targets: vec![],
        changes: vec![],
        account_ids: vec![],
        payload: json!({}),
        idempotency_key: None,
        timeout_secs: 300,
    }
}

fn state_of(service: &Service, actor: &ActionActor) -> ConfirmationState {
    service
        .store()
        .list_confirmations(ConfirmationFilter::All, Some(actor), None, 1)
        .unwrap()[0]
        .state
}

/// Review M4a: nothing waits for a confirmation of an earlier run, so
/// starting Nail cancels them (approving one would run nothing).
#[tokio::test]
async fn starting_cancels_confirmations_of_an_earlier_run() {
    let (service, _data, _config) = setup();
    let client = create_client(&service).await;
    let actor = ActionActor::mcp_client(client);
    let now = chrono::Utc::now().timestamp();
    service
        .store()
        .insert_confirmation(pending(&actor), now, 20)
        .unwrap();
    service.start();
    assert_eq!(state_of(&service, &actor), ConfirmationState::Cancelled);
    service.shutdown().await;
}

/// Review M4a: pending writes were checked under the permissions of the
/// time; changing an MCP client's grant, a plug-in's grants or disabling
/// the plug-in cancels them.
#[tokio::test]
async fn grant_changes_and_disabling_cancel_pending_confirmations() {
    let (service, _data, _config) = setup();
    let now = chrono::Utc::now().timestamp();
    let client = create_client(&service).await;
    let client_actor = ActionActor::mcp_client(client.clone());
    service
        .store()
        .insert_confirmation(pending(&client_actor), now, 20)
        .unwrap();
    let saved = service
        .handle(Request::UpdateMcpGrants {
            client_id: client,
            grant: McpGrant::default(),
        })
        .await;
    assert!(!matches!(saved, Response::Error { .. }), "{saved:?}");
    assert_eq!(
        state_of(&service, &client_actor),
        ConfirmationState::Cancelled
    );

    let manifest = PluginManifest::parse(
        json!({
            "id": "crm", "name": "CRM", "version": "1.0.0", "apiVersion": "1",
            "main": "index.js", "permissions": ["mail.send"]
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();
    service
        .store()
        .install_plugin(&PluginInstall {
            manifest: &manifest,
            source: &PluginPackageSource::Dev {
                folder: "/dev/crm".into(),
            },
            package_sha256: None,
            grants: &[],
            enabled: false,
        })
        .unwrap();
    let plugin_actor = ActionActor::plugin("crm");
    service
        .store()
        .insert_confirmation(pending(&plugin_actor), now, 20)
        .unwrap();
    let saved = service
        .handle(Request::UpdatePluginGrants {
            plugin_id: "crm".into(),
            grants: vec![],
        })
        .await;
    assert!(!matches!(saved, Response::Error { .. }), "{saved:?}");
    assert_eq!(
        state_of(&service, &plugin_actor),
        ConfirmationState::Cancelled
    );
    service
        .store()
        .insert_confirmation(pending(&plugin_actor), now, 20)
        .unwrap();
    let saved = service
        .handle(Request::DisablePlugin {
            plugin_id: "crm".into(),
        })
        .await;
    assert!(!matches!(saved, Response::Error { .. }), "{saved:?}");
    assert_eq!(
        state_of(&service, &plugin_actor),
        ConfirmationState::Cancelled
    );
}
