//! M4a mail hooks through a running host: arrival (filters, account grants,
//! actions through the core), before send (vetoes, headers, content), open
//! (banners, badges, banner buttons), per-hook budgets, failures that never
//! block, and the statistics the core records.

#![cfg(test)]

#[expect(
    dead_code,
    reason = "the shared test helpers serve several test crates; each uses a subset"
)]
mod common;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use common::*;
use nail_api::agents::ActionKind;
use nail_api::plugins::hooks::PluginContentPosition;
use nail_api::plugins::{
    PluginHookKind, PluginHookOutcome, PluginMailAction, PluginOutgoingMessage, PluginRunState,
    PluginStopReason, PluginUiTarget,
};
use nail_api::{Address, ErrorCode, Event};
use nail_plugins::{AccountScope, PluginWrite};
use serde_json::{Value, json};

fn hooks(defs: Value) -> Value {
    Value::Object(serde_json::Map::from_iter([("hooks".to_owned(), defs)]))
}

const TAGGER: &str = r#"
  export default { activate(ctx) {
    ctx.hooks.register("arrival", ({ message, body }) => {
      nail.log.info("SAW " + message.id + " " + (body ? "body" : "nobody"));
      return { actions: [
        { type: "addTag", tag: "seen" },
        { type: "setFlagged", flagged: true },
        { type: "move", folderId: "archive-2026" },
      ] };
    });
  } };
"#;

#[tokio::test(flavor = "multi_thread")]
async fn arrival_hooks_tag_mail_through_the_core() {
    let fake = Fake::new();
    let host = host(&fake);
    host.start(spec_with(
        "tagger",
        &["mail.read", "mail.organize", "hooks.arrival"],
        hooks(json!([{ "hook": "arrival" }])),
        TAGGER,
    ))
    .await
    .unwrap();
    let report = host.arrival_hooks(&"m1".into()).await;
    assert_eq!(report.runs.len(), 1);
    assert_eq!(report.runs[0].outcome, PluginHookOutcome::Ok);
    assert_eq!(report.runs[0].plugin_name, "Test tagger");
    assert_eq!(report.actions.len(), 1);
    assert_eq!(report.actions[0].1.len(), 3);
    assert_eq!(result(&fake, "tagger", "SAW ").await, "m1 nobody");
    let write = wait_for("write", || fake.writes().into_iter().next()).await;
    assert_eq!(write.kind, ActionKind::Organize);
    assert_eq!(write.operation, "hooks.arrival");
    assert_eq!(write.scope, AccountScope::Only(vec!["work".into()]));
    let PluginWrite::UpdateMessages(args) = &write.write else {
        panic!()
    };
    assert_eq!(args.message_ids, ["m1"]);
    assert_eq!(args.add_tags, ["seen"]);
    assert_eq!(args.flagged, Some(true));
    assert_eq!(args.move_to_folder_id.as_deref(), Some("archive-2026"));
    let calls = wait_for("stats", || {
        let calls = fake.hook_calls("tagger");
        (!calls.is_empty()).then_some(calls)
    })
    .await;
    assert_eq!(calls[0].hook, PluginHookKind::Arrival);
    assert_eq!(calls[0].outcome, PluginHookOutcome::Ok);
}

#[tokio::test(flavor = "multi_thread")]
async fn arrival_hooks_follow_filters_grants_and_body_permissions() {
    let fake = Fake::new();
    let host = host(&fake);
    // Only subjects containing "m1"; bodies readable.
    host.start(spec_with(
        "filtered",
        &["mail.read", "mail.read.body", "hooks.arrival"],
        hooks(json!([{ "hook": "arrival", "filter": "subject:m1" }])),
        TAGGER,
    ))
    .await
    .unwrap();
    // Granted for `home` only.
    let mut home = spec_with(
        "home-only",
        &["mail.read", "hooks.arrival"],
        hooks(json!([{ "hook": "arrival" }])),
        TAGGER,
    );
    home.grants = vec![
        grant("mail.read", None),
        grant("hooks.arrival", Some(&["home"])),
    ];
    host.start(home).await.unwrap();
    // A filter the core cannot evaluate: skipped.
    host.start(spec_with(
        "broken-filter",
        &["mail.read", "hooks.arrival"],
        hooks(json!([{ "hook": "arrival", "filter": "fail" }])),
        TAGGER,
    ))
    .await
    .unwrap();
    // Declares the hook but the user did not grant it.
    let mut ungranted = spec_with(
        "ungranted",
        &["mail.read", "hooks.arrival"],
        hooks(json!([{ "hook": "arrival" }])),
        TAGGER,
    );
    ungranted.grants = vec![grant("mail.read", None)];
    host.start(ungranted).await.unwrap();

    let m1 = host.arrival_hooks(&"m1".into()).await;
    let ran: Vec<&str> = m1.runs.iter().map(|r| r.plugin_id.as_str()).collect();
    assert_eq!(ran, ["filtered"]);
    assert_eq!(result(&fake, "filtered", "SAW ").await, "m1 body");
    // Without mail.organize the actions are dropped (and said so).
    assert!(m1.actions.is_empty());
    assert_eq!(
        fake.localized("filtered", "plugin-host-hook-actions-denied")
            .len(),
        1
    );

    let m2 = host.arrival_hooks(&"m2".into()).await;
    let ran: Vec<&str> = m2.runs.iter().map(|r| r.plugin_id.as_str()).collect();
    assert_eq!(ran, ["home-only"]);
    // Unknown messages run nothing.
    assert!(host.arrival_hooks(&"nope".into()).await.runs.is_empty());
    assert!(fake.writes().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn failing_hooks_are_skipped_counted_and_never_stop_the_plugin() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      let n = 0;
      export default { activate(ctx) {
        ctx.hooks.register("arrival", async ({ message }) => {
          n++;
          if (message.id === "m1") throw new Error("boom " + n);
          return { actions: [{ type: "delete" }] };
        });
      } };
    "#;
    host.start(spec_with(
        "thrower",
        &["mail.read", "mail.organize", "hooks.arrival"],
        hooks(json!([{ "hook": "arrival" }])),
        js,
    ))
    .await
    .unwrap();
    let report = host.arrival_hooks(&"m1".into()).await;
    assert_eq!(report.runs[0].outcome, PluginHookOutcome::Failed);
    assert!(report.runs[0].error.as_deref().unwrap().contains("boom 1"));
    // An invalid answer (no delete action exists) is a failure too.
    let report = host.arrival_hooks(&"m2".into()).await;
    assert_eq!(report.runs[0].outcome, PluginHookOutcome::Failed);
    assert!(report.actions.is_empty());
    assert_eq!(
        host.state(&plugin_id("thrower")),
        Some(PluginRunState::Running)
    );
    let calls = wait_for("stats", || {
        let calls = fake.hook_calls("thrower");
        (calls.len() == 2).then_some(calls)
    })
    .await;
    assert!(
        calls
            .iter()
            .all(|c| c.outcome == PluginHookOutcome::Failed && c.error.is_some())
    );
    // One log line a minute per plug-in and hook.
    assert_eq!(
        fake.localized("thrower", "plugin-host-hook-failed").len(),
        1
    );
    assert!(fake.writes().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn slow_hooks_time_out_by_their_own_budget() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export default { activate(ctx) {
        ctx.hooks.register("arrival", () => new Promise((r) => setTimeout(() => r(null), 3000)));
        ctx.hooks.register("open", () => ({ badges: [{ text: "late" }] }));
      } };
    "#;
    host.start(spec_with(
        "slow",
        &["mail.read", "hooks.arrival", "hooks.open"],
        hooks(json!([{ "hook": "arrival", "timeoutMs": 150 }, { "hook": "open" }])),
        js,
    ))
    .await
    .unwrap();
    let started = Instant::now();
    let report = host.arrival_hooks(&"m1".into()).await;
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(report.runs[0].outcome, PluginHookOutcome::TimedOut);
    assert!(report.runs[0].elapsed >= Duration::from_millis(150));
    let line = &fake.localized("slow", "plugin-host-hook-timed-out")[0];
    assert_eq!(
        line.localized.as_ref().unwrap().args["ms"].to_string(),
        "150"
    );
    // The plug-in keeps serving its other hooks.
    let open = host.open_hooks(&"m1".into(), None).await;
    assert_eq!(open.badges.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_runaway_hook_is_stopped_and_the_mail_still_arrives() {
    let fake = Fake::new();
    let host = host(&fake);
    host.start(spec_with(
        "runaway",
        &["mail.read", "hooks.arrival"],
        hooks(json!([{ "hook": "arrival", "timeoutMs": 200 }])),
        r#"export default { activate(ctx) { ctx.hooks.register("arrival", () => { for (;;) {} }); } };"#,
    ))
    .await
    .unwrap();
    host.start(spec_with(
        "steady",
        &["mail.read", "mail.organize", "hooks.arrival"],
        hooks(json!([{ "hook": "arrival" }])),
        TAGGER,
    ))
    .await
    .unwrap();
    let started = Instant::now();
    let report = host.arrival_hooks(&"m1".into()).await;
    assert!(started.elapsed() < Duration::from_secs(2));
    let outcomes: Vec<_> = report
        .runs
        .iter()
        .map(|r| (r.plugin_id.as_str(), r.outcome))
        .collect();
    assert_eq!(
        outcomes,
        [
            ("runaway", PluginHookOutcome::TimedOut),
            ("steady", PluginHookOutcome::Ok)
        ]
    );
    // The per-turn CPU budget stops the looping plug-in.
    wait_for("cpu stop", || {
        stop_reasons(&fake, "runaway")
            .contains(&PluginStopReason::CpuBudget)
            .then_some(())
    })
    .await;
    assert_eq!(
        host.state(&plugin_id("runaway")),
        Some(PluginRunState::Crashed)
    );
    // Later mail skips it.
    let next = host.arrival_hooks(&"m2".into()).await;
    let ran: Vec<&str> = next.runs.iter().map(|r| r.plugin_id.as_str()).collect();
    assert_eq!(ran, ["steady"]);
}

fn outgoing(account: &str) -> PluginOutgoingMessage {
    PluginOutgoingMessage {
        draft_id: "d1".into(),
        account_id: account.into(),
        identity_id: "ada@example.com".into(),
        from: Address {
            name: None,
            email: "ada@example.com".into(),
        },
        to: vec![Address {
            name: None,
            email: "bob@example.com".into(),
        }],
        cc: vec![],
        bcc: vec![],
        subject: "See attached".into(),
        text: "Please see the attached file.".into(),
        html: None,
        attachments: vec![],
        in_reply_to: None,
        encrypted: false,
    }
}

/// Review M4a: before-send hooks never see the content of a message that
/// will be end-to-end encrypted.
#[tokio::test(flavor = "multi_thread")]
async fn before_send_hooks_never_see_encrypted_content() {
    let fake = Fake::new();
    let host = host(&fake);
    let spy = r#"
      export default { activate(ctx) {
        ctx.hooks.register("beforeSend", (m) => {
          nail.log.info("SEEN " + JSON.stringify({ text: m.text, html: m.html ?? null,
                                                  subject: m.subject, encrypted: m.encrypted }));
          return null;
        });
      } };
    "#;
    host.start(spec_with(
        "spy",
        &["hooks.beforeSend"],
        hooks(json!([{ "hook": "beforeSend" }])),
        spy,
    ))
    .await
    .unwrap();
    let mut message = outgoing("work");
    message.html = Some("<p>Secret plan</p>".into());
    message.text = "Secret plan".into();
    message.encrypted = true;
    let report = host.before_send_hooks(&message).await;
    assert!(!report.vetoed());
    let seen = json_result(&fake, "spy", "SEEN ").await;
    assert_eq!(
        seen,
        json!({"text": "", "html": null, "subject": "See attached", "encrypted": true})
    );
    assert_eq!(
        nail_plugins::hooks::before_send_view(&message).text,
        "",
        "the view is redacted"
    );
    message.encrypted = false;
    assert_eq!(
        nail_plugins::hooks::before_send_view(&message).text,
        "Secret plan"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn before_send_hooks_veto_and_add_headers_and_content() {
    let fake = Fake::new();
    let host = host(&fake);
    let checker = r#"
      export default { activate(ctx) {
        ctx.hooks.register("beforeSend", (m) =>
          /attach/i.test(m.text) && m.attachments.length === 0
            ? { veto: { reason: "The message mentions an attachment but has none." } }
            : null);
      } };
    "#;
    let stamper = r#"
      export default { activate(ctx) {
        ctx.hooks.register("beforeSend", (m) => ({
          headers: [{ name: "X-Crm-Ticket", value: "4711" }],
          addContent: [{ position: "top", text: "Ticket 4711",
                         html: "<p>Ticket <b>4711</b><script>alert(1)</script></p>" }],
        }));
      } };
    "#;
    let bad = r#"
      export default { activate(ctx) {
        ctx.hooks.register("beforeSend", () => ({ headers: [{ name: "Bcc", value: "evil@example.com" }] }));
      } };
    "#;
    for (id, js) in [("checker", checker), ("stamper", stamper), ("bad", bad)] {
        host.start(spec_with(
            id,
            &["hooks.beforeSend"],
            hooks(json!([{ "hook": "beforeSend" }])),
            js,
        ))
        .await
        .unwrap();
    }
    // Granted for `home` only: not asked for `work` mail.
    let mut home = spec_with(
        "home-checker",
        &["hooks.beforeSend"],
        hooks(json!([{ "hook": "beforeSend" }])),
        checker,
    );
    home.grants = vec![grant("hooks.beforeSend", Some(&["home"]))];
    host.start(home).await.unwrap();

    let report = host.before_send_hooks(&outgoing("work")).await;
    assert!(report.vetoed());
    assert_eq!(report.vetoes.len(), 1);
    assert_eq!(report.vetoes[0].plugin_id.as_str(), "checker");
    assert_eq!(report.vetoes[0].plugin_name, "Test checker");
    assert!(report.vetoes[0].reason.contains("attachment"));
    assert_eq!(report.headers.len(), 1);
    assert_eq!(report.headers[0].1.name, "X-Crm-Ticket");
    assert_eq!(report.additions.len(), 1);
    let addition = &report.additions[0].1;
    assert_eq!(addition.position, PluginContentPosition::Top);
    let html = addition.html.as_deref().unwrap();
    assert!(
        html.contains("<b>4711</b>") && !html.contains("script"),
        "{html}"
    );
    let outcomes: BTreeMap<&str, PluginHookOutcome> = report
        .runs
        .iter()
        .map(|r| (r.plugin_id.as_str(), r.outcome))
        .collect();
    assert_eq!(
        outcomes,
        BTreeMap::from([
            ("bad", PluginHookOutcome::Failed),
            ("checker", PluginHookOutcome::Vetoed),
            ("stamper", PluginHookOutcome::Ok),
        ])
    );
    wait_for("veto stats", || {
        fake.hook_calls("checker")
            .iter()
            .any(|c| c.outcome == PluginHookOutcome::Vetoed && c.hook == PluginHookKind::BeforeSend)
            .then_some(())
    })
    .await;

    let mut with_file = outgoing("home");
    with_file.attachments = vec![nail_api::plugins::PluginAttachment {
        name: "a.pdf".into(),
        mime_type: "application/pdf".into(),
        size: 3,
    }];
    let report = host.before_send_hooks(&with_file).await;
    assert!(!report.vetoed());
    let ran: Vec<&str> = report.runs.iter().map(|r| r.plugin_id.as_str()).collect();
    assert_eq!(ran, ["bad", "checker", "home-checker", "stamper"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn open_hooks_show_banners_and_badges_and_get_their_buttons() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export default { activate(ctx) {
        ctx.hooks.register("open", ({ message }) => {
          nail.ui.notify("opened " + message.id);
          return {
            banners: [{ id: "customer", level: "warning", text: "Key customer",
                        buttons: [{ id: "open-crm", label: "Open in CRM" }] }],
            badges: [{ text: "VIP", icon: "star", tooltip: "Since 2019" }],
          };
        });
        ctx.hooks.onBannerAction((a) => nail.log.info("BANNER " + JSON.stringify(a)));
      } };
    "#;
    host.start(spec_with(
        "crm",
        &["mail.read", "hooks.open", "notifications"],
        hooks(json!([{ "hook": "open" }])),
        js,
    ))
    .await
    .unwrap();
    host.start(spec_with(
        "badopen",
        &["mail.read", "hooks.open"],
        hooks(json!([{ "hook": "open" }])),
        r#"export default { activate(ctx) { ctx.hooks.register("open", () => ({ banners: [{ id: "Bad Id", text: "x" }] })); } };"#,
    ))
    .await
    .unwrap();
    let report = host.open_hooks(&"m1".into(), Some("ui-7".into())).await;
    assert_eq!(report.banners.len(), 1);
    assert_eq!(report.banners[0].plugin_name, "Test crm");
    assert_eq!(report.banners[0].banner.id, "customer");
    assert_eq!(report.badges[0].badge.text, "VIP");
    let outcomes: Vec<_> = report.runs.iter().map(|r| r.outcome).collect();
    assert_eq!(outcomes, [PluginHookOutcome::Failed, PluginHookOutcome::Ok]);
    // What the hook raises goes to the UI that opened the message.
    let note = wait_for("notification", || {
        fake.events().into_iter().find_map(|e| match e {
            Event::PluginNotification { notification } => Some(notification),
            _ => None,
        })
    })
    .await;
    assert_eq!(note.client_id.as_deref(), Some("ui-7"));

    let target = |banner: &str| PluginUiTarget::Banner {
        message_id: "m1".into(),
        banner: banner.into(),
    };
    host.ui_action(&target("crm.customer"), "open-crm", BTreeMap::new(), None)
        .await
        .unwrap();
    let action = json_result(&fake, "crm", "BANNER ").await;
    assert_eq!(
        action,
        json!({"messageId": "m1", "bannerId": "customer", "buttonId": "open-crm"})
    );
    // Only buttons of banners the plug-in showed with that message.
    for (banner, button, message) in [
        ("crm.customer", "delete-all", "m1"),
        ("crm.other", "open-crm", "m1"),
        ("crm.customer", "open-crm", "m2"),
        ("nonsense", "x", "m1"),
    ] {
        let err = host
            .ui_action(
                &PluginUiTarget::Banner {
                    message_id: message.into(),
                    banner: banner.into(),
                },
                button,
                BTreeMap::new(),
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound, "{banner} {button} {message}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn hooks_without_a_registered_handler_fail_quietly() {
    let fake = Fake::new();
    let host = host(&fake);
    host.start(spec_with(
        "lazy",
        &["mail.read", "hooks.open"],
        hooks(json!([{ "hook": "open" }])),
        "export default { activate() {} };",
    ))
    .await
    .unwrap();
    let report = host.open_hooks(&"m1".into(), None).await;
    assert_eq!(report.runs[0].outcome, PluginHookOutcome::Failed);
    assert!(report.banners.is_empty());
    // Registering an undeclared hook is refused.
    host.start(spec(
        "undeclared",
        &[],
        r#"export default { activate(ctx) {
             try { ctx.hooks.register("open", () => null); } catch (e) { nail.log.info("REG " + e.code); }
           } };"#,
    ))
    .await
    .unwrap();
    assert_eq!(result(&fake, "undeclared", "REG ").await, "invalid_request");
}

#[test]
fn hook_actions_are_the_contract_types() {
    // The arrival result type the host parses is the contract's.
    let action: PluginMailAction = serde_json::from_value(json!({"type": "archive"})).unwrap();
    assert_eq!(action, PluginMailAction::Archive);
}
