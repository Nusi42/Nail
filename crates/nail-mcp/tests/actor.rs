//! M4a WP105: writes of actors that are not MCP clients (plug-ins) through
//! the MCP write tools' plans, and plug-in tools offered to MCP clients.

#![cfg(test)]

#[expect(
    dead_code,
    reason = "the shared test helpers serve several test crates; each uses a subset"
)]
mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{FakeData, create_client, grant, start_with};
use nail_api::agents::{
    ActionActor, ActionAttribution, ActionKind, ActionPolicyMode, ConfirmationFilter,
    ConfirmationSource,
};
use nail_api::agents::{ConfirmationState, McpClientRef};
use nail_api::mcp::tools::{McpEditScope, McpWriteResult};
use nail_api::mcp::{McpCallKind, McpScope};
use nail_api::mcp::{McpOutcome, McpRateLimits};
use nail_api::{ErrorCode, Request};
use nail_mcp::{ActorWrite, ActorWriteRequest, ActorWrites, PimDelete, PluginTool, WriteSettings};
use nail_store::Store;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, ClientConfig};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use serde_json::json;

fn plugin() -> ActionAttribution {
    ActionAttribution {
        actor: ActionActor::Plugin {
            plugin_id: "follow-up".into(),
        },
        name: "Follow up".into(),
        on_behalf_of: None,
    }
}

fn writer(store: &Store, data: Arc<FakeData>) -> ActorWrites<FakeData> {
    ActorWrites::new(
        store.clone(),
        data,
        WriteSettings {
            poll_interval: Duration::from_millis(20),
            ..WriteSettings::default()
        },
    )
}

fn request(write: ActorWrite, accounts: Option<&[&str]>) -> ActorWriteRequest {
    ActorWriteRequest {
        by: plugin(),
        operation: "nail.test".into(),
        write,
        accounts: accounts.map(|a| a.iter().map(|x| (*x).into()).collect()),
        allow_destructive: true,
    }
}

/// Runs a write in a task (it waits for a confirmation).
fn spawn_run(
    writes: &ActorWrites<FakeData>,
    request: ActorWriteRequest,
) -> tokio::task::JoinHandle<Result<McpWriteResult, nail_api::ApiError>> {
    let writes = writes.clone();
    tokio::spawn(async move { writes.run(request).await })
}

/// Denies `pending` in Nail; the waiting write ends denied.
async fn deny(
    store: &Store,
    pending: &nail_api::agents::Confirmation,
    waiting: tokio::task::JoinHandle<Result<McpWriteResult, nail_api::ApiError>>,
) {
    store
        .decide_confirmation(&pending.id, false, ConfirmationSource::Nail, now())
        .unwrap();
    assert!(matches!(
        waiting.await.unwrap().unwrap(),
        McpWriteResult::Denied { .. }
    ));
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn args<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> T {
    serde_json::from_value(value).unwrap()
}

#[tokio::test]
async fn plugin_writes_share_the_mcp_plans_and_stay_in_their_accounts() {
    let store = Store::open_in_memory().unwrap();
    let data = Arc::new(FakeData::default());
    let writes = writer(&store, data.clone());

    // Organizing runs without asking, attributed to the plug-in.
    let done = writes
        .run(request(
            ActorWrite::UpdateMessages(args(json!({"message_ids": ["m1"], "add_tags": ["later"]}))),
            Some(&["work"]),
        ))
        .await
        .unwrap();
    assert!(matches!(done, McpWriteResult::Done { .. }), "{done:?}");
    let written = data.writes.lock().unwrap().clone();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].0, plugin());

    // A message outside the plug-in's accounts is refused before anything runs.
    let err = writes
        .run(request(
            ActorWrite::UpdateMessages(args(json!({"message_ids": ["m2"], "flagged": true}))),
            Some(&["work"]),
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden, "{err:?}");
    assert_eq!(data.writes.lock().unwrap().len(), 1);

    // A task linked to a message (the Follow-up example).
    let done = writes
        .run(request(
            ActorWrite::UpsertTask(args(
                json!({"title": "Follow up", "related_message_id": "m1"}),
            )),
            None,
        ))
        .await
        .unwrap();
    let McpWriteResult::Done { created, .. } = done else {
        panic!("done expected");
    };
    assert_eq!(created, vec!["task-new".to_owned()]);
    assert!(
        store
            .list_confirmations(ConfirmationFilter::All, None, None, 10)
            .unwrap()
            .is_empty(),
        "nothing asked"
    );
}

#[tokio::test]
async fn destructive_plugin_writes_always_ask_and_run_once_approved() {
    let store = Store::open_in_memory().unwrap();
    let data = Arc::new(FakeData::default());
    let writes = writer(&store, data.clone());

    let waiting = spawn_run(
        &writes,
        request(
            ActorWrite::Delete(PimDelete::Task {
                task_id: "t1".into(),
            }),
            None,
        ),
    );
    let pending = common::pending(&store).await;
    assert_eq!(pending.kind, ActionKind::PimDelete);
    assert_eq!(pending.operation, "nail.test");
    assert_eq!(pending.requested_by, plugin());
    assert!(
        data.writes.lock().unwrap().is_empty(),
        "nothing before the decision"
    );
    store
        .decide_confirmation(
            &pending.id,
            true,
            ConfirmationSource::Nail,
            chrono::Utc::now().timestamp(),
        )
        .unwrap();
    let result = waiting.await.unwrap().unwrap();
    assert!(
        matches!(&result, McpWriteResult::Done { confirmation_id: Some(id), .. } if *id == pending.id.as_str()),
        "{result:?}"
    );
    assert!(matches!(
        data.written().as_slice(),
        [Request::DeleteTask {
            with_subtasks: false,
            ..
        }]
    ));

    // Even an "always allow" policy cannot be stored for deletes, and a
    // denied Trash changes nothing.
    assert!(
        store
            .set_action_policy(
                &plugin().actor,
                ActionKind::Trash,
                Some(ActionPolicyMode::Allow),
                0
            )
            .is_err()
    );
    let waiting = spawn_run(
        &writes,
        request(
            ActorWrite::Delete(PimDelete::Event {
                event_id: "e1".into(),
                occurrence: None,
                scope: McpEditScope::All,
            }),
            None,
        ),
    );
    let pending = common::pending(&store).await;
    deny(&store, &pending, waiting).await;
    assert_eq!(data.written().len(), 1, "the denied delete did not run");
}

#[tokio::test]
async fn plugin_tools_are_offered_by_scope_and_answer_untrusted() {
    let data = FakeData {
        plugin_tools: vec![PluginTool {
            name: "crm__lookup".into(),
            plugin_name: "CRM".into(),
            description: "Look up a customer".into(),
            input_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
            scopes: vec![McpScope::Read],
        }],
        ..FakeData::default()
    };
    let env = start_with(data).await;
    let (id, token) = create_client(&env.store, "Agent").await;
    let transport = || {
        StreamableHttpClientTransport::with_client(
            nail_mcp::bridge::loopback_http_client().unwrap(),
            StreamableHttpClientTransportConfig::with_uri(env.server.url().to_owned())
                .auth_header(token.clone()),
        )
    };

    // Without `read` the tool is neither listed nor callable.
    let client = ClientConfig::default().serve(transport()).await.unwrap();
    let names: Vec<String> = client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(!names.iter().any(|n| n == "crm__lookup"));
    let refused = client
        .call_tool(
            CallToolRequestParams::new("crm__lookup")
                .with_arguments(serde_json::from_value(json!({"q": "x"})).unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(refused.is_error, Some(true));
    client.cancel().await.unwrap();

    grant(&env.store, &id, &[McpScope::Read], &["work"]).await;
    let client = ClientConfig::default().serve(transport()).await.unwrap();
    let tools = client.list_all_tools().await.unwrap();
    let tool = tools
        .iter()
        .find(|t| t.name == "crm__lookup")
        .expect("listed");
    assert!(tool.description.as_deref().unwrap().contains("CRM"));
    let answer = client
        .call_tool(
            CallToolRequestParams::new("crm__lookup")
                .with_arguments(serde_json::from_value(json!({"q": "ada"})).unwrap()),
        )
        .await
        .unwrap();
    let value = answer.structured_content.unwrap();
    assert_eq!(value["untrusted_content"], true, "{value}");
    assert_eq!(value["data"]["arguments"]["q"], "ada");
    assert_eq!(value["data"]["client"], "Agent");
    client.cancel().await.unwrap();

    let audit = env.store.list_mcp_audit(Some(&id), None, 10).unwrap();
    assert!(
        audit
            .iter()
            .any(|e| e.kind == McpCallKind::Tool && e.name == "crm__lookup"),
        "{audit:?}"
    );
}

/// Review M4a: a send is approved for the draft the confirmation showed;
/// a draft changed after the request is not sent.
#[tokio::test]
async fn a_draft_changed_after_the_send_request_is_not_sent() {
    let store = Store::open_in_memory().unwrap();
    let data = Arc::new(FakeData::default());
    let writes = writer(&store, data.clone());
    let send = || {
        let writes = writes.clone();
        tokio::spawn(async move {
            writes
                .run(request(
                    ActorWrite::SendMessage(args(json!({"draft_id": "d1"}))),
                    None,
                ))
                .await
        })
    };
    let waiting = send();
    let pending = common::pending(&store).await;
    assert_eq!(pending.kind, ActionKind::Send);
    // The draft is edited (another recipient) while the user looks at it.
    data.changed_drafts.lock().unwrap().push("d1".into());
    store
        .decide_confirmation(&pending.id, true, ConfirmationSource::Nail, now())
        .unwrap();
    let err = waiting.await.unwrap().unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict, "{err:?}");
    assert!(data.written().is_empty(), "nothing was sent");
    let ended = store.get_confirmation(&pending.id).unwrap();
    assert_eq!(ended.state, ConfirmationState::Failed);
    assert_eq!(ended.error.unwrap().id, "mcp-tool-error-draft-changed");

    // Unchanged, the approved draft goes out.
    data.changed_drafts.lock().unwrap().clear();
    let waiting = send();
    let pending = common::pending(&store).await;
    store
        .decide_confirmation(&pending.id, true, ConfirmationSource::Nail, now())
        .unwrap();
    assert!(matches!(
        waiting.await.unwrap().unwrap(),
        McpWriteResult::Done { .. }
    ));
    // The send is pinned to the approved revision: the core checks it in
    // the transaction that queues the draft (post-M4a, ADR 0086).
    let approved = {
        use nail_mcp::McpData as _;
        nail_store::draft_revision(&data.draft(&"d1".into()).await.unwrap())
    };
    assert!(matches!(
        data.written().as_slice(),
        [Request::SendDraft { expected_revision: Some(r), .. }] if *r == approved
    ));
}

/// Review M4a: moving into Trash is a delete — it always asks; arrival
/// hooks (never a delete) are refused.
#[tokio::test]
async fn moves_to_trash_ask_and_arrival_actions_never_delete() {
    let store = Store::open_in_memory().unwrap();
    let data = Arc::new(FakeData::default());
    let writes = writer(&store, data.clone());
    // Even "always allow" for organizing does not cover a move to Trash.
    store
        .set_action_policy(
            &plugin().actor,
            ActionKind::Organize,
            Some(ActionPolicyMode::Allow),
            0,
        )
        .unwrap();
    let to_trash = || {
        ActorWrite::UpdateMessages(args(
            json!({"message_ids": ["m1"], "move_to_folder_id": "work-trash"}),
        ))
    };
    let waiting = spawn_run(&writes, request(to_trash(), None));
    let pending = common::pending(&store).await;
    assert_eq!(pending.kind, ActionKind::Trash);
    assert_eq!(pending.summary.id, "confirmation-summary-trash");
    assert!(!pending.can_always_allow);
    deny(&store, &pending, waiting).await;
    assert_eq!(data.written().len(), 0);

    let mut arrival = request(to_trash(), Some(&["work"]));
    arrival.operation = "hooks.arrival".into();
    arrival.allow_destructive = false;
    let err = writes.run(arrival).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden, "{err:?}");
    assert_eq!(data.written().len(), 0);
    assert_eq!(
        store
            .list_confirmations(ConfirmationFilter::Pending, None, None, 10)
            .unwrap()
            .len(),
        0
    );
    // Other folders stay organizing.
    let done = writes
        .run(request(
            ActorWrite::UpdateMessages(args(
                json!({"message_ids": ["m1"], "move_to_folder_id": "work-hr"}),
            )),
            None,
        ))
        .await
        .unwrap();
    assert!(matches!(done, McpWriteResult::Done { .. }));
}

/// Review M4a: a plug-in serving an MCP client writes within the overlap
/// of both grants, asks when either policy asks, counts against the
/// client's limits, and every plug-in write is in the MCP audit log.
#[tokio::test]
async fn plugins_acting_for_a_client_stay_within_its_grant() {
    let store = Store::open_in_memory().unwrap();
    let data = Arc::new(FakeData::default());
    let writes = writer(&store, data.clone());
    let (client_id, _) = create_client(&store, "Agent").await;
    grant(
        &store,
        &client_id,
        &[McpScope::Read, McpScope::Organize],
        &["work"],
    )
    .await;
    // The plug-in may send without asking; the client may not.
    store
        .set_action_policy(
            &plugin().actor,
            ActionKind::Send,
            Some(ActionPolicyMode::Allow),
            0,
        )
        .unwrap();
    let for_client = |write: ActorWrite| {
        let mut r = request(write, None);
        r.by.on_behalf_of = Some(McpClientRef {
            client_id: client_id.clone(),
            name: "Agent".into(),
        });
        r
    };
    let flag =
        |id: &str| ActorWrite::UpdateMessages(args(json!({"message_ids": [id], "flagged": true})));
    let send = || ActorWrite::SendMessage(args(json!({"draft_id": "d1"})));

    // Another account than the client's: refused, although the plug-in
    // reaches every account.
    let err = writes.run(for_client(flag("m2"))).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden, "{err:?}");
    // Sending is not in the client's grant.
    let err = writes.run(for_client(send())).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden, "{err:?}");
    assert_eq!(data.written().len(), 0);

    // Within both grants it runs, attributed to the plug-in for the client.
    assert!(matches!(
        writes.run(for_client(flag("m1"))).await.unwrap(),
        McpWriteResult::Done { .. }
    ));
    let by = data.writes.lock().unwrap()[0].0.clone();
    assert_eq!(by.on_behalf_of.unwrap().client_id, client_id);

    // With `send`, the client's policy (ask) wins over the plug-in's (allow).
    grant(
        &store,
        &client_id,
        &[McpScope::Read, McpScope::Organize, McpScope::Send],
        &["work"],
    )
    .await;
    let waiting = spawn_run(&writes, for_client(send()));
    let pending = common::pending(&store).await;
    assert_eq!(pending.kind, ActionKind::Send);
    assert_eq!(
        pending
            .requested_by
            .on_behalf_of
            .as_ref()
            .unwrap()
            .client_id,
        client_id
    );
    deny(&store, &pending, waiting).await;

    // The client's writes_per_minute counts the plug-in's writes for it.
    store
        .set_mcp_rate_limits(
            &client_id,
            &McpRateLimits {
                requests_per_minute: None,
                writes_per_minute: Some(2),
                sends_per_hour: None,
            },
        )
        .unwrap();
    let err = writes.run(for_client(flag("m1"))).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::RateLimited, "{err:?}");
    // The plug-in's own writes are not the client's.
    assert!(matches!(
        writes.run(request(flag("m1"), None)).await.unwrap(),
        McpWriteResult::Done { .. }
    ));

    // The MCP audit log has every write (the client's view: those for it).
    let audit = store.list_mcp_audit(Some(&client_id), None, 50).unwrap();
    let names: Vec<(&str, McpOutcome)> = audit
        .iter()
        .rev()
        .map(|e| (e.name.as_str(), e.outcome))
        .collect();
    assert_eq!(
        names,
        [
            ("plugin:follow-up nail.test", McpOutcome::Denied),
            ("plugin:follow-up nail.test", McpOutcome::Denied),
            ("plugin:follow-up nail.test", McpOutcome::Allowed),
            ("plugin:follow-up nail.test", McpOutcome::Declined),
            ("plugin:follow-up nail.test", McpOutcome::Denied),
        ]
    );
    assert!(audit.iter().all(|e| e.client_name == "Follow up"));
    let all = store.list_mcp_audit(None, None, 50).unwrap();
    assert_eq!(all.len(), 6, "the plug-in's own write too: {all:?}");
    assert!(all[0].client_id.is_none());
    assert!(
        all[0]
            .detail
            .as_deref()
            .unwrap()
            .starts_with(nail_mcp::messages::WRITE_DETAIL_PREFIX),
        "{all:?}"
    );

    // A revoked client reaches nothing through the plug-in.
    store.revoke_mcp_client(&client_id, now()).unwrap();
    let err = writes.run(for_client(flag("m1"))).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden, "{err:?}");
}
