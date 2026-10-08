//! M2d review fixes: a stopped plug-in runs nothing that was still queued
//! (resolved calls, timers) and its API refuses everything but logging;
//! every log line a plug-in causes counts against the per-call cap and
//! the per-minute rate (refused calls included).

#![cfg(test)]

#[expect(
    dead_code,
    reason = "the shared test helpers serve several test crates; each uses a subset"
)]
mod common;

use std::time::{Duration, Instant};

use common::*;
use nail_api::UiContext;
use nail_api::plugins::{PluginCommandSurface, PluginLogSource, PluginStopReason};

async fn run(host: &nail_plugins::PluginHost, plugin: &str) {
    host.run_command(
        &format!("plugin.{plugin}.run"),
        PluginCommandSurface::Global,
        &UiContext::default(),
        None,
    )
    .await
    .unwrap();
}

/// Each resolved `storage.get` burns CPU and then fetches: without the
/// fix, the queued resolutions kept fetching for seconds after `Disabled`.
const QUEUED_FETCHES: &str = r#"
export default {
  activate(ctx) {
    ctx.commands.register("run", () => {
      for (let i = 0; i < 20; i++) {
        nail.storage.get("k" + i).then(() => {
          const until = Date.now() + 100;
          while (Date.now() < until) {}
          nail.http.fetch("https://api.example.com/x?" + i).catch(() => {});
        });
      }
    });
  },
  deactivate() {
    nail.log.info("BYE");
    nail.storage.set("after-stop", 1).catch(() => {});
  },
};
"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_plugin_runs_none_of_its_queued_work() {
    let (fake, host) = fake_host();
    host.start(spec(
        "queued",
        &["storage", "network:api.example.com"],
        QUEUED_FETCHES,
    ))
    .await
    .unwrap();
    run(&host, "queued").await;
    wait_for("the first fetch", || {
        (!fake.fetches.lock().unwrap().is_empty()).then_some(())
    })
    .await;
    assert!(host.stop(&plugin_id("queued"), PluginStopReason::Disabled));
    // A fetch issued just before the stop may still reach the core.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let at_stop = fake.fetches.lock().unwrap().len();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let later = fake.fetches.lock().unwrap().len();
    assert_eq!(later, at_stop, "fetches after the plug-in was disabled");
    assert!(at_stop < 20, "every queued resolution ran ({at_stop})");
    // `deactivate` ran, but its API calls were refused.
    wait_for("deactivate", || {
        fake.results("queued", "BYE").first().cloned()
    })
    .await;
    assert!(
        !fake
            .storage
            .lock()
            .unwrap()
            .contains_key(&("queued".to_owned(), "after-stop".to_owned()))
    );
    // Being stopped is not a crash.
    assert_eq!(stop_reasons(&fake, "queued"), [PluginStopReason::Disabled]);
}

const BUSY: &str = r#"
export default {
  activate(ctx) {
    ctx.commands.register("run", () => { nail.log.info("BUSY"); for (;;) {} });
  },
  deactivate() { nail.log.info("BYE"); },
};
"#;

#[tokio::test(flavor = "multi_thread")]
async fn stopping_interrupts_a_running_turn_at_once() {
    let fake = Fake::new();
    let mut limits = limits();
    limits.call_budget = Duration::from_mins(1);
    limits.reply_timeout = Duration::from_millis(200);
    let host = host_with(&fake, limits);
    host.start(spec("busy", &[], BUSY)).await.unwrap();
    let runner = host.clone();
    let command = tokio::spawn(async move {
        drop(
            runner
                .run_command(
                    "plugin.busy.run",
                    PluginCommandSurface::Global,
                    &UiContext::default(),
                    None,
                )
                .await,
        );
    });
    wait_for("the busy turn", || {
        fake.results("busy", "BUSY").first().cloned()
    })
    .await;
    let stopped = Instant::now();
    host.stop(&plugin_id("busy"), PluginStopReason::Uninstalled);
    wait_for("deactivate", || {
        fake.results("busy", "BYE").first().cloned()
    })
    .await;
    assert!(
        stopped.elapsed() < Duration::from_secs(5),
        "the turn ran on for {:?}",
        stopped.elapsed()
    );
    // Uninstalled, not flagged for its CPU use.
    assert_eq!(stop_reasons(&fake, "busy"), [PluginStopReason::Uninstalled]);
    command.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn refused_calls_count_against_the_per_call_log_cap() {
    let (fake, host) = fake_host();
    let js = r#"
      export default { activate(ctx) {
        ctx.commands.register("run", () => {
          for (let i = 0; i < 5000; i++) nail.mail.get("x").catch(() => {});
          nail.log.info("DONE");
        });
      } };
    "#;
    host.start(spec("flood", &[], js)).await.unwrap();
    run(&host, "flood").await;
    let dropped = wait_for("the dropped line", || {
        fake.log_lines("flood").into_iter().find(|l| {
            l.localized
                .as_ref()
                .is_some_and(|m| m.id == "plugin-host-log-dropped")
        })
    })
    .await;
    let cap = limits().max_log_lines_per_call as usize;
    let refused = fake
        .log_lines("flood")
        .iter()
        .filter(|l| l.source == PluginLogSource::Api)
        .count();
    assert_eq!(refused, cap);
    // 5000 refusals + "DONE" beyond the cap.
    assert_eq!(
        dropped.localized.unwrap().args["count"].to_string(),
        (5001 - cap).to_string()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn log_lines_are_limited_per_minute_across_turns() {
    let fake = Fake::new();
    let mut limits = limits();
    limits.max_log_lines_per_call = 1000;
    limits.max_log_lines_per_minute = 60;
    let host = host_with(&fake, limits);
    // 200 turns (timers) of one line each.
    let js = r#"
      export default { activate(ctx) {
        ctx.commands.register("run", () => {
          let n = 0;
          const tick = () => { nail.log.info("L " + n); if (++n < 200) setTimeout(tick, 0); };
          tick();
        });
      } };
    "#;
    host.start(spec("chatty", &[], js)).await.unwrap();
    let started = Instant::now();
    run(&host, "chatty").await;
    // Every line is either written or counted as dropped.
    let (kept, dropped) = wait_for("every line accounted for", || {
        let lines = fake.log_lines("chatty");
        let kept = lines
            .iter()
            .filter(|l| l.source == PluginLogSource::Plugin)
            .count();
        let dropped: usize = lines
            .iter()
            .filter_map(|l| l.localized.as_ref())
            .filter(|m| m.id == "plugin-host-log-dropped")
            .map(|m| m.args["count"].to_string().parse::<usize>().unwrap())
            .sum();
        (kept + dropped == 200).then_some((kept, dropped))
    })
    .await;
    // The burst plus what refilled meanwhile (one line a second), minus
    // the dropped-lines notices.
    let refilled = usize::try_from(started.elapsed().as_secs()).unwrap() + 1;
    assert!(kept <= 60 + refilled, "{kept} lines kept");
    assert!(dropped >= 200 - 60 - refilled, "{dropped} lines dropped");
}
