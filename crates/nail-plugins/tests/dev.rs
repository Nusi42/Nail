//! Dev mode: loading a folder, hot reload on change, API calls in the log
//! stream.

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use common::*;
use nail_api::UiContext;
use nail_api::plugins::{PluginCommandSurface, PluginLogSource, PluginRunState, PluginStopReason};
use nail_plugins::package::PackageErrorKind;
use nail_plugins::{DevLoadError, PluginReload, ReloadOutcome};
use tokio::sync::broadcast;

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

/// The bound of every wait below: only there so a broken watcher fails
/// instead of hanging; generous because the full workspace suite loads the
/// machine (post-M4c: 10 and 20 s bounds flaked).
const WAIT: Duration = Duration::from_secs(120);

/// A host with activation and reply timeouts that hold under load.
fn patient_host(fake: &std::sync::Arc<Fake>) -> nail_plugins::PluginHost {
    host_with(
        fake,
        nail_plugins::HostLimits {
            activation_timeout: WAIT,
            reply_timeout: WAIT,
            ..limits()
        },
    )
}

/// Waits for the next reload notice of `devplug` with `outcome`; other
/// notices (a burst of changes may reload twice) are skipped.
async fn reloaded(
    reloads: &mut broadcast::Receiver<PluginReload>,
    outcome: ReloadOutcome,
) -> PluginReload {
    let deadline = Instant::now() + WAIT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, reloads.recv()).await {
            Ok(Ok(notice))
                if notice.plugin_id.as_str() == "devplug" && notice.outcome == outcome =>
            {
                return notice;
            }
            Ok(Ok(_) | Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(e)) => panic!("reload notices: {e}"),
            Err(_) => panic!("no {outcome:?} reload within {WAIT:?}"),
        }
    }
}

/// Waits (woken by reload notices, rechecked every 50 ms) until `check`
/// holds.
async fn until<T>(
    reloads: &mut broadcast::Receiver<PluginReload>,
    what: &str,
    mut check: impl FnMut() -> Option<T>,
) -> T {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(found) = check() {
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "{what} did not happen in {WAIT:?}"
        );
        let _ = tokio::time::timeout(Duration::from_millis(50), reloads.recv()).await;
    }
}

/// The real watcher: file changes reload the plug-in. Every wait is on the
/// host's reload notices or its state (no fixed sleeps, no short bounds).
#[tokio::test(flavor = "multi_thread")]
async fn dev_folders_load_log_api_calls_and_hot_reload() {
    let fake = Fake::new();
    let host = patient_host(&fake);
    let mut logs = host.subscribe_logs();
    let mut reloads = host.subscribe_reloads();
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

    // Changing a file reloads the plug-in with the new code and manifest
    // (the watcher is armed when `load_dev` returns).
    write_plugin(&folder, "0.2.0", &version_js("two"));
    until(&mut reloads, "the reload to 0.2.0", || {
        let id = plugin_id("devplug");
        (host.state(&id) == Some(PluginRunState::Running)
            && host.manifest(&id).is_some_and(|m| m.version == "0.2.0")
            && !fake.results("devplug", "VERSION two").is_empty())
        .then_some(())
    })
    .await;
    assert!(stop_reasons(&fake, "devplug").contains(&PluginStopReason::Reloaded));
    assert!(fake.log_lines("devplug").iter().any(|l| {
        l.localized
            .as_ref()
            .is_some_and(|m| m.id == "plugin-host-reloading")
    }));

    // A broken manifest is reported and the plug-in stays stopped until
    // it is fixed.
    std::fs::write(folder.join("manifest.json"), "{ not json").unwrap();
    reloaded(&mut reloads, ReloadOutcome::Invalid).await;
    assert!(fake.log_lines("devplug").iter().any(|l| {
        l.localized
            .as_ref()
            .is_some_and(|m| m.id == "plugin-host-reload-invalid")
    }));
    assert_eq!(
        host.state(&plugin_id("devplug")),
        Some(PluginRunState::Stopped)
    );
    write_plugin(&folder, "0.3.0", &version_js("three"));
    until(&mut reloads, "the reload to 0.3.0", || {
        (host.state(&plugin_id("devplug")) == Some(PluginRunState::Running)
            && !fake.results("devplug", "VERSION three").is_empty())
        .then_some(())
    })
    .await;

    // A disabled dev plug-in is not brought back by changes: the change
    // is seen and skipped.
    host.stop(&plugin_id("devplug"), PluginStopReason::Disabled);
    write_plugin(&folder, "0.4.0", &version_js("four"));
    reloaded(&mut reloads, ReloadOutcome::SkippedDisabled).await;
    assert!(fake.results("devplug", "VERSION four").is_empty());
    assert_eq!(
        host.state(&plugin_id("devplug")),
        Some(PluginRunState::Disabled)
    );
    host.stop(&plugin_id("devplug"), PluginStopReason::Uninstalled);
}

/// Reloads through the host API: deterministic, no watcher timing.
/// Concurrent reloads (the API and the watcher at once) run one after the
/// other and keep the folder watched.
#[tokio::test(flavor = "multi_thread")]
async fn dev_reloads_through_the_api_run_one_at_a_time() {
    let fake = Fake::new();
    let host = patient_host(&fake);
    let mut reloads = host.subscribe_reloads();
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("plugin");
    write_plugin(&folder, "0.1.0", &version_js("one"));
    host.load_dev(
        &folder,
        all_grants(&manifest("devplug", &["storage"])),
        BTreeMap::new(),
    )
    .await
    .unwrap();
    let id = plugin_id("devplug");
    let before = host.generation(&id).unwrap();

    write_plugin(&folder, "0.2.0", &version_js("two"));
    host.reload(&id).await.unwrap();
    // `reload` returns once the new runtime's `activate` settled (the
    // watcher may reload once more on its own; same files, same result).
    assert_eq!(host.manifest(&id).unwrap().version, "0.2.0");
    assert!(!fake.results("devplug", "VERSION two").is_empty());
    let notice = reloaded(&mut reloads, ReloadOutcome::Started).await;
    assert!(notice.generation.unwrap() > before);

    // Several at once (e.g. a bundler writing many files): serialized, the
    // plug-in ends up running with the folder's state.
    write_plugin(&folder, "0.3.0", &version_js("three"));
    let (a, b, c) = tokio::join!(host.reload(&id), host.reload(&id), host.reload(&id));
    a.unwrap();
    b.unwrap();
    c.unwrap();
    assert_eq!(host.manifest(&id).unwrap().version, "0.3.0");
    until(&mut reloads, "running at 0.3.0", || {
        (host.state(&id) == Some(PluginRunState::Running)).then_some(())
    })
    .await;

    // An invalid manifest through the API: an error and a notice.
    std::fs::write(folder.join("manifest.json"), "{ not json").unwrap();
    assert!(host.reload(&id).await.is_err());
    reloaded(&mut reloads, ReloadOutcome::Invalid).await;
    assert_eq!(host.state(&id), Some(PluginRunState::Stopped));

    // The watcher survived the concurrent restarts: a change still reloads.
    write_plugin(&folder, "0.4.0", &version_js("four"));
    until(&mut reloads, "the watcher's reload to 0.4.0", || {
        (host.state(&id) == Some(PluginRunState::Running)
            && host.manifest(&id).is_some_and(|m| m.version == "0.4.0"))
        .then_some(())
    })
    .await;

    // An unknown plug-in fails.
    assert!(host.reload(&plugin_id("nope")).await.is_err());
    host.stop(&id, PluginStopReason::Uninstalled);
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
