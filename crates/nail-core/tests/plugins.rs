//! Wave M2d integration (WP64, ADR 0055): the plug-in host inside the core.
//! Packages are previewed, installed with the approved hash, started,
//! run (commands, dialogs, storage, notifications, panels, log lines),
//! placed in `layout.kdl`, disabled and uninstalled; the permission gate
//! meets real mail (bodies need `mail.read.body`, encrypted bodies are
//! never given out); a runaway plug-in is stopped and flagged while the
//! core keeps answering; dev folders load and reload; `nail.http.fetch`
//! goes through the core; enabled plug-ins start with the core and a
//! tampered package is refused.

#![cfg(test)]

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nail_api::customize::MergeTake;
use nail_api::plugins::admin::{PluginRunState, PluginStopReason};
use nail_api::plugins::{
    PluginCommandSurface, PluginGrant, PluginInfo, PluginLogEntry, PluginPackageSource,
    PluginPermission, PluginUiNode,
};
use nail_api::security::SecuritySettings;
use nail_api::{
    ConfigFile, ErrorCode, Event, FolderRole, MessageArg, MessageId, PluginId, Request, Response,
    ToolbarItem, UiContext,
};
use nail_core::demo::{demo_options, seed_demo};
use nail_core::{Service, ServiceOptions};
use nail_plugins::package::build_package;
use serde_json::{Value, json};
use tokio::sync::broadcast;

struct Fixture {
    service: Service,
    data: tempfile::TempDir,
    config: tempfile::TempDir,
    files: tempfile::TempDir,
}

fn options(data: &Path, config: &Path) -> ServiceOptions {
    let mut options = ServiceOptions::new(data, config);
    options.watch_config = false;
    options.start_engines = false;
    demo_options(options)
}

fn setup() -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let service = Service::open(options(data.path(), config.path())).unwrap();
    seed_demo(service.store(), service.blobs()).unwrap();
    Fixture {
        service,
        data,
        config,
        files,
    }
}

const FOLLOW_UP_JS: &str = r#"
export default {
  activate(ctx) {
    ctx.commands.register("follow-up", async (c) => {
      const id = c.selection.messageIds[0] ?? c.selection.openMessageId;
      const msg = await nail.mail.get(id);
      const res = await nail.ui.showDialog({
        title: "Follow up",
        body: [
          { type: "text", value: "Remind me about " + msg.subject },
          { type: "date", id: "due", label: "Due", default: "tomorrow" },
        ],
        buttons: [{ id: "save", label: "Save", primary: true }, { id: "cancel", label: "Cancel" }],
      });
      if (res.button !== "save") return;
      await nail.storage.set("reminder:" + id, { due: res.values.due, subject: msg.subject });
      nail.ui.notify({ title: "Reminder set for " + res.values.due, level: "success" });
      nail.log.info("saved " + id);
    });
    ctx.commands.register("body", async (c) => {
      try {
        const body = await nail.mail.getBody(c.selection.messageIds[0]);
        nail.log.info("BODY " + body.text.length);
      } catch (e) {
        nail.log.warn("DENIED " + e.code);
      }
    });
    ctx.panels.register("due", async () => {
      const keys = await nail.storage.keys("reminder:");
      return [{ type: "text", value: keys.length + " reminders" }];
    });
  },
};
"#;

fn manifest(id: &str, version: &str, permissions: &[&str]) -> Value {
    json!({
        "id": id, "name": "%name%", "version": version, "apiVersion": "1",
        "main": "index.js", "permissions": permissions,
        "contributes": {
            "commands": [
                {"id": "follow-up", "title": "%follow-up%", "contexts": ["list", "reader"]},
                {"id": "body", "title": "Body", "contexts": ["list", "reader"]}
            ],
            "placements": [{"toolbar": "reader", "after": "mail.forward", "command": "follow-up"}],
            "keys": [{"key": "shift+f", "command": "follow-up", "context": "list"}],
            "panels": [{"id": "due", "title": "Due"}],
            "locales": {"en": "locales/en.ftl"}
        }
    })
}

const EN_FTL: &str = "name = Follow-up reminders\nfollow-up = Follow up…\n";

fn package(id: &str, version: &str, permissions: &[&str], js: &str) -> Vec<u8> {
    let manifest = manifest(id, version, permissions).to_string();
    build_package([
        ("manifest.json", manifest.as_bytes()),
        ("index.js", js.as_bytes()),
        ("locales/en.ftl", EN_FTL.as_bytes()),
    ])
    .unwrap()
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn all(permissions: &[PluginPermission]) -> Vec<PluginGrant> {
    permissions.iter().cloned().map(PluginGrant::all).collect()
}

fn error(response: Response) -> (ErrorCode, Option<String>) {
    match response {
        Response::Error {
            code, localized, ..
        } => (code, localized.map(|l| l.id)),
        other => panic!("expected an error, got {other:?}"),
    }
}

fn plugin(response: Response) -> PluginInfo {
    match response {
        Response::Plugin { plugin } => *plugin,
        other => panic!("expected a plug-in, got {other:?}"),
    }
}

async fn get(f: &Fixture, id: &str) -> PluginInfo {
    plugin(
        f.service
            .handle(Request::GetPlugin {
                plugin_id: PluginId::new(id),
            })
            .await,
    )
}

/// Waits for an event matching `pick`.
async fn wait_event<T>(
    rx: &mut broadcast::Receiver<Event>,
    what: &str,
    mut pick: impl FnMut(&Event) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Ok(event)) => {
                if let Some(found) = pick(&event) {
                    return found;
                }
                seen.push(event);
            }
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(e)) => panic!("{what}: {e}"),
            Err(elapsed) => panic!("no {what} within 20 s ({elapsed}); saw {seen:#?}"),
        }
    }
}

/// Polls until `check` holds.
async fn wait_until<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(20);
    while !check().await {
        assert!(Instant::now() < deadline, "{what} did not happen in 20 s");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn inbox_messages(f: &Fixture) -> Vec<MessageId> {
    let folders = match f
        .service
        .handle(Request::ListFolders {
            account_id: nail_core::demo::DEMO_ACCOUNT_ID.into(),
        })
        .await
    {
        Response::Folders { folders } => folders,
        other => panic!("{other:?}"),
    };
    let inbox = folders
        .into_iter()
        .find(|f| f.role == FolderRole::Inbox)
        .unwrap();
    match f
        .service
        .handle(Request::ListMessages {
            folder_id: inbox.id,
            offset: 0,
            limit: 5,
        })
        .await
    {
        Response::Messages { messages, .. } => messages.into_iter().map(|m| m.id).collect(),
        other => panic!("{other:?}"),
    }
}

fn selection(ids: &[MessageId]) -> UiContext {
    UiContext {
        selected_message_ids: ids.to_vec(),
        ..UiContext::default()
    }
}

async fn install(f: &Fixture, id: &str, permissions: &[&str], js: &str) -> PluginInfo {
    let bytes = package(id, "1.0.0", permissions, js);
    let path = write(f.files.path(), &format!("{id}.nailplugin"), &bytes);
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
    plugin(
        f.service
            .handle(Request::InstallPlugin {
                source,
                package_sha256: sha,
                grants,
                enable: true,
            })
            .await,
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_package_is_previewed_installed_run_placed_and_uninstalled() {
    let f = setup();
    let permissions = ["mail.read", "ui.dialogs", "storage", "notifications"];
    let bytes = package("follow-up", "1.0.0", &permissions, FOLLOW_UP_JS);
    let path = write(f.files.path(), "follow-up.nailplugin", &bytes);
    let source = PluginPackageSource::File {
        path: path.display().to_string(),
    };

    // Preview: texts from the plug-in's catalog, the package's hash.
    let preview = match f
        .service
        .handle(Request::PreviewPluginInstall {
            source: source.clone(),
        })
        .await
    {
        Response::PluginInstallPreview { preview } => preview,
        other => panic!("{other:?}"),
    };
    let manifest = preview.manifest.clone().expect("a valid manifest");
    assert_eq!(manifest.name, "Follow-up reminders");
    assert_eq!(manifest.contributes.commands[0].title, "Follow up…");
    assert!(preview.issues.is_empty());
    assert_eq!(preview.size, bytes.len() as u64);
    assert_eq!(preview.installed_version, None);

    // Another package than the previewed one is refused.
    let changed = error(
        f.service
            .handle(Request::InstallPlugin {
                source: source.clone(),
                package_sha256: "0".repeat(64),
                grants: vec![],
                enable: true,
            })
            .await,
    );
    assert_eq!(
        changed,
        (
            ErrorCode::Conflict,
            Some("plugin-package-hash-mismatch".into())
        )
    );
    // Grants of undeclared permissions are refused before anything is written.
    let undeclared = f
        .service
        .handle(Request::InstallPlugin {
            source: source.clone(),
            package_sha256: preview.package_sha256.clone(),
            grants: all(&[PluginPermission::MailReadBody]),
            enable: true,
        })
        .await;
    assert_eq!(error(undeclared).0, ErrorCode::InvalidRequest);
    assert!(!f.data.path().join("plugins/follow-up.nailplugin").exists());

    let mut events = f.service.subscribe();
    let installed = plugin(
        f.service
            .handle(Request::InstallPlugin {
                source,
                package_sha256: preview.package_sha256.clone(),
                grants: all(&[
                    PluginPermission::MailRead,
                    PluginPermission::UiDialogs,
                    PluginPermission::Storage,
                    PluginPermission::Notifications,
                ]),
                enable: true,
            })
            .await,
    );
    assert_eq!(installed.state, PluginRunState::Running);
    assert_eq!(installed.manifest.name, "Follow-up reminders");
    assert_eq!(
        installed.package_sha256.as_deref(),
        Some(preview.package_sha256.as_str())
    );
    // Kept packed, exactly the approved bytes.
    assert_eq!(
        std::fs::read(f.data.path().join("plugins/follow-up.nailplugin")).unwrap(),
        bytes
    );

    // The command answers at once; its dialog carries the UI's id.
    let ids = inbox_messages(&f).await;
    let answered = f
        .service
        .handle(Request::RunPluginCommand {
            command: "plugin.follow-up.follow-up".into(),
            surface: PluginCommandSurface::List,
            context: selection(&ids[..1]),
            client_id: Some("ui-1".into()),
        })
        .await;
    assert_eq!(answered, Response::Done);
    let dialog = wait_event(&mut events, "a dialog", |e| match e {
        Event::PluginDialogOpened { dialog } => Some(dialog.clone()),
        _ => None,
    })
    .await;
    assert_eq!(dialog.client_id.as_deref(), Some("ui-1"));
    assert_eq!(dialog.spec.title, "Follow up");
    let PluginUiNode::Text { value, .. } = &dialog.spec.body[0] else {
        panic!("{:?}", dialog.spec.body);
    };
    assert!(value.starts_with("Remind me about "), "{value}");

    assert_eq!(
        f.service
            .handle(Request::RespondPluginDialog {
                dialog_id: dialog.dialog_id.clone(),
                button: Some("save".into()),
                values: BTreeMap::from([("due".to_owned(), json!("2031-03-05"))]),
            })
            .await,
        Response::Done
    );
    let notification = wait_event(&mut events, "a notification", |e| match e {
        Event::PluginNotification { notification } => Some(notification.clone()),
        _ => None,
    })
    .await;
    assert_eq!(notification.spec.title, "Reminder set for 2031-03-05");
    assert_eq!(notification.client_id.as_deref(), Some("ui-1"));
    let key = format!("reminder:{}", ids[0]);
    let id = PluginId::new("follow-up");
    let stored = f.service.store().plugin_storage_get(&id, &key).unwrap();
    assert_eq!(stored.unwrap()["due"], json!("2031-03-05"));
    // Its log line was stored and broadcast.
    let saved = format!("saved {}", ids[0]);
    wait_event(&mut events, "the log line", |e| match e {
        Event::PluginLogLine { entry } if entry.message == saved => Some(()),
        _ => None,
    })
    .await;
    // A dialog is answered once.
    assert_eq!(
        error(
            f.service
                .handle(Request::RespondPluginDialog {
                    dialog_id: dialog.dialog_id.clone(),
                    button: None,
                    values: BTreeMap::new(),
                })
                .await
        )
        .0,
        ErrorCode::NotFound
    );

    // The panel renders from storage.
    match f
        .service
        .handle(Request::GetPluginPanel {
            panel: "follow-up.due".into(),
            context: UiContext::default(),
            client_id: None,
        })
        .await
    {
        Response::PluginPanel { tree, .. } => {
            assert!(matches!(&tree[0], PluginUiNode::Text { value, .. } if value == "1 reminders"));
        }
        other => panic!("{other:?}"),
    }
    // Unknown commands and panels.
    assert_eq!(
        error(
            f.service
                .handle(Request::RunPluginCommand {
                    command: "plugin.follow-up.nope".into(),
                    surface: PluginCommandSurface::Global,
                    context: UiContext::default(),
                    client_id: None,
                })
                .await
        )
        .0,
        ErrorCode::NotFound
    );

    // Placements: the user's layout.kdl with the toolbar item and the key,
    // applied through the merge; placing again proposes nothing.
    let (incoming, preview) = match f
        .service
        .handle(Request::PreviewPluginPlacements {
            plugin_id: id.clone(),
        })
        .await
    {
        Response::PluginPlacements {
            incoming, preview, ..
        } => (incoming, preview),
        other => panic!("{other:?}"),
    };
    assert!(
        incoming.contains("item \"plugin.follow-up.follow-up\""),
        "{incoming}"
    );
    assert!(incoming.contains("bind \"shift+f\" \"plugin.follow-up.follow-up\""));
    // The plug-in's own items are not flagged as missing.
    assert!(
        preview
            .sections
            .iter()
            .flat_map(|s| &s.items)
            .all(|i| i.conflicts.is_empty()),
        "{preview:#?}"
    );
    let applied = f
        .service
        .handle(Request::ApplyConfigMerge {
            file: ConfigFile::Layout,
            incoming,
            choices: vec![],
            default: MergeTake::Theirs,
        })
        .await;
    assert!(
        matches!(applied, Response::ConfigMergeApplied { .. }),
        "{applied:?}"
    );
    let written = std::fs::read_to_string(f.config.path().join("layout.kdl")).unwrap();
    assert!(written.contains("plugin.follow-up.follow-up"), "{written}");
    let layout = f.service.config().layout();
    let reader = &layout.toolbars["reader"];
    let at = reader
        .iter()
        .position(|i| matches!(i, ToolbarItem::Action { action, .. } if action == "mail.forward"))
        .unwrap();
    assert!(matches!(
        &reader[at + 1],
        ToolbarItem::Action { action, .. } if action == "plugin.follow-up.follow-up"
    ));
    match f
        .service
        .handle(Request::PreviewPluginPlacements {
            plugin_id: id.clone(),
        })
        .await
    {
        Response::PluginPlacements {
            incoming, preview, ..
        } => {
            assert_eq!(incoming, "");
            assert!(preview.sections.is_empty());
        }
        other => panic!("{other:?}"),
    }

    // Disabled: commands are unavailable; enabled again it runs.
    let disabled = plugin(
        f.service
            .handle(Request::DisablePlugin {
                plugin_id: id.clone(),
            })
            .await,
    );
    assert_eq!(disabled.state, PluginRunState::Disabled);
    assert_eq!(
        error(
            f.service
                .handle(Request::RunPluginCommand {
                    command: "plugin.follow-up.follow-up".into(),
                    surface: PluginCommandSurface::List,
                    context: selection(&ids[..1]),
                    client_id: None,
                })
                .await
        )
        .0,
        ErrorCode::Unavailable
    );
    let enabled = plugin(
        f.service
            .handle(Request::EnablePlugin {
                plugin_id: id.clone(),
            })
            .await,
    );
    assert_eq!(enabled.state, PluginRunState::Running);

    // Uninstalled: stopped, rows and package gone.
    assert_eq!(
        f.service
            .handle(Request::UninstallPlugin {
                plugin_id: id.clone(),
            })
            .await,
        Response::Done
    );
    assert!(!f.data.path().join("plugins/follow-up.nailplugin").exists());
    assert_eq!(
        error(f.service.handle(Request::GetPlugin { plugin_id: id }).await).0,
        ErrorCode::NotFound
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bodies_need_mail_read_body_and_encrypted_bodies_are_never_given() {
    let f = setup();
    let mut events = f.service.subscribe();
    let headers_only = install(&f, "headers", &["mail.read"], FOLLOW_UP_JS).await;
    assert_eq!(headers_only.state, PluginRunState::Running);
    let ids = inbox_messages(&f).await;
    let run = |plugin: &str, ids: Vec<MessageId>| Request::RunPluginCommand {
        command: format!("plugin.{plugin}.body"),
        surface: PluginCommandSurface::List,
        context: selection(&ids),
        client_id: None,
    };
    assert_eq!(
        f.service.handle(run("headers", ids[..1].to_vec())).await,
        Response::Done
    );
    // The refusal is logged by the host (source api) and reaches the
    // plug-in as `forbidden`.
    wait_event(&mut events, "the refusal", |e| match e {
        Event::PluginLogLine { entry }
            if entry.plugin_id.as_str() == "headers"
                && entry
                    .localized
                    .as_ref()
                    .is_some_and(|l| l.id == "plugin-host-api-denied") =>
        {
            Some(())
        }
        _ => None,
    })
    .await;
    wait_event(&mut events, "the plug-in's answer", |e| match e {
        Event::PluginLogLine { entry } if entry.message == "DENIED forbidden" => Some(()),
        _ => None,
    })
    .await;

    install(&f, "bodies", &["mail.read", "mail.read.body"], FOLLOW_UP_JS).await;
    f.service.handle(run("bodies", ids[..1].to_vec())).await;
    wait_event(&mut events, "the body", |e| match e {
        Event::PluginLogLine { entry } if entry.message.starts_with("BODY ") => Some(()),
        _ => None,
    })
    .await;

    // An end-to-end encrypted message (opened from a file): never its body.
    let eml = "From: a@example.com\r\nTo: b@example.com\r\nSubject: secret\r\nMIME-Version: 1.0\r\nContent-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n--b\r\nContent-Type: application/octet-stream\r\n\r\n-----BEGIN PGP MESSAGE-----\r\n\r\nhQEMA0000000000000AQf/abc=\r\n-----END PGP MESSAGE-----\r\n--b--\r\n";
    let path = write(f.files.path(), "secret.eml", eml.as_bytes());
    let encrypted = match f
        .service
        .handle(Request::ImportMessageFile {
            path: path.display().to_string(),
        })
        .await
    {
        Response::MessageImported { message } => message.id,
        other => panic!("{other:?}"),
    };
    f.service.handle(run("bodies", vec![encrypted])).await;
    wait_event(
        &mut events,
        "the refusal of the encrypted body",
        |e| match e {
            Event::PluginLogLine { entry }
                if entry.plugin_id.as_str() == "bodies" && entry.message == "DENIED forbidden" =>
            {
                Some(())
            }
            _ => None,
        },
    )
    .await;
}

const RUNAWAY_JS: &str = r#"
export default {
  activate(ctx) {
    ctx.commands.register("follow-up", () => { for (;;) {} });
  },
};
"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_runaway_plugin_is_stopped_and_flagged_while_the_core_answers() {
    let f = setup();
    let mut events = f.service.subscribe();
    install(&f, "runaway", &[], RUNAWAY_JS).await;
    let started = Instant::now();
    let answered = f
        .service
        .handle(Request::RunPluginCommand {
            command: "plugin.runaway.follow-up".into(),
            surface: PluginCommandSurface::Global,
            context: UiContext::default(),
            client_id: Some("ui-1".into()),
        })
        .await;
    assert_eq!(answered, Response::Done);
    assert!(started.elapsed() < Duration::from_millis(500));
    // The core keeps answering while the plug-in loops.
    assert!(matches!(
        f.service.handle(Request::ListAccounts).await,
        Response::Accounts { .. }
    ));
    let (reason, localized) = wait_event(&mut events, "the stop", |e| match e {
        Event::PluginStopped {
            plugin_id,
            reason,
            localized,
            ..
        } if plugin_id.as_str() == "runaway" => Some((*reason, localized.clone())),
        _ => None,
    })
    .await;
    assert_eq!(reason, PluginStopReason::CpuBudget);
    let localized = localized.unwrap();
    assert_eq!(localized.id, "plugin-stopped-cpu-budget");
    assert_eq!(
        localized.args["plugin"],
        MessageArg::from("Follow-up reminders")
    );
    let info = get(&f, "runaway").await;
    assert_eq!(info.state, PluginRunState::Crashed);
    assert_eq!(
        info.localized.map(|l| l.id).as_deref(),
        Some("plugin-stopped-cpu-budget")
    );
    // The crash is recorded in the store for the next start.
    let id = PluginId::new("runaway");
    wait_until("the stored crash", || {
        let store = f.service.store().clone();
        let id = id.clone();
        async move { store.plugin(&id).unwrap().last_error.is_some() }
    })
    .await;
    // Enabling again restarts it.
    let restarted = plugin(
        f.service
            .handle(Request::EnablePlugin { plugin_id: id })
            .await,
    );
    assert_eq!(restarted.state, PluginRunState::Running);
    assert_eq!(restarted.error, None);
}

const DEV_JS_1: &str = r#"
export default { activate() { nail.log.info("dev one"); } };
"#;
const DEV_JS_2: &str = r#"
export default { activate() { nail.log.info("dev two"); } };
"#;

#[tokio::test(flavor = "multi_thread")]
async fn dev_folders_load_and_reload() {
    let f = setup();
    let folder = f.files.path().join("dev-plugin");
    std::fs::create_dir_all(folder.join("locales")).unwrap();
    let write_manifest = |version: &str| {
        std::fs::write(
            folder.join("manifest.json"),
            manifest("devplug", version, &["storage"]).to_string(),
        )
        .unwrap();
    };
    write_manifest("1.0.0");
    std::fs::write(folder.join("index.js"), DEV_JS_1).unwrap();
    std::fs::write(folder.join("locales/en.ftl"), EN_FTL).unwrap();

    assert_eq!(
        error(
            f.service
                .handle(Request::LoadDevPlugin {
                    folder: "relative/folder".into(),
                    grants: vec![],
                })
                .await
        )
        .0,
        ErrorCode::InvalidRequest
    );
    let loaded = plugin(
        f.service
            .handle(Request::LoadDevPlugin {
                folder: folder.display().to_string(),
                grants: all(&[PluginPermission::Storage]),
            })
            .await,
    );
    assert_eq!(loaded.state, PluginRunState::Running);
    assert!(matches!(loaded.source, PluginPackageSource::Dev { .. }));
    assert_eq!(loaded.manifest.name, "Follow-up reminders");

    // A change reloads it; a new manifest version reaches the store. The
    // host's log line names the plug-in by its resolved name, not by the
    // manifest's `%name%` (post-M2 verify finding). Waits are on the
    // host's reload notices and the stored state (post-M4c: the event
    // stream with a 20 s bound flaked under load).
    let mut reloads = f.service.subscribe_plugin_reloads();
    std::fs::write(folder.join("index.js"), DEV_JS_2).unwrap();
    write_manifest("1.0.1");
    wait_reloaded(&f, &mut reloads, "the reload to 1.0.1", |plugin, logs| {
        plugin.manifest.version == "1.0.1"
            && plugin.state == PluginRunState::Running
            && logs.iter().any(|l| l.message == "dev two")
    })
    .await;
    let names = reload_names(&dev_logs(&f).await);
    assert!(!names.is_empty());
    assert!(
        names.iter().all(|n| n == "Follow-up reminders"),
        "resolved names: {names:?}"
    );

    // A change of only the plug-in's locale file (the manifest stays the
    // same) reloads its texts too (post-M2 verify finding).
    let reloads_before = names.len();
    std::fs::write(
        folder.join("locales/en.ftl"),
        "name = Follow-up reminders, edited\nfollow-up = Follow up…\n",
    )
    .unwrap();
    wait_reloaded(
        &f,
        &mut reloads,
        "the edited plug-in name",
        |plugin, logs| {
            plugin.manifest.name == "Follow-up reminders, edited"
                && plugin.state == PluginRunState::Running
                && reload_names(logs).len() > reloads_before
        },
    )
    .await;
    let names = reload_names(&dev_logs(&f).await);
    assert!(
        names.iter().all(|n| n.starts_with("Follow-up reminders")),
        "resolved names: {names:?}"
    );
    assert_eq!(get(&f, "devplug").await.manifest.version, "1.0.1");
}

/// The bound of the dev reload waits: only there so a broken watcher fails
/// instead of hanging; generous because the full workspace suite loads the
/// machine.
const RELOAD_WAIT: Duration = Duration::from_mins(2);

/// The stored log of the dev plug-in.
async fn dev_logs(f: &Fixture) -> Vec<PluginLogEntry> {
    match f
        .service
        .handle(Request::ListPluginLogs {
            plugin_id: Some(PluginId::new("devplug")),
            after_seq: None,
            limit: 1000,
        })
        .await
    {
        Response::PluginLogs { entries } => entries,
        other => panic!("{other:?}"),
    }
}

/// Waits until the stored plug-in and its stored log satisfy `check`;
/// woken by the host's reload notices (the store follows a reload a moment
/// later, so it also rechecks every 100 ms).
async fn wait_reloaded(
    f: &Fixture,
    reloads: &mut broadcast::Receiver<nail_plugins::PluginReload>,
    what: &str,
    check: impl Fn(&PluginInfo, &[PluginLogEntry]) -> bool,
) {
    let deadline = Instant::now() + RELOAD_WAIT;
    loop {
        let plugin = get(f, "devplug").await;
        let logs = dev_logs(f).await;
        if check(&plugin, &logs) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{what} did not happen in {RELOAD_WAIT:?}: {plugin:?} {logs:#?}"
        );
        drop(tokio::time::timeout(Duration::from_millis(100), reloads.recv()).await);
    }
}

/// The plug-in names of the host's "stopped for a reload" lines.
fn reload_names(logs: &[PluginLogEntry]) -> Vec<String> {
    logs.iter()
        .filter_map(|entry| match entry.localized.as_deref() {
            Some(m) if m.id == "plugin-stopped-reloaded" => match m.args.get("plugin") {
                Some(MessageArg::Text(name)) => Some(name.clone()),
                other => panic!("plug-in argument {other:?}"),
            },
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn http_fetch_goes_through_the_core() {
    let f = setup();
    let upstream = nail_proxy::testing::Upstream::start().await.unwrap();
    upstream.route(
        "/data",
        nail_proxy::testing::Reply::ok("application/json", "{\"answer\":42}"),
    );
    let js = format!(
        r#"
export default {{
  activate(ctx) {{
    ctx.commands.register("follow-up", async () => {{
      const res = await nail.http.fetch("http://127.0.0.1:{port}/data");
      nail.log.info("HTTP " + res.status + " " + res.body);
      try {{
        await nail.http.fetch("https://elsewhere.example.org/");
      }} catch (e) {{
        nail.log.info("REFUSED " + e.code);
      }}
    }});
  }},
}};
"#,
        port = upstream.port()
    );
    let mut events = f.service.subscribe();
    install(&f, "fetcher", &["network:127.0.0.1"], &js).await;
    f.service
        .handle(Request::RunPluginCommand {
            command: "plugin.fetcher.follow-up".into(),
            surface: PluginCommandSurface::Global,
            context: UiContext::default(),
            client_id: None,
        })
        .await;
    wait_event(&mut events, "the answer", |e| match e {
        Event::PluginLogLine { entry } if entry.message == "HTTP 200 {\"answer\":42}" => Some(()),
        _ => None,
    })
    .await;
    wait_event(&mut events, "the refusal", |e| match e {
        Event::PluginLogLine { entry } if entry.message == "REFUSED forbidden" => Some(()),
        _ => None,
    })
    .await;
    assert_eq!(upstream.requests().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn enabled_plugins_start_with_the_core_and_tampered_packages_are_refused() {
    let f = setup();
    install(&f, "keeper", &["storage"], DEV_JS_1).await;
    install(&f, "tampered", &["storage"], DEV_JS_1).await;
    let Fixture {
        service,
        data,
        config,
        files: _files,
    } = f;
    service.shutdown().await;
    drop(service);
    // Changed after the approval: refused at the next start.
    let path = data.path().join("plugins/tampered.nailplugin");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(b"extra");
    std::fs::write(&path, bytes).unwrap();

    let mut opts = options(data.path(), config.path());
    opts.start_engines = true;
    let service = Service::open(opts).unwrap();
    let f = Fixture {
        service,
        data,
        config,
        files: tempfile::tempdir().unwrap(),
    };
    wait_until("the kept plug-in running", || {
        let service = f.service.clone();
        async move {
            matches!(
                service.handle(Request::GetPlugin { plugin_id: PluginId::new("keeper") }).await,
                Response::Plugin { plugin } if plugin.state == PluginRunState::Running
            )
        }
    })
    .await;
    wait_until("the tampered plug-in flagged", || {
        let service = f.service.clone();
        async move {
            matches!(
                service.handle(Request::GetPlugin { plugin_id: PluginId::new("tampered") }).await,
                Response::Plugin { plugin } if plugin.state == PluginRunState::Crashed
                    && plugin.localized.as_ref().is_some_and(|l| l.id == "plugin-package-hash-mismatch")
            )
        }
    })
    .await;
    f.service.shutdown().await;
}

const SYSTEM_JS: &str = r#"
export default {
  activate(ctx) {
    ctx.commands.register("follow-up", () => {
      nail.ui.notify({ title: "Due today", body: "Call Ada", level: "info", system: true });
    });
  },
};
"#;

#[tokio::test(flavor = "multi_thread")]
async fn system_notifications_follow_the_notification_settings() {
    let f = setup();
    let mut events = f.service.subscribe();
    install(&f, "notifier", &["notifications"], SYSTEM_JS).await;
    let run = || Request::RunPluginCommand {
        command: "plugin.notifier.follow-up".into(),
        surface: PluginCommandSurface::Global,
        context: UiContext::default(),
        client_id: None,
    };
    f.service.handle(run()).await;
    let shown = wait_event(&mut events, "a notification", |e| match e {
        Event::PluginNotification { notification } => Some(notification.clone()),
        _ => None,
    })
    .await;
    assert!(shown.spec.system);
    // The OS notification names the plug-in first, in its title; the
    // plug-in's line breaks cannot push that out of view (M2d review).
    let (title, body) = f.service.plugin_notification_texts(&shown);
    assert_eq!(title, "Follow-up reminders: Due today");
    assert_eq!(body, "Call Ada");
    let mut spoof = shown.clone();
    spoof.spec.title = "Your session expired\n\n\n".into();
    spoof.spec.body = Some("Sign in again\n\n\n\n\nreally".into());
    let (title, body) = f.service.plugin_notification_texts(&spoof);
    assert!(title.starts_with("Follow-up reminders: "), "{title}");
    assert!(
        !title.contains('\n') && !body.contains('\n'),
        "{title} / {body}"
    );
    assert_eq!(body, "Sign in again really");

    // Notifications switched off: the toast stays, the OS one goes.
    let mut settings = f.service.store().notification_settings().unwrap();
    settings.enabled = false;
    f.service
        .store()
        .set_notification_settings(&settings)
        .unwrap();
    f.service.handle(run()).await;
    let quiet = wait_event(&mut events, "a second notification", |e| match e {
        Event::PluginNotification { notification } if notification.id != shown.id => {
            Some(notification.clone())
        }
        _ => None,
    })
    .await;
    assert!(!quiet.spec.system);
}

const SEARCH_JS: &str = r#"
export default {
  activate(ctx) {
    ctx.commands.register("follow-up", async () => {
      const find = async (q) => {
        try {
          return (await nail.mail.search(q, { limit: 500 })).map((m) => m.subject);
        } catch (e) {
          return "ERR " + e.code;
        }
      };
      nail.log.info("OUT " + JSON.stringify({
        body: await find('body:aboard'),
        filename: await find('filename:pdf'),
        free: await find('aboard'),
        subject: await find('Welcome'),
        secret: await find('subject:secret'),
      }));
    });
  },
};
"#;

/// Runs the search command of `plugin` and returns what it found.
async fn plugin_searches(
    f: &Fixture,
    events: &mut broadcast::Receiver<Event>,
    plugin: &str,
) -> Value {
    f.service
        .handle(Request::RunPluginCommand {
            command: format!("plugin.{plugin}.follow-up"),
            surface: PluginCommandSurface::Global,
            context: UiContext::default(),
            client_id: None,
        })
        .await;
    let text = wait_event(events, "the search results", |e| match e {
        Event::PluginLogLine { entry }
            if entry.plugin_id.as_str() == plugin && entry.message.starts_with("OUT ") =>
        {
            Some(entry.message[4..].to_owned())
        }
        _ => None,
    })
    .await;
    serde_json::from_str(&text).unwrap()
}

fn subjects(found: &Value) -> Vec<String> {
    found
        .as_array()
        .unwrap_or_else(|| panic!("not a list: {found}"))
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect()
}

/// M2d review: `mail.read` alone must not let a plug-in probe message
/// text through search, and encrypted mail is never found while the index
/// holds its decrypted text.
#[tokio::test(flavor = "multi_thread")]
async fn search_needs_mail_read_body_for_text_and_hides_decrypted_mail() {
    let f = setup();
    let mut events = f.service.subscribe();
    assert!(f.service.rebuild_search_index().unwrap() > 0);
    let eml = "From: a@example.com\r\nTo: b@example.com\r\nSubject: secret\r\nMIME-Version: 1.0\r\nContent-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n--b\r\nContent-Type: application/octet-stream\r\n\r\n-----BEGIN PGP MESSAGE-----\r\n\r\nhQEMA0000000000000AQf/abc=\r\n-----END PGP MESSAGE-----\r\n--b--\r\n";
    let path = write(f.files.path(), "secret.eml", eml.as_bytes());
    match f
        .service
        .handle(Request::ImportMessageFile {
            path: path.display().to_string(),
        })
        .await
    {
        Response::MessageImported { .. } => {}
        other => panic!("{other:?}"),
    }
    install(&f, "headers", &["mail.read"], SEARCH_JS).await;
    install(&f, "bodies", &["mail.read", "mail.read.body"], SEARCH_JS).await;

    let headers = plugin_searches(&f, &mut events, "headers").await;
    // Message text needs `mail.read.body`: refused, and free text looks at
    // header fields only ("aboard" is only in the welcome mail's body).
    assert_eq!(headers["body"], "ERR forbidden", "{headers}");
    assert_eq!(headers["filename"], "ERR forbidden", "{headers}");
    assert_eq!(subjects(&headers["free"]), Vec::<String>::new());
    assert!(subjects(&headers["subject"]).contains(&"Welcome to Nail".to_owned()));
    // No decrypted text in the index: encrypted mail is found by headers.
    assert_eq!(subjects(&headers["secret"]), ["secret"]);

    let bodies = plugin_searches(&f, &mut events, "bodies").await;
    assert_eq!(subjects(&bodies["body"]), ["Welcome to Nail"], "{bodies}");
    assert_eq!(subjects(&bodies["free"]), ["Welcome to Nail"]);
    assert_eq!(subjects(&bodies["secret"]), ["secret"]);

    // Decrypted indexing on (any account): encrypted mail is never found.
    match f
        .service
        .handle(Request::UpdateSecuritySettings {
            settings: SecuritySettings {
                index_decrypted: vec![nail_core::demo::DEMO_ACCOUNT_ID.into()],
                ..SecuritySettings::default()
            },
        })
        .await
    {
        Response::SecuritySettings { .. } => {}
        other => panic!("{other:?}"),
    }
    for plugin in ["headers", "bodies"] {
        let found = plugin_searches(&f, &mut events, plugin).await;
        assert_eq!(subjects(&found["secret"]), Vec::<String>::new(), "{plugin}");
        assert!(subjects(&found["subject"]).contains(&"Welcome to Nail".to_owned()));
    }
}

const CALENDAR_JS: &str = r#"
export default {
  activate(ctx) {
    ctx.commands.register("body", async () => {
      try {
        const events = await nail.calendar.events("2026-10-01T00:00:00Z", "2026-10-31T00:00:00Z");
        nail.log.info("EVENTS " + events.map((e) => e.title + "@" + e.start).join(","));
      } catch (e) {
        nail.log.warn("DENIED " + e.code);
      }
    });
  },
};
"#;

/// M3a (ADR 0063): `calendar.read` is real — the occurrences of the
/// user's calendars; without it the call is refused.
#[tokio::test(flavor = "multi_thread")]
async fn calendar_read_gives_event_occurrences() {
    let f = setup();
    let mut events = f.service.subscribe();
    let calendar = match f
        .service
        .handle(Request::ListCalendars { source_id: None })
        .await
    {
        Response::Calendars { calendars } => {
            calendars
                .into_iter()
                .find(nail_api::Calendar::has_events)
                .unwrap()
                .id
        }
        other => panic!("{other:?}"),
    };
    let mut rule = nail_api::pim::RecurrenceRule::new(nail_api::pim::calendar::Frequency::Weekly);
    rule.end = nail_api::pim::RecurrenceEnd::Count { count: 2 };
    let created = f
        .service
        .handle(Request::CreateEvent {
            calendar_id: calendar,
            fields: Box::new(nail_api::pim::EventFields::new(
                "Yoga",
                nail_api::pim::PimTime::utc("2026-10-06T17:00:00"),
                nail_api::pim::PimTime::utc("2026-10-06T18:00:00"),
            )),
            recurrence: Some(nail_api::pim::Recurrence::rule(rule)),
        })
        .await;
    assert!(
        matches!(created, Response::EventSaved { .. }),
        "{created:?}"
    );
    let run = |plugin: &str| Request::RunPluginCommand {
        command: format!("plugin.{plugin}.body"),
        surface: PluginCommandSurface::List,
        context: UiContext::default(),
        client_id: None,
    };
    install(&f, "cal", &["calendar.read"], CALENDAR_JS).await;
    assert_eq!(f.service.handle(run("cal")).await, Response::Done);
    let line = wait_event(&mut events, "the events", |e| match e {
        Event::PluginLogLine { entry } if entry.message.starts_with("EVENTS ") => {
            Some(entry.message.clone())
        }
        _ => None,
    })
    .await;
    assert_eq!(
        line,
        "EVENTS Yoga@2026-10-06T17:00:00Z,Yoga@2026-10-13T17:00:00Z"
    );
    install(&f, "nocal", &["storage"], CALENDAR_JS).await;
    f.service.handle(run("nocal")).await;
    wait_event(&mut events, "the refusal", |e| match e {
        Event::PluginLogLine { entry }
            if entry.plugin_id.as_str() == "nocal" && entry.message == "DENIED forbidden" =>
        {
            Some(())
        }
        _ => None,
    })
    .await;
}
