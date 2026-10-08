//! Portability jobs in memory (ADR 0094): ids, progress, cancellation,
//! the finished reports (kept [`FINISHED_JOB_RETENTION_SECS`]).
//!
//! A job runs its file work on the blocking pool
//! ([`Service::run_portability_job`]): `nail-portability` does synchronous
//! file I/O (and Argon2id takes about a second of CPU). Progress goes out as
//! [`Event::PortabilityJobProgress`] (throttled by the crate's
//! [`JobContext`]), the end as [`Event::PortabilityJobFinished`].

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use nail_api::engine::BoxFuture;
use nail_api::portability::{
    FINISHED_JOB_RETENTION_SECS, JobKind, JobProgress, JobReport, JobState, JobStatus,
};
use nail_api::{ApiError, Event, JobId, LocalizedMessage};
use nail_portability::{CancelFlag, JobContext};

use crate::error::{CoreError, Result};
use crate::service::{Service, lock};

struct Entry {
    status: JobStatus,
    cancel: CancelFlag,
    finished: Option<Instant>,
}

/// The jobs of this run.
#[derive(Default)]
pub(crate) struct PortabilityCtl {
    jobs: Mutex<BTreeMap<u64, Entry>>,
    next: AtomicU64,
}

impl std::fmt::Debug for PortabilityCtl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PortabilityCtl")
            .field("jobs", &lock(&self.jobs).len())
            .finish_non_exhaustive()
    }
}

fn number_of(id: &JobId) -> Option<u64> {
    id.as_str().strip_prefix("job-")?.parse().ok()
}

fn unknown(id: &JobId) -> CoreError {
    ApiError::not_found(format!("portability job {id}"))
        .with_localized(LocalizedMessage::new("portability-job-unknown"))
        .into()
}

impl PortabilityCtl {
    fn sweep(jobs: &mut BTreeMap<u64, Entry>) {
        let keep = Duration::from_secs(FINISHED_JOB_RETENTION_SECS);
        jobs.retain(|_, e| e.finished.is_none_or(|at| at.elapsed() < keep));
    }

    /// Registers a running job; returns its status and cancel flag.
    pub(crate) fn begin(&self, kind: JobKind, path: &str) -> (JobStatus, CancelFlag) {
        let n = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let status = JobStatus {
            job_id: JobId::new(format!("job-{n}")),
            kind,
            state: JobState::Running,
            started_at: nail_util::time::rfc3339_utc(nail_util::time::unix_now()),
            path: path.to_owned(),
            progress: JobProgress::queued(),
            report: None,
        };
        let cancel = CancelFlag::new();
        let mut jobs = lock(&self.jobs);
        Self::sweep(&mut jobs);
        jobs.insert(
            n,
            Entry {
                status: status.clone(),
                cancel: cancel.clone(),
                finished: None,
            },
        );
        (status, cancel)
    }

    pub(crate) fn progress(&self, id: &JobId, progress: &JobProgress) {
        if let Some(n) = number_of(id)
            && let Some(entry) = lock(&self.jobs).get_mut(&n)
        {
            entry.status.progress = progress.clone();
        }
    }

    pub(crate) fn finish(&self, report: &JobReport) {
        if let Some(n) = number_of(&report.job_id)
            && let Some(entry) = lock(&self.jobs).get_mut(&n)
        {
            entry.status.state = report.state;
            entry.status.report = Some(report.clone());
            entry.finished = Some(Instant::now());
        }
    }

    /// One job.
    pub(crate) fn get(&self, id: &JobId) -> Result<JobStatus> {
        let mut jobs = lock(&self.jobs);
        Self::sweep(&mut jobs);
        number_of(id)
            .and_then(|n| jobs.get(&n))
            .map(|e| e.status.clone())
            .ok_or_else(|| unknown(id))
    }

    /// Running jobs first, then finished ones; newest first.
    pub(crate) fn list(&self) -> Vec<JobStatus> {
        let mut jobs = lock(&self.jobs);
        Self::sweep(&mut jobs);
        let mut all: Vec<(bool, u64, JobStatus)> = jobs
            .iter()
            .map(|(n, e)| (e.finished.is_some(), *n, e.status.clone()))
            .collect();
        all.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        all.into_iter().map(|(_, _, s)| s).collect()
    }

    /// Asks a running job to stop (what was written stays).
    pub(crate) fn cancel(&self, id: &JobId) -> Result<JobStatus> {
        let jobs = lock(&self.jobs);
        let entry = number_of(id)
            .and_then(|n| jobs.get(&n))
            .ok_or_else(|| unknown(id))?;
        entry.cancel.cancel();
        Ok(entry.status.clone())
    }
}

/// What a job's blocking part hands to its async finish (queued
/// operations to announce, PIM items to write through the core's async
/// paths).
pub(crate) type After = Box<dyn FnOnce(Service, JobReport) -> BoxFuture<'static, JobReport> + Send>;

/// The blocking part of a job: file work with the context; returns the
/// result and the async finish.
pub(crate) type JobWork = Box<
    dyn FnOnce(&Service, &mut JobContext<'_>) -> (nail_portability::Result<()>, Option<After>)
        + Send,
>;

impl Service {
    /// Starts a job: registers it, runs `work` on the blocking pool with
    /// progress events, then its async finish, finishes the report and
    /// broadcasts it; answers the job's first status.
    pub(crate) fn run_portability_job(
        &self,
        kind: JobKind,
        path: &str,
        work: JobWork,
    ) -> JobStatus {
        let ctl = &self.inner.portability;
        let (status, cancel) = ctl.begin(kind, path);
        let service = self.clone();
        let job_id = status.job_id.clone();
        let started_at = status.started_at.clone();
        tokio::spawn(async move {
            let runner = service.clone();
            let id = job_id.clone();
            let joined = tokio::task::spawn_blocking(move || {
                let events = runner.clone();
                let progress_id = id.clone();
                let mut ctx = JobContext::new(
                    JobReport::started(id, kind, started_at),
                    cancel,
                    move |p: &JobProgress| {
                        events.inner.portability.progress(&progress_id, p);
                        events.broadcast(Event::PortabilityJobProgress {
                            job_id: progress_id.clone(),
                            progress: p.clone(),
                        });
                    },
                );
                let (result, after) = work(&runner, &mut ctx);
                (result, after, ctx.into_report())
            })
            .await;
            let now = || nail_util::time::rfc3339_utc(nail_util::time::unix_now());
            let report = match joined {
                Ok((result, after, report)) => {
                    let mut report = match after {
                        Some(after) => after(service.clone(), report).await,
                        None => report,
                    };
                    nail_portability::job::finish_report(&mut report, &result, now());
                    report
                }
                Err(err) => {
                    tracing::error!(%err, "a portability job panicked");
                    let mut report = JobReport::started(job_id.clone(), kind, now());
                    report.state = JobState::Failed;
                    report.error = Some(nail_api::ErrorCode::Internal);
                    report.message = Some(err.to_string());
                    report.finished_at = now();
                    report
                }
            };
            tracing::info!(
                job = %report.job_id,
                kind = report.kind.as_str(),
                state = report.state.as_str(),
                "portability job finished"
            );
            service.inner.portability.finish(&report);
            service.broadcast(Event::PortabilityJobFinished {
                report: Box::new(report),
            });
        });
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_are_listed_running_first_and_cancelled_by_flag() {
        let ctl = PortabilityCtl::default();
        let (a, flag_a) = ctl.begin(JobKind::Import, "a");
        let (b, _) = ctl.begin(JobKind::Export, "b");
        assert_ne!(a.job_id, b.job_id);
        assert_eq!(ctl.list().len(), 2);
        let mut report = JobReport::started(a.job_id.clone(), JobKind::Import, "t");
        report.state = JobState::Completed;
        ctl.finish(&report);
        let listed = ctl.list();
        assert_eq!(listed[0].job_id, b.job_id, "running first");
        assert_eq!(listed[1].state, JobState::Completed);
        assert!(ctl.get(&a.job_id).unwrap().report.is_some());
        ctl.cancel(&a.job_id).unwrap();
        assert!(flag_a.is_cancelled());
        assert!(ctl.get(&"job-99".into()).is_err());
        assert!(ctl.cancel(&"nonsense".into()).is_err());
        let progress = JobProgress {
            done: 5,
            ..JobProgress::queued()
        };
        ctl.progress(&b.job_id, &progress);
        assert_eq!(ctl.get(&b.job_id).unwrap().progress.done, 5);
    }
}
