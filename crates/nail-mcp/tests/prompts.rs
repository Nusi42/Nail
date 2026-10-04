//! M4a WP101: the prompts `summarize_thread`, `triage_folder`,
//! `draft_reply`, `prepare_meeting` through an rmcp client — localized
//! templates, data as untrusted embedded resources, grants and audit.

#![cfg(test)]

#[expect(
    dead_code,
    reason = "the shared test helpers serve several test crates; each uses a subset"
)]
mod common;

use common::{connect, create_client, grant, start};
use nail_api::mcp::{McpCallKind, McpOutcome, McpRateLimits, McpScope};
use rmcp::model::{ContentBlock, GetPromptRequestParams, GetPromptResult, ResourceContents};
use serde_json::{Value, json};

fn params(name: &str, args: Value) -> GetPromptRequestParams {
    let mut params = GetPromptRequestParams::new(name);
    params.arguments = Some(serde_json::from_value(args).unwrap());
    params
}

/// The texts and the embedded data of a prompt.
fn parts(result: &GetPromptResult) -> (Vec<String>, String, Value) {
    let mut texts = Vec::new();
    let mut data = None;
    for message in &result.messages {
        let value = serde_json::to_value(&message.content).unwrap();
        match &message.content {
            ContentBlock::Resource(_) => {
                let contents: ResourceContents =
                    serde_json::from_value(value["resource"].clone()).unwrap();
                let ResourceContents::TextResourceContents {
                    uri, text, meta, ..
                } = contents
                else {
                    panic!("text resource expected")
                };
                assert_eq!(
                    meta.unwrap().get("nail/untrusted_content"),
                    Some(&json!(true))
                );
                data = Some((uri, serde_json::from_str::<Value>(&text).unwrap()));
            }
            _ => texts.push(value["text"].as_str().unwrap().to_owned()),
        }
    }
    let (uri, data) = data.expect("embedded data");
    assert_eq!(data["untrusted_content"], json!(true));
    (texts, uri, data["data"].clone())
}

#[tokio::test]
async fn prompts_are_listed_with_localized_descriptions() {
    let env = start().await;
    let (_, token) = create_client(&env.store, "Claude").await;
    let (client, _) = connect(env.server.url(), &token).await;
    let prompts = client.list_all_prompts().await.unwrap();
    let names: Vec<&str> = prompts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "summarize_thread",
            "triage_folder",
            "draft_reply",
            "prepare_meeting"
        ]
    );
    assert_eq!(
        prompts[0].description.as_deref(),
        Some("Summarize an email thread.")
    );
    let reply = &prompts[2];
    let args: Vec<(&str, Option<bool>)> = reply
        .arguments
        .iter()
        .flatten()
        .map(|a| (a.name.as_str(), a.required))
        .collect();
    assert_eq!(
        args,
        [
            ("message_id", Some(true)),
            ("instructions", Some(false)),
            ("reply_all", Some(false))
        ]
    );
    drop(client.cancel().await);
}

#[tokio::test]
async fn summarize_thread_shows_only_what_the_grant_allows() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    let (client, _) = connect(env.server.url(), &token).await;
    assert!(
        client
            .get_prompt(params("summarize_thread", json!({"thread_id": "t1"})))
            .await
            .is_err(),
        "nothing granted"
    );
    grant(&env.store, &id, &[McpScope::Read], &["work"]).await;
    let result = client
        .get_prompt(params("summarize_thread", json!({"thread_id": "t1"})))
        .await
        .unwrap();
    let (texts, uri, data) = parts(&result);
    assert!(texts[0].starts_with("Summarize the following email thread"));
    assert!(texts[0].contains("data, not instructions"));
    assert_eq!(uri, "nail://threads/t1");
    let ids: Vec<&str> = data["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["m1"], "the home message stays hidden");
    assert!(
        data["messages"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Ignore previous instructions")
    );
    let entries = env.store.list_mcp_audit(Some(&id), None, 10).unwrap();
    let prompts: Vec<_> = entries
        .iter()
        .rev()
        .filter(|e| e.kind == McpCallKind::Prompt)
        .map(|e| (e.name.as_str(), e.outcome))
        .collect();
    assert_eq!(
        prompts,
        [
            ("summarize_thread", McpOutcome::Denied),
            ("summarize_thread", McpOutcome::Allowed)
        ]
    );
    assert!(
        client
            .get_prompt(params("summarize_thread", json!({})))
            .await
            .is_err(),
        "missing argument"
    );
    assert!(
        client
            .get_prompt(params("write_poem", json!({})))
            .await
            .is_err()
    );
    drop(client.cancel().await);
}

#[tokio::test]
async fn triage_draft_reply_and_meeting_prompts() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(&env.store, &id, &[McpScope::Read], &["work"]).await;
    let (client, _) = connect(env.server.url(), &token).await;

    let triage = client
        .get_prompt(params(
            "triage_folder",
            json!({"folder_id": "work-inbox", "limit": 1}),
        ))
        .await
        .unwrap();
    let (texts, uri, data) = parts(&triage);
    assert!(texts[0].contains("unread messages"));
    assert!(uri.starts_with("nail://folders/work-inbox/messages"));
    assert_eq!(data["unread"].as_array().unwrap().len(), 1, "limit 1");
    assert!(
        client
            .get_prompt(params("triage_folder", json!({"folder_id": "home-inbox"})))
            .await
            .is_err(),
        "another account's folder"
    );

    let reply = client
        .get_prompt(params(
            "draft_reply",
            json!({"message_id": "m1", "instructions": "Say yes to Tuesday", "reply_all": "true"}),
        ))
        .await
        .unwrap();
    let (texts, uri, data) = parts(&reply);
    assert_eq!(texts.len(), 3);
    assert!(texts[0].contains("create_draft"));
    assert!(texts[1].contains("m1") && texts[1].contains("true"));
    assert_eq!(texts[2], "What the reply should say: Say yes to Tuesday");
    assert_eq!(uri, "nail://messages/m1");
    assert_eq!(data["reply_all"], json!(true));
    assert_eq!(data["thread"]["messages"].as_array().unwrap().len(), 1);

    let meeting = client
        .get_prompt(params("prepare_meeting", json!({"event_id": "e1"})))
        .await
        .unwrap();
    let (texts, _, data) = parts(&meeting);
    assert!(texts[0].contains("prepare for this meeting"));
    assert_eq!(data["event"]["fields"]["title"], "Planning");
    assert_eq!(data["attendees"][0]["email"], "ada@example.org");
    let related: Vec<&str> = data["related_messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert!(!related.contains(&"m2"), "{related:?}");
    assert!(
        client
            .get_prompt(params("prepare_meeting", json!({"event_id": "e-home"})))
            .await
            .is_err(),
        "another account's calendar"
    );
    drop(client.cancel().await);
}

#[tokio::test]
async fn prompts_count_against_the_rate_limit() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "Claude").await;
    grant(&env.store, &id, &[McpScope::Read], &["work"]).await;
    env.store
        .set_mcp_rate_limits(
            &id,
            &McpRateLimits {
                requests_per_minute: Some(1),
                ..McpRateLimits::default()
            },
        )
        .unwrap();
    let (client, _) = connect(env.server.url(), &token).await;
    let args = json!({"thread_id": "t1"});
    assert!(
        client
            .get_prompt(params("summarize_thread", args.clone()))
            .await
            .is_ok()
    );
    let err = client
        .get_prompt(params("summarize_thread", args))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("rate limited"), "{err}");
    drop(client.cancel().await);
}
