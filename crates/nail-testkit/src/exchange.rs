//! Checks the Exchange engine tests share: both mock tenants (Graph and
//! EWS) hold the same demo inbox rules, and both engines must treat them
//! alike.

use nail_api::engine::{DeltaOutcome, EngineError};
use nail_api::exchange::engine::ExchangeSettingsEngine;
use nail_api::exchange::rules::ExchangeInboxRule;

/// The demo tenant's inbox rules: "Newsletters" (expressible) and a second
/// rule with conditions Nail does not model.
///
/// # Panics
///
/// When the engine fails or the rules differ.
pub async fn demo_inbox_rules(engine: &dyn ExchangeSettingsEngine) -> Vec<ExchangeInboxRule> {
    let rules = engine
        .inbox_rules()
        .await
        .unwrap_or_else(|e| panic!("inbox rules: {e:?}"));
    assert_eq!(rules.len(), 2, "{rules:?}");
    assert_eq!(rules[0].name, "Newsletters");
    assert!(!rules[0].has_unsupported);
    assert!(rules[1].has_unsupported);
    rules
}

/// Deletes the rule `id` twice (the second time a no-op) and checks that
/// the demo rules are left.
///
/// # Panics
///
/// When a delete fails or other rules are left.
pub async fn delete_inbox_rule_twice(engine: &dyn ExchangeSettingsEngine, id: &str) {
    for _ in 0..2 {
        engine
            .delete_inbox_rule(id)
            .await
            .unwrap_or_else(|e| panic!("delete rule {id}: {e:?}"));
    }
    demo_inbox_rules(engine).await;
}

/// The first inbox delta of the demo mailbox: messages whose thread id is
/// their Exchange conversation id (`all_threaded`: every message has one).
///
/// # Panics
///
/// When the delta failed, is empty, or a thread id differs.
pub fn assert_conversation_threads(outcome: Result<DeltaOutcome, EngineError>, all_threaded: bool) {
    let Ok(DeltaOutcome::Delta(delta)) = outcome else {
        panic!("delta: {outcome:?}");
    };
    assert!(!delta.added.is_empty(), "{:?}", delta.added);
    for m in &delta.added {
        let conversation = m.exchange.as_ref().and_then(|e| e.conversation_id.clone());
        assert!(!all_threaded || conversation.is_some(), "{m:?}");
        assert_eq!(
            m.thread_id, conversation,
            "the conversation id is the thread id"
        );
    }
    assert!(delta.added.iter().any(|m| m.thread_id.is_some()));
}
