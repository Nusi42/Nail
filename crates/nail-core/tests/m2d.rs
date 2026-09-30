//! Wave M2d contract stage (ADR 0050): the store-backed plug-in
//! administration served by the core before the plug-in host is wired in
//! (listing, enabling, grants, settings, logs, uninstalling), and the
//! requests that answer `unsupported` until WP64.

use std::collections::BTreeMap;
use std::time::Duration;

use nail_api::plugins::admin::PluginRunState;
use nail_api::plugins::{
    PluginCommandSurface, PluginGrant, PluginLogLevel, PluginLogSource, PluginManifest,
    PluginPackageSource, PluginPermission,
};
use nail_api::{ApiError, ErrorCode, Event, PluginId, Request, Response, UiContext};
use nail_core::{Service, ServiceOptions};
use nail_store::{PluginInstall, PluginLogLine};
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::sync::broadcast;

struct Fixture {
    service: Service,
    _data: tempfile::TempDir,
    _config: tempfile::TempDir,
}

fn setup() -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    options.start_engines = false;
    let service = Service::open(nail_core::demo::demo_options(options)).unwrap();
    Fixture {
        service,
        _data: data,
        _config: config,
    }
}

fn manifest() -> PluginManifest {
    PluginManifest::parse(
        json!({
            "id": "follow-up", "name": "Follow-up", "version": "1.0.0", "apiVersion": "1",
            "main": "dist/index.js", "permissions": ["mail.read", "storage", "ui.dialogs"],
            "contributes": {
                "commands": [{"id": "remind", "title": "Remind me", "contexts": ["reader"]}],
                "settings": [{"type": "number", "key": "days", "title": "Days", "default": 1, "min": 0}]
            }
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap()
}

fn id() -> PluginId {
    PluginId::new("follow-up")
}

/// Installs the plug-in directly in the store (installing through the API
/// needs the host).
fn install(f: &Fixture) {
    let m = manifest();
    f.service
        .store()
        .install_plugin(&PluginInstall {
            manifest: &m,
            source: &PluginPackageSource::File {
                path: "C:/x/follow-up.nailplugin".into(),
            },
            package_sha256: Some(&"a".repeat(64)),
            grants: &[PluginGrant::all(PluginPermission::Storage)],
            enabled: false,
        })
        .unwrap();
}

fn error(response: Response) -> (ErrorCode, Option<String>) {
    match response {
        Response::Error {
            code, localized, ..
        } => (code, localized.map(|l| l.id)),
        other => panic!("expected an error, got {other:?}"),
    }
}

async fn next_event(rx: &mut broadcast::Receiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("no event within 10 s")
        .unwrap()
}

#[tokio::test]
async fn plugins_are_listed_enabled_and_uninstalled() {
    let f = setup();
    assert_eq!(
        f.service.handle(Request::ListPlugins).await,
        Response::Plugins { plugins: vec![] }
    );
    install(&f);
    let plugins = match f.service.handle(Request::ListPlugins).await {
        Response::Plugins { plugins } => plugins,
        other => panic!("{other:?}"),
    };
    assert_eq!(plugins.len(), 1);
    let plugin = &plugins[0];
    assert_eq!(plugin.id, id());
    assert_eq!(plugin.state, PluginRunState::Disabled);
    assert_eq!(plugin.grants, [PluginGrant::all(PluginPermission::Storage)]);
    assert_eq!(plugin.missing_grants().len(), 2);
    assert_eq!(plugin.storage.quota_bytes, 5 * 1024 * 1024);

    let mut events = f.service.subscribe();
    let enabled = f
        .service
        .handle(Request::EnablePlugin { plugin_id: id() })
        .await;
    match enabled {
        Response::Plugin { plugin } => {
            assert!(plugin.enabled);
            // No host yet: enabled but not running.
            assert_eq!(plugin.state, PluginRunState::Stopped);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(next_event(&mut events).await, Event::PluginsChanged);

    // A recorded crash shows as `crashed` until the plug-in is enabled again.
    f.service
        .store()
        .set_plugin_error(&id(), Some(&ApiError::internal("CPU budget")))
        .unwrap();
    match f
        .service
        .handle(Request::GetPlugin { plugin_id: id() })
        .await
    {
        Response::Plugin { plugin } => {
            assert_eq!(plugin.state, PluginRunState::Crashed);
            assert_eq!(plugin.error.as_deref(), Some("CPU budget"));
        }
        other => panic!("{other:?}"),
    }
    match f
        .service
        .handle(Request::DisablePlugin { plugin_id: id() })
        .await
    {
        Response::Plugin { plugin } => assert_eq!(plugin.state, PluginRunState::Disabled),
        other => panic!("{other:?}"),
    }
    assert_eq!(next_event(&mut events).await, Event::PluginsChanged);

    assert_eq!(
        f.service
            .handle(Request::UninstallPlugin { plugin_id: id() })
            .await,
        Response::Done
    );
    assert_eq!(next_event(&mut events).await, Event::PluginsChanged);
    assert_eq!(
        error(
            f.service
                .handle(Request::GetPlugin { plugin_id: id() })
                .await
        )
        .0,
        ErrorCode::NotFound
    );
    assert_eq!(
        error(
            f.service
                .handle(Request::EnablePlugin { plugin_id: id() })
                .await
        )
        .0,
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn grants_must_be_declared() {
    let f = setup();
    install(&f);
    let grants = vec![
        PluginGrant {
            permission: PluginPermission::MailRead,
            accounts: Some(vec!["work".into()]),
        },
        PluginGrant::all(PluginPermission::UiDialogs),
    ];
    match f
        .service
        .handle(Request::UpdatePluginGrants {
            plugin_id: id(),
            grants: grants.clone(),
        })
        .await
    {
        Response::Plugin { plugin } => assert_eq!(plugin.grants, grants),
        other => panic!("{other:?}"),
    }
    let undeclared = f
        .service
        .handle(Request::UpdatePluginGrants {
            plugin_id: id(),
            grants: vec![PluginGrant::all(PluginPermission::Notifications)],
        })
        .await;
    assert_eq!(error(undeclared).0, ErrorCode::InvalidRequest);
}

#[tokio::test]
async fn settings_are_validated() {
    let f = setup();
    install(&f);
    let expected = |days: i64| Response::PluginSettings {
        plugin_id: id(),
        schema: manifest().contributes.settings,
        values: BTreeMap::from([("days".to_owned(), json!(days))]),
    };
    assert_eq!(
        f.service
            .handle(Request::GetPluginSettings { plugin_id: id() })
            .await,
        expected(1)
    );
    assert_eq!(
        f.service
            .handle(Request::UpdatePluginSettings {
                plugin_id: id(),
                values: BTreeMap::from([("days".to_owned(), json!(7))]),
            })
            .await,
        expected(7)
    );
    let bad = f
        .service
        .handle(Request::UpdatePluginSettings {
            plugin_id: id(),
            values: BTreeMap::from([("days".to_owned(), json!("soon"))]),
        })
        .await;
    assert_eq!(
        error(bad),
        (
            ErrorCode::InvalidRequest,
            Some("plugin-settings-invalid".to_owned())
        )
    );
}

#[tokio::test]
async fn logs_are_listed_and_cleared() {
    let f = setup();
    install(&f);
    for (i, level) in [PluginLogLevel::Info, PluginLogLevel::Error]
        .into_iter()
        .enumerate()
    {
        f.service
            .store()
            .append_plugin_log(&PluginLogLine {
                plugin_id: &id(),
                at_ms: 1_790_000_000_000 + i as i64,
                level,
                source: PluginLogSource::Plugin,
                message: &format!("line {i}"),
                localized: None,
            })
            .unwrap();
    }
    let entries = match f
        .service
        .handle(Request::ListPluginLogs {
            plugin_id: Some(id()),
            after_seq: None,
            limit: 0,
        })
        .await
    {
        Response::PluginLogs { entries } => entries,
        other => panic!("{other:?}"),
    };
    // A limit of 0 is raised to 1: the newest line.
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].message, "line 1");
    let all = f
        .service
        .handle(Request::ListPluginLogs {
            plugin_id: None,
            after_seq: None,
            limit: 100,
        })
        .await;
    assert!(matches!(all, Response::PluginLogs { ref entries } if entries.len() == 2));
    assert_eq!(
        f.service
            .handle(Request::ClearPluginLogs { plugin_id: id() })
            .await,
        Response::Done
    );
    assert_eq!(
        error(
            f.service
                .handle(Request::ListPluginLogs {
                    plugin_id: Some(PluginId::new("nope")),
                    after_seq: None,
                    limit: 10,
                })
                .await
        )
        .0,
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn host_requests_are_unsupported_until_the_integration() {
    let f = setup();
    install(&f);
    for request in [
        Request::PreviewPluginInstall {
            source: PluginPackageSource::File {
                path: "C:/x.nailplugin".into(),
            },
        },
        Request::LoadDevPlugin {
            folder: "C:/dev/x".into(),
            grants: vec![],
        },
        Request::PreviewPluginPlacements { plugin_id: id() },
        Request::RunPluginCommand {
            command: "plugin.follow-up.remind".into(),
            surface: PluginCommandSurface::Reader,
            context: UiContext::default(),
            client_id: Some("ui-1".into()),
        },
        Request::GetPluginPanel {
            panel: "follow-up.due".into(),
            context: UiContext::default(),
            client_id: None,
        },
    ] {
        assert_eq!(
            error(f.service.handle(request).await).0,
            ErrorCode::Unsupported
        );
    }
}
