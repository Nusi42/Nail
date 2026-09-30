//! The sandbox: CPU and memory limits, crash isolation, no host access,
//! module loading, API versions and packages.

mod common;

use std::time::{Duration, Instant};

use common::*;
use nail_api::plugins::{PluginCommandSurface, PluginRunState, PluginStopReason};
use nail_api::{ErrorCode, Event, UiContext};
use nail_plugins::package::{build_package, sha256_hex};
use nail_plugins::{PackageStore, PluginPackage, PluginSpec};

const HEALTHY: &str = r#"
export default { activate(ctx) {
  ctx.commands.register("run", () => nail.log.info("ALIVE"));
} };
"#;

async fn run(host: &nail_plugins::PluginHost, id: &str) -> Result<(), nail_api::ApiError> {
    host.run_command(
        &format!("plugin.{id}.run"),
        PluginCommandSurface::Global,
        &UiContext::default(),
        None,
    )
    .await
}

fn crashed_with(fake: &Fake, host: &nail_plugins::PluginHost, id: &str, reason: PluginStopReason) {
    assert_eq!(
        host.state(&plugin_id(id)),
        Some(PluginRunState::Crashed),
        "{id}"
    );
    assert_eq!(stop_reasons(fake, id), [reason], "{id}");
    let error = host.error(&plugin_id(id)).expect("an error is recorded");
    assert_eq!(error.localized.unwrap().id, reason.message_id());
    assert!(fake.events().iter().any(|e| matches!(
        e,
        Event::PluginStopped { plugin_id, error: Some(_), localized: Some(_), .. } if plugin_id.as_str() == id
    )));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_endless_loop_is_stopped_and_others_keep_running() {
    let fake = Fake::new();
    let host = host(&fake);
    let runaway = r#"
      export default { activate(ctx) {
        ctx.commands.register("run", () => { for (;;) {} });
      } };
    "#;
    host.start(spec("runaway", &[], runaway)).await.unwrap();
    host.start(spec("healthy", &[], HEALTHY)).await.unwrap();

    let started = Instant::now();
    let err = run(&host, "runaway").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    wait_for("crash", || {
        (host.state(&plugin_id("runaway")) == Some(PluginRunState::Crashed)).then_some(())
    })
    .await;
    crashed_with(&fake, &host, "runaway", PluginStopReason::CpuBudget);

    // The other plug-in is unaffected.
    run(&host, "healthy").await.unwrap();
    result(&fake, "healthy", "ALIVE").await;
    assert_eq!(
        host.state(&plugin_id("healthy")),
        Some(PluginRunState::Running)
    );

    // Commands of a crashed plug-in are refused; starting it again works.
    assert_eq!(
        run(&host, "runaway").await.unwrap_err().code,
        ErrorCode::Unavailable
    );
    host.start(spec("runaway", &[], HEALTHY)).await.unwrap();
    assert_eq!(
        host.state(&plugin_id("runaway")),
        Some(PluginRunState::Running)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_loop_after_an_await_is_stopped_too() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export default { activate(ctx) {
        ctx.commands.register("run", async () => {
          await nail.storage.get("x");
          while (true) {}
        });
      } };
    "#;
    host.start(spec("later", &["storage"], js)).await.unwrap();
    run(&host, "later").await.unwrap();
    wait_for("crash", || {
        (host.state(&plugin_id("later")) == Some(PluginRunState::Crashed)).then_some(())
    })
    .await;
    crashed_with(&fake, &host, "later", PluginStopReason::CpuBudget);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_loop_while_activating_fails_the_start() {
    let fake = Fake::new();
    let host = host(&fake);
    let err = host
        .start(spec("hang", &[], "for (;;) {}"))
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable);
    crashed_with(&fake, &host, "hang", PluginStopReason::CpuBudget);
    assert!(fake.log_lines("hang").iter().any(|l| {
        l.localized
            .as_ref()
            .is_some_and(|m| m.id == "plugin-host-start-failed")
    }));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_catch_cannot_swallow_the_budget() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export default { activate(ctx) {
        ctx.commands.register("run", () => {
          for (;;) { try { for (;;) {} } catch (e) { nail.log.info("caught"); } }
        });
      } };
    "#;
    host.start(spec("sneaky", &[], js)).await.unwrap();
    assert!(run(&host, "sneaky").await.is_err());
    crashed_with(&fake, &host, "sneaky", PluginStopReason::CpuBudget);
    assert!(fake.results("sneaky", "caught").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn runaway_memory_is_stopped() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export default { activate(ctx) {
        ctx.commands.register("run", () => {
          const hoard = [];
          for (;;) hoard.push(new Array(64 * 1024).fill(1));
        });
      } };
    "#;
    host.start(spec("hog", &[], js)).await.unwrap();
    host.start(spec("healthy", &[], HEALTHY)).await.unwrap();
    assert!(run(&host, "hog").await.is_err());
    wait_for("crash", || {
        (host.state(&plugin_id("hog")) == Some(PluginRunState::Crashed)).then_some(())
    })
    .await;
    crashed_with(&fake, &host, "hog", PluginStopReason::MemoryLimit);
    run(&host, "healthy").await.unwrap();
    result(&fake, "healthy", "ALIVE").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn deep_recursion_throws_instead_of_crashing_the_process() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      function down(n) { return down(n + 1) + 1; }
      export default { activate(ctx) {
        ctx.commands.register("run", () => {
          try { down(0); } catch (e) { nail.log.info("R " + e.name); }
        });
      } };
    "#;
    host.start(spec("deep", &[], js)).await.unwrap();
    run(&host, "deep").await.unwrap();
    assert_eq!(result(&fake, "deep", "R ").await, "RangeError");
    assert_eq!(
        host.state(&plugin_id("deep")),
        Some(PluginRunState::Running)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn throwing_handlers_stop_the_plugin() {
    let fake = Fake::new();
    let host = host(&fake);
    let sync_throw = r#"
      export default { activate(ctx) {
        ctx.commands.register("run", () => { throw new Error("boom"); });
      } };
    "#;
    host.start(spec("thrower", &[], sync_throw)).await.unwrap();
    let err = run(&host, "thrower").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable);
    crashed_with(&fake, &host, "thrower", PluginStopReason::Error);
    assert!(
        host.error(&plugin_id("thrower"))
            .unwrap()
            .message
            .contains("boom")
    );

    let async_throw = r#"
      export default { activate(ctx) {
        ctx.commands.register("run", async () => { await null; throw new Error("later"); });
      } };
    "#;
    host.start(spec("rejecter", &[], async_throw))
        .await
        .unwrap();
    // The synchronous part succeeded; the rejection stops it afterwards.
    let _ = run(&host, "rejecter").await;
    wait_for("crash", || {
        (host.state(&plugin_id("rejecter")) == Some(PluginRunState::Crashed)).then_some(())
    })
    .await;
    crashed_with(&fake, &host, "rejecter", PluginStopReason::Error);

    let bad_activate = "export default { activate() { throw new TypeError('nope'); } };";
    let err = host
        .start(spec("bad-start", &[], bad_activate))
        .await
        .unwrap_err();
    assert!(err.message.contains("nope"), "{}", err.message);
    crashed_with(&fake, &host, "bad-start", PluginStopReason::Error);

    let syntax = "export default {";
    assert!(host.start(spec("syntax", &[], syntax)).await.is_err());
    assert_eq!(
        host.state(&plugin_id("syntax")),
        Some(PluginRunState::Crashed)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn unhandled_rejections_are_logged_not_fatal() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export default { activate() { nail.storage.get("x"); Promise.reject(new Error("floating")); } };
    "#;
    host.start(spec("floating", &[], js)).await.unwrap();
    wait_for("rejection lines", || {
        let lines: Vec<_> = fake
            .log_lines("floating")
            .into_iter()
            .filter(|l| {
                l.localized
                    .as_ref()
                    .is_some_and(|m| m.id == "plugin-host-unhandled-rejection")
            })
            .collect();
        (lines.len() >= 2).then_some(())
    })
    .await;
    assert_eq!(
        host.state(&plugin_id("floating")),
        Some(PluginRunState::Running)
    );
}

/// A call that fails at once (a denied permission) and is awaited inside
/// `try` is handled: no "unhandled rejection" line.
#[tokio::test(flavor = "multi_thread")]
async fn awaited_immediate_rejections_are_not_reported() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export default {
        async activate() {
          try { await nail.storage.get("x"); } catch (e) { nail.log.info("CAUGHT " + e.code); }
        },
      };
    "#;
    host.start(spec("careful", &[], js)).await.unwrap();
    wait_for("the caught error", || {
        fake.log_lines("careful")
            .iter()
            .any(|l| l.message == "CAUGHT forbidden")
            .then_some(())
    })
    .await;
    assert!(
        !fake.log_lines("careful").iter().any(|l| l
            .localized
            .as_ref()
            .is_some_and(|m| m.id == "plugin-host-unhandled-rejection")),
        "{:#?}",
        fake.log_lines("careful")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn plugins_have_no_host_access() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      const kinds = [typeof require, typeof process, typeof std, typeof os, typeof fetch,
        typeof XMLHttpRequest, typeof WebSocket, typeof Deno, typeof Bun, typeof module,
        typeof __filename, typeof hostCall, typeof globalThis.hostCall].join();
      export default { activate() {
        nail.log.info("KINDS " + kinds);
        import("os").then(() => nail.log.info("OS loaded"), (e) => nail.log.info("OS " + e.message));
        import("../outside.js").then(() => nail.log.info("UP loaded"), (e) => nail.log.info("UP " + e.message));
        import("node:fs").then(() => nail.log.info("FS loaded"), (e) => nail.log.info("FS failed"));
        import("./assets/logo.png").then(() => nail.log.info("PNG loaded"), () => nail.log.info("PNG failed"));
      } };
    "#;
    host.start(spec("sandboxed", &[], js)).await.unwrap();
    assert_eq!(
        result(&fake, "sandboxed", "KINDS ").await,
        ["undefined"; 13].join(",")
    );
    assert!(
        result(&fake, "sandboxed", "OS ")
            .await
            .contains("bundle dependencies")
    );
    assert!(
        result(&fake, "sandboxed", "UP ")
            .await
            .contains("bundle dependencies")
    );
    result(&fake, "sandboxed", "FS failed").await;
    result(&fake, "sandboxed", "PNG failed").await;
    assert!(fake.results("sandboxed", "OS loaded").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn plugins_cannot_see_each_other() {
    let fake = Fake::new();
    let host = host(&fake);
    host.start(spec(
        "first",
        &[],
        "globalThis.secret = 42; export default {};",
    ))
    .await
    .unwrap();
    host.start(spec(
        "second",
        &[],
        "export default { activate() { nail.log.info('SEEN ' + typeof globalThis.secret); } };",
    ))
    .await
    .unwrap();
    assert_eq!(result(&fake, "second", "SEEN ").await, "undefined");
}

#[tokio::test(flavor = "multi_thread")]
async fn unsupported_api_versions_are_incompatible() {
    let fake = Fake::new();
    let host = host(&fake);
    let mut spec = spec("future", &[], HEALTHY);
    spec.manifest.api_version = "2".into();
    let err = host.start(spec).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Unsupported);
    assert_eq!(
        host.state(&plugin_id("future")),
        Some(PluginRunState::Incompatible)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_main_file_fails_the_start() {
    let fake = Fake::new();
    let host = host(&fake);
    let spec = spec_with_files("nomain", &[], &[("other.js", "")]);
    let err = host.start(spec).await.unwrap_err();
    assert!(err.message.contains("index.js"), "{}", err.message);
    assert_eq!(
        host.state(&plugin_id("nomain")),
        Some(PluginRunState::Crashed)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn uninstall_and_shutdown_forget_or_stop_plugins() {
    let fake = Fake::new();
    let host = host(&fake);
    host.start(spec("one", &[], HEALTHY)).await.unwrap();
    host.start(spec("two", &[], HEALTHY)).await.unwrap();
    assert!(host.stop(&plugin_id("one"), PluginStopReason::Uninstalled));
    assert_eq!(host.state(&plugin_id("one")), None);
    assert_eq!(host.plugins(), [plugin_id("two")]);
    host.shutdown();
    assert_eq!(host.state(&plugin_id("two")), Some(PluginRunState::Stopped));
    assert_eq!(stop_reasons(&fake, "two"), [PluginStopReason::Shutdown]);
    // Stopping twice reports nothing new.
    assert!(!host.stop(&plugin_id("two"), PluginStopReason::Shutdown));
}

#[tokio::test(flavor = "multi_thread")]
async fn restarting_replaces_the_running_instance() {
    let fake = Fake::new();
    let host = host(&fake);
    host.start(spec("again", &[], HEALTHY)).await.unwrap();
    host.start(spec("again", &[], HEALTHY)).await.unwrap();
    assert_eq!(stop_reasons(&fake, "again"), [PluginStopReason::Reloaded]);
    run(&host, "again").await.unwrap();
    result(&fake, "again", "ALIVE").await;
    host.reload(&plugin_id("again")).await.unwrap();
    assert_eq!(
        stop_reasons(&fake, "again"),
        [PluginStopReason::Reloaded, PluginStopReason::Reloaded]
    );
    assert_eq!(
        host.state(&plugin_id("again")),
        Some(PluginRunState::Running)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn installed_packages_run_from_memory() {
    let fake = Fake::new();
    let host = host(&fake);
    let manifest = serde_json::json!({
        "id": "packed", "name": "Packed", "version": "1.2.3", "apiVersion": "1",
        "main": "dist/index.js", "permissions": ["storage"],
        "contributes": {"commands": [{"id": "run", "title": "Run"}]}
    })
    .to_string();
    let bytes = build_package([
        ("manifest.json", manifest.as_bytes()),
        (
            "dist/index.js",
            br#"import { hi } from "./hi.js"; export default { activate(ctx) { ctx.commands.register("run", () => nail.log.info(hi)); } };"#.as_slice(),
        ),
        ("dist/hi.js", b"export const hi = 'HELLO';".as_slice()),
    ])
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = PackageStore::new(dir.path());
    store.install(&bytes, &sha256_hex(&bytes)).unwrap();
    let package: PluginPackage = store
        .load(&plugin_id("packed"), Some(&sha256_hex(&bytes)))
        .unwrap();
    let manifest = package.valid_manifest().unwrap().clone();
    let grants = all_grants(&manifest);
    host.start(PluginSpec {
        grants,
        ..PluginSpec::new(manifest, package.files.clone())
    })
    .await
    .unwrap();
    run(&host, "packed").await.unwrap();
    result(&fake, "packed", "HELLO").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn too_many_pending_calls_are_refused() {
    let fake = Fake::new();
    let mut limits = limits();
    limits.max_pending_calls = 3;
    let host = host_with(&fake, limits);
    let js = r#"
      export default { activate(ctx) {
        ctx.commands.register("run", async () => {
          const results = await Promise.allSettled([1, 2, 3, 4, 5].map((i) => nail.storage.get("k" + i)));
          nail.log.info("OUT " + results.map((r) => r.status === "fulfilled" ? "ok" : r.reason.code).join());
        });
      } };
    "#;
    host.start(spec("busy", &["storage"], js)).await.unwrap();
    run(&host, "busy").await.unwrap();
    assert_eq!(
        result(&fake, "busy", "OUT ").await,
        "ok,ok,ok,rate_limited,rate_limited"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_the_last_host_stops_its_plugins() {
    let fake = Fake::new();
    let host = host(&fake);
    host.start(spec("orphan", &[], HEALTHY)).await.unwrap();
    let clone = host.clone();
    drop(host);
    assert!(stop_reasons(&fake, "orphan").is_empty());
    drop(clone);
    assert_eq!(stop_reasons(&fake, "orphan"), [PluginStopReason::Shutdown]);
}
