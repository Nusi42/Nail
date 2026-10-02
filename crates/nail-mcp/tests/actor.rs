//! M4a WP105: writes of actors that are not MCP clients (plug-ins) through
//! the MCP write tools' plans, and plug-in tools offered to MCP clients.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{FakeData, create_client, grant, start_with};
use nail_api::agents::{
    ActionActor, ActionAttribution, ActionKind, ActionPolicyMode, ConfirmationFilter,
    ConfirmationSource,
};
use nail_api::mcp::tools::{McpEditScope, McpWriteResult};
use nail_api::mcp::{McpCallKind, McpScope};
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
    }
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

    let waiting = tokio::spawn({
        let writes = writes.clone();
        async move {
            writes
                .run(request(
                    ActorWrite::Delete(PimDelete::Task {
                        task_id: "t1".into(),
                    }),
                    None,
                ))
                .await
        }
    });
    let pending = loop {
        let list = store
            .list_confirmations(ConfirmationFilter::Pending, None, None, 10)
            .unwrap();
        if let Some(first) = list.into_iter().next() {
            break first;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
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
    let waiting = tokio::spawn({
        let writes = writes.clone();
        async move {
            writes
                .run(request(
                    ActorWrite::Delete(PimDelete::Event {
                        event_id: "e1".into(),
                        occurrence: None,
                        scope: McpEditScope::All,
                    }),
                    None,
                ))
                .await
        }
    });
    let pending = loop {
        let list = store
            .list_confirmations(ConfirmationFilter::Pending, None, None, 10)
            .unwrap();
        if let Some(first) = list.into_iter().next() {
            break first;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    store
        .decide_confirmation(
            &pending.id,
            false,
            ConfirmationSource::Nail,
            chrono::Utc::now().timestamp(),
        )
        .unwrap();
    assert!(matches!(
        waiting.await.unwrap().unwrap(),
        McpWriteResult::Denied { .. }
    ));
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
