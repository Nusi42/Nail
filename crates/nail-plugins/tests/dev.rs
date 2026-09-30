//! Dev mode: loading a folder, hot reload on change, API calls in the log
//! stream.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::*;
use nail_api::UiContext;
use nail_api::plugins::{PluginCommandSurface, PluginLogSource, PluginRunState, PluginStopReason};
use nail_plugins::DevLoadError;
use nail_plugins::package::PackageErrorKind;

fn write_plugin(dir: &Path, version: &str, js: &str) {
    let manifest = serde_json::json!({
        "id": "devplug", "name": "Dev", "version": version, "apiVersion": "1",
        "main": "dist/index.js", "permissions": ["storage"],
        "contributes": {"commands": [{"id": "run", "title": "Run"}]}
    });
    std::fs::create_dir_all(dir.join("dist")).unwrap();
    std::fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
    std::fs::write(dir.join("dist/index.js"), js).unwrap();
}

fn version_js(tag: &str) -> String {
    format!(
        r#"export default {{ activate(ctx) {{
            nail.log.info("VERSION {tag}");
            ctx.commands.register("run", async () => {{ await nail.storage.set("k", 1); nail.log.info("RAN {tag}"); }});
        }} }};"#
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn dev_folders_load_log_api_calls_and_hot_reload() {
    let fake = Fake::new();
    let host = host(&fake);
    let mut logs = host.subscribe_logs();
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("plugin");
    write_plugin(&folder, "0.1.0", &version_js("one"));

    let manifest = host
        .load_dev(
            &folder,
            all_grants(&manifest("devplug", &["storage"])),
            BTreeMap::new(),
        )
        .await
        .unwrap();
    assert_eq!(manifest.version, "0.1.0");
    result(&fake, "devplug", "VERSION one").await;
    let first = logs.recv().await.unwrap();
    assert_eq!(first.plugin_id.as_str(), "devplug");

    // Dev plug-ins log their API calls (the dev console).
    host.run_command(
        "plugin.devplug.run",
        PluginCommandSurface::Global,
        &UiContext::default(),
        None,
    )
    .await
    .unwrap();
    result(&fake, "devplug", "RAN one").await;
    let api: Vec<String> = fake
        .log_lines("devplug")
        .into_iter()
        .filter(|l| l.source == PluginLogSource::Api)
        .map(|l| l.message)
        .collect();
    assert!(
        api.iter().any(|m| m.starts_with("nail.storage.set")),
        "{api:?}"
    );
    assert!(api.iter().any(|m| m == "nail.storage.set -> ok"), "{api:?}");

    // Changing a file reloads the plug-in with the new code and manifest.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    write_plugin(&folder, "0.2.0", &version_js("two"));
    result(&fake, "devplug", "VERSION two").await;
    wait_for("running again", || {
        (host.state(&plugin_id("devplug")) == Some(PluginRunState::Running)).then_some(())
    })
    .await;
    assert_eq!(
        host.manifest(&plugin_id("devplug")).unwrap().version,
        "0.2.0"
    );
    assert!(stop_reasons(&fake, "devplug").contains(&PluginStopReason::Reloaded));
    assert!(fake.log_lines("devplug").iter().any(|l| {
        l.localized
            .as_ref()
            .is_some_and(|m| m.id == "plugin-host-reloading")
    }));

    // A broken manifest is reported and the plug-in stays stopped until
    // it is fixed.
    std::fs::write(folder.join("manifest.json"), "{ not json").unwrap();
    wait_for("invalid reload", || {
        fake.log_lines("devplug").into_iter().find(|l| {
            l.localized
                .as_ref()
                .is_some_and(|m| m.id == "plugin-host-reload-invalid")
        })
    })
    .await;
    wait_for("stopped", || {
        (host.state(&plugin_id("devplug")) == Some(PluginRunState::Stopped)).then_some(())
    })
    .await;
    write_plugin(&folder, "0.3.0", &version_js("three"));
    result(&fake, "devplug", "VERSION three").await;

    // A disabled dev plug-in is not brought back by changes.
    host.stop(&plugin_id("devplug"), PluginStopReason::Disabled);
    write_plugin(&folder, "0.4.0", &version_js("four"));
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert!(fake.results("devplug", "VERSION four").is_empty());
    assert_eq!(
        host.state(&plugin_id("devplug")),
        Some(PluginRunState::Disabled)
    );
    host.stop(&plugin_id("devplug"), PluginStopReason::Uninstalled);
}

#[tokio::test(flavor = "multi_thread")]
async fn dev_loading_reports_folder_and_manifest_problems() {
    let fake = Fake::new();
    let host = host(&fake);
    let dir = tempfile::tempdir().unwrap();
    let err = host
        .load_dev(&dir.path().join("missing"), vec![], BTreeMap::new())
        .await
        .unwrap_err();
    assert!(matches!(&err, DevLoadError::Package(e) if e.kind == PackageErrorKind::Unreadable));
    let err = host
        .load_dev(dir.path(), vec![], BTreeMap::new())
        .await
        .unwrap_err();
    assert!(matches!(&err, DevLoadError::Package(e) if e.kind == PackageErrorKind::NoManifest));
    std::fs::write(dir.path().join("manifest.json"), r#"{"id":"x"}"#).unwrap();
    let err = host
        .load_dev(dir.path(), vec![], BTreeMap::new())
        .await
        .unwrap_err();
    let DevLoadError::Manifest(issues) = &err else {
        panic!("{err:?}")
    };
    assert!(!issues.is_empty());
    assert_eq!(err.to_api_error().code, nail_api::ErrorCode::InvalidRequest);
    assert!(err.to_api_error().localized.is_some());
}
