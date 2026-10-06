//! The `nail.*` API through a running host: commands, dialogs,
//! notifications, panels, mail reads, storage, HTTP, settings, logs, and
//! the permission gate in front of them.

#![cfg(test)]

#[expect(
    dead_code,
    reason = "the shared test helpers serve several test crates; each uses a subset"
)]
mod common;

use std::collections::BTreeMap;

use common::*;
use nail_api::plugins::{
    PluginCommandSurface, PluginLogLevel, PluginLogSource, PluginRunState, PluginStopReason,
    PluginUiNode, PluginUiTarget,
};
use nail_api::{ErrorCode, Event, UiContext};
use serde_json::{Value, json};

fn ui(selected: &[&str], open: Option<&str>) -> UiContext {
    UiContext {
        account_id: Some("work".into()),
        folder_id: Some("inbox".into()),
        message_id: open.map(Into::into),
        thread_id: open.map(|_| "t1".into()),
        selected_message_ids: selected.iter().map(|s| (*s).into()).collect(),
        ..UiContext::default()
    }
}

const RUN_LOGS_CONTEXT: &str = r#"
export default {
  activate(ctx) {
    ctx.commands.register("run", (c) => nail.log.info("CTX " + JSON.stringify(c)));
  },
  deactivate() { nail.log.info("BYE"); },
};
"#;

#[tokio::test(flavor = "multi_thread")]
async fn starts_runs_commands_and_filters_the_context() {
    let fake = Fake::new();
    let host = host(&fake);
    let mut spec = spec("cmd", &["mail.read"], RUN_LOGS_CONTEXT);
    spec.grants = vec![grant("mail.read", Some(&["work"]))];
    host.start(spec).await.unwrap();
    let id = plugin_id("cmd");
    assert_eq!(host.state(&id), Some(PluginRunState::Running));
    assert!(fake.events().contains(&Event::PluginsChanged));
    assert!(fake.log_lines("cmd").iter().any(|l| {
        l.localized
            .as_ref()
            .is_some_and(|m| m.id == "plugin-host-started")
    }));

    // m2 lives in `home`, which is not granted.
    host.run_command(
        "plugin.cmd.run",
        PluginCommandSurface::List,
        &ui(&["m1", "m2", "nope"], Some("m1")),
        Some("ui-1".into()),
    )
    .await
    .unwrap();
    let ctx = json_result(&fake, "cmd", "CTX ").await;
    assert_eq!(
        ctx,
        json!({
            "surface": "list",
            "selection": {"messageIds": ["m1"], "openMessageId": "m1", "threadId": "t1"},
            "accountId": "work",
            "folderId": "inbox"
        })
    );

    // Unknown and unregistered commands.
    let err = host
        .run_command(
            "plugin.cmd.missing",
            PluginCommandSurface::Global,
            &UiContext::default(),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    let err = host
        .run_command(
            "plugin.cmd.other",
            PluginCommandSurface::Global,
            &UiContext::default(),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    let err = host
        .run_command(
            "mail.reply",
            PluginCommandSurface::Global,
            &UiContext::default(),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);

    // Disabling calls `deactivate` and refuses further commands.
    assert!(host.stop(&id, PluginStopReason::Disabled));
    assert_eq!(host.state(&id), Some(PluginRunState::Disabled));
    result(&fake, "cmd", "BYE").await;
    assert_eq!(stop_reasons(&fake, "cmd"), [PluginStopReason::Disabled]);
    let err = host
        .run_command(
            "plugin.cmd.run",
            PluginCommandSurface::List,
            &UiContext::default(),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable);
}

#[tokio::test(flavor = "multi_thread")]
async fn without_mail_read_the_context_is_empty() {
    let fake = Fake::new();
    let host = host(&fake);
    host.start(spec("blind", &[], RUN_LOGS_CONTEXT))
        .await
        .unwrap();
    host.run_command(
        "plugin.blind.run",
        PluginCommandSurface::Reader,
        &ui(&["m1"], Some("m1")),
        None,
    )
    .await
    .unwrap();
    let ctx = json_result(&fake, "blind", "CTX ").await;
    assert_eq!(
        ctx,
        json!({"surface": "reader", "selection": {"messageIds": []}})
    );
}

const DIALOG: &str = r#"
export default {
  activate(ctx) {
    ctx.commands.register("run", async () => {
      const result = await nail.ui.showDialog({
        title: "Follow up",
        body: [
          { type: "text", value: "Remind me" },
          { type: "date", id: "due", label: "Due", default: "tomorrow" },
          { type: "message-list", messageIds: ["m1", "m2", "m3"] },
          { type: "image", src: "assets/logo.png", alt: "" },
          { type: "button", id: "preview", label: "Preview" },
        ],
        buttons: [{ id: "ok", label: "Save", primary: true }, { id: "cancel", label: "Cancel" }],
      }, { onAction: (a) => nail.log.info("ACTION " + JSON.stringify(a)) });
      nail.log.info("RESULT " + JSON.stringify(result));
    });
  },
};
"#;

fn opened(fake: &Fake) -> Option<Box<nail_api::plugins::PluginDialog>> {
    fake.events().into_iter().find_map(|e| match e {
        Event::PluginDialogOpened { dialog } => Some(dialog),
        _ => None,
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn dialogs_open_act_and_resolve_with_values() {
    let fake = Fake::new();
    let host = host(&fake);
    let mut spec = spec("dlg", &["ui.dialogs", "mail.read"], DIALOG);
    spec.grants = vec![
        grant("ui.dialogs", None),
        grant("mail.read", Some(&["work"])),
    ];
    host.start(spec).await.unwrap();
    host.run_command(
        "plugin.dlg.run",
        PluginCommandSurface::Reader,
        &UiContext::default(),
        Some("ui-7".into()),
    )
    .await
    .unwrap();
    let dialog = wait_for("dialog", || opened(&fake)).await;
    assert_eq!(dialog.client_id.as_deref(), Some("ui-7"));
    assert_eq!(dialog.plugin_id.as_str(), "dlg");
    // The host filled the message list with readable messages only and
    // inlined the package image.
    let PluginUiNode::MessageList { messages, .. } = &dialog.spec.body[2] else {
        panic!("{:?}", dialog.spec.body)
    };
    let ids: Vec<&str> = messages.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["m1", "m3"]);
    let PluginUiNode::Image { src, .. } = &dialog.spec.body[3] else {
        panic!()
    };
    assert!(src.starts_with("data:image/png;base64,"));

    // A button inside the body reaches `onAction`; unknown ids do not.
    let target = PluginUiTarget::Dialog {
        dialog_id: dialog.dialog_id.clone(),
    };
    host.ui_action(
        &target,
        "preview",
        BTreeMap::from([("due".to_owned(), json!("2026-10-01"))]),
        None,
    )
    .await
    .unwrap();
    let action = json_result(&fake, "dlg", "ACTION ").await;
    assert_eq!(
        action,
        json!({"componentId": "preview", "values": {"due": "2026-10-01"}})
    );
    let err = host
        .ui_action(&target, "nope", BTreeMap::new(), None)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);

    host.respond_dialog(
        &dialog.dialog_id,
        Some("ok".into()),
        BTreeMap::from([
            ("due".to_owned(), json!("2026-10-01")),
            ("injected".to_owned(), json!("x")),
        ]),
    )
    .unwrap();
    let result = json_result(&fake, "dlg", "RESULT ").await;
    assert_eq!(
        result,
        json!({"button": "ok", "values": {"due": "2026-10-01"}})
    );
    assert!(fake.events().iter().any(|e| matches!(
        e,
        Event::PluginDialogClosed { dialog_id, .. } if dialog_id == &dialog.dialog_id
    )));
    // A dialog is answered once.
    assert_eq!(
        host.respond_dialog(&dialog.dialog_id, None, BTreeMap::new())
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dismissed_dialogs_and_unknown_buttons_resolve_with_null() {
    let fake = Fake::new();
    let host = host(&fake);
    host.start(spec("dlg2", &["ui.dialogs"], DIALOG))
        .await
        .unwrap();
    host.run_command(
        "plugin.dlg2.run",
        PluginCommandSurface::Global,
        &UiContext::default(),
        None,
    )
    .await
    .unwrap();
    let dialog = wait_for("dialog", || opened(&fake)).await;
    assert_eq!(dialog.client_id, None);
    // Without mail.read the list stays empty.
    let PluginUiNode::MessageList { messages, .. } = &dialog.spec.body[2] else {
        panic!()
    };
    assert!(messages.is_empty(), "{messages:?}");
    host.respond_dialog(&dialog.dialog_id, Some("forged".into()), BTreeMap::new())
        .unwrap();
    let result = json_result(&fake, "dlg2", "RESULT ").await;
    assert_eq!(result, json!({"button": null, "values": {}}));
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_a_plugin_closes_its_dialogs() {
    let fake = Fake::new();
    let host = host(&fake);
    host.start(spec("dlg3", &["ui.dialogs"], DIALOG))
        .await
        .unwrap();
    host.run_command(
        "plugin.dlg3.run",
        PluginCommandSurface::Global,
        &UiContext::default(),
        None,
    )
    .await
    .unwrap();
    let dialog = wait_for("dialog", || opened(&fake)).await;
    host.stop(&plugin_id("dlg3"), PluginStopReason::Disabled);
    assert!(fake.events().iter().any(|e| matches!(
        e,
        Event::PluginDialogClosed { dialog_id, .. } if dialog_id == &dialog.dialog_id
    )));
    assert_eq!(
        host.respond_dialog(&dialog.dialog_id, None, BTreeMap::new())
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_dialogs_are_refused_and_logged() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export function activate(ctx) {
        ctx.commands.register("run", async () => {
          for (const spec of [
            { title: "", body: [] },
            { title: "x", body: [{ type: "image", src: "https://evil.example/a.png", alt: "" }] },
            { title: "x", body: [{ type: "nope" }] },
            { title: "x", body: [], buttons: [{ id: "a b", label: "A" }] },
          ]) {
            try { await nail.ui.showDialog(spec); } catch (e) { nail.log.info("ERR " + e.code); }
          }
          nail.log.info("DONE");
        });
      }
    "#;
    host.start(spec("bad-dlg", &["ui.dialogs"], js))
        .await
        .unwrap();
    host.run_command(
        "plugin.bad-dlg.run",
        PluginCommandSurface::Global,
        &UiContext::default(),
        None,
    )
    .await
    .unwrap();
    result(&fake, "bad-dlg", "DONE").await;
    assert_eq!(
        fake.results("bad-dlg", "ERR "),
        [
            "invalid_request",
            "invalid_request",
            "invalid_request",
            "invalid_request"
        ]
    );
    assert!(fake.log_lines("bad-dlg").iter().any(|l| {
        l.localized
            .as_ref()
            .is_some_and(|m| m.id == "plugin-ui-issue-invalid-image")
    }));
    assert!(opened(&fake).is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn notifications_carry_the_client_and_are_limited() {
    let fake = Fake::new();
    let mut limits = limits();
    limits.notifications_per_minute = 2;
    let host = host_with(&fake, limits);
    let js = r#"
      export default { activate(ctx) {
        ctx.commands.register("run", async () => {
          await nail.ui.notify("Saved");
          await nail.ui.notify({ title: "Careful", body: "More", level: "warning", system: true });
          try { await nail.ui.notify("third"); } catch (e) { nail.log.info("ERR " + e.code); }
        });
      } };
    "#;
    host.start(spec("note", &["notifications"], js))
        .await
        .unwrap();
    host.run_command(
        "plugin.note.run",
        PluginCommandSurface::Global,
        &UiContext::default(),
        Some("c1".into()),
    )
    .await
    .unwrap();
    assert_eq!(result(&fake, "note", "ERR ").await, "rate_limited");
    let notes: Vec<_> = fake
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::PluginNotification { notification } => Some(notification),
            _ => None,
        })
        .collect();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].spec.title, "Saved");
    assert_eq!(notes[0].client_id.as_deref(), Some("c1"));
    assert!(notes[1].spec.system);
    assert_ne!(notes[0].id, notes[1].id);
}

#[tokio::test(flavor = "multi_thread")]
async fn denied_permissions_are_forbidden_and_logged() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      const calls = {
        dialog: () => nail.ui.showDialog({ title: "x", body: [] }),
        notify: () => nail.ui.notify("x"),
        storage: () => nail.storage.get("k"),
        mail: () => nail.mail.get("m1"),
        body: () => nail.mail.getBody("m1"),
        search: () => nail.mail.search("Subject"),
        thread: () => nail.mail.thread("t1"),
        fetch: () => nail.http.fetch("https://api.example.com/"),
      };
      export default { activate(ctx) {
        ctx.commands.register("run", async () => {
          const out = {};
          for (const [name, call] of Object.entries(calls)) {
            try { await call(); out[name] = "allowed"; } catch (e) { out[name] = e.code; }
          }
          nail.log.info("OUT " + JSON.stringify(out));
        });
      } };
    "#;
    // Everything declared, nothing granted.
    let mut spec = spec(
        "denied",
        &[
            "ui.dialogs",
            "notifications",
            "storage",
            "mail.read",
            "mail.read.body",
            "network:api.example.com",
        ],
        js,
    );
    spec.grants.clear();
    host.start(spec).await.unwrap();
    host.run_command(
        "plugin.denied.run",
        PluginCommandSurface::Global,
        &UiContext::default(),
        None,
    )
    .await
    .unwrap();
    let out = json_result(&fake, "denied", "OUT ").await;
    for (name, code) in map(out) {
        assert_eq!(code, json!("forbidden"), "{name}");
    }
    let denials: Vec<_> = fake
        .log_lines("denied")
        .into_iter()
        .filter(|l| l.source == PluginLogSource::Api && l.level == PluginLogLevel::Warn)
        .collect();
    assert_eq!(denials.len(), 8);
    let first = denials[0].localized.as_ref().unwrap();
    assert_eq!(first.id, "plugin-host-api-denied");
    assert_eq!(first.args["call"].to_string(), "nail.ui.showDialog");
    assert_eq!(first.args["permission"].to_string(), "ui.dialogs");
    assert!(fake.fetches.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn grants_can_change_while_running() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export default { activate(ctx) {
        ctx.commands.register("run", async () => {
          try { await nail.storage.set("k", 1); nail.log.info("R ok"); } catch (e) { nail.log.info("R " + e.code); }
        });
      } };
    "#;
    host.start(spec("regrant", &["storage"], js)).await.unwrap();
    let id = plugin_id("regrant");
    let empty = UiContext::default();
    let run = || {
        host.run_command(
            "plugin.regrant.run",
            PluginCommandSurface::Global,
            &empty,
            None,
        )
    };
    run().await.unwrap();
    wait_for("first", || {
        (fake.results("regrant", "R ").len() == 1).then_some(())
    })
    .await;
    assert!(host.update_grants(&id, &[]));
    run().await.unwrap();
    let results = wait_for("second", || {
        let r = fake.results("regrant", "R ");
        (r.len() == 2).then_some(r)
    })
    .await;
    assert_eq!(results, ["ok", "forbidden"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn mail_reads_follow_account_grants_and_never_expose_encrypted_bodies() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      async function attempt(f) { try { return await f(); } catch (e) { return "ERR " + e.code; } }
      export default { activate(ctx) {
        ctx.commands.register("run", async () => {
          const out = {
            get: (await attempt(() => nail.mail.get("m1"))).subject,
            otherAccount: await attempt(() => nail.mail.get("m2")),
            missing: await attempt(() => nail.mail.get("zzz")),
            body: (await attempt(() => nail.mail.getBody("m1"))).text,
            encrypted: await attempt(() => nail.mail.getBody("m3")),
            search: (await nail.mail.search("Subject", { limit: 9999 })).map((m) => m.id),
            thread: (await nail.mail.thread("t1")).map((m) => m.id),
          };
          nail.log.info("OUT " + JSON.stringify(out));
        });
      } };
    "#;
    let mut spec = spec("reader", &["mail.read", "mail.read.body"], js);
    spec.grants = vec![
        grant("mail.read", Some(&["work"])),
        grant("mail.read.body", None),
    ];
    host.start(spec).await.unwrap();
    host.run_command(
        "plugin.reader.run",
        PluginCommandSurface::Global,
        &UiContext::default(),
        None,
    )
    .await
    .unwrap();
    let out = json_result(&fake, "reader", "OUT ").await;
    assert_eq!(
        out,
        json!({
            "get": "Subject m1",
            "otherAccount": "ERR not_found",
            "missing": "ERR not_found",
            "body": "Body of m1",
            "encrypted": "ERR forbidden",
            "search": ["m1", "m3"],
            "thread": ["m1"],
        })
    );
    let searches = fake.searches.lock().unwrap().clone();
    assert_eq!(
        searches[0].1,
        nail_plugins::AccountScope::Only(vec!["work".into()])
    );
    // Body text only where `mail.read.body` meets `mail.read`.
    assert_eq!(
        searches[0].2,
        nail_plugins::AccountScope::Only(vec!["work".into()])
    );
    assert_eq!(searches[0].3, limits().max_search_results);
    assert!(fake.log_lines("reader").iter().any(|l| {
        l.localized.as_ref().is_some_and(|m| {
            m.id == "plugin-host-api-denied"
                && m.args["permission"].to_string() == "mail.read.encrypted"
        })
    }));
}

#[tokio::test(flavor = "multi_thread")]
async fn bodies_need_their_own_grant_and_virtual_accounts_expand() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      async function attempt(f) { try { return (await f()).id; } catch (e) { return e.code; } }
      export default { activate(ctx) {
        ctx.commands.register("run", async () => {
          nail.log.info("OUT " + JSON.stringify([
            await attempt(() => nail.mail.get("m2")),
            await attempt(() => nail.mail.getBody("m1")),
          ]));
        });
      } };
    "#;
    let mut spec = spec("headers", &["mail.read", "mail.read.body"], js);
    spec.grants = vec![grant("mail.read", Some(&["everything"]))];
    host.start(spec).await.unwrap();
    host.run_command(
        "plugin.headers.run",
        PluginCommandSurface::Global,
        &UiContext::default(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        json_result(&fake, "headers", "OUT ").await,
        json!(["m2", "forbidden"])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn storage_round_trips_within_its_quota() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export default { activate(ctx) {
        ctx.commands.register("run", async () => {
          const out = {};
          await nail.storage.set("reminder:m1", { due: "2026-10-01", note: "call" });
          await nail.storage.set("reminder:m2", 2);
          await nail.storage.set("other", true);
          out.get = await nail.storage.get("reminder:m1");
          out.missing = await nail.storage.get("nope");
          out.keys = await nail.storage.keys("reminder:");
          out.deleted = await nail.storage.delete("reminder:m2");
          out.usage = await nail.storage.usage();
          try { await nail.storage.set("big", "x".repeat(500)); } catch (e) { out.quota = e.code; }
          try { await nail.storage.set("", 1); } catch (e) { out.emptyKey = e.code; }
          try { await nail.storage.set("k".repeat(300), 1); } catch (e) { out.longKey = e.code; }
          try { await nail.storage.set("huge", "x".repeat(1024 * 1024 + 10)); } catch (e) { out.huge = e.code; }
          await nail.storage.clear();
          out.afterClear = await nail.storage.keys();
          nail.log.info("OUT " + JSON.stringify(out));
        });
      } };
    "#;
    host.start(spec("store", &["storage"], js)).await.unwrap();
    host.run_command(
        "plugin.store.run",
        PluginCommandSurface::Global,
        &UiContext::default(),
        None,
    )
    .await
    .unwrap();
    let out = json_result(&fake, "store", "OUT ").await;
    assert_eq!(out["get"], json!({"due": "2026-10-01", "note": "call"}));
    assert_eq!(out["missing"], Value::Null);
    assert_eq!(out["keys"], json!(["reminder:m1", "reminder:m2"]));
    assert_eq!(out["deleted"], json!(true));
    assert_eq!(out["usage"]["keys"], json!(2));
    assert_eq!(out["usage"]["quotaBytes"], json!(QUOTA));
    assert_eq!(out["quota"], json!("too_large"));
    assert_eq!(out["emptyKey"], json!("invalid_request"));
    assert_eq!(out["longKey"], json!("invalid_request"));
    assert_eq!(out["huge"], json!("too_large"));
    assert_eq!(out["afterClear"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn fetch_goes_through_the_core_to_declared_hosts_only() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      async function attempt(f) { try { return await f(); } catch (e) { return e.code; } }
      export default { activate(ctx) {
        ctx.commands.register("run", async () => {
          const r = await nail.http.fetch("https://api.example.com/v1/items?x=1", {
            method: "POST", headers: { "Content-Type": "application/json", Authorization: "Bearer t" },
            body: JSON.stringify({ a: 1 }),
          });
          const out = {
            status: r.status, ok: r.ok, json: await r.json(), multi: r.headers["x-multi"],
            other: await attempt(() => nail.http.fetch("https://evil.example.com/")),
            plain: await attempt(() => nail.http.fetch("http://api.example.com/")),
            port: await attempt(() => nail.http.fetch("https://api.example.com:444/")),
            hostHeader: await attempt(() => nail.http.fetch("https://api.example.com/", { headers: { Host: "evil" } })),
          };
          nail.log.info("OUT " + JSON.stringify(out));
        });
      } };
    "#;
    host.start(spec("net", &["network:api.example.com"], js))
        .await
        .unwrap();
    host.run_command(
        "plugin.net.run",
        PluginCommandSurface::Global,
        &UiContext::default(),
        None,
    )
    .await
    .unwrap();
    let out = json_result(&fake, "net", "OUT ").await;
    assert_eq!(
        out,
        json!({
            "status": 200, "ok": true, "json": {"hello": "world"}, "multi": "a, b",
            "other": "forbidden", "plain": "invalid_request", "port": "invalid_request",
            "hostHeader": "invalid_request",
        })
    );
    let fetches = fake.fetches.lock().unwrap().clone();
    assert_eq!(fetches.len(), 1);
    let request = &fetches[0];
    assert_eq!(request.method, "POST");
    assert_eq!(request.url, "https://api.example.com/v1/items?x=1");
    assert_eq!(request.body.as_deref(), Some(br#"{"a":1}"#.as_slice()));
    assert_eq!(request.allowed_hosts, ["api.example.com"]);
    assert_eq!(request.max_response_bytes, limits().http_max_response_bytes);
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_have_defaults_and_follow_updates() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export default { activate(ctx) {
        nail.log.info("START " + JSON.stringify(nail.settings.all()));
        ctx.settings.onChange((values) => nail.log.info("CHANGED " + JSON.stringify(values)));
      } };
    "#;
    let mut spec = spec("prefs", &[], js);
    spec.settings = BTreeMap::from([
        ("name".to_owned(), json!("Ada")),
        ("days".to_owned(), json!(-5)),
        ("unknown".to_owned(), json!(1)),
    ]);
    host.start(spec).await.unwrap();
    assert_eq!(
        json_result(&fake, "prefs", "START ").await,
        json!({"days": 1, "name": "Ada"})
    );
    host.update_settings(
        &plugin_id("prefs"),
        BTreeMap::from([("days".to_owned(), json!(7))]),
    );
    assert_eq!(
        json_result(&fake, "prefs", "CHANGED ").await,
        json!({"days": 7, "name": ""})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn logs_console_and_the_per_call_cap() {
    let fake = Fake::new();
    let mut limits = limits();
    limits.max_log_lines_per_call = 5;
    let host = host_with(&fake, limits);
    let js = r#"
      export default { activate(ctx) {
        console.log("hello", { a: 1 }, [2]);
        console.warn("careful");
        nail.log.error(new TypeError("bad"));
        nail.log.debug("dbg");
        ctx.commands.register("run", () => { for (let i = 0; i < 20; i++) console.log("spam " + i); });
      } };
    "#;
    host.start(spec("logger", &[], js)).await.unwrap();
    let lines = fake.log_lines("logger");
    let plugin: Vec<_> = lines
        .iter()
        .filter(|l| l.source == PluginLogSource::Plugin)
        .map(|l| (l.level, l.message.clone()))
        .collect();
    assert_eq!(
        plugin[0],
        (PluginLogLevel::Info, r#"hello {"a":1} [2]"#.to_owned())
    );
    assert_eq!(plugin[1], (PluginLogLevel::Warn, "careful".to_owned()));
    assert_eq!(plugin[2].0, PluginLogLevel::Error);
    assert!(plugin[2].1.starts_with("TypeError: bad"), "{:?}", plugin[2]);
    assert_eq!(plugin[3].0, PluginLogLevel::Debug);

    host.run_command(
        "plugin.logger.run",
        PluginCommandSurface::Global,
        &UiContext::default(),
        None,
    )
    .await
    .unwrap();
    let dropped = wait_for("dropped line", || {
        fake.log_lines("logger").into_iter().find(|l| {
            l.localized
                .as_ref()
                .is_some_and(|m| m.id == "plugin-host-log-dropped")
        })
    })
    .await;
    assert_eq!(dropped.localized.unwrap().args["count"].to_string(), "15");
    assert_eq!(fake.results("logger", "spam ").len(), 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn panels_render_update_and_act() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      let clicks = 0;
      export default { activate(ctx) {
        ctx.panels.register("side", {
          render: async (c) => [
            { type: "text", value: "Clicks: " + clicks },
            { type: "message-list", messageIds: c.selection.messageIds },
            { type: "button", id: "more", label: "More" },
          ],
          onAction: async (a) => {
            clicks++;
            await nail.ui.updatePanel("side", [{ type: "text", value: "Clicks: " + clicks }]);
            nail.log.info("ACTION " + a.componentId);
          },
        });
        try { ctx.panels.register("undeclared", () => []); } catch (e) { nail.log.info("REG " + e.code); }
        try { nail.ui.registerCommand("undeclared", () => {}); } catch (e) { nail.log.info("CMD " + e.code); }
      } };
    "#;
    host.start(spec("panel", &["mail.read"], js)).await.unwrap();
    assert_eq!(result(&fake, "panel", "REG ").await, "invalid_request");
    assert_eq!(result(&fake, "panel", "CMD ").await, "invalid_request");
    let tree = host
        .panel("panel.side", &ui(&["m1", "zzz"], None), Some("c".into()))
        .await
        .unwrap();
    assert_eq!(tree.len(), 3);
    let PluginUiNode::MessageList { messages, .. } = &tree[1] else {
        panic!()
    };
    assert_eq!(messages.len(), 1);
    host.ui_action(
        &PluginUiTarget::Panel {
            panel: "panel.side".into(),
        },
        "more",
        BTreeMap::new(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(result(&fake, "panel", "ACTION ").await, "more");
    let changed = wait_for("panel change", || {
        fake.events().into_iter().find_map(|e| match e {
            Event::PluginPanelChanged { panel, tree } => Some((panel, tree)),
            _ => None,
        })
    })
    .await;
    assert_eq!(changed.0, "panel.side");
    assert_eq!(
        changed.1,
        [PluginUiNode::Text {
            value: "Clicks: 1".into(),
            style: nail_api::plugins::ui::PluginTextStyle::default()
        }]
    );
    let err = host
        .panel("panel.missing", &UiContext::default(), None)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_panel_trees_are_refused() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export default { activate(ctx) {
        ctx.panels.register("side", () => [{ type: "progress", value: 250 }]);
      } };
    "#;
    host.start(spec("badpanel", &[], js)).await.unwrap();
    let err = host
        .panel("badpanel.side", &UiContext::default(), None)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    assert!(fake.log_lines("badpanel").iter().any(|l| {
        l.localized
            .as_ref()
            .is_some_and(|m| m.id == "plugin-ui-issue-invalid-progress")
    }));
    // A refused tree is not a crash.
    assert_eq!(
        host.state(&plugin_id("badpanel")),
        Some(PluginRunState::Running)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn timers_fire_and_can_be_cleared() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export default { activate() {
        const cleared = setTimeout(() => nail.log.info("T cleared"), 20);
        clearTimeout(cleared);
        setTimeout((a, b) => nail.log.info("T " + a + b), 30, "x", "y");
        setTimeout(() => nail.log.info("T first"), 0);
        queueMicrotask(() => nail.log.info("T micro"));
      } };
    "#;
    host.start(spec("timer", &[], js)).await.unwrap();
    let results = wait_for("timers", || {
        let r = fake.results("timer", "T ");
        (r.len() == 3).then_some(r)
    })
    .await;
    assert_eq!(results, ["micro", "first", "xy"]);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(fake.results("timer", "T ").len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn relative_imports_inside_the_package_work() {
    let fake = Fake::new();
    let host = host(&fake);
    let spec = spec_with_files(
        "mods",
        &[],
        &[
            (
                "index.js",
                r#"import { twice } from "./lib/util.js";
                   export default { activate() { nail.log.info("R " + twice(21)); } };"#,
            ),
            (
                "lib/util.js",
                r#"import { base } from "../base.mjs"; export const twice = (x) => x * base;"#,
            ),
            ("base.mjs", "export const base = 2;"),
        ],
    );
    host.start(spec).await.unwrap();
    assert_eq!(result(&fake, "mods", "R ").await, "42");
}

#[tokio::test(flavor = "multi_thread")]
async fn activate_may_be_async_and_plugins_see_their_identity() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export async function activate(ctx) {
        await new Promise((r) => setTimeout(r, 10));
        nail.log.info("ID " + ctx.plugin.id + " " + nail.plugin.version + " " + nail.apiVersion);
        nail.log.info("FROZEN " + Object.isFrozen(nail) + " " + Reflect.deleteProperty(globalThis, "nail"));
      }
    "#;
    host.start(spec("ident", &[], js)).await.unwrap();
    assert_eq!(result(&fake, "ident", "ID ").await, "ident 1.0.0 1");
    assert_eq!(result(&fake, "ident", "FROZEN ").await, "true false");
}
