//! Schema v14 (M4a, ADR 0079): what plug-ins do in the background — job
//! state, hook statistics and CPU time for the hourly quota. Everything
//! goes with its plug-in. Times are Unix seconds passed in by the caller.

use nail_api::PluginId;
use nail_api::plugins::{PluginHookKind, PluginHookOutcome, PluginHookStats, PluginJobOutcome};
use rusqlite::{OptionalExtension, params};

use crate::error::{Error, Result};
use crate::store::Store;

const RFC3339: &str = "'%Y-%m-%dT%H:%M:%SZ'";

/// Longest stored error text (bytes; longer ones are cut).
pub const MAX_ACTIVITY_ERROR: usize = 2000;

/// A job's stored state (the plug-in manager adds title, schedule and
/// whether it runs now from the manifest and the host).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginJobState {
    /// The job's name.
    pub job: String,
    /// The last run (Unix seconds).
    pub last_run_at: Option<i64>,
    /// How it ended.
    pub last_outcome: Option<PluginJobOutcome>,
    /// The last error.
    pub last_error: Option<String>,
    /// The next scheduled run (Unix seconds).
    pub next_run_at: Option<i64>,
    /// Runs so far (skipped runs not counted).
    pub run_count: u64,
    /// Failed runs so far.
    pub failure_count: u64,
}

fn cut(text: &str) -> String {
    if text.len() <= MAX_ACTIVITY_ERROR {
        return text.to_owned();
    }
    let mut end = MAX_ACTIVITY_ERROR;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn ensure_plugin(conn: &rusqlite::Connection, plugin: &PluginId) -> Result<()> {
    conn.query_row(
        "SELECT 1 FROM plugins WHERE id = ?1",
        [plugin.as_str()],
        |_| Ok(()),
    )
    .optional()?
    .ok_or_else(|| Error::not_found("plug-in", plugin))
}

impl Store {
    /// The stored state of a plug-in's jobs, by name.
    pub fn plugin_job_states(&self, plugin: &PluginId) -> Result<Vec<PluginJobState>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT job, last_run_at, last_outcome, last_error, next_run_at, run_count,
                        failure_count
                 FROM plugin_jobs WHERE plugin_id = ?1 ORDER BY job",
            )?;
            let rows = stmt
                .query_map([plugin.as_str()], |r| {
                    let outcome: Option<String> = r.get(2)?;
                    Ok(PluginJobState {
                        job: r.get(0)?,
                        last_run_at: r.get(1)?,
                        last_outcome: outcome.as_deref().and_then(PluginJobOutcome::parse),
                        last_error: r.get(3)?,
                        next_run_at: r.get(4)?,
                        run_count: r.get::<_, i64>(5)?.max(0).unsigned_abs(),
                        failure_count: r.get::<_, i64>(6)?.max(0).unsigned_abs(),
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    /// Sets when a job runs next (`None`: no schedule).
    pub fn set_plugin_job_next_run(
        &self,
        plugin: &PluginId,
        job: &str,
        next_run_at: Option<i64>,
    ) -> Result<()> {
        self.with_conn(|conn| {
            ensure_plugin(conn, plugin)?;
            conn.execute(
                "INSERT INTO plugin_jobs (plugin_id, job, next_run_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (plugin_id, job) DO UPDATE SET next_run_at = excluded.next_run_at",
                params![plugin.as_str(), job, next_run_at],
            )?;
            Ok(())
        })
    }

    /// Records a job run that ended at `at` and when it runs next. Skipped
    /// runs (`cpu-quota`, `overlap`) update the outcome but not the counts.
    pub fn record_plugin_job_run(
        &self,
        plugin: &PluginId,
        job: &str,
        at: i64,
        outcome: PluginJobOutcome,
        error: Option<&str>,
        next_run_at: Option<i64>,
    ) -> Result<()> {
        let ran = !matches!(
            outcome,
            PluginJobOutcome::CpuQuota | PluginJobOutcome::Overlap
        );
        self.with_conn(|conn| {
            ensure_plugin(conn, plugin)?;
            conn.execute(
                "INSERT INTO plugin_jobs (plugin_id, job, last_run_at, last_outcome, last_error,
                     next_run_at, run_count, failure_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (plugin_id, job) DO UPDATE SET
                     last_run_at = excluded.last_run_at,
                     last_outcome = excluded.last_outcome,
                     last_error = excluded.last_error,
                     next_run_at = excluded.next_run_at,
                     run_count = run_count + excluded.run_count,
                     failure_count = failure_count + excluded.failure_count",
                params![
                    plugin.as_str(),
                    job,
                    at,
                    outcome.as_str(),
                    error.map(cut),
                    next_run_at,
                    i64::from(ran),
                    i64::from(outcome.is_failure()),
                ],
            )?;
            Ok(())
        })
    }

    /// Forgets the state of jobs a plug-in no longer declares.
    pub fn retain_plugin_jobs(&self, plugin: &PluginId, jobs: &[&str]) -> Result<()> {
        self.with_tx(|tx| {
            let stored: Vec<String> = {
                let mut stmt = tx.prepare("SELECT job FROM plugin_jobs WHERE plugin_id = ?1")?;
                stmt.query_map([plugin.as_str()], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?
            };
            for job in stored.iter().filter(|j| !jobs.contains(&j.as_str())) {
                tx.execute(
                    "DELETE FROM plugin_jobs WHERE plugin_id = ?1 AND job = ?2",
                    params![plugin.as_str(), job],
                )?;
            }
            Ok(())
        })
    }

    /// Counts one hook call that took `ms` and ended at `at`.
    pub fn record_plugin_hook_call(
        &self,
        plugin: &PluginId,
        hook: PluginHookKind,
        at: i64,
        ms: u64,
        outcome: PluginHookOutcome,
        error: Option<&str>,
    ) -> Result<()> {
        let ms = i64::try_from(ms).unwrap_or(i64::MAX);
        let (failure, timeout, veto) = match outcome {
            PluginHookOutcome::Ok => (0, 0, 0),
            PluginHookOutcome::Vetoed => (0, 0, 1),
            PluginHookOutcome::Failed => (1, 0, 0),
            PluginHookOutcome::TimedOut => (0, 1, 0),
        };
        self.with_conn(|conn| {
            ensure_plugin(conn, plugin)?;
            conn.execute(
                "INSERT INTO plugin_hook_stats (plugin_id, hook, calls, failures, timeouts,
                     vetoes, total_ms, max_ms, last_error, last_at)
                 VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?6, ?7, ?8)
                 ON CONFLICT (plugin_id, hook) DO UPDATE SET
                     calls = calls + 1,
                     failures = failures + excluded.failures,
                     timeouts = timeouts + excluded.timeouts,
                     vetoes = vetoes + excluded.vetoes,
                     total_ms = total_ms + excluded.total_ms,
                     max_ms = MAX(max_ms, excluded.max_ms),
                     last_error = COALESCE(excluded.last_error, last_error),
                     last_at = excluded.last_at",
                params![
                    plugin.as_str(),
                    hook.as_str(),
                    failure,
                    timeout,
                    veto,
                    ms,
                    error.map(cut),
                    at
                ],
            )?;
            Ok(())
        })
    }

    /// A plug-in's hook statistics, in [`PluginHookKind::ALL`] order (only
    /// hooks that were called).
    pub fn plugin_hook_stats(&self, plugin: &PluginId) -> Result<Vec<PluginHookStats>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT hook, calls, failures, timeouts, vetoes, total_ms, max_ms, last_error,
                        strftime({RFC3339}, last_at, 'unixepoch')
                 FROM plugin_hook_stats WHERE plugin_id = ?1"
            ))?;
            let mut rows = stmt
                .query_map([plugin.as_str()], |r| {
                    let hook: String = r.get(0)?;
                    let n = |i: usize| r.get::<_, i64>(i).map(|v| v.max(0).unsigned_abs());
                    Ok((
                        hook,
                        n(1)?,
                        n(2)?,
                        n(3)?,
                        n(4)?,
                        n(5)?,
                        n(6)?,
                        r.get::<_, Option<String>>(7)?,
                        r.get::<_, Option<String>>(8)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .filter_map(
                    |(hook, calls, failures, timeouts, vetoes, total_ms, max_ms, error, at)| {
                        Some(PluginHookStats {
                            hook: PluginHookKind::parse(&hook)?,
                            calls,
                            failures,
                            timeouts,
                            vetoes,
                            total_ms,
                            max_ms,
                            last_error: error,
                            last_at: at,
                        })
                    },
                )
                .collect::<Vec<_>>();
            rows.sort_by_key(|s| s.hook);
            Ok(rows)
        })
    }

    /// Adds CPU time a plug-in used at `at` (counted per minute).
    pub fn add_plugin_cpu(&self, plugin: &PluginId, at: i64, cpu_ms: u64) -> Result<()> {
        let cpu_ms = i64::try_from(cpu_ms).unwrap_or(i64::MAX);
        self.with_conn(|conn| {
            ensure_plugin(conn, plugin)?;
            conn.execute(
                "INSERT INTO plugin_cpu (plugin_id, minute, cpu_ms) VALUES (?1, ?2, ?3)
                 ON CONFLICT (plugin_id, minute) DO UPDATE SET cpu_ms = cpu_ms + excluded.cpu_ms",
                params![plugin.as_str(), at.div_euclid(60), cpu_ms],
            )?;
            Ok(())
        })
    }

    /// CPU time a plug-in used in the hour before `now` (ms), and drops
    /// older minutes of every plug-in.
    pub fn plugin_cpu_last_hour(&self, plugin: &PluginId, now: i64) -> Result<u64> {
        let since = now.div_euclid(60) - 59;
        self.with_conn(|conn| {
            conn.execute("DELETE FROM plugin_cpu WHERE minute < ?1", [since])?;
            let used: i64 = conn.query_row(
                "SELECT COALESCE(SUM(cpu_ms), 0) FROM plugin_cpu
                 WHERE plugin_id = ?1 AND minute >= ?2",
                params![plugin.as_str(), since],
                |r| r.get(0),
            )?;
            Ok(used.max(0).unsigned_abs())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::plugins::{PluginManifest, PluginPackageSource};

    const NOW: i64 = 1_790_000_000;

    fn store_with_plugin() -> (Store, PluginId) {
        let store = Store::open_in_memory().unwrap();
        let manifest = PluginManifest::parse(
            br#"{"id":"jobs","name":"Jobs","version":"1.0.0","apiVersion":"1","main":"i.js"}"#,
        )
        .unwrap();
        store
            .install_plugin(&crate::PluginInstall {
                manifest: &manifest,
                source: &PluginPackageSource::Dev {
                    folder: "/dev/jobs".into(),
                },
                package_sha256: None,
                grants: &[],
                enabled: true,
            })
            .unwrap();
        (store, PluginId::new("jobs"))
    }

    #[test]
    fn job_runs_are_recorded() {
        let (store, id) = store_with_plugin();
        assert_eq!(store.plugin_job_states(&id).unwrap().len(), 0);
        store
            .set_plugin_job_next_run(&id, "digest", Some(NOW + 60))
            .unwrap();
        store
            .record_plugin_job_run(
                &id,
                "digest",
                NOW,
                PluginJobOutcome::Ok,
                None,
                Some(NOW + 900),
            )
            .unwrap();
        store
            .record_plugin_job_run(
                &id,
                "digest",
                NOW + 900,
                PluginJobOutcome::Timeout,
                Some(&"x".repeat(MAX_ACTIVITY_ERROR + 10)),
                Some(NOW + 1800),
            )
            .unwrap();
        store
            .record_plugin_job_run(
                &id,
                "digest",
                NOW + 1800,
                PluginJobOutcome::CpuQuota,
                None,
                Some(NOW + 2700),
            )
            .unwrap();
        let states = store.plugin_job_states(&id).unwrap();
        assert_eq!(states.len(), 1);
        let digest = &states[0];
        assert_eq!(digest.run_count, 2);
        assert_eq!(digest.failure_count, 1);
        assert_eq!(digest.last_outcome, Some(PluginJobOutcome::CpuQuota));
        assert_eq!(digest.next_run_at, Some(NOW + 2700));
        assert_eq!(digest.last_error, None);
        store
            .record_plugin_job_run(&id, "old", NOW, PluginJobOutcome::Ok, None, None)
            .unwrap();
        store.retain_plugin_jobs(&id, &["digest"]).unwrap();
        assert_eq!(store.plugin_job_states(&id).unwrap().len(), 1);
        assert!(
            store
                .record_plugin_job_run(
                    &PluginId::new("ghost"),
                    "x",
                    NOW,
                    PluginJobOutcome::Ok,
                    None,
                    None
                )
                .unwrap_err()
                .is_not_found()
        );
    }

    #[test]
    fn hook_calls_are_counted() {
        let (store, id) = store_with_plugin();
        for (ms, outcome, error) in [
            (10, PluginHookOutcome::Ok, None),
            (30, PluginHookOutcome::Vetoed, None),
            (2000, PluginHookOutcome::TimedOut, Some("budget")),
            (5, PluginHookOutcome::Failed, Some("TypeError")),
        ] {
            store
                .record_plugin_hook_call(&id, PluginHookKind::BeforeSend, NOW, ms, outcome, error)
                .unwrap();
        }
        store
            .record_plugin_hook_call(
                &id,
                PluginHookKind::Arrival,
                NOW,
                1,
                PluginHookOutcome::Ok,
                None,
            )
            .unwrap();
        let stats = store.plugin_hook_stats(&id).unwrap();
        assert_eq!(
            stats.iter().map(|s| s.hook).collect::<Vec<_>>(),
            [PluginHookKind::Arrival, PluginHookKind::BeforeSend]
        );
        let send = &stats[1];
        assert_eq!(
            (send.calls, send.failures, send.timeouts, send.vetoes),
            (4, 1, 1, 1)
        );
        assert_eq!((send.total_ms, send.max_ms), (2045, 2000));
        assert_eq!(send.last_error.as_deref(), Some("TypeError"));
        assert_eq!(send.last_at.as_deref(), Some("2026-09-21T14:13:20Z"));
    }

    #[test]
    fn cpu_time_is_counted_per_hour_and_goes_with_the_plugin() {
        let (store, id) = store_with_plugin();
        store.add_plugin_cpu(&id, NOW - 3700, 500).unwrap();
        store.add_plugin_cpu(&id, NOW - 600, 300).unwrap();
        store.add_plugin_cpu(&id, NOW - 590, 200).unwrap();
        store.add_plugin_cpu(&id, NOW, 1).unwrap();
        assert_eq!(store.plugin_cpu_last_hour(&id, NOW).unwrap(), 501);
        store.remove_plugin(&id).unwrap();
        assert_eq!(store.plugin_job_states(&id).unwrap().len(), 0);
        assert_eq!(store.plugin_hook_stats(&id).unwrap().len(), 0);
        assert_eq!(store.plugin_cpu_last_hour(&id, NOW).unwrap(), 0);
    }
}
