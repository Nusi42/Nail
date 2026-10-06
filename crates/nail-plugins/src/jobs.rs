//! Background jobs (M4a, ADR 0079): cron schedules, event subscriptions,
//! the cumulative CPU quota and the job runner.
//!
//! * [`next_run`] computes a schedule's next firing in local time (the
//!   core's clock and zone, [`crate::PluginServices::now`] /
//!   [`crate::PluginServices::local_time`]).
//! * [`CpuMeter`] sums the CPU time of every turn of a plug-in per minute;
//!   over [`crate::HostLimits::cpu_quota_per_hour`] in the last hour its
//!   jobs are skipped (`cpu-quota`).
//! * The host side ([`crate::PluginHost::run_job`],
//!   [`crate::PluginHost::deliver_event`],
//!   [`crate::PluginHost::run_due_jobs`]) starts runs: never two of the same
//!   job at once (`overlap`), each with a wall-clock limit that includes its
//!   pending promises (`timeout`), records every run
//!   ([`crate::PluginServices::record_job_run`]) and announces it
//!   (`PluginActivityChanged`). A schedule missed while Nail was not running
//!   runs once when the plug-in starts.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, NaiveDateTime, TimeZone, Utc};
use nail_api::plugins::jobs::MAX_TRIGGER_MESSAGES;
use nail_api::plugins::{
    CronSchedule, PluginEventName, PluginJobDef, PluginJobOutcome, PluginJobTrigger,
    PluginLogLevel, PluginPermission,
};
use nail_api::{ApiError, ErrorCode, Event, LocalizedMessage, MessageId, PluginId};

use crate::host::{Inner, PluginHost, lock};
use crate::l10n;
use crate::runtime::{Invoke, Registry};
use crate::services::JobRunRecord;

/// How far ahead [`next_run`] looks (days; a schedule like `0 0 29 2 *`
/// fires within four years).
const LOOKAHEAD_DAYS: u32 = 4 * 366 + 1;

/// The first time after `after` at which `schedule` fires, in the local
/// time `local` gives for a UTC time. Local times that do not exist (a DST
/// gap) are skipped; `None` when nothing fires within four years.
pub fn next_run(
    schedule: &CronSchedule,
    after: DateTime<Utc>,
    local: &dyn Fn(DateTime<Utc>) -> NaiveDateTime,
) -> Option<DateTime<Utc>> {
    let offset = |t: DateTime<Utc>| local(t) - t.naive_utc();
    let to_utc = |naive: NaiveDateTime| -> Option<DateTime<Utc>> {
        let guess = Utc.from_utc_datetime(&(naive - offset(after)));
        let utc = Utc.from_utc_datetime(&(naive - offset(guess)));
        (local(utc) == naive).then_some(utc)
    };
    let mut day = local(after).date();
    for _ in 0..LOOKAHEAD_DAYS {
        let (d, m, wd) = (day.day(), day.month(), day.weekday().num_days_from_sunday());
        let fires_at = |minute: u32, hour: u32| schedule.matches(minute, hour, d, m, wd);
        for hour in 0..24 {
            if !(0..60).any(|minute| fires_at(minute, hour)) {
                continue;
            }
            for minute in (0..60).filter(|minute| fires_at(*minute, hour)) {
                let Some(naive) = day.and_hms_opt(hour, minute, 0) else {
                    continue;
                };
                if let Some(utc) = to_utc(naive)
                    && utc > after
                {
                    return Some(utc);
                }
            }
        }
        day = day.succ_opt()?;
    }
    None
}

/// CPU time of one plug-in by minute (for the hourly quota) and what has
/// not been reported to the store yet.
#[derive(Debug, Default)]
pub struct CpuMeter {
    /// (minute since the epoch, microseconds).
    buckets: VecDeque<(i64, u64)>,
    unreported: u64,
    reported_minute: Option<i64>,
}

impl CpuMeter {
    /// Adds the time of one turn at `minute` (minutes since the epoch).
    pub fn add(&mut self, minute: i64, used: Duration) {
        let micros = u64::try_from(used.as_micros()).unwrap_or(u64::MAX);
        match self.buckets.back_mut() {
            Some((m, total)) if *m == minute => *total = total.saturating_add(micros),
            _ => self.buckets.push_back((minute, micros)),
        }
        self.unreported = self.unreported.saturating_add(micros);
        self.prune(minute);
    }

    fn prune(&mut self, minute: i64) {
        while self
            .buckets
            .front()
            .is_some_and(|(m, _)| *m <= minute - 60 || *m > minute + 1)
        {
            self.buckets.pop_front();
        }
    }

    /// CPU time in the hour up to `minute`.
    pub fn last_hour(&mut self, minute: i64) -> Duration {
        self.prune(minute);
        Duration::from_micros(
            self.buckets
                .iter()
                .filter(|(m, _)| *m > minute - 60 && *m <= minute)
                .map(|(_, us)| *us)
                .sum(),
        )
    }

    /// What to report now: once per minute, the milliseconds not yet
    /// reported (sub-millisecond rests stay for later).
    pub fn take_report(&mut self, minute: i64) -> Option<u64> {
        if self.reported_minute == Some(minute) || self.unreported < 1000 {
            return None;
        }
        self.reported_minute = Some(minute);
        let ms = self.unreported / 1000;
        self.unreported %= 1000;
        Some(ms)
    }
}

/// Minutes since the epoch.
pub fn minute_of(at: DateTime<Utc>) -> i64 {
    at.timestamp().div_euclid(60)
}

/// One declared job of a running plug-in.
#[derive(Debug)]
pub(crate) struct JobEntry {
    pub def: PluginJobDef,
    pub schedule: Option<CronSchedule>,
    pub next: Option<DateTime<Utc>>,
    pub running: bool,
    /// Events collected for the next run, with their messages.
    pub pending: BTreeMap<PluginEventName, Vec<MessageId>>,
    /// Events whose collection window is open (a flush is scheduled).
    pub collecting: BTreeSet<PluginEventName>,
}

/// What a running plug-in that may run jobs needs for them
/// ([`crate::host::Inner::job_context`]).
pub(crate) struct JobContext {
    /// The generation of the running plug-in (a restart counts up).
    pub(crate) generation: u64,
    /// Its job table.
    pub(crate) table: Arc<Mutex<JobTable>>,
    /// Its CPU meter.
    pub(crate) cpu: Arc<Mutex<CpuMeter>>,
    /// Its display name.
    pub(crate) name: String,
}

/// The jobs of one plug-in start (rebuilt on every start).
#[derive(Debug, Default)]
pub(crate) struct JobTable {
    pub jobs: BTreeMap<String, JobEntry>,
    /// The stored state was loaded and missed runs were started.
    pub ready: bool,
}

impl JobTable {
    pub(crate) fn new(defs: &[PluginJobDef]) -> Self {
        let jobs = defs
            .iter()
            .map(|def| {
                let schedule = def
                    .schedule
                    .as_deref()
                    .and_then(|s| CronSchedule::parse(s).ok());
                (
                    def.id.clone(),
                    JobEntry {
                        def: def.clone(),
                        schedule,
                        next: None,
                        running: false,
                        pending: BTreeMap::new(),
                        collecting: BTreeSet::new(),
                    },
                )
            })
            .collect();
        Self { jobs, ready: false }
    }
}

fn unix(at: DateTime<Utc>) -> i64 {
    at.timestamp()
}

fn rfc3339(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Why a run was not started.
enum Skip {
    Overlap,
    CpuQuota,
}

/// Starts a run of `job` unless it runs already or the plug-in is over its
/// CPU quota (both recorded as skipped runs). `Ok(false)` when it was
/// skipped.
fn start_run(
    inner: &Arc<Inner>,
    plugin: &PluginId,
    job: &str,
    trigger: &PluginJobTrigger,
) -> Result<bool, ApiError> {
    let JobContext {
        generation,
        table,
        cpu,
        name,
    } = inner.job_context(plugin)?;
    let services = Arc::clone(&inner.services);
    let now = services.now();
    let skip = {
        let mut jobs = lock(&table);
        let entry = jobs
            .jobs
            .get_mut(job)
            .ok_or_else(|| ApiError::not_found(format!("the plug-in declares no job `{job}`")))?;
        let over = lock(&cpu).last_hour(minute_of(now)) >= inner.limits.cpu_quota_per_hour;
        if entry.running {
            Some(Skip::Overlap)
        } else if over {
            Some(Skip::CpuQuota)
        } else {
            entry.running = true;
            None
        }
    };
    if let Some(skip) = skip {
        let (outcome, message, id) = match skip {
            Skip::Overlap => (
                PluginJobOutcome::Overlap,
                format!("job {job} skipped: the previous run has not finished"),
                l10n::JOB_OVERLAP,
            ),
            Skip::CpuQuota => (
                PluginJobOutcome::CpuQuota,
                format!("job {job} skipped: the plug-in used its CPU time for this hour"),
                l10n::JOB_CPU_QUOTA,
            ),
        };
        inner.host_log(
            plugin,
            PluginLogLevel::Warn,
            message,
            LocalizedMessage::new(id)
                .arg("plugin", name.as_str())
                .arg("job", job),
        );
        record(inner, plugin, job, outcome, None, &table);
        return Ok(false);
    }
    let inner2 = Arc::clone(inner);
    let plugin2 = plugin.clone();
    let job2 = job.to_owned();
    let input = serde_json::to_value(trigger).unwrap_or_default();
    inner.handle.spawn(async move {
        let started = Instant::now();
        let call = Invoke::new(Registry::Job, &job2, input);
        let outcome = match inner2.invoke(&plugin2, Some(generation), call) {
            Ok(rx) => match tokio::time::timeout(inner2.limits.job_wall_clock, rx).await {
                Ok(Ok(Ok(_))) => (PluginJobOutcome::Ok, None),
                Ok(Ok(Err(e))) => (PluginJobOutcome::Error, Some(e.message)),
                Ok(Err(_)) => (
                    PluginJobOutcome::Error,
                    Some("the plug-in stopped".to_owned()),
                ),
                Err(_) => (
                    PluginJobOutcome::Timeout,
                    Some(format!(
                        "did not finish within {:?}",
                        inner2.limits.job_wall_clock
                    )),
                ),
            },
            Err(e) => (PluginJobOutcome::Error, Some(e.message)),
        };
        let (outcome, error) = outcome;
        if outcome != PluginJobOutcome::Ok {
            let id = if outcome == PluginJobOutcome::Timeout {
                l10n::JOB_TIMED_OUT
            } else {
                l10n::JOB_FAILED
            };
            inner2.host_log(
                &plugin2,
                PluginLogLevel::Warn,
                format!(
                    "job {job2} ended after {:?}: {}",
                    started.elapsed(),
                    error.as_deref().unwrap_or_default()
                ),
                LocalizedMessage::new(id)
                    .arg("job", job2.as_str())
                    .arg("detail", error.as_deref().unwrap_or_default()),
            );
        }
        if let Some(entry) = lock(&table).jobs.get_mut(&job2) {
            entry.running = false;
        }
        record(&inner2, &plugin2, &job2, outcome, error, &table);
    });
    Ok(true)
}

/// Records a run (or a skipped one) and announces it.
fn record(
    inner: &Arc<Inner>,
    plugin: &PluginId,
    job: &str,
    outcome: PluginJobOutcome,
    error: Option<String>,
    table: &Arc<Mutex<JobTable>>,
) {
    let services = Arc::clone(&inner.services);
    let next_run_at = lock(table).jobs.get(job).and_then(|e| e.next).map(unix);
    let record = JobRunRecord {
        plugin_id: plugin.clone(),
        job: job.to_owned(),
        at: unix(services.now()),
        outcome,
        error,
        next_run_at,
    };
    let plugin = plugin.clone();
    inner.handle.spawn(async move {
        if let Err(e) = services.record_job_run(record).await {
            tracing::warn!(plugin = %plugin, "cannot record a job run: {}", e.message);
        }
        services.emit(Event::PluginActivityChanged { plugin_id: plugin });
    });
}

/// After a plug-in with `background` started: loads the stored schedule
/// state, runs schedules missed meanwhile once, stores the next runs and
/// delivers `app.started`.
pub(crate) async fn activated(inner: Arc<Inner>, plugin: PluginId, generation: u64) {
    let Ok(JobContext {
        generation: current,
        table,
        ..
    }) = inner.job_context(&plugin)
    else {
        return;
    };
    {
        let mut jobs = lock(&table);
        // Started already (a grant change after the start), or nothing to do.
        if current != generation || jobs.ready || jobs.jobs.is_empty() {
            return;
        }
        // Claimed: a second call (start and grant change at once) stops
        // here. Schedules have no next run until it is computed below.
        jobs.ready = true;
    }
    let services = Arc::clone(&inner.services);
    let stored = services
        .job_states(plugin.clone())
        .await
        .unwrap_or_default();
    let now = services.now();
    let local = |t: DateTime<Utc>| services.local_time(t);
    let mut missed = Vec::new();
    let mut updates = Vec::new();
    {
        let mut jobs = lock(&table);
        for (name, entry) in &mut jobs.jobs {
            let Some(schedule) = &entry.schedule else {
                continue;
            };
            let stored_next = stored
                .iter()
                .find(|s| &s.job == name)
                .and_then(|s| s.next_run_at)
                .and_then(|t| DateTime::from_timestamp(t, 0));
            if let Some(due) = stored_next.filter(|t| *t <= now) {
                missed.push((name.clone(), due));
            }
            entry.next = next_run(schedule, now, &local);
            if entry.next != stored_next {
                updates.push((name.clone(), entry.next.map(unix)));
            }
        }
        jobs.ready = true;
    }
    for (job, next) in updates {
        drop(services.set_job_next_run(plugin.clone(), job, next).await);
    }
    for (job, due) in missed {
        drop(start_run(
            &inner,
            &plugin,
            &job,
            &PluginJobTrigger::Schedule {
                scheduled_for: rfc3339(due),
            },
        ));
    }
    deliver(&inner, &plugin, PluginEventName::AppStarted, &[]);
}

/// Starts every job whose schedule is due.
pub(crate) fn run_due(inner: &Arc<Inner>) {
    let now = inner.services.now();
    let services = Arc::clone(&inner.services);
    let local = |t: DateTime<Utc>| services.local_time(t);
    for plugin in inner.background_plugins() {
        let Ok(JobContext { table, .. }) = inner.job_context(&plugin) else {
            continue;
        };
        let due: Vec<(String, DateTime<Utc>)> = {
            let mut jobs = lock(&table);
            if !jobs.ready {
                continue;
            }
            let mut due = Vec::new();
            for (name, entry) in &mut jobs.jobs {
                let (Some(schedule), Some(next)) = (&entry.schedule, entry.next) else {
                    continue;
                };
                if next <= now {
                    due.push((name.clone(), next));
                    // Missed runs in between run once (this one).
                    entry.next = next_run(schedule, now, &local);
                }
            }
            due
        };
        for (job, scheduled) in due {
            drop(start_run(
                inner,
                &plugin,
                &job,
                &PluginJobTrigger::Schedule {
                    scheduled_for: rfc3339(scheduled),
                },
            ));
        }
    }
}

/// Collects an event for the jobs of `plugin` that subscribe to it; the
/// run starts when the collection window closes.
fn deliver(inner: &Arc<Inner>, plugin: &PluginId, event: PluginEventName, ids: &[MessageId]) {
    let Ok(JobContext {
        generation, table, ..
    }) = inner.job_context(plugin)
    else {
        return;
    };
    let mut flush = Vec::new();
    {
        let mut jobs = lock(&table);
        for (name, entry) in &mut jobs.jobs {
            if !entry.def.events.contains(&event) {
                continue;
            }
            let pending = entry.pending.entry(event).or_default();
            for id in ids {
                if pending.len() < MAX_TRIGGER_MESSAGES && !pending.contains(id) {
                    pending.push(id.clone());
                }
            }
            if entry.collecting.insert(event) {
                flush.push(name.clone());
            }
        }
    }
    for job in flush {
        let inner = Arc::clone(inner);
        let plugin = plugin.clone();
        let table = Arc::clone(&table);
        let wait = inner.limits.event_coalesce;
        let handle = inner.handle.clone();
        handle.spawn(async move {
            tokio::time::sleep(wait).await;
            let ids = {
                let mut jobs = lock(&table);
                let Some(entry) = jobs.jobs.get_mut(&job) else {
                    return;
                };
                entry.collecting.remove(&event);
                entry.pending.remove(&event).unwrap_or_default()
            };
            // A restart meanwhile drops the delivery.
            if inner.job_context(&plugin).map(|c| c.generation).ok() != Some(generation) {
                return;
            }
            let ids = inner.readable_ids(&plugin, event, ids).await;
            drop(start_run(
                &inner,
                &plugin,
                &job,
                &PluginJobTrigger::Event {
                    event,
                    message_ids: ids,
                },
            ));
        });
    }
}

impl Inner {
    /// Message ids of an event the plug-in may read (`mail.read`); none
    /// without the event's data permission.
    async fn readable_ids(
        &self,
        plugin: &PluginId,
        event: PluginEventName,
        ids: Vec<MessageId>,
    ) -> Vec<MessageId> {
        if ids.is_empty() || event.data_permission() != Some(PluginPermission::MailRead) {
            return Vec::new();
        }
        let gate = self.gate_of(plugin);
        let Ok(scope) = gate.mail_scope("jobs", &|a| self.services.expand_accounts(a)) else {
            return Vec::new();
        };
        let readable =
            crate::tree::readable_messages(self.services.as_ref(), Some(&scope), ids.clone()).await;
        ids.into_iter()
            .filter(|id| readable.contains_key(id))
            .collect()
    }
}

impl PluginHost {
    /// Runs a plug-in's job now (`RunPluginJob`, trigger `manual`). Returns
    /// once it started: `not_found` for an unknown job, `unavailable` when
    /// the plug-in is not running or lacks `background`, `conflict` while
    /// the job runs, `rate_limited` over the CPU quota.
    pub fn run_job(&self, plugin: &PluginId, job: &str) -> Result<(), ApiError> {
        let JobContext { table, cpu, .. } = self.inner().job_context(plugin)?;
        if lock(&table).jobs.get(job).is_some_and(|e| e.running) {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                format!("the job `{job}` is running"),
            ));
        }
        let now = minute_of(self.inner().services.now());
        if lock(&cpu).last_hour(now) >= self.inner().limits.cpu_quota_per_hour {
            return Err(ApiError::new(
                ErrorCode::RateLimited,
                "the plug-in used its CPU time for this hour",
            ));
        }
        if start_run(self.inner(), plugin, job, &PluginJobTrigger::Manual)? {
            Ok(())
        } else {
            Err(ApiError::new(
                ErrorCode::Conflict,
                format!("the job `{job}` is running"),
            ))
        }
    }

    /// Delivers an event to every running plug-in with `background` whose
    /// jobs subscribe to it (coalesced for
    /// [`crate::HostLimits::event_coalesce`]; message ids are passed only to
    /// plug-ins that may read them). `app.started` is delivered by the host
    /// itself when a plug-in starts.
    pub fn deliver_event(&self, event: PluginEventName, message_ids: &[MessageId]) {
        for plugin in self.inner().background_plugins() {
            deliver(self.inner(), &plugin, event, message_ids);
        }
    }

    /// Starts every job whose schedule is due now (the scheduler does this
    /// every [`crate::HostLimits::job_tick`]; the core calls it when its
    /// clock was moved).
    pub fn run_due_jobs(&self) {
        run_due(self.inner());
    }

    /// Whether a job of a plug-in is running.
    pub fn job_running(&self, plugin: &PluginId, job: &str) -> bool {
        self.inner()
            .job_context(plugin)
            .is_ok_and(|c| lock(&c.table).jobs.get(job).is_some_and(|e| e.running))
    }

    /// When a job runs next (RFC 3339 is the caller's business).
    pub fn job_next_run(&self, plugin: &PluginId, job: &str) -> Option<DateTime<Utc>> {
        let JobContext { table, .. } = self.inner().job_context(plugin).ok()?;
        lock(&table).jobs.get(job).and_then(|e| e.next)
    }

    /// CPU time a plug-in used in the last hour (all its turns).
    pub fn cpu_last_hour(&self, plugin: &PluginId) -> Duration {
        let now = minute_of(self.inner().services.now());
        self.inner()
            .cpu_of(plugin)
            .map(|cpu| lock(&cpu).last_hour(now))
            .unwrap_or_default()
    }
}

/// The schedule loop of a host: looks for due jobs every `job_tick` until
/// the host is gone.
pub(crate) fn spawn_scheduler(inner: &Arc<Inner>) {
    let weak = Arc::downgrade(inner);
    let tick = inner.limits.job_tick.max(Duration::from_millis(10));
    inner.handle.spawn(async move {
        loop {
            tokio::time::sleep(tick).await;
            let Some(inner) = weak.upgrade() else {
                return;
            };
            run_due(&inner);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration as TimeDelta, FixedOffset};

    fn utc(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().to_utc()
    }

    fn in_utc(t: DateTime<Utc>) -> NaiveDateTime {
        t.naive_utc()
    }

    #[test]
    fn next_runs_in_utc() {
        let quarter = CronSchedule::parse("*/15 * * * *").unwrap();
        let at = utc("2026-10-02T10:07:30Z");
        assert_eq!(
            next_run(&quarter, at, &in_utc),
            Some(utc("2026-10-02T10:15:00Z"))
        );
        // Strictly after: on the minute goes to the next one.
        assert_eq!(
            next_run(&quarter, utc("2026-10-02T10:15:00Z"), &in_utc),
            Some(utc("2026-10-02T10:30:00Z"))
        );
        let workdays = CronSchedule::parse("0 8 * * 1-5").unwrap();
        // 2026-10-02 is a Friday: next Monday 08:00.
        assert_eq!(
            next_run(&workdays, utc("2026-10-02T09:00:00Z"), &in_utc),
            Some(utc("2026-10-05T08:00:00Z"))
        );
        let leap = CronSchedule::parse("0 0 29 2 *").unwrap();
        assert_eq!(
            next_run(&leap, at, &in_utc),
            Some(utc("2028-02-29T00:00:00Z"))
        );
        assert_eq!(
            next_run(&CronSchedule::parse("@daily").unwrap(), at, &in_utc),
            Some(utc("2026-10-03T00:00:00Z"))
        );
    }

    #[test]
    fn next_runs_in_local_time() {
        let plus2 = FixedOffset::east_opt(2 * 3600).unwrap();
        let local = move |t: DateTime<Utc>| t.with_timezone(&plus2).naive_local();
        let eight = CronSchedule::parse("0 8 * * *").unwrap();
        // 08:00 at +02:00 is 06:00 UTC.
        assert_eq!(
            next_run(&eight, utc("2026-10-02T05:00:00Z"), &local),
            Some(utc("2026-10-02T06:00:00Z"))
        );
        assert_eq!(
            next_run(&eight, utc("2026-10-02T06:00:00Z"), &local),
            Some(utc("2026-10-03T06:00:00Z"))
        );
        // A zone whose offset changes (DST gap at 02:00 local on one day):
        // 02:30 does not exist that day and is skipped.
        let gap_day = utc("2026-03-29T01:00:00Z");
        let dst = move |t: DateTime<Utc>| {
            let offset = if t >= gap_day { 2 } else { 1 };
            (t + TimeDelta::hours(offset)).naive_utc()
        };
        let half_two = CronSchedule::parse("30 2 * * *").unwrap();
        assert_eq!(
            next_run(&half_two, utc("2026-03-28T12:00:00Z"), &dst),
            Some(utc("2026-03-30T00:30:00Z"))
        );
    }

    #[test]
    fn the_cpu_meter_counts_one_hour() {
        let mut meter = CpuMeter::default();
        meter.add(100, Duration::from_millis(400));
        meter.add(100, Duration::from_millis(600));
        meter.add(130, Duration::from_millis(500));
        assert_eq!(meter.last_hour(130), Duration::from_millis(1500));
        assert_eq!(meter.last_hour(159), Duration::from_millis(1500));
        // Minute 100 leaves the window at 160.
        assert_eq!(meter.last_hour(160), Duration::from_millis(500));
        assert_eq!(meter.last_hour(190), Duration::ZERO);
    }

    #[test]
    fn the_cpu_meter_reports_once_a_minute() {
        let mut meter = CpuMeter::default();
        meter.add(10, Duration::from_micros(1500));
        assert_eq!(meter.take_report(10), Some(1));
        // The rest (0.5 ms) waits; nothing twice in a minute.
        meter.add(10, Duration::from_millis(5));
        assert_eq!(meter.take_report(10), None);
        assert_eq!(meter.take_report(11), Some(5));
        assert_eq!(meter.take_report(12), None);
        meter.add(12, Duration::from_micros(400));
        assert_eq!(meter.take_report(13), None);
    }
}
