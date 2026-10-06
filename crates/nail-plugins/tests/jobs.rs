//! M4a background jobs through a running host: cron schedules on the core's
//! clock, missed runs at start, event subscriptions (coalesced, message ids
//! only when readable), `app.started`, no overlapping runs, the wall-clock
//! limit (pending promises included) and the cumulative CPU quota.

#![cfg(test)]

#[expect(
    dead_code,
    reason = "the shared test helpers serve several test crates; each uses a subset"
)]
mod common;

use std::time::Duration;

use common::*;
use nail_api::MessageId;
use nail_api::plugins::{PluginEventName, PluginJobOutcome, PluginRunState};
use nail_api::{ErrorCode, Event};
use nail_plugins::HostLimits;
use serde_json::{Value, json};

fn job_limits() -> HostLimits {
    HostLimits {
        job_tick: Duration::from_millis(30),
        event_coalesce: Duration::from_millis(150),
        job_wall_clock: Duration::from_millis(600),
        ..limits()
    }
}

fn jobs(defs: Value) -> Value {
    Value::Object(serde_json::Map::from_iter([("jobs".to_owned(), defs)]))
}

const LOGGER: &str = r#"
  export default { activate(ctx) {
    for (const id of ["digest", "tagger", "boot"]) {
      try {
        ctx.jobs.register(id, (trigger) => nail.log.info("RUN " + id + " " + JSON.stringify(trigger)));
      } catch (_) {}
    }
  } };
"#;

fn ts(at: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(at)
        .unwrap()
        .timestamp()
}

async fn runs_of(fake: &Fake, plugin: &str, count: usize) -> Vec<nail_plugins::JobRunRecord> {
    wait_for(&format!("{count} runs of {plugin}"), || {
        let runs = fake.job_runs(plugin);
        (runs.len() >= count).then_some(runs)
    })
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn schedules_run_on_the_cores_clock() {
    let fake = Fake::new();
    fake.set_now("2026-10-02T10:07:00Z");
    let host = host_with(&fake, job_limits());
    host.start(spec_with(
        "sched",
        &["background"],
        jobs(json!([{ "id": "digest", "schedule": "*/15 * * * *" }])),
        LOGGER,
    ))
    .await
    .unwrap();
    let id = plugin_id("sched");
    let next = wait_for("next run", || host.job_next_run(&id, "digest")).await;
    assert_eq!(next.timestamp(), ts("2026-10-02T10:15:00Z"));
    wait_for("stored", || {
        (fake
            .job_states
            .lock()
            .unwrap()
            .get(&("sched".into(), "digest".into()))
            == Some(&Some(ts("2026-10-02T10:15:00Z"))))
        .then_some(())
    })
    .await;
    // Nothing is due yet.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(fake.results("sched", "RUN ").len(), 0);

    fake.set_now("2026-10-02T10:15:20Z");
    host.run_due_jobs();
    let line = result(&fake, "sched", "RUN digest ").await;
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap(),
        json!({"kind": "schedule", "scheduledFor": "2026-10-02T10:15:00Z"})
    );
    let runs = runs_of(&fake, "sched", 1).await;
    assert_eq!(runs[0].outcome, PluginJobOutcome::Ok);
    assert_eq!(runs[0].next_run_at, Some(ts("2026-10-02T10:30:00Z")));
    assert!(fake.events().contains(&Event::PluginActivityChanged {
        plugin_id: id.clone()
    }));
    // The scheduler's own tick finds the next one.
    fake.set_now("2026-10-02T10:31:00Z");
    runs_of(&fake, "sched", 2).await;
    assert_eq!(fake.results("sched", "RUN digest ").len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missed_schedule_runs_once_at_start() {
    let fake = Fake::new();
    fake.set_now("2026-10-02T12:00:00Z");
    // Nail was not running at 08:00 (or 09:00, 10:00, 11:00).
    fake.job_states.lock().unwrap().insert(
        ("missed".into(), "digest".into()),
        Some(ts("2026-10-02T08:00:00Z")),
    );
    let host = host_with(&fake, job_limits());
    host.start(spec_with(
        "missed",
        &["background"],
        jobs(json!([{ "id": "digest", "schedule": "@hourly" }])),
        LOGGER,
    ))
    .await
    .unwrap();
    let runs = runs_of(&fake, "missed", 1).await;
    assert_eq!(runs[0].next_run_at, Some(ts("2026-10-02T13:00:00Z")));
    tokio::time::sleep(Duration::from_millis(150)).await;
    let lines = fake.results("missed", "RUN digest ");
    assert_eq!(lines.len(), 1, "once, not once per missed hour");
    assert!(lines[0].contains("2026-10-02T08:00:00Z"));
}

#[tokio::test(flavor = "multi_thread")]
async fn events_are_coalesced_and_carry_readable_messages_only() {
    let fake = Fake::new();
    let host = host_with(&fake, job_limits());
    let mut s = spec_with(
        "events",
        &["background", "mail.read"],
        jobs(json!([
            { "id": "tagger", "events": ["message.received"] },
            { "id": "boot", "events": ["app.started"] }
        ])),
        LOGGER,
    );
    s.grants = vec![
        grant("background", None),
        grant("mail.read", Some(&["work"])),
    ];
    host.start(s).await.unwrap();
    assert_eq!(
        json_result(&fake, "events", "RUN boot ").await,
        json!({"kind": "event", "event": "app.started", "messageIds": []})
    );
    host.deliver_event(
        PluginEventName::MessageReceived,
        &[MessageId::from("m1"), MessageId::from("m2")],
    );
    host.deliver_event(
        PluginEventName::MessageReceived,
        &[MessageId::from("m3"), MessageId::from("nope")],
    );
    // Not subscribed: nothing.
    host.deliver_event(PluginEventName::MessageSent, &[MessageId::from("m1")]);
    let line = result(&fake, "events", "RUN tagger ").await;
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap(),
        json!({"kind": "event", "event": "message.received", "messageIds": ["m1", "m3"]})
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(fake.results("events", "RUN tagger ").len(), 1);

    // Without mail.read the event still runs the job, without ids.
    host.start(spec_with(
        "blind",
        &["background"],
        jobs(json!([{ "id": "tagger", "events": ["message.received"] }])),
        LOGGER,
    ))
    .await
    .unwrap();
    host.deliver_event(PluginEventName::MessageReceived, &[MessageId::from("m1")]);
    assert_eq!(
        json_result(&fake, "blind", "RUN tagger ").await,
        json!({"kind": "event", "event": "message.received", "messageIds": []})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn jobs_never_overlap_and_have_a_wall_clock_limit() {
    let fake = Fake::new();
    let host = host_with(&fake, job_limits());
    let js = r#"
      export default { activate(ctx) {
        ctx.jobs.register("slow", () => new Promise(() => {}));
        ctx.jobs.register("fails", async () => { throw new Error("broken job"); });
      } };
    "#;
    host.start(spec_with(
        "worker",
        &["background"],
        jobs(json!([
            { "id": "slow", "schedule": "@daily" },
            { "id": "fails", "schedule": "@daily" }
        ])),
        js,
    ))
    .await
    .unwrap();
    let id = plugin_id("worker");
    host.run_job(&id, "slow").unwrap();
    assert!(host.job_running(&id, "slow"));
    let err = host.run_job(&id, "slow").unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    // A run that never settles ends at the wall-clock limit.
    let runs = runs_of(&fake, "worker", 1).await;
    assert_eq!(runs[0].job, "slow");
    assert_eq!(runs[0].outcome, PluginJobOutcome::Timeout);
    assert!(!host.job_running(&id, "slow"));
    assert_eq!(
        fake.localized("worker", "plugin-host-job-timed-out").len(),
        1
    );

    host.run_job(&id, "fails").unwrap();
    let runs = runs_of(&fake, "worker", 2).await;
    assert_eq!(runs[1].outcome, PluginJobOutcome::Error);
    assert!(runs[1].error.as_deref().unwrap().contains("broken job"));
    assert_eq!(host.state(&id), Some(PluginRunState::Running));
    let err = host.run_job(&id, "unknown").unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_scheduled_run_while_one_runs_is_skipped_as_overlap() {
    let fake = Fake::new();
    fake.set_now("2026-10-02T10:00:30Z");
    let host = host_with(
        &fake,
        HostLimits {
            job_wall_clock: Duration::from_secs(5),
            ..job_limits()
        },
    );
    host.start(spec_with(
        "busy",
        &["background"],
        jobs(json!([{ "id": "slow", "schedule": "*/5 * * * *" }])),
        r#"export default { activate(ctx) { ctx.jobs.register("slow", () => new Promise(() => {})); } };"#,
    ))
    .await
    .unwrap();
    let id = plugin_id("busy");
    wait_for("next", || host.job_next_run(&id, "slow")).await;
    host.run_job(&id, "slow").unwrap();
    fake.set_now("2026-10-02T10:05:10Z");
    host.run_due_jobs();
    let runs = runs_of(&fake, "busy", 1).await;
    assert_eq!(runs[0].outcome, PluginJobOutcome::Overlap);
    assert_eq!(fake.localized("busy", "plugin-host-job-overlap").len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_cpu_quota_skips_jobs_until_the_hour_has_passed() {
    let fake = Fake::new();
    fake.set_now("2026-10-02T10:00:00Z");
    let host = host_with(
        &fake,
        HostLimits {
            cpu_quota_per_hour: Duration::from_millis(80),
            ..job_limits()
        },
    );
    let js = r#"
      export default { activate(ctx) {
        ctx.jobs.register("burn", () => {
          const until = Date.now() + 150;
          while (Date.now() < until) {}
          nail.log.info("BURNED");
        });
      } };
    "#;
    host.start(spec_with(
        "burner",
        &["background"],
        jobs(json!([{ "id": "burn", "schedule": "*/5 * * * *" }])),
        js,
    ))
    .await
    .unwrap();
    let id = plugin_id("burner");
    host.run_job(&id, "burn").unwrap();
    result(&fake, "burner", "BURNED").await;
    runs_of(&fake, "burner", 1).await;
    assert!(host.cpu_last_hour(&id) >= Duration::from_millis(100));
    // Reported to the core.
    wait_for("cpu report", || {
        fake.cpu
            .lock()
            .unwrap()
            .iter()
            .any(|(p, _, ms)| p == "burner" && *ms >= 100)
            .then_some(())
    })
    .await;
    let err = host.run_job(&id, "burn").unwrap_err();
    assert_eq!(err.code, ErrorCode::RateLimited);
    // A due schedule is skipped and recorded as such.
    fake.set_now("2026-10-02T10:05:30Z");
    host.run_due_jobs();
    let runs = runs_of(&fake, "burner", 2).await;
    assert_eq!(runs[1].outcome, PluginJobOutcome::CpuQuota);
    assert_eq!(
        fake.localized("burner", "plugin-host-job-cpu-quota").len(),
        1
    );
    // An hour later the quota is back.
    fake.set_now("2026-10-02T11:01:00Z");
    assert_eq!(host.cpu_last_hour(&id), Duration::ZERO);
    host.run_job(&id, "burn").unwrap();
    wait_for("second burn", || {
        (fake.results("burner", "BURNED").len() == 2).then_some(())
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn jobs_need_the_background_permission() {
    let fake = Fake::new();
    let host = host_with(&fake, job_limits());
    let mut s = spec_with(
        "nobg",
        &["background"],
        jobs(json!([
            { "id": "boot", "events": ["app.started"] },
            { "id": "tagger", "events": ["message.received"] }
        ])),
        LOGGER,
    );
    s.grants = Vec::new();
    host.start(s).await.unwrap();
    let err = host.run_job(&plugin_id("nobg"), "boot").unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable);
    host.deliver_event(PluginEventName::MessageReceived, &[MessageId::from("m1")]);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(fake.results("nobg", "RUN ").len(), 0);
    assert_eq!(fake.job_runs("nobg").len(), 0);
    // Granting it later starts the jobs (app.started once).
    assert!(host.update_grants(&plugin_id("nobg"), &[grant("background", None)]));
    assert_eq!(
        json_result(&fake, "nobg", "RUN boot ").await,
        json!({"kind": "event", "event": "app.started", "messageIds": []})
    );
    host.update_grants(&plugin_id("nobg"), &[grant("background", None)]);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fake.results("nobg", "RUN boot ").len(), 1);
    // Registering an undeclared job is refused.
    host.start(spec(
        "undeclared",
        &[],
        r#"export default { activate(ctx) {
             try { ctx.jobs.register("x", () => {}); } catch (e) { nail.log.info("REG " + e.code); }
           } };"#,
    ))
    .await
    .unwrap();
    assert_eq!(result(&fake, "undeclared", "REG ").await, "invalid_request");
}
