//! M4a WP101: the write tools through an rmcp client — scopes per grant,
//! drafts without asking, confirmations approved / denied / expired in
//! Nail (decisions written to the store, as the core does), elicitation
//! in a session and as MRTR rounds, idempotency keys, rate limits and the
//! audit log.

#![cfg(test)]

#[expect(
    dead_code,
    reason = "the shared test helpers serve several test crates; each uses a subset"
)]
mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{FakeData, create_client, grant, start, start_with_settings, summary};
use nail_api::agents::{
    ActionActor, ActionKind, ActionPolicyMode, ActionTargetKind, Confirmation, ConfirmationFilter,
    ConfirmationSource, ConfirmationState,
};
use nail_api::mcp::{McpCallKind, McpGrant, McpOutcome, McpRateLimits, McpScope};
use nail_api::{Event, Request, UiContext};
use nail_mcp::WriteSettings;
use nail_store::Store;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientConfig, ElicitRequestParams, ElicitResult,
    ElicitationAction, ElicitationCapability, ErrorData, FormElicitationCapability,
    ProtocolVersion,
};
use rmcp::service::{ClientLifecycleMode, ClientServiceExt, RequestContext, RunningService};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use serde_json::{Value, json};

fn transport(url: &str, token: &str) -> StreamableHttpClientTransport<reqwest::Client> {
    StreamableHttpClientTransport::with_client(
        nail_mcp::bridge::loopback_http_client().unwrap(),
        StreamableHttpClientTransportConfig::with_uri(url.to_owned()).auth_header(token.to_owned()),
    )
}

/// A client without elicitation (legacy session).
async fn plain(url: &str, token: &str) -> RunningService<RoleClient, ClientConfig> {
    ClientConfig::default()
        .serve(transport(url, token))
        .await
        .unwrap()
}

/// A client that answers elicitation forms the same way every time.
#[derive(Clone)]
struct Answering {
    action: ElicitationAction,
    content: Option<Value>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Answering {
    fn new(action: ElicitationAction, content: Option<Value>) -> Self {
        Self {
            action,
            content,
            asked: Arc::default(),
        }
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl ClientHandler for Answering {
    fn get_info(&self) -> ClientConfig {
        let mut config = ClientConfig::default();
        config.capabilities.elicitation =
            Some(ElicitationCapability::new().with_form(FormElicitationCapability::new()));
        config
    }

    fn create_elicitation(
        &self,
        request: ElicitRequestParams,
        _context: RequestContext<RoleClient>,
    ) -> impl Future<Output = Result<ElicitResult, ErrorData>> + Send {
        std::future::ready({
            if let ElicitRequestParams::FormElicitationParams { message, .. } = request {
                self.asked.lock().unwrap().push(message);
            }
            let mut result = ElicitResult::new(self.action.clone());
            if let Some(content) = &self.content {
                result = result.with_content(content.clone());
            }
            Ok(result)
        })
    }
}

async fn call<H: ClientHandler>(
    client: &RunningService<RoleClient, H>,
    name: &'static str,
    args: Value,
) -> CallToolResult {
    let params =
        CallToolRequestParams::new(name).with_arguments(serde_json::from_value(args).unwrap());
    client.call_tool(params).await.unwrap()
}

fn value(result: &CallToolResult) -> Value {
    result.structured_content.clone().unwrap_or(Value::Null)
}

/// The client's oldest pending confirmation, once it shows up.
async fn pending(store: &Store) -> Confirmation {
    for _ in 0..250 {
        let list = store
            .list_confirmations(ConfirmationFilter::Pending, None, None, 10)
            .unwrap();
        if let Some(first) = list.into_iter().next() {
            return first;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no pending confirmation");
}

/// Decides in Nail, as the core's `DecideConfirmation` does.
fn decide(store: &Store, confirmation: &Confirmation, approve: bool) {
    store
        .decide_confirmation(
            &confirmation.id,
            approve,
            ConfirmationSource::Nail,
            chrono::Utc::now().timestamp(),
        )
        .unwrap();
}

fn audit(store: &Store, id: &nail_api::McpClientId) -> Vec<nail_api::mcp::McpAuditEntry> {
    let mut entries = store.list_mcp_audit(Some(id), None, 100).unwrap();
    entries.reverse();
    entries
}

async fn grant_all(store: &Store, id: &nail_api::McpClientId, scopes: &[McpScope]) {
    nail_mcp::admin::handle(
        store,
        None,
        Request::UpdateMcpGrants {
            client_id: id.clone(),
            grant: McpGrant {
                scopes: scopes.to_vec(),
                all_accounts: true,
                ..McpGrant::default()
            },
        },
        |_| {},
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn drafts_run_without_asking_and_retries_do_not_double_act() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(
        &env.store,
        &id,
        &[McpScope::Read, McpScope::Draft],
        &["work"],
    )
    .await;
    let client = plain(env.server.url(), &token).await;

    let args = json!({
        "to": ["Ada <ada@example.org>"], "subject": "Plan", "body": "**Hello**",
        "attachments": [{"filename": "notes.txt", "data_base64": "aGk="}],
        "idempotency_key": "draft-1",
    });
    let first = call(&client, "create_draft", args.clone()).await;
    assert_ne!(first.is_error, Some(true), "{first:?}");
    let result = value(&first);
    assert_eq!(result["status"], "done");
    assert_eq!(result["created"], json!(["draft-new"]));
    assert!(result.get("confirmation_id").is_none(), "no confirmation");
    assert_eq!(env.data.write_names(), ["UploadAttachment", "SaveDraft"]);
    let Request::SaveDraft { draft } = &env.data.written()[1] else {
        panic!("draft expected")
    };
    assert_eq!(draft.subject, "Plan");
    assert_eq!(draft.to[0].email, "ada@example.org");
    assert_eq!(draft.attachments[0].filename, "notes.txt");
    assert_eq!(draft.attachments[0].mime, "text/plain");
    let body = draft.markdown.as_deref().unwrap();
    assert!(
        body.starts_with("**Hello**") && body.ends_with("Me"),
        "{body}"
    );
    let by = env.data.writes.lock().unwrap()[1].0.clone();
    assert_eq!(by.actor, ActionActor::mcp_client(id.clone()));
    assert_eq!(by.name, "Claude");

    // The same key: the first result, nothing runs again.
    let again = call(&client, "create_draft", args).await;
    assert_eq!(value(&again), result);
    assert_eq!(env.data.written().len(), 2);
    // The key belongs to create_draft now.
    let other = call(
        &client,
        "update_draft",
        json!({"draft_id": "d1", "subject": "x", "idempotency_key": "draft-1"}),
    )
    .await;
    assert_eq!(other.is_error, Some(true));
    assert_eq!(value(&other)["error"], "conflict");
    // A malformed key is a protocol error.
    let bad = CallToolRequestParams::new("create_draft").with_arguments(
        json!({"idempotency_key": "has space"})
            .as_object()
            .unwrap()
            .clone(),
    );
    assert!(client.call_tool(bad).await.is_err());

    let entries = audit(&env.store, &id);
    let writes: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == McpCallKind::Tool)
        .map(|e| (e.name.as_str(), e.outcome))
        .collect();
    assert_eq!(
        writes,
        [
            ("create_draft", McpOutcome::Allowed),
            ("create_draft", McpOutcome::Allowed),
            ("update_draft", McpOutcome::Error),
            ("create_draft", McpOutcome::Error),
        ]
    );
    let detail = entries[0].detail.as_deref().unwrap();
    assert!(detail.starts_with("write "), "{detail}");
    assert!(
        !detail.contains("Plan") && !detail.contains("Hello") && !detail.contains("ada@"),
        "no mailbox content in the audit log: {detail}"
    );
    let parsed: Value = serde_json::from_str(&detail["write ".len()..]).unwrap();
    assert!(
        parsed["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["field"] == "to" && c["after"] == "1")
    );
    assert_eq!(
        nail_mcp::messages::audit_detail(detail)
            .map(|m| m.id)
            .as_deref(),
        Some("mcp-tool-audit-done")
    );
    assert_eq!(
        entries[1].detail.as_deref(),
        Some(nail_mcp::messages::DETAIL_REPLAYED)
    );
    drop(client.cancel().await);
}

#[tokio::test]
async fn every_write_checks_the_grant() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    let client = plain(env.server.url(), &token).await;
    let denied = |r: &CallToolResult| {
        assert_eq!(r.is_error, Some(true), "{r:?}");
        let v = value(r);
        assert_eq!(v["error"], "denied", "{v}");
        v["detail"].as_str().unwrap().to_owned()
    };
    // Nothing granted.
    let r = call(
        &client,
        "update_messages",
        json!({"message_ids": ["m1"], "flagged": true}),
    )
    .await;
    assert_eq!(denied(&r), "scope `organize` not granted");

    grant(&env.store, &id, &[McpScope::Organize], &["work"]).await;
    // Organize does not need `read`, but the account must be granted.
    let r = call(
        &client,
        "update_messages",
        json!({"message_ids": ["m1"], "flagged": true}),
    )
    .await;
    assert_eq!(value(&r)["status"], "done");
    let r = call(
        &client,
        "update_messages",
        json!({"message_ids": ["m1", "m2"], "read": true}),
    )
    .await;
    assert_eq!(denied(&r), "account `home` not granted");
    // Permanent delete needs its own grant.
    let r = call(
        &client,
        "delete_messages",
        json!({"message_ids": ["m1"], "permanent": true}),
    )
    .await;
    assert_eq!(denied(&r), "scope `permanent-delete` not granted");
    for (tool, args, scope) in [
        ("create_draft", json!({"subject": "x"}), "draft"),
        ("send_message", json!({"draft_id": "d1"}), "send"),
        (
            "create_event",
            json!({"title": "x", "start": "2026-10-05", "end": "2026-10-06"}),
            "calendar",
        ),
        ("upsert_contact", json!({"full_name": "Ada"}), "pim"),
        ("upsert_task", json!({"title": "x"}), "pim"),
        ("create_rule", json!({"text": "rule \"x\" {}"}), "rules"),
        (
            "edit_open_draft",
            json!({"op": "insert", "text": "x"}),
            "context",
        ),
    ] {
        let r = call(&client, tool, json!(args)).await;
        assert_eq!(denied(&r), format!("scope `{scope}` not granted"), "{tool}");
    }
    // Excluded folders are excluded for writes too.
    nail_mcp::admin::handle(
        &env.store,
        None,
        Request::UpdateMcpGrants {
            client_id: id.clone(),
            grant: McpGrant {
                scopes: vec![McpScope::Organize],
                accounts: vec!["work".into()],
                excluded_folders: vec!["work-inbox".into()],
                ..McpGrant::default()
            },
        },
        |_| {},
    )
    .await
    .unwrap();
    let r = call(
        &client,
        "update_messages",
        json!({"message_ids": ["m1"], "flagged": true}),
    )
    .await;
    assert!(denied(&r).contains("work-inbox"));
    // Malformed arguments are protocol errors.
    let bad = CallToolRequestParams::new("update_messages")
        .with_arguments(json!({"message_ids": "m1"}).as_object().unwrap().clone());
    assert!(client.call_tool(bad).await.is_err());
    assert_eq!(env.data.write_names(), ["UpdateFlags"]);
    drop(client.cancel().await);
}

#[tokio::test]
async fn sending_waits_for_the_users_approval_in_nail() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(&env.store, &id, &[McpScope::Send], &["work"]).await;
    let client = Arc::new(plain(env.server.url(), &token).await);
    let waiting = {
        let client = client.clone();
        tokio::spawn(async move {
            call(
                &client,
                "send_message",
                json!({"draft_id": "d1", "idempotency_key": "s1"}),
            )
            .await
        })
    };
    let confirmation = pending(&env.store).await;
    assert_eq!(confirmation.kind, ActionKind::Send);
    assert_eq!(confirmation.operation, "send_message");
    assert_eq!(confirmation.requested_by.name, "Claude");
    assert_eq!(confirmation.summary.id, "confirmation-summary-send");
    assert_eq!(confirmation.summary.args["count"], 1u32.into());
    assert!(
        confirmation
            .targets
            .iter()
            .any(|t| t.kind == ActionTargetKind::Recipient && t.id == "bob@example.org")
    );
    assert!(confirmation.can_always_allow);
    assert!(
        env.data.written().is_empty(),
        "nothing runs before the decision"
    );
    assert!(env.data.events().iter().any(|e| matches!(
        e,
        Event::ConfirmationRequested { confirmation: c } if c.id == confirmation.id
    )));
    decide(&env.store, &confirmation, true);
    let result = tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap();
    let v = value(&result);
    assert_eq!(v["status"], "done", "{v}");
    assert_eq!(v["confirmation_id"], json!(confirmation.id.as_str()));
    assert_eq!(
        v["created"].as_array().unwrap().len(),
        1,
        "the outbox entry"
    );
    assert_eq!(env.data.write_names(), ["SendDraft"]);
    let entries = audit(&env.store, &id);
    let last = entries.last().unwrap();
    assert_eq!(
        (last.name.as_str(), last.outcome),
        ("send_message", McpOutcome::Allowed)
    );
    assert!(
        last.detail
            .as_deref()
            .unwrap()
            .contains(confirmation.id.as_str())
    );
    // The retry with the key returns the result without asking again.
    let again = call(
        &client,
        "send_message",
        json!({"draft_id": "d1", "idempotency_key": "s1"}),
    )
    .await;
    assert_eq!(value(&again), v);
    assert_eq!(env.data.write_names(), ["SendDraft"]);
}

#[tokio::test]
async fn a_denied_delete_changes_nothing_and_deletes_always_ask() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(
        &env.store,
        &id,
        &[McpScope::Organize, McpScope::PermanentDelete],
        &["work"],
    )
    .await;
    // Trash can never be set to "always allow".
    let actor = ActionActor::mcp_client(id.clone());
    assert!(
        env.store
            .set_action_policy(&actor, ActionKind::Trash, Some(ActionPolicyMode::Allow), 0)
            .is_err()
    );
    let client = Arc::new(plain(env.server.url(), &token).await);
    let waiting = {
        let client = client.clone();
        tokio::spawn(async move {
            call(&client, "delete_messages", json!({"message_ids": ["m1"]})).await
        })
    };
    let confirmation = pending(&env.store).await;
    assert_eq!(confirmation.kind, ActionKind::Trash);
    assert!(!confirmation.can_always_allow);
    assert_eq!(confirmation.targets[0].label, "Subject of m1");
    decide(&env.store, &confirmation, false);
    let result = tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    let v = value(&result);
    assert_eq!(v["status"], "denied");
    assert_eq!(v["confirmation_id"], json!(confirmation.id.as_str()));
    assert!(
        env.data.written().is_empty(),
        "a denied delete changes nothing"
    );
    let entries = audit(&env.store, &id);
    assert_eq!(entries.last().unwrap().outcome, McpOutcome::Declined);
    assert_eq!(
        nail_mcp::messages::audit_detail(entries.last().unwrap().detail.as_deref().unwrap())
            .map(|m| m.id)
            .as_deref(),
        Some("mcp-tool-audit-denied")
    );

    // Permanent delete asks even when organizing is allowed without asking.
    let waiting = {
        let client = client.clone();
        tokio::spawn(async move {
            call(
                &client,
                "delete_messages",
                json!({"message_ids": ["m1"], "permanent": true}),
            )
            .await
        })
    };
    let confirmation = pending(&env.store).await;
    assert_eq!(confirmation.kind, ActionKind::PermanentDelete);
    assert_eq!(
        confirmation.summary.id,
        "confirmation-summary-permanent-delete"
    );
    decide(&env.store, &confirmation, true);
    let result = tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(value(&result)["status"], "done");
    assert_eq!(
        env.data.written(),
        [Request::DeleteMessages {
            message_ids: vec!["m1".into()],
            permanent: true
        }]
    );
}

#[tokio::test]
async fn policies_relax_or_tighten_confirmations() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(
        &env.store,
        &id,
        &[McpScope::Send, McpScope::Organize],
        &["work"],
    )
    .await;
    let actor = ActionActor::mcp_client(id.clone());
    // "Always allow" sending: no confirmation.
    env.store
        .set_action_policy(&actor, ActionKind::Send, Some(ActionPolicyMode::Allow), 0)
        .unwrap();
    // Flags ask now.
    env.store
        .set_action_policy(&actor, ActionKind::Organize, Some(ActionPolicyMode::Ask), 0)
        .unwrap();
    let client = Arc::new(plain(env.server.url(), &token).await);
    let sent = call(&client, "send_message", json!({"draft_id": "d1"})).await;
    assert_eq!(value(&sent)["status"], "done");
    assert!(
        env.store
            .list_confirmations(ConfirmationFilter::All, None, None, 10)
            .unwrap()
            .is_empty(),
        "{:?}",
        env.store
            .list_confirmations(ConfirmationFilter::All, None, None, 10)
            .unwrap()
    );
    let waiting = {
        let client = client.clone();
        tokio::spawn(async move {
            call(
                &client,
                "update_messages",
                json!({"message_ids": ["m1"], "flagged": true}),
            )
            .await
        })
    };
    let confirmation = pending(&env.store).await;
    assert_eq!(confirmation.kind, ActionKind::Organize);
    let flagged = confirmation
        .changes
        .iter()
        .find(|c| c.field.id == "action-field-flagged")
        .unwrap();
    assert_eq!(
        (flagged.before.as_deref(), flagged.after.as_deref()),
        (Some("No"), Some("Yes"))
    );
    decide(&env.store, &confirmation, true);
    let result = tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(value(&result)["status"], "done");
    assert_eq!(env.data.write_names(), ["SendDraft", "UpdateFlags"]);
}

#[tokio::test]
async fn unanswered_confirmations_expire() {
    let env = start_with_settings(
        FakeData::default(),
        WriteSettings {
            confirmation_timeout_secs: 1,
            poll_interval: Duration::from_millis(50),
        },
    )
    .await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(&env.store, &id, &[McpScope::Send], &["work"]).await;
    let client = plain(env.server.url(), &token).await;
    let started = std::time::Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        call(&client, "send_message", json!({"draft_id": "d1"})),
    )
    .await
    .unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(result.is_error, Some(true));
    let v = value(&result);
    assert_eq!(v["status"], "expired", "{v}");
    assert!(env.data.written().is_empty(), "{:?}", env.data.written());
    let decided = env
        .store
        .list_confirmations(ConfirmationFilter::Decided, None, None, 10)
        .unwrap();
    assert_eq!(decided[0].state, ConfirmationState::Expired);
    assert!(env.data.events().iter().any(|e| matches!(
        e,
        Event::ConfirmationResolved { confirmation } if confirmation.state == ConfirmationState::Expired
    )));
    assert_eq!(
        audit(&env.store, &id).last().unwrap().outcome,
        McpOutcome::Declined
    );
}

#[tokio::test]
async fn rate_limits_trip_per_client() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(
        &env.store,
        &id,
        &[McpScope::Read, McpScope::Organize, McpScope::Send],
        &["work"],
    )
    .await;
    env.store
        .set_action_policy(
            &ActionActor::mcp_client(id.clone()),
            ActionKind::Send,
            Some(ActionPolicyMode::Allow),
            0,
        )
        .unwrap();
    env.store
        .set_mcp_rate_limits(
            &id,
            &McpRateLimits {
                requests_per_minute: Some(6),
                writes_per_minute: Some(3),
                sends_per_hour: Some(1),
            },
        )
        .unwrap();
    let client = plain(env.server.url(), &token).await;
    let flag = json!({"message_ids": ["m1"], "flagged": true});
    assert_eq!(
        value(&call(&client, "send_message", json!({"draft_id": "d1"})).await)["status"],
        "done"
    );
    // The second message this hour is over sends_per_hour.
    let r = call(&client, "send_message", json!({"draft_id": "d1"})).await;
    assert_eq!(value(&r)["error"], "rate_limited");
    assert_eq!(value(&r)["limit"], "sends_per_hour");
    // Three writes a minute: this is the third (the refused send counted).
    assert_eq!(
        value(&call(&client, "update_messages", flag.clone()).await)["status"],
        "done"
    );
    let r = call(&client, "update_messages", flag.clone()).await;
    assert_eq!(r.is_error, Some(true));
    let v = value(&r);
    assert_eq!(v["error"], "rate_limited");
    assert_eq!(v["limit"], "writes_per_minute");
    assert!(v["retry_after_secs"].as_u64().unwrap() >= 1);
    // Four calls so far; the fifth and sixth work, the seventh does not.
    for _ in 0..2 {
        assert!(
            client
                .read_resource(rmcp::model::ReadResourceRequestParams::new(
                    "nail://accounts"
                ))
                .await
                .is_ok()
        );
    }
    let err = client
        .read_resource(rmcp::model::ReadResourceRequestParams::new(
            "nail://accounts",
        ))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("rate limited"), "{err}");
    assert_eq!(env.data.write_names(), ["SendDraft", "UpdateFlags"]);
    let entries = audit(&env.store, &id);
    let denied: Vec<_> = entries
        .iter()
        .filter(|e| e.outcome == McpOutcome::Denied)
        .map(|e| e.detail.clone().unwrap())
        .collect();
    assert_eq!(denied.len(), 3, "{denied:?}");
    assert!(denied.iter().all(|d| d.starts_with("rate limit `")));
    assert_eq!(
        nail_mcp::messages::audit_detail(&denied[0])
            .map(|m| m.id)
            .as_deref(),
        Some("mcp-tool-audit-rate-limited")
    );
    drop(client.cancel().await);
}

#[tokio::test]
async fn elicitation_in_a_session_approves_sends_but_never_deletes() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(
        &env.store,
        &id,
        &[McpScope::Send, McpScope::Organize],
        &["work"],
    )
    .await;
    let answering = Answering::new(ElicitationAction::Accept, Some(json!({"approve": true})));
    let client = Arc::new(
        answering
            .clone()
            .serve(transport(env.server.url(), &token))
            .await
            .unwrap(),
    );
    let sent = tokio::time::timeout(
        Duration::from_secs(10),
        call(&client, "send_message", json!({"draft_id": "d1"})),
    )
    .await
    .unwrap();
    assert_eq!(value(&sent)["status"], "done", "{sent:?}");
    let asked = answering.asked();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].contains("Claude wants to send"), "{asked:?}");
    let decided = env
        .store
        .list_confirmations(ConfirmationFilter::Decided, None, None, 10)
        .unwrap();
    assert_eq!(
        decided[0].decided_via,
        Some(ConfirmationSource::Elicitation)
    );

    // The same "approve" for a delete is not enough: Nail decides.
    let waiting = {
        let client = client.clone();
        tokio::spawn(async move {
            call(&client, "delete_messages", json!({"message_ids": ["m1"]})).await
        })
    };
    let confirmation = pending(&env.store).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        env.store.get_confirmation(&confirmation.id).unwrap().state,
        ConfirmationState::Pending,
        "elicitation cannot approve a delete"
    );
    assert!(answering.asked()[1].contains("only be approved in Nail"));
    decide(&env.store, &confirmation, true);
    let deleted = tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(value(&deleted)["status"], "done");
    assert_eq!(env.data.write_names(), ["SendDraft", "DeleteMessages"]);
}

#[tokio::test]
async fn declining_the_form_denies_even_a_delete() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(&env.store, &id, &[McpScope::Organize], &["work"]).await;
    let answering = Answering::new(ElicitationAction::Decline, None);
    let client = answering
        .clone()
        .serve(transport(env.server.url(), &token))
        .await
        .unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        call(&client, "delete_messages", json!({"message_ids": ["m1"]})),
    )
    .await
    .unwrap();
    assert_eq!(value(&result)["status"], "denied");
    assert!(env.data.written().is_empty(), "{:?}", env.data.written());
    drop(client.cancel().await);
}

#[tokio::test]
async fn stateless_clients_answer_through_mrtr_rounds() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Modern").await;
    grant(&env.store, &id, &[McpScope::Send], &["work"]).await;
    let answering = Answering::new(ElicitationAction::Accept, Some(json!({"approve": true})));
    let client = answering
        .clone()
        .serve_with_lifecycle(
            transport(env.server.url(), &token),
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        call(&client, "send_message", json!({"draft_id": "d1"})),
    )
    .await
    .unwrap();
    assert_eq!(value(&result)["status"], "done", "{result:?}");
    assert_eq!(answering.asked().len(), 1);
    assert_eq!(env.data.write_names(), ["SendDraft"]);
    // A forged request state is refused.
    let mut forged = CallToolRequestParams::new("send_message")
        .with_arguments(json!({"draft_id": "d1"}).as_object().unwrap().clone());
    forged.request_state = Some("not-a-state".into());
    assert!(client.call_tool_once(forged).await.is_err());
    drop(client.cancel().await);
}

#[tokio::test]
async fn a_vetoed_send_reports_the_reasons() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(&env.store, &id, &[McpScope::Send], &["work"]).await;
    env.store
        .set_action_policy(
            &ActionActor::mcp_client(id.clone()),
            ActionKind::Send,
            Some(ActionPolicyMode::Allow),
            0,
        )
        .unwrap();
    let client = plain(env.server.url(), &token).await;
    let result = call(&client, "send_message", json!({"draft_id": "draft-veto"})).await;
    let v = value(&result);
    assert_eq!(v["status"], "vetoed");
    assert_eq!(
        v["reasons"],
        json!(["Attachment checker: The text mentions an attachment"])
    );
    let empty = call(&client, "send_message", json!({"draft_id": "draft-empty"})).await;
    assert_eq!(value(&empty)["error"], "invalid_request");
    drop(client.cancel().await);
}

#[tokio::test]
async fn mail_organizing_validates_and_queues() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(&env.store, &id, &[McpScope::Organize], &["work", "home"]).await;
    let client = plain(env.server.url(), &token).await;
    let r = call(
        &client,
        "update_messages",
        json!({"message_ids": ["m1", "m3", "m1"], "read": true, "add_tags": ["work"], "move_to_folder_id": "work-hr"}),
    )
    .await;
    assert_eq!(value(&r)["status"], "done", "{r:?}");
    let written = env.data.written();
    assert_eq!(written.len(), 2);
    let Request::UpdateFlags {
        message_ids,
        change,
    } = &written[0]
    else {
        panic!()
    };
    assert_eq!(message_ids.len(), 2, "duplicates removed");
    assert_eq!(
        (change.seen, change.add_tags.clone()),
        (Some(true), vec!["work".to_owned()])
    );
    assert!(matches!(&written[1], Request::MoveMessages { .. }));
    for (args, needle) in [
        (json!({"message_ids": ["m1"]}), "nothing to change"),
        (json!({"message_ids": [], "flagged": true}), "empty"),
        (
            json!({"message_ids": ["m2"], "move_to_folder_id": "work-hr"}),
            "account",
        ),
        (
            json!({"message_ids": ["m1"], "archive": true, "snooze_until": "2030-01-01T00:00:00Z"}),
            "choose one",
        ),
        (
            json!({"message_ids": ["m1"], "snooze_until": "2020-01-01T00:00:00Z"}),
            "past",
        ),
        (
            json!({"message_ids": ["m1"], "add_tags": ["two words"]}),
            "tag",
        ),
    ] {
        let r = call(&client, "update_messages", args).await;
        assert_eq!(r.is_error, Some(true));
        let detail = value(&r)["detail"].as_str().unwrap().to_owned();
        assert!(detail.contains(needle), "{detail}");
    }
    let r = call(
        &client,
        "update_messages",
        json!({"message_ids": ["m1"], "archive": true}),
    )
    .await;
    assert_eq!(value(&r)["status"], "done");
    let r = call(
        &client,
        "update_messages",
        json!({"message_ids": ["m1"], "snooze_until": "2099-01-01T08:00:00Z"}),
    )
    .await;
    assert_eq!(value(&r)["status"], "done");
    assert_eq!(
        env.data.write_names(),
        [
            "UpdateFlags",
            "MoveMessages",
            "MoveMessages",
            "SnoozeMessages"
        ]
    );
    drop(client.cancel().await);
}

#[tokio::test]
async fn drafts_reply_update_and_edit_the_open_one() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(
        &env.store,
        &id,
        &[McpScope::Draft, McpScope::Context],
        &["work"],
    )
    .await;
    let client = plain(env.server.url(), &token).await;
    // Replying quotes the original: it needs `read`.
    let r = call(
        &client,
        "create_draft",
        json!({"reply_to_message_id": "m1", "body": "Yes"}),
    )
    .await;
    assert_eq!(value(&r)["detail"], "scope `read` not granted");
    grant(
        &env.store,
        &id,
        &[McpScope::Read, McpScope::Draft, McpScope::Context],
        &["work"],
    )
    .await;
    let r = call(
        &client,
        "create_draft",
        json!({"reply_to_message_id": "m1", "body": "Yes", "format": "text"}),
    )
    .await;
    assert_eq!(value(&r)["status"], "done", "{r:?}");
    let Request::SaveDraft { draft } = env.data.written().pop().unwrap() else {
        panic!()
    };
    assert_eq!(draft.subject, "Re: Subject of m1");
    assert_eq!(draft.context.as_ref().unwrap().message_id.as_str(), "m1");
    assert!(draft.text.starts_with("Yes\n\n"));
    // Replying to mail of another account is refused.
    let r = call(
        &client,
        "create_draft",
        json!({"reply_to_message_id": "m2"}),
    )
    .await;
    assert_eq!(value(&r)["error"], "denied");
    // Forwarding m1 carries its attachment: that needs the attachments
    // grant too.
    let r = call(&client, "create_draft", json!({"forward_message_id": "m1"})).await;
    assert_eq!(value(&r)["detail"], "scope `attachments` not granted");
    // Attachments from a message need the attachments grant.
    let r = call(
        &client,
        "create_draft",
        json!({"attachments": [{"filename": "plan.csv", "from_message_id": "m1", "from_attachment_index": 0}]}),
    )
    .await;
    assert_eq!(value(&r)["detail"], "scope `attachments` not granted");

    let r = call(
        &client,
        "update_draft",
        json!({"draft_id": "d1", "subject": "New", "body": "<p>Hi</p>", "format": "html"}),
    )
    .await;
    assert_eq!(value(&r)["status"], "done", "{r:?}");
    let Request::SaveDraft { draft } = env.data.written().pop().unwrap() else {
        panic!()
    };
    assert_eq!(draft.id.as_ref().unwrap().as_str(), "d1");
    assert_eq!(draft.html.as_deref(), Some("<p>Hi</p>"));
    let r = call(
        &client,
        "update_draft",
        json!({"draft_id": "draft-home", "subject": "x"}),
    )
    .await;
    assert_eq!(value(&r)["error"], "denied");

    // The open draft.
    let r = call(
        &client,
        "edit_open_draft",
        json!({"op": "insert", "text": "Hello"}),
    )
    .await;
    assert_eq!(value(&r)["error"], "not_found");
    env.data.set_context(UiContext {
        draft_id: Some("d1".into()),
        ..UiContext::default()
    });
    let r = call(
        &client,
        "edit_open_draft",
        json!({"op": "insert", "text": "Hello", "draft_id": "other"}),
    )
    .await;
    assert_eq!(value(&r)["error"], "conflict");
    let r = call(
        &client,
        "edit_open_draft",
        json!({"op": "replace-selection", "text": "Hello"}),
    )
    .await;
    assert_eq!(value(&r)["status"], "done", "{r:?}");
    let events = env.data.events();
    assert!(events.iter().any(|e| matches!(
        e,
        Event::ComposerEditRequested { draft_id, client_id: None, edit, .. }
            if draft_id.as_str() == "d1" && edit.text == "Hello"
    )));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::McpContextAccess { edit: true, .. }))
    );
    drop(client.cancel().await);
}

#[tokio::test]
async fn calendar_contact_and_task_writes() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(
        &env.store,
        &id,
        &[McpScope::Calendar, McpScope::Pim],
        &["work"],
    )
    .await;
    let client = Arc::new(plain(env.server.url(), &token).await);
    let r = call(
        &client,
        "create_event",
        json!({"title": "Review", "start": "2026-10-07T09:00:00Z", "end": "2026-10-07T10:00:00Z",
               "tz": "Europe/Berlin", "attendees": ["ada@example.org"], "recurrence": "FREQ=WEEKLY;COUNT=3"}),
    )
    .await;
    assert_eq!(value(&r)["status"], "done", "{r:?}");
    assert_eq!(value(&r)["created"], json!(["e-new"]));
    let Request::CreateEvent {
        calendar_id,
        fields,
        recurrence,
    } = env.data.written().pop().unwrap()
    else {
        panic!()
    };
    assert_eq!(
        calendar_id.as_str(),
        "cal-work",
        "the first writable calendar"
    );
    assert_eq!(
        fields.start,
        nail_api::pim::PimTime::zoned("2026-10-07T11:00:00", "Europe/Berlin")
    );
    assert_eq!(recurrence.unwrap().rules.len(), 1);
    // Invitations need `send`.
    let r = call(
        &client,
        "create_event",
        json!({"title": "Review", "start": "2026-10-07", "end": "2026-10-08",
               "attendees": ["ada@example.org"], "send_invitations": true}),
    )
    .await;
    assert_eq!(value(&r)["detail"], "scope `send` not granted");
    for (args, error) in [
        (
            json!({"title": "x", "start": "2026-10-07", "end": "2026-10-08", "calendar_id": "cal-ro"}),
            "forbidden",
        ),
        (
            json!({"title": "x", "start": "2026-10-07", "end": "2026-10-08", "calendar_id": "cal-home"}),
            "not_found",
        ),
        (
            json!({"title": "x", "start": "2026-10-08", "end": "2026-10-07"}),
            "invalid_request",
        ),
        (
            json!({"title": "x", "start": "2026-10-07T09:00:00", "end": "2026-10-07T10:00:00"}),
            "invalid_request",
        ),
    ] {
        let r = call(&client, "create_event", args.clone()).await;
        assert_eq!(value(&r)["error"], error, "{args}");
    }
    // Updates; another account's event is not found.
    let r = call(
        &client,
        "update_event",
        json!({"event_id": "e-home", "title": "x"}),
    )
    .await;
    assert_eq!(value(&r)["error"], "not_found");
    let r = call(
        &client,
        "update_event",
        json!({"event_id": "e1", "start": "2026-10-05T10:00:00", "end": "2026-10-05T11:00:00", "add_attendees": ["Bob <bob@example.org>"]}),
    )
    .await;
    assert_eq!(value(&r)["status"], "done", "{r:?}");
    let Request::UpdateEvent { fields, .. } = env.data.written().pop().unwrap() else {
        panic!()
    };
    assert_eq!(
        fields.start,
        nail_api::pim::PimTime::zoned("2026-10-05T10:00:00", "Europe/Berlin"),
        "local times use the event's zone"
    );
    assert_eq!(fields.attendees.len(), 2);
    let r = call(
        &client,
        "update_event",
        json!({"event_id": "e1", "scope": "this"}),
    )
    .await;
    assert_eq!(value(&r)["error"], "invalid_request");

    // Answering without mail is a calendar write; telling the organizer
    // needs `send`.
    let r = call(
        &client,
        "respond_to_invite",
        json!({"event_id": "e1", "response": "accept", "send": false}),
    )
    .await;
    assert_eq!(value(&r)["status"], "done", "{r:?}");
    let r = call(
        &client,
        "respond_to_invite",
        json!({"message_id": "m1", "response": "decline"}),
    )
    .await;
    assert_eq!(value(&r)["detail"], "scope `send` not granted");

    // Contacts and tasks.
    let r = call(
        &client,
        "upsert_contact",
        json!({"full_name": "Grace", "emails": ["grace@example.org"]}),
    )
    .await;
    assert_eq!(value(&r)["created"], json!(["c-new"]), "{r:?}");
    let r = call(
        &client,
        "upsert_contact",
        json!({"contact_id": "c1", "phones": ["+49 30 1"]}),
    )
    .await;
    assert_eq!(value(&r)["status"], "done");
    let Request::UpdateContact { fields, .. } = env.data.written().pop().unwrap() else {
        panic!()
    };
    assert_eq!(fields.display_name, "Ada Lovelace");
    assert_eq!(fields.phones[0].value, "+49 30 1");
    let r = call(&client, "upsert_contact", json!({"contact_id": "c1"})).await;
    assert_eq!(value(&r)["detail"], "nothing to change");
    let r = call(
        &client,
        "upsert_task",
        json!({"title": "Call Ada", "due": "2026-10-09", "priority": 1, "related_message_id": "m1"}),
    )
    .await;
    assert_eq!(value(&r)["created"], json!(["task-new"]), "{r:?}");
    let Request::CreateTask {
        calendar_id,
        fields,
        ..
    } = env.data.written().pop().unwrap()
    else {
        panic!()
    };
    assert_eq!(calendar_id.as_str(), "tasks-work");
    assert_eq!(fields.links.len(), 1);
    assert_eq!(fields.due, Some(nail_api::pim::PimTime::date("2026-10-09")));
    let r = call(
        &client,
        "upsert_task",
        json!({"task_id": "t1", "completed": true}),
    )
    .await;
    assert_eq!(value(&r)["status"], "done");
    assert!(matches!(
        env.data.written().pop().unwrap(),
        Request::CompleteTask {
            completed: true,
            ..
        }
    ));
    let r = call(
        &client,
        "upsert_task",
        json!({"related_message_id": "m2", "title": "x"}),
    )
    .await;
    assert_eq!(value(&r)["error"], "denied");
    let r = call(
        &client,
        "upsert_task",
        json!({"title": "x", "priority": 12}),
    )
    .await;
    assert_eq!(value(&r)["error"], "invalid_request");
}

#[tokio::test]
async fn invitations_ask_and_then_mail_the_attendees() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(
        &env.store,
        &id,
        &[McpScope::Calendar, McpScope::Send],
        &["work"],
    )
    .await;
    let client = Arc::new(plain(env.server.url(), &token).await);
    let waiting = {
        let client = client.clone();
        tokio::spawn(async move {
            call(
                &client,
                "create_event",
                json!({"title": "Review", "start": "2026-10-07", "end": "2026-10-08",
                       "attendees": ["ada@example.org", "bob@example.org"], "send_invitations": true}),
            )
            .await
        })
    };
    let confirmation = pending(&env.store).await;
    assert_eq!(confirmation.kind, ActionKind::Invite);
    assert_eq!(confirmation.summary.id, "confirmation-summary-invite");
    assert_eq!(confirmation.summary.args["count"], 2u32.into());
    decide(&env.store, &confirmation, true);
    let result = tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(value(&result)["status"], "done");
    assert_eq!(env.data.write_names(), ["CreateEvent", "SendInvitations"]);
}

#[tokio::test]
async fn rules_need_all_accounts_and_ask_first() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(
        &env.store,
        &id,
        &[McpScope::Read, McpScope::Rules],
        &["work"],
    )
    .await;
    let client = Arc::new(plain(env.server.url(), &token).await);
    let r = call(
        &client,
        "test_rule",
        json!({"text": "rule \"Receipts\" {}"}),
    )
    .await;
    assert_eq!(value(&r)["detail"], "needs a grant for all accounts");
    grant_all(&env.store, &id, &[McpScope::Read, McpScope::Rules]).await;
    let r = call(
        &client,
        "test_rule",
        json!({"text": "rule \"Receipts\" {}"}),
    )
    .await;
    assert_ne!(r.is_error, Some(true), "{r:?}");
    let v = value(&r);
    assert_eq!(v["untrusted_content"], json!(true));
    let ids: Vec<&str> = v["data"]["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["message"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["m1", "m2"], "encrypted m3 needs the encrypted grant");
    assert!(env.data.written().is_empty(), "test_rule changes nothing");
    let r = call(&client, "create_rule", json!({"text": "rule broken"})).await;
    assert_eq!(value(&r)["error"], "invalid_request");
    assert!(value(&r)["detail"].as_str().unwrap().contains("line 1"));

    let waiting = {
        let client = client.clone();
        tokio::spawn(async move {
            call(
                &client,
                "create_rule",
                json!({"text": "rule \"Receipts\" {}", "enabled": false}),
            )
            .await
        })
    };
    let confirmation = pending(&env.store).await;
    assert_eq!(confirmation.kind, ActionKind::Rule);
    assert_eq!(confirmation.summary.args["name"], "Receipts".into());
    decide(&env.store, &confirmation, true);
    let result = tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(value(&result)["created"], json!(["rule-new"]));
    let Request::SaveRule { rule, .. } = env.data.written().pop().unwrap() else {
        panic!()
    };
    assert!(
        rule.id.as_str().is_empty(),
        "a new rule, never a replacement"
    );
    assert!(!rule.enabled);
}

/// Review M4a: an idempotency key names one request. Reusing it for other
/// arguments (or another tool) while the first waits is refused, so an
/// approval of what the user saw never runs something else.
#[tokio::test]
async fn a_reused_key_never_attaches_another_request_to_a_confirmation() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(
        &env.store,
        &id,
        &[McpScope::Organize, McpScope::PermanentDelete],
        &["work"],
    )
    .await;
    let client = Arc::new(plain(env.server.url(), &token).await);
    let waiting = {
        let client = client.clone();
        tokio::spawn(async move {
            call(
                &client,
                "delete_messages",
                json!({"message_ids": ["m1"], "idempotency_key": "k1"}),
            )
            .await
        })
    };
    let confirmation = pending(&env.store).await;
    assert_eq!(confirmation.kind, ActionKind::Trash);
    // Same tool and key, other arguments: refused, nothing new is asked.
    let other = call(
        &client,
        "delete_messages",
        json!({"message_ids": ["m3"], "permanent": true, "idempotency_key": "k1"}),
    )
    .await;
    assert_eq!(other.is_error, Some(true));
    assert_eq!(
        value(&other)["error"],
        "invalid_request",
        "{:?}",
        value(&other)
    );
    let still = env
        .store
        .list_confirmations(ConfirmationFilter::Pending, None, None, 10)
        .unwrap();
    assert_eq!(still.len(), 1);
    assert_eq!(still[0].id, confirmation.id);
    // Another tool under the key: refused too.
    let other = call(
        &client,
        "update_messages",
        json!({"message_ids": ["m1"], "flagged": true, "idempotency_key": "k1"}),
    )
    .await;
    assert_eq!(other.is_error, Some(true));
    // The approval runs exactly what was shown.
    decide(&env.store, &confirmation, true);
    let result = tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(value(&result)["status"], "done");
    assert_eq!(
        env.data.written(),
        [Request::DeleteMessages {
            message_ids: vec!["m1".into()],
            permanent: false
        }]
    );
}

/// Review M4a: a pending confirmation nothing in this server waits for
/// (left from before a restart) is cancelled instead of joined.
#[tokio::test]
async fn an_orphaned_confirmation_under_a_key_is_cancelled() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(&env.store, &id, &[McpScope::Organize], &["work"]).await;
    let client = plain(env.server.url(), &token).await;
    // What `delete_messages` with key k2 stores, left behind by an earlier
    // run of the server.
    let args = json!({"message_ids": ["m1"], "idempotency_key": "k2"});
    let payload = json!({
        "action": "requests",
        "requests": [{"type": "DeleteMessages", "message_ids": ["m1"], "permanent": false}],
    });
    let orphan = env
        .store
        .insert_confirmation(
            nail_api::agents::NewConfirmation {
                requested_by: nail_api::agents::ActionAttribution {
                    actor: ActionActor::mcp_client(id.clone()),
                    name: "Claude".into(),
                    on_behalf_of: None,
                },
                kind: ActionKind::Trash,
                operation: "delete_messages".into(),
                summary: nail_api::LocalizedMessage::new("confirmation-summary-trash"),
                targets: vec![],
                changes: vec![],
                account_ids: vec!["work".into()],
                payload,
                idempotency_key: Some("k2".into()),
                timeout_secs: 300,
            },
            chrono::Utc::now().timestamp(),
            20,
        )
        .unwrap();
    let result = call(&client, "delete_messages", args).await;
    assert_eq!(value(&result)["status"], "expired", "{:?}", value(&result));
    let ended = env
        .store
        .get_confirmation(&orphan.confirmation().id)
        .unwrap();
    assert_eq!(ended.state, ConfirmationState::Cancelled);
    assert!(env.data.written().is_empty(), "{:?}", env.data.written());
}

/// Review M4a: moving into Trash with `update_messages` is a delete: it
/// asks even when organizing is allowed without asking.
#[tokio::test]
async fn moving_to_trash_asks_like_a_delete() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(&env.store, &id, &[McpScope::Organize], &["work"]).await;
    env.store
        .set_action_policy(
            &ActionActor::mcp_client(id.clone()),
            ActionKind::Organize,
            Some(ActionPolicyMode::Allow),
            0,
        )
        .unwrap();
    let client = Arc::new(plain(env.server.url(), &token).await);
    let waiting = {
        let client = client.clone();
        tokio::spawn(async move {
            call(
                &client,
                "update_messages",
                json!({"message_ids": ["m1"], "move_to_folder_id": "work-trash"}),
            )
            .await
        })
    };
    let confirmation = pending(&env.store).await;
    assert_eq!(confirmation.kind, ActionKind::Trash);
    assert!(!confirmation.can_always_allow);
    decide(&env.store, &confirmation, false);
    let result = tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(value(&result)["status"], "denied");
    assert!(env.data.written().is_empty(), "{:?}", env.data.written());
}

/// P3 (ADR 0164): conversations resolve per action in the core and run as
/// one mail-action batch on the resolved messages the grant covers; the
/// confirmation names the conversation and message counts; deletes ask.
#[tokio::test]
async fn conversations_act_within_the_grant_and_confirm_with_counts() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(&env.store, &id, &[McpScope::Organize], &["work"]).await;
    let client = Arc::new(plain(env.server.url(), &token).await);
    // Thread t1 = m1 (work) + m2 (home): m2 is outside the grant and left
    // out, not refused.
    let r = call(
        &client,
        "update_messages",
        json!({"conversations": [{"thread_id": "t1"}], "read": true, "flagged": true}),
    )
    .await;
    assert_eq!(value(&r)["status"], "done", "{r:?}");
    let written = env.data.written();
    assert_eq!(written.len(), 2, "{written:?}");
    for (request, expected) in written.iter().zip([
        nail_api::MailAction::MarkRead { read: true },
        nail_api::MailAction::Flag { flagged: true },
    ]) {
        let Request::ApplyMailAction { targets, action } = request else {
            panic!("{request:?}");
        };
        assert_eq!(*action, expected);
        assert_eq!(*targets, vec![nail_api::MessageTarget::message("m1")]);
    }
    // A conversation listed in an excluded folder is refused.
    nail_mcp::admin::handle(
        &env.store,
        None,
        Request::UpdateMcpGrants {
            client_id: id.clone(),
            grant: McpGrant {
                scopes: vec![McpScope::Organize],
                accounts: vec!["work".into()],
                excluded_folders: vec!["work-hr".into()],
                ..McpGrant::default()
            },
        },
        |_| {},
    )
    .await
    .unwrap();
    let r = call(
        &client,
        "update_messages",
        json!({"conversations": [{"thread_id": "t1", "folder_id": "work-hr"}], "read": true}),
    )
    .await;
    assert_eq!(r.is_error, Some(true));
    assert_eq!(value(&r)["error"], "denied");
    // Nothing of the conversation is granted: nothing to change.
    let r = call(
        &client,
        "update_messages",
        json!({"conversations": [{"thread_id": "t-none"}], "read": true}),
    )
    .await;
    assert_eq!(r.is_error, Some(true), "{r:?}");

    // Delete to Trash always asks; the summary counts the messages.
    let waiting = {
        let client = client.clone();
        tokio::spawn(async move {
            call(
                &client,
                "delete_messages",
                json!({"conversations": [{"thread_id": "t1", "folder_id": "work-inbox"}]}),
            )
            .await
        })
    };
    let confirmation = pending(&env.store).await;
    assert_eq!(confirmation.kind, ActionKind::Trash);
    assert_eq!(confirmation.summary.id, "mail-action-confirmation-trash");
    let arg = |name: &str| serde_json::to_value(&confirmation.summary.args).unwrap()[name].clone();
    assert_eq!(arg("count"), json!(1));
    assert_eq!(arg("conversations"), json!(1));
    assert_eq!(confirmation.targets.len(), 1);
    decide(&env.store, &confirmation, true);
    let result = tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(value(&result)["status"], "done", "{result:?}");
    let Some(Request::ApplyMailAction { targets, action }) = env.data.written().pop() else {
        panic!("no mail action");
    };
    assert_eq!(action, nail_api::MailAction::Trash);
    assert_eq!(targets, vec![nail_api::MessageTarget::message("m1")]);
    // More than the limit is refused before anything is resolved.
    let many: Vec<Value> = (0..501)
        .map(|i| json!({"thread_id": format!("t{i}")}))
        .collect();
    let r = call(
        &client,
        "update_messages",
        json!({"conversations": many, "read": true}),
    )
    .await;
    assert_eq!(r.is_error, Some(true));
}

/// P3 review: the confirmation counts only conversations with a message
/// the grant covers (a thread wholly outside the grant is neither counted
/// nor hinted at).
#[tokio::test]
async fn conversation_counts_leave_out_conversations_outside_the_grant() {
    let mut data = FakeData::default();
    data.messages
        .push(summary("m9", "home", "home-inbox", "t9"));
    let env = start_with_settings(
        data,
        WriteSettings {
            poll_interval: Duration::from_millis(50),
            ..WriteSettings::default()
        },
    )
    .await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(&env.store, &id, &[McpScope::Organize], &["work"]).await;
    let client = Arc::new(plain(env.server.url(), &token).await);
    let waiting = {
        let client = client.clone();
        tokio::spawn(async move {
            call(
                &client,
                "delete_messages",
                json!({"conversations": [{"thread_id": "t9"}, {"thread_id": "t3"}]}),
            )
            .await
        })
    };
    let confirmation = pending(&env.store).await;
    let arg = |name: &str| serde_json::to_value(&confirmation.summary.args).unwrap()[name].clone();
    assert_eq!(arg("conversations"), json!(1));
    assert_eq!(arg("count"), json!(1));
    decide(&env.store, &confirmation, false);
    drop(tokio::time::timeout(Duration::from_secs(10), waiting).await);
}

/// P3 review: "to Trash" on a message already in Trash would delete it for
/// good, so it is refused without `permanent`.
#[tokio::test]
async fn trash_of_a_message_in_trash_needs_permanent() {
    let mut data = FakeData::default();
    data.messages
        .push(summary("m8", "work", "work-trash", "t8"));
    let env = start_with_settings(
        data,
        WriteSettings {
            poll_interval: Duration::from_millis(50),
            ..WriteSettings::default()
        },
    )
    .await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(&env.store, &id, &[McpScope::Organize], &["work"]).await;
    let client = plain(env.server.url(), &token).await;
    let r = call(&client, "delete_messages", json!({"message_ids": ["m8"]})).await;
    assert_eq!(r.is_error, Some(true), "{r:?}");
    let written = env.data.written();
    assert!(written.is_empty(), "{written:?}");
}
