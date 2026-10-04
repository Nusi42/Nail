//! What every job shares: progress (throttled), cancellation and the
//! report the core created ([`JobReport::started`]).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use nail_api::portability::{
    IssueSeverity, ItemIssue, ItemRef, JobPhase, JobProgress, JobReport, MAX_PREVIEW_ISSUES,
    PortabilityIssueCode,
};

use crate::error::{PortabilityError, Result};

/// Least time between two progress callbacks of one phase ("at most a few
/// per second", ADR 0094).
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// A cancellation flag shared between the core (which sets it on
/// `CancelPortabilityJob`) and the running job (which checks it between
/// items).
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    /// A flag that is not set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks the job to stop (what was written stays).
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether the job was asked to stop.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The running job's context: progress callback, cancellation, and the
/// report items are recorded in.
pub struct JobContext<'a> {
    progress: JobProgress,
    callback: Box<dyn FnMut(&JobProgress) + Send + 'a>,
    cancel: CancelFlag,
    last_emit: Option<Instant>,
    interval: Duration,
    /// The report being filled (counts, issues, proposals).
    pub report: JobReport,
}

impl std::fmt::Debug for JobContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobContext")
            .field("progress", &self.progress)
            .field("cancel", &self.cancel)
            .field("report", &self.report)
            .finish_non_exhaustive()
    }
}

impl<'a> JobContext<'a> {
    /// A context reporting progress to `callback`.
    pub fn new(
        report: JobReport,
        cancel: CancelFlag,
        callback: impl FnMut(&JobProgress) + Send + 'a,
    ) -> Self {
        Self {
            progress: JobProgress::queued(),
            callback: Box::new(callback),
            cancel,
            last_emit: None,
            interval: PROGRESS_INTERVAL,
            report,
        }
    }

    /// A context without progress callbacks (previews, tests).
    pub fn silent(report: JobReport) -> JobContext<'static> {
        JobContext::new(report, CancelFlag::new(), |_| {})
    }

    /// Reports every progress change (tests).
    #[must_use]
    pub fn unthrottled(mut self) -> Self {
        self.interval = Duration::ZERO;
        self
    }

    /// The latest progress.
    pub fn progress(&self) -> &JobProgress {
        &self.progress
    }

    /// The cancellation flag.
    pub fn cancel_flag(&self) -> &CancelFlag {
        &self.cancel
    }

    /// `Err(Cancelled)` once the job was cancelled.
    pub fn check_cancelled(&self) -> Result<()> {
        if self.cancel.is_cancelled() {
            Err(PortabilityError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Starts a phase (always reported); the counters restart.
    pub fn phase(&mut self, phase: JobPhase, total: Option<u64>, bytes_total: Option<u64>) {
        self.progress.phase = phase;
        self.progress.done = 0;
        self.progress.bytes_done = 0;
        self.progress.total = total;
        self.progress.bytes_total = bytes_total;
        self.emit(true);
    }

    /// Sets the totals once known (reported).
    pub fn set_totals(&mut self, total: Option<u64>, bytes_total: Option<u64>) {
        self.progress.total = total;
        self.progress.bytes_total = bytes_total;
        self.emit(true);
    }

    /// Sets what is being worked on (reported when the throttle allows).
    pub fn set_current(&mut self, current: Option<String>) {
        if self.progress.current != current {
            self.progress.current = current;
            self.emit(false);
        }
    }

    /// Counts done items and bytes (reported when the throttle allows).
    pub fn advance(&mut self, items: u64, bytes: u64) {
        self.progress.done += items;
        self.progress.bytes_done += bytes;
        self.emit(false);
    }

    /// Sets the bytes done to an absolute value.
    pub fn set_bytes_done(&mut self, bytes: u64) {
        self.progress.bytes_done = bytes;
        self.emit(false);
    }

    /// Reports the current progress now.
    pub fn flush(&mut self) {
        self.emit(true);
    }

    fn emit(&mut self, force: bool) {
        let now = Instant::now();
        let due = self
            .last_emit
            .is_none_or(|last| now.duration_since(last) >= self.interval);
        if force || due {
            self.last_emit = Some(now);
            (self.callback)(&self.progress);
        }
    }

    /// Records an issue in the report.
    pub fn issue(&mut self, issue: ItemIssue) {
        tracing::debug!(code = issue.code.as_str(), detail = %issue.detail, "portability item issue");
        self.report.push_issue(issue);
    }

    /// Records a skipped item.
    pub fn skipped(
        &mut self,
        code: PortabilityIssueCode,
        item: ItemRef,
        detail: impl Into<String>,
    ) {
        self.issue(ItemIssue::new(code, IssueSeverity::Skipped, item, detail));
    }

    /// Records a changed item.
    pub fn warning(
        &mut self,
        code: PortabilityIssueCode,
        item: ItemRef,
        detail: impl Into<String>,
    ) {
        self.issue(ItemIssue::new(code, IssueSeverity::Warning, item, detail));
    }

    /// Takes the report out (the core finishes it).
    pub fn into_report(self) -> JobReport {
        self.report
    }
}

/// Issues of a preview, capped at [`MAX_PREVIEW_ISSUES`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreviewIssues {
    /// The first issues.
    pub issues: Vec<ItemIssue>,
    /// Issues not kept.
    pub truncated: u64,
}

impl PreviewIssues {
    /// Records an issue.
    pub fn push(&mut self, issue: ItemIssue) {
        if self.issues.len() < MAX_PREVIEW_ISSUES {
            self.issues.push(issue);
        } else {
            self.truncated += 1;
        }
    }
}

/// Ends a report: `completed` / `completed-with-issues` when the job ran
/// to the end, `cancelled` / `failed` from the error otherwise.
pub fn finish_report(report: &mut JobReport, result: &Result<()>, finished_at: impl Into<String>) {
    report.finished_at = finished_at.into();
    match result {
        Ok(()) => report.state = report.finished_state(),
        Err(error) => error.finish_report(report),
    }
}

/// An [`ItemRef`] for a file.
pub fn file_ref(file: impl Into<String>) -> ItemRef {
    ItemRef {
        file: Some(file.into()),
        ..ItemRef::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::portability::JobKind;
    use std::sync::Mutex;

    fn report() -> JobReport {
        JobReport::started("j1".into(), JobKind::Import, "2026-10-02T10:00:00Z")
    }

    #[test]
    fn progress_is_throttled_but_phases_always_report() {
        let seen = Mutex::new(Vec::new());
        {
            let mut ctx = JobContext::new(report(), CancelFlag::new(), |p: &JobProgress| {
                seen.lock().unwrap().push((p.phase, p.done));
            });
            ctx.phase(JobPhase::Reading, Some(1000), None);
            for _ in 0..1000 {
                ctx.advance(1, 10);
            }
            ctx.phase(JobPhase::Finishing, None, None);
            assert_eq!(ctx.progress().done, 0);
        }
        let seen = seen.into_inner().unwrap();
        // The phase changes, and only a few of the thousand advances.
        assert!(seen.len() < 10, "{seen:?}");
        assert_eq!(seen.first(), Some(&(JobPhase::Reading, 0)));
        assert_eq!(seen.last(), Some(&(JobPhase::Finishing, 0)));
    }

    #[test]
    fn unthrottled_reports_everything() {
        let count = Mutex::new(0);
        {
            let mut ctx = JobContext::new(report(), CancelFlag::new(), |_: &JobProgress| {
                *count.lock().unwrap() += 1;
            })
            .unthrottled();
            ctx.phase(JobPhase::Writing, Some(3), None);
            ctx.advance(1, 0);
            ctx.advance(1, 0);
            ctx.set_current(Some("Inbox".into()));
            ctx.set_current(Some("Inbox".into()));
        }
        assert_eq!(*count.lock().unwrap(), 4);
    }

    #[test]
    fn cancellation_and_issues() {
        let flag = CancelFlag::new();
        let mut ctx = JobContext::new(report(), flag.clone(), |_| {});
        assert!(ctx.check_cancelled().is_ok());
        ctx.skipped(PortabilityIssueCode::TooLarge, file_ref("a"), "big");
        ctx.warning(PortabilityIssueCode::UnknownFlag, ItemRef::default(), "x");
        flag.cancel();
        assert!(matches!(
            ctx.check_cancelled(),
            Err(PortabilityError::Cancelled)
        ));
        let report = ctx.into_report();
        assert_eq!(report.counts.skipped, 1);
        assert_eq!(report.issues.len(), 2);
    }

    #[test]
    fn finishing() {
        let mut ok = report();
        finish_report(&mut ok, &Ok(()), "2026-10-02T11:00:00Z");
        assert_eq!(ok.state, nail_api::portability::JobState::Completed);
        assert_eq!(ok.finished_at, "2026-10-02T11:00:00Z");
        let mut failed = report();
        finish_report(&mut failed, &Err(PortabilityError::WrongPassphrase), "x");
        assert_eq!(failed.state, nail_api::portability::JobState::Failed);
    }

    #[test]
    fn preview_issues_are_capped() {
        let mut issues = PreviewIssues::default();
        for _ in 0..(MAX_PREVIEW_ISSUES + 3) {
            issues.push(ItemIssue::new(
                PortabilityIssueCode::MalformedMessage,
                IssueSeverity::Skipped,
                ItemRef::default(),
                "",
            ));
        }
        assert_eq!(issues.issues.len(), MAX_PREVIEW_ISSUES);
        assert_eq!(issues.truncated, 3);
    }
}
