//! Wave M4a integration (WP105, ADR 0084) against the demo-multi mock
//! servers: plug-in hooks in the arrival / send / open paths (an arrival
//! hook tags new mail, a before-send hook vetoes and "Send anyway" sends,
//! another adds a header and content, an open hook shows a banner), plug-in
//! writes through the shared write path (a task created without asking, a
//! Trash that waits for the user's approval), a background job on the
//! controlled clock, plug-in tools offered to MCP clients, revoking a
//! client cancelling its confirmations, and (post-M4a, ADR 0086) confirmed
//! sends pinned to the approved draft revision.

use std::future::Future;
use std::time::Duration;

use nail_api::agents::{
    ActionActor, ActionAttribution, ActionKind, ActionTarget, ActionTargetKind,
    ConfirmationDecision, ConfirmationFilter, ConfirmationState, McpClientRef, NewConfirmation,
};
use nail_api::mcp::McpGrant;
use nail_api::plugins::{PluginGrant, PluginPackageSource, PluginPermission};
use nail_api::{
    Address, Draft, ErrorCode, FolderRole, LocalizedMessage, MessageSummary, PluginId, Request,
    Response, UiContext,
};
use nail_core::demo::{DemoMulti, MULTI_WORK_EMAIL, MULTI_WORK_ID, start_demo_multi};
use nail_core::{Service, ServiceOptions};
use nail_mcp::CoreAccess as _;
use nail_plugins::package::build_package;
use serde_json::{Value, json};
use tempfile::TempDir;

const TIMEOUT: Duration = Duration::from_secs(30);

struct Fixture {
    service: Service,
    demo: DemoMulti,
    files: TempDir,
    _config: TempDir,
    _data: TempDir,
}

async fn setup() -> Fixture {
    let demo = start_demo_multi().await.unwrap();
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    demo.write_config(config.path()).unwrap();
    let options = demo.service_options(ServiceOptions::new(data.path(), config.path()));
    let service = Service::open(options).unwrap();
    Fixture {
        service,
        demo,
        files: tempfile::tempdir().unwrap(),
        _config: config,
        _data: data,
    }
}

async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while !check().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for: {what}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn control(f: &Fixture, command: &str, args: Value) -> Value {
    f.demo.control(command, &args).unwrap()
}

async fn inbox(f: &Fixture, role: FolderRole) -> Vec<MessageSummary> {
    let Response::Folders { folders } = f
        .service
        .handle(Request::ListFolders {
            account_id: MULTI_WORK_ID.into(),
        })
        .await
    else {
        panic!("folders expected")
    };
    let Some(folder) = folders.into_iter().find(|x| x.role == role) else {
        return Vec::new();
    };
    match f
        .service
        .handle(Request::ListMessages {
            folder_id: folder.id,
            offset: 0,
            limit: 500,
        })
        .await
    {
        Response::Messages { messages, .. } => messages,
        other => panic!("{other:?}"),
    }
}

async fn synced(f: &Fixture) {
    eventually("the work inbox synced", || async {
        inbox(f, FolderRole::Inbox).await.len() >= 15
    })
    .await;
}

/// Installs and starts a plug-in package with every permission granted.
async fn install(f: &Fixture, manifest: Value, js: &str) -> PluginId {
    install_with(f, manifest, js, &[]).await
}

/// [`install`] with more files in the package (catalogs).
async fn install_with(f: &Fixture, manifest: Value, js: &str, files: &[(&str, &str)]) -> PluginId {
    let id = manifest["id"].as_str().unwrap().to_owned();
    let permissions: Vec<String> = manifest["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_owned())
        .collect();
    let text = manifest.to_string();
    let mut entries = vec![
        ("manifest.json", text.as_bytes()),
        ("index.js", js.as_bytes()),
    ];
    entries.extend(files.iter().map(|(path, body)| (*path, body.as_bytes())));
    let bytes = build_package(entries).unwrap();
    let path = f.files.path().join(format!("{id}.nailplugin"));
    std::fs::write(&path, bytes).unwrap();
    let source = PluginPackageSource::File {
        path: path.display().to_string(),
    };
    let sha = match f
        .service
        .handle(Request::PreviewPluginInstall {
            source: source.clone(),
        })
        .await
    {
        Response::PluginInstallPreview { preview } => preview.package_sha256,
        other => panic!("{other:?}"),
    };
    let grants = permissions
        .iter()
        .map(|p| PluginGrant::all(PluginPermission::parse(p).unwrap()))
        .collect();
    match f
        .service
        .handle(Request::InstallPlugin {
            source,
            package_sha256: sha,
            grants,
            enable: true,
        })
        .await
    {
        Response::Plugin { .. } => {}
        other => panic!("{other:?}"),
    }
    PluginId::new(id)
}

fn manifest(id: &str, permissions: &[&str], contributes: Value) -> Value {
    json!({
        "id": id, "name": id, "version": "1.0.0", "apiVersion": "1",
        "main": "index.js", "permissions": permissions, "contributes": contributes,
    })
}

fn draft(subject: &str, text: &str) -> Draft {
    let mut draft = Draft::new(MULTI_WORK_EMAIL);
    draft.to = vec![Address {
        name: None,
        email: "bob@example.org".into(),
    }];
    draft.subject = subject.into();
    draft.text = text.into();
    draft
}

async fn save(f: &Fixture, draft: Draft) -> nail_api::DraftId {
    match f.service.handle(Request::SaveDraft { draft }).await {
        Response::DraftSaved { draft_id } => draft_id,
        other => panic!("{other:?}"),
    }
}

fn send(draft_id: nail_api::DraftId) -> Request {
    Request::SendDraft {
        draft_id,
        delay_secs: Some(0),
        send_at: None,
        crypto: None,
        expected_revision: None,
    }
}

fn smtp_data(f: &Fixture) -> Vec<String> {
    control(f, "smtp-messages", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["data"].as_str().unwrap().to_owned())
        .collect()
}

const HOOKS_JS: &str = r#"
export default {
  activate(ctx) {
    ctx.hooks.register("arrival", async ({ message }) =>
      message.subject.includes("Hooked") ? { actions: [{ type: "addTag", tag: "from-plugin" }] } : null);
    ctx.hooks.register("beforeSend", async (m) =>
      m.subject.includes("veto")
        ? { veto: { reason: "Nothing is attached." } }
        : { headers: [{ name: "X-Checked", value: "yes" }], addContent: [{ position: "bottom", text: "-- checked" }] });
    ctx.hooks.register("open", async ({ message }) => ({
      banners: [{ id: "note", level: "info", text: "Seen by the checker: " + message.id }],
      badges: [{ text: "OK" }],
    }));
  },
};
"#;

#[tokio::test(flavor = "multi_thread")]
async fn hooks_tag_arrivals_veto_sends_and_annotate_messages() {
    let f = setup().await;
    synced(&f).await;
    let plugin = install(
        &f,
        manifest(
            "checker",
            &[
                "mail.read",
                "mail.organize",
                "hooks.arrival",
                "hooks.beforeSend",
                "hooks.open",
            ],
            json!({"hooks": [{"hook": "arrival"}, {"hook": "beforeSend"}, {"hook": "open"}]}),
        ),
        HOOKS_JS,
    )
    .await;

    // An arrival hook tags new mail through the plug-in write path.
    control(
        &f,
        "imap-deliver",
        json!({"raw": "From: ann@example.org\r\nTo: me@work.example\r\nSubject: Hooked message\r\nMessage-ID: <hooked@example.org>\r\nDate: Thu, 01 Oct 2026 09:00:00 +0000\r\n\r\nHello\r\n"}),
    );
    eventually("the arrival hook tagged the new mail", || async {
        inbox(&f, FolderRole::Inbox)
            .await
            .iter()
            .any(|m| m.subject == "Hooked message" && m.tags.iter().any(|t| t == "from-plugin"))
    })
    .await;
    let hooked = inbox(&f, FolderRole::Inbox)
        .await
        .into_iter()
        .find(|m| m.subject == "Hooked message")
        .unwrap();

    // The open hook's banner and badge name the plug-in.
    let Response::MessagePluginAnnotations {
        banners, badges, ..
    } = f
        .service
        .handle(Request::GetMessagePluginAnnotations {
            message_id: hooked.id.clone(),
            client_id: None,
        })
        .await
    else {
        panic!("annotations expected");
    };
    assert_eq!(banners.len(), 1);
    assert_eq!(banners[0].plugin_id, plugin);
    assert!(banners[0].banner.text.contains(hooked.id.as_str()));
    assert_eq!(badges[0].badge.text, "OK");

    // A veto stops the send; nothing is queued until "Send anyway".
    let vetoed = save(&f, draft("Please veto", "See attached")).await;
    let Response::SendVetoed {
        veto_id, vetoes, ..
    } = f.service.handle(send(vetoed.clone())).await
    else {
        panic!("vetoed expected");
    };
    assert_eq!(vetoes[0].plugin_id, plugin);
    assert_eq!(vetoes[0].reason, "Nothing is attached.");
    let Response::Outbox { entries } = f.service.handle(Request::ListOutbox).await else {
        panic!()
    };
    assert!(entries.is_empty(), "nothing queued: {entries:?}");
    assert!(matches!(
        f.service
            .handle(Request::OverrideSendVeto {
                veto_id: veto_id.clone()
            })
            .await,
        Response::SendQueued { .. }
    ));
    // A veto is used once.
    assert!(matches!(
        f.service
            .handle(Request::OverrideSendVeto { veto_id })
            .await,
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));
    eventually("the vetoed message went out anyway", || async {
        smtp_data(&f)
            .iter()
            .any(|d| d.contains("Subject: Please veto"))
    })
    .await;

    // Without a veto the hook's header and content are applied.
    let normal = save(&f, draft("Normal mail", "Hello Bob")).await;
    assert!(matches!(
        f.service.handle(send(normal)).await,
        Response::SendQueued { .. }
    ));
    eventually("the checked message went out", || async {
        smtp_data(&f).iter().any(|d| {
            d.contains("Subject: Normal mail")
                && d.contains("X-Checked: yes")
                && d.contains("-- checked")
        })
    })
    .await;

    // The hooks were counted.
    let Response::PluginActivity { activity } = f
        .service
        .handle(Request::GetPluginActivity {
            plugin_id: plugin.clone(),
        })
        .await
    else {
        panic!()
    };
    assert!(activity.hooks.iter().any(|h| h.calls > 0), "{activity:?}");
}

const WRITER_JS: &str = r#"
export default {
  activate(ctx) {
    ctx.commands.register("go", async (c) => {
      const id = c.selection.messageIds[0];
      const task = await nail.tasks.create({ title: "Follow up", relatedMessageId: id });
      nail.log.info("TASK " + task.status + " " + (task.created || []).join(","));
      const trashed = await nail.mail.delete([id]);
      nail.log.info("TRASH " + trashed.status);
    });
    ctx.jobs.register("tick", async (trigger) => {
      const n = (await nail.storage.get("ticks")) || 0;
      await nail.storage.set("ticks", n + 1);
    });
    ctx.mcp.registerTool("lookup", async (args) => {
      let mail = "ok";
      try { await nail.mail.get(args.id); } catch (e) { mail = e.code; }
      return { found: args.q, mail };
    });
  },
};
"#;

async fn log_lines(f: &Fixture, plugin: &PluginId) -> Vec<String> {
    match f
        .service
        .handle(Request::ListPluginLogs {
            plugin_id: Some(plugin.clone()),
            after_seq: None,
            limit: 500,
        })
        .await
    {
        Response::PluginLogs { entries } => entries.into_iter().map(|e| e.message).collect(),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn plugin_writes_jobs_and_tools_run_through_the_core() {
    let f = setup().await;
    synced(&f).await;
    let plugin = install_with(
        &f,
        manifest(
            "writer",
            &[
                "mail.read",
                "mail.organize",
                "tasks.write",
                "storage",
                "background",
                "mcp.expose",
            ],
            json!({
                "commands": [{"id": "go", "title": "Go", "contexts": ["list"]}],
                "jobs": [{"id": "tick", "title": "%tick-title%", "schedule": "@hourly"}],
                "mcpTools": [{"id": "lookup", "description": "%lookup-description%", "scopes": ["read"]}],
                "locales": {"en": "locales/en.ftl"},
            }),
        ),
        WRITER_JS,
        &[(
            "locales/en.ftl",
            "tick-title = Count the hours\nlookup-description = Look something up\n",
        )],
    )
    .await;
    let target = inbox(&f, FolderRole::Inbox).await.remove(0);

    let response = f
        .service
        .handle(Request::RunPluginCommand {
            command: "plugin.writer.go".into(),
            surface: nail_api::plugins::PluginCommandSurface::List,
            context: UiContext {
                selected_message_ids: vec![target.id.clone()],
                ..UiContext::default()
            },
            client_id: None,
        })
        .await;
    assert_eq!(response, Response::Done);

    // The task is created without asking; the Trash waits for the user.
    eventually("the task was created", || async {
        log_lines(&f, &plugin)
            .await
            .iter()
            .any(|l| l.starts_with("TASK done"))
    })
    .await;
    let pending = loop {
        let Response::Confirmations { confirmations } = f
            .service
            .handle(Request::ListConfirmations {
                filter: ConfirmationFilter::Pending,
                actor: None,
                before: None,
                limit: 10,
            })
            .await
        else {
            panic!()
        };
        if let Some(first) = confirmations.into_iter().next() {
            break first;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(pending.kind, ActionKind::Trash);
    assert_eq!(
        pending.requested_by.actor,
        ActionActor::Plugin {
            plugin_id: plugin.clone()
        }
    );
    assert_eq!(pending.operation, "nail.mail.delete");
    assert!(
        inbox(&f, FolderRole::Inbox)
            .await
            .iter()
            .any(|m| m.id == target.id),
        "nothing moved before the decision"
    );
    let decided = f
        .service
        .handle(Request::DecideConfirmation {
            confirmation_id: pending.id.clone(),
            decision: ConfirmationDecision::Approve,
            always_allow: false,
        })
        .await;
    assert!(
        matches!(decided, Response::ConfirmationDecided { .. }),
        "{decided:?}"
    );
    eventually("the approved Trash ran", || async {
        log_lines(&f, &plugin)
            .await
            .iter()
            .any(|l| l == "TRASH done")
    })
    .await;
    eventually("the message left the inbox", || async {
        !inbox(&f, FolderRole::Inbox)
            .await
            .iter()
            .any(|m| m.id == target.id)
    })
    .await;
    assert!(
        log_lines(&f, &plugin)
            .await
            .iter()
            .any(|l| l == "write nail.tasks.create: done"),
        "every write is logged"
    );

    // The hourly job runs when the core's clock passes the hour.
    control(&f, "clock-advance", json!({"seconds": 7200}));
    f.service.clock_moved();
    eventually("the job ran on the controlled clock", || async {
        let Response::PluginActivity { activity } = f
            .service
            .handle(Request::GetPluginActivity {
                plugin_id: plugin.clone(),
            })
            .await
        else {
            return false;
        };
        activity
            .jobs
            .iter()
            .any(|j| j.job == "tick" && j.run_count >= 1 && j.next_run_at.is_some())
    })
    .await;
    // Post-M4a: the job's `%id%` title is localized, in the activity and
    // in the manifest the plug-in manager lists.
    let Response::PluginActivity { activity } = f
        .service
        .handle(Request::GetPluginActivity {
            plugin_id: plugin.clone(),
        })
        .await
    else {
        panic!("activity expected")
    };
    assert_eq!(activity.jobs[0].title.as_deref(), Some("Count the hours"));
    let Response::Plugin { plugin: info } = f
        .service
        .handle(Request::GetPlugin {
            plugin_id: plugin.clone(),
        })
        .await
    else {
        panic!("plug-in expected")
    };
    assert_eq!(
        info.manifest.contributes.jobs[0].title.as_deref(),
        Some("Count the hours")
    );
    assert_eq!(
        info.manifest.contributes.mcp_tools[0].description,
        "Look something up"
    );

    // The plug-in's tool is offered to MCP clients and answers.
    let core = nail_core::McpCore(f.service.clone());
    let tools = core.plugin_tools();
    assert_eq!(tools.len(), 1, "{tools:?}");
    assert_eq!(tools[0].name, "writer__lookup");
    assert_eq!(tools[0].description, "Look something up");
    let Response::McpClientCreated { client, .. } = f
        .service
        .handle(Request::CreateMcpClient {
            name: "Agent".into(),
            grant: McpGrant {
                scopes: vec![nail_api::mcp::McpScope::Read],
                all_accounts: true,
                ..Default::default()
            },
        })
        .await
    else {
        panic!("client expected")
    };
    let by = McpClientRef {
        client_id: client.id.clone(),
        name: "Agent".into(),
    };
    let message = inbox(&f, FolderRole::Inbox).await.remove(0);
    let args = json!({"q": "ada", "id": message.id.as_str()});
    let answer = core
        .call_plugin_tool("writer__lookup", args.clone(), by.clone())
        .await
        .unwrap();
    assert_eq!(answer, json!({"found": "ada", "mail": "ok"}));
    // Review M4a: the plug-in reads within the client's grant too — with
    // another account only, the message is not found.
    let saved = f
        .service
        .handle(Request::UpdateMcpGrants {
            client_id: client.id.clone(),
            grant: McpGrant {
                scopes: vec![nail_api::mcp::McpScope::Read],
                accounts: vec!["somewhere-else".into()],
                ..Default::default()
            },
        })
        .await;
    assert!(!matches!(saved, Response::Error { .. }), "{saved:?}");
    let answer = core
        .call_plugin_tool("writer__lookup", args, by)
        .await
        .unwrap();
    assert_eq!(answer, json!({"found": "ada", "mail": "not_found"}));
    // An unknown client reaches nothing.
    let err = core
        .call_plugin_tool(
            "writer__lookup",
            json!({"q": "x"}),
            McpClientRef {
                client_id: "999".into(),
                name: "Ghost".into(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);

    // Review M4a: plug-in writes are in the MCP audit log, which the
    // plug-in cannot push out like its own log.
    let Response::McpAudit { entries } = f
        .service
        .handle(Request::ListMcpAudit {
            client_id: None,
            before: None,
            limit: 100,
        })
        .await
    else {
        panic!("audit expected")
    };
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(
        names.contains(&"plugin:writer nail.tasks.create")
            && names.contains(&"plugin:writer nail.mail.delete"),
        "{names:?}"
    );
}

async fn stored_draft(f: &Fixture, id: &nail_api::DraftId) -> Draft {
    match f
        .service
        .handle(Request::GetDraft {
            draft_id: id.clone(),
        })
        .await
    {
        Response::Draft { draft } => *draft,
        other => panic!("{other:?}"),
    }
}

async fn outbox_len(f: &Fixture) -> usize {
    let Response::Outbox { entries } = f.service.handle(Request::ListOutbox).await else {
        panic!("outbox expected")
    };
    entries.len()
}

fn send_revision(draft_id: nail_api::DraftId, revision: String) -> Request {
    Request::SendDraft {
        draft_id,
        delay_secs: Some(0),
        send_at: None,
        crypto: None,
        expected_revision: Some(revision),
    }
}

fn assert_draft_changed(response: Response) {
    match response {
        Response::Error {
            code: ErrorCode::Conflict,
            localized: Some(localized),
            ..
        } => assert_eq!(localized.id, "mcp-tool-error-draft-changed"),
        other => panic!("draft-changed expected, got {other:?}"),
    }
}

/// Post-M4a (ADR 0086): a send pinned to the draft revision a
/// confirmation showed queues exactly that draft. An edit after the
/// approval makes the send fail with `draft-changed` and nothing is queued
/// or sent; the hooks' own content keeps the pin intact; a plug-in's send
/// approved after the user edited the draft fails the confirmation.
#[tokio::test(flavor = "multi_thread")]
async fn a_confirmed_send_queues_only_the_approved_draft() {
    let f = setup().await;
    synced(&f).await;

    // The draft changes between the approval and the send: refused in the
    // transaction that would queue it.
    let id = save(&f, draft("Approved send", "Hello Bob")).await;
    let approved = nail_store::draft_revision(&stored_draft(&f, &id).await);
    let mut edited = stored_draft(&f, &id).await;
    edited.to[0].email = "mallory@example.org".into();
    save(&f, edited).await;
    assert_draft_changed(f.service.handle(send_revision(id.clone(), approved)).await);
    assert_eq!(outbox_len(&f).await, 0, "nothing queued");

    // With before-send hooks: a stale revision never reaches them, the
    // current one is sent with the hook's content.
    install(
        &f,
        manifest(
            "checker",
            &[
                "mail.read",
                "mail.organize",
                "hooks.arrival",
                "hooks.beforeSend",
                "hooks.open",
            ],
            json!({"hooks": [{"hook": "arrival"}, {"hook": "beforeSend"}, {"hook": "open"}]}),
        ),
        HOOKS_JS,
    )
    .await;
    let current = nail_store::draft_revision(&stored_draft(&f, &id).await);
    let mut again = stored_draft(&f, &id).await;
    again.subject = "Approved send (edited)".into();
    save(&f, again).await;
    assert_draft_changed(f.service.handle(send_revision(id.clone(), current)).await);
    assert_eq!(outbox_len(&f).await, 0, "nothing queued");
    let current = nail_store::draft_revision(&stored_draft(&f, &id).await);
    assert!(matches!(
        f.service.handle(send_revision(id.clone(), current)).await,
        Response::SendQueued { .. }
    ));
    eventually(
        "the approved draft went out with the hook's content",
        || async {
            smtp_data(&f).iter().any(|d| {
                d.contains("Subject: Approved send (edited)")
                    && d.contains("mallory@example.org")
                    && d.contains("-- checked")
            })
        },
    )
    .await;
    assert!(
        !smtp_data(&f)
            .iter()
            .any(|d| d.contains("Subject: Approved send\r\n")),
        "the stale revisions were never sent"
    );

    // A plug-in's send waits for the user; the draft is edited before the
    // approval: the confirmation fails and nothing is sent.
    let pinned = save(&f, draft("Plug-in send", "Hello from the plug-in")).await;
    let js = format!(
        r#"export default {{
  activate(ctx) {{
    ctx.commands.register("send", async () => {{
      try {{
        const r = await nail.mail.send({id});
        nail.log.info("SEND " + r.status);
      }} catch (e) {{
        nail.log.info("SEND error " + e.code);
      }}
    }});
  }},
}};"#,
        id = serde_json::to_string(pinned.as_str()).unwrap()
    );
    let sender = install(
        &f,
        manifest(
            "sender",
            &["mail.read", "mail.send"],
            json!({"commands": [{"id": "send", "title": "Send", "contexts": ["list"]}]}),
        ),
        &js,
    )
    .await;
    let response = f
        .service
        .handle(Request::RunPluginCommand {
            command: "plugin.sender.send".into(),
            surface: nail_api::plugins::PluginCommandSurface::List,
            context: UiContext::default(),
            client_id: None,
        })
        .await;
    assert_eq!(response, Response::Done);
    let pending = loop {
        let Response::Confirmations { confirmations } = f
            .service
            .handle(Request::ListConfirmations {
                filter: ConfirmationFilter::Pending,
                actor: None,
                before: None,
                limit: 10,
            })
            .await
        else {
            panic!()
        };
        if let Some(first) = confirmations.into_iter().next() {
            break first;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(pending.kind, ActionKind::Send);
    let mut changed = stored_draft(&f, &pinned).await;
    changed.bcc.push(Address {
        name: None,
        email: "eve@example.org".into(),
    });
    save(&f, changed).await;
    let decided = f
        .service
        .handle(Request::DecideConfirmation {
            confirmation_id: pending.id.clone(),
            decision: ConfirmationDecision::Approve,
            always_allow: false,
        })
        .await;
    assert!(
        matches!(decided, Response::ConfirmationDecided { .. }),
        "{decided:?}"
    );
    eventually("the plug-in heard the send failed", || async {
        log_lines(&f, &sender)
            .await
            .iter()
            .any(|l| l.starts_with("SEND error"))
    })
    .await;
    let Response::Confirmations { confirmations } = f
        .service
        .handle(Request::ListConfirmations {
            filter: ConfirmationFilter::Decided,
            actor: None,
            before: None,
            limit: 10,
        })
        .await
    else {
        panic!()
    };
    let ended = confirmations
        .into_iter()
        .find(|c| c.id == pending.id)
        .unwrap();
    assert_eq!(ended.state, ConfirmationState::Failed);
    assert_eq!(ended.error.unwrap().id, "mcp-tool-error-draft-changed");
    assert_eq!(outbox_len(&f).await, 0, "nothing queued");
    assert!(
        !smtp_data(&f)
            .iter()
            .any(|d| d.contains("Subject: Plug-in send")),
        "nothing sent"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_a_client_cancels_its_confirmations() {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    options.start_engines = false;
    let service = Service::open(nail_core::demo::demo_options(options)).unwrap();
    let Response::McpClientCreated { client, .. } = service
        .handle(Request::CreateMcpClient {
            name: "Agent".into(),
            grant: McpGrant::default(),
        })
        .await
    else {
        panic!()
    };
    let actor = ActionActor::mcp_client(client.id.clone());
    let now = chrono::Utc::now().timestamp();
    let new = NewConfirmation {
        requested_by: ActionAttribution {
            actor: actor.clone(),
            name: "Agent".into(),
            on_behalf_of: None,
        },
        kind: ActionKind::Send,
        operation: "send_message".into(),
        summary: LocalizedMessage::new("confirmation-summary-send"),
        targets: vec![ActionTarget {
            kind: ActionTargetKind::Draft,
            id: "d1".into(),
            label: "Hi".into(),
        }],
        changes: vec![],
        account_ids: vec![],
        payload: json!({}),
        idempotency_key: None,
        timeout_secs: 300,
    };
    service.store().insert_confirmation(new, now, 20).unwrap();
    let mut events = service.subscribe();
    assert!(matches!(
        service
            .handle(Request::RevokeMcpClient {
                client_id: client.id.clone()
            })
            .await,
        Response::McpClientSaved { .. } | Response::Done
    ));
    let list = service
        .store()
        .list_confirmations(ConfirmationFilter::All, Some(&actor), None, 10)
        .unwrap();
    assert_eq!(list[0].state, ConfirmationState::Cancelled);
    let mut resolved = false;
    while let Ok(event) = events.try_recv() {
        resolved |= matches!(event, nail_api::Event::ConfirmationResolved { .. });
    }
    assert!(resolved, "the UIs were told");
}
