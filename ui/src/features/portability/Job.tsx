/**
 * A job's progress and its report, shared by every wizard and the job
 * lists: the phase, items and bytes with a progress bar (indeterminate
 * while the total is unknown), Cancel while it runs; then the outcome, what
 * was done (counted), and every per-item issue (what, where, the core's
 * detail on demand), capped like the core caps them. Wizards add their own
 * part of the report (Thunderbird proposals, the restore summary) below.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { errorMessageId, errorMessageOf } from "../../api/client";
import type {
  ItemIssue,
  JobCounts,
  JobReport,
  JobStatus,
  LocalizedMessage,
} from "../../api/generated";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { useDateFormat } from "../../i18n/format";
import { useClient } from "../../state/queries";
import {
  COUNT_KEYS,
  countId,
  formatBytes,
  issueCodeId,
  issueLocation,
  jobKindId,
  jobPhaseId,
  jobStateId,
  progressFraction,
  severityId,
} from "./model";
import { rememberJob, useJob } from "./queries";
import { hasText } from "../../text/empty";

/** A problem text from the core (`LocalizedMessage`). */
export function Problem({
  problem,
  part = "form.error",
}: {
  problem: LocalizedMessage;
  part?: string;
}) {
  const t = useT();
  return (
    <p data-part={part} role="alert">
      {t(problem.id, problem.args)}
    </p>
  );
}

/** Non-zero counts as a list (`312 messages`, `2 folders`). */
export function Counts({
  counts,
  part = "portability.counts",
}: {
  counts: JobCounts;
  part?: string;
}) {
  const t = useT();
  const shown = COUNT_KEYS.filter((k) => counts[k] > 0);
  if (shown.length === 0) {
    return (
      <p data-part={part} data-empty="">
        {t("portability-counts-none")}
      </p>
    );
  }
  return (
    <ul data-part={part}>
      {shown.map((key) => (
        <li key={key} data-part="portability.count" data-count={key}>
          {t(countId(key), { count: counts[key] })}
        </li>
      ))}
    </ul>
  );
}

function IssueRow({ issue }: { issue: ItemIssue }) {
  const t = useT();
  const where = issueLocation(issue);
  return (
    <li data-part="portability.issue" data-severity={issue.severity} data-code={issue.code}>
      <span data-part="portability.issue.severity">{t(severityId(issue.severity))}</span>
      <span data-part="portability.issue.text">
        {t(issue.localized.id || issueCodeId(issue.code), issue.localized.args)}
      </span>
      {(hasText(issue.item.label) || hasText(where) || issue.item.index !== null) && (
        <span data-part="portability.issue.where">
          {issue.item.label && <span data-part="portability.issue.label">{issue.item.label}</span>}
          {where && <span data-part="portability.issue.path">{where}</span>}
          {issue.item.index !== null && (
            <span data-part="portability.issue.index">
              {t("portability-issue-index", { index: issue.item.index })}
            </span>
          )}
        </span>
      )}
      {issue.detail !== "" && (
        <details data-part="portability.issue.details">
          <summary data-part="portability.issue.details.summary">
            {t("portability-issue-details")}
          </summary>
          <code data-part="portability.issue.detail">{issue.detail}</code>
        </details>
      )}
    </li>
  );
}

/** Per-item issues with the "and N more" note. */
export function IssueList({
  issues,
  truncated,
  titleId = "portability-issues-title",
}: {
  issues: readonly ItemIssue[];
  truncated: number;
  titleId?: string;
}) {
  const t = useT();
  if (issues.length === 0 && truncated === 0) return null;
  return (
    <section data-part="portability.issues" aria-label={t(titleId)}>
      <h4 data-part="portability.section.title">
        <span data-part="portability.section.title.text">{t(titleId)}</span>
        <span data-part="portability.issues.count">{issues.length + truncated}</span>
      </h4>
      <ul data-part="portability.issues.list">
        {issues.map((issue, index) => (
          <IssueRow key={index} issue={issue} />
        ))}
      </ul>
      {truncated > 0 && (
        <p data-part="form.hint" data-field="issues-truncated">
          {t("portability-issues-truncated", { count: truncated })}
        </p>
      )}
    </section>
  );
}

/** The report of a finished job. */
export function JobReportView({ report, children }: { report: JobReport; children?: ReactNode }) {
  const t = useT();
  const format = useDateFormat();
  return (
    <section data-part="portability.report" data-state={report.state} data-kind={report.kind}>
      <div data-part="portability.report.heading" role="status">
        <h3 data-part="portability.report.title">
          {t("portability-report-title", {
            kind: t(jobKindId(report.kind)),
            state: t(jobStateId(report.state)),
          })}
        </h3>
      </div>
      <p data-part="portability.report.when">
        {t("portability-report-when", {
          started: format.dateTime(report.started_at),
          finished: format.dateTime(report.finished_at),
        })}
      </p>
      {(report.source ?? report.target) && (
        <p data-part="portability.report.path">{report.source ?? report.target}</p>
      )}
      {report.state === "failed" && (
        <Problem
          problem={
            report.localized ??
            (report.error ? { id: errorMessageId(report.error) } : { id: "portability-job-failed" })
          }
        />
      )}
      <Counts counts={report.counts} />
      <IssueList issues={report.issues} truncated={report.issues_truncated} />
      {children}
    </section>
  );
}

/** Progress of a running job with Cancel. */
export function JobProgressView({ job }: { job: JobStatus }) {
  const t = useT();
  const locale = useLocale();
  const client = useClient();
  const queryClient = useQueryClient();
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const progress = job.progress;
  const fraction = progressFraction(progress);

  const cancel = async () => {
    setCancelling(true);
    setProblem(null);
    try {
      const answer = await client.request(
        { type: "CancelPortabilityJob", job_id: job.job_id },
        "PortabilityJob",
      );
      rememberJob(queryClient, answer.job);
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setCancelling(false);
    }
  };

  return (
    <section data-part="portability.progress" data-phase={progress.phase} data-kind={job.kind}>
      <p data-part="portability.progress.phase" role="status">
        {t("portability-progress-phase", {
          kind: t(jobKindId(job.kind)),
          phase: t(jobPhaseId(progress.phase)),
        })}
      </p>
      <progress
        data-part="portability.progress.bar"
        aria-label={t("common-progress")}
        max={1}
        {...(fraction === null ? {} : { value: fraction })}
      />
      {progress.total !== null && (
        <p data-part="portability.progress.items">
          {t("portability-progress-bytes", { done: progress.done, total: progress.total })}
        </p>
      )}
      {progress.bytes_done > 0 && (
        <p data-part="portability.progress.bytes">
          {progress.bytes_total === null
            ? formatBytes(progress.bytes_done, locale)
            : t("portability-progress-bytes", {
                done: formatBytes(progress.bytes_done, locale),
                total: formatBytes(progress.bytes_total, locale),
              })}
        </p>
      )}
      {progress.current && (
        <p data-part="portability.progress.current">
          {t("portability-progress-current", { name: progress.current })}
        </p>
      )}
      <div data-part="portability.buttons">
        <button
          type="button"
          data-part="form.button"
          data-action="cancel-job"
          disabled={cancelling}
          onClick={() => void cancel()}
        >
          {t("common-cancel")}
        </button>
      </div>
      <p data-part="form.hint">{t("portability-cancel-hint")}</p>
      {problem && <Problem problem={problem} />}
    </section>
  );
}

/** A job by id: progress while it runs, then the report (+ `children`). */
export function JobView({
  jobId,
  children,
}: {
  jobId: string;
  children?: (report: JobReport) => ReactNode;
}) {
  const t = useT();
  const job = useJob(jobId);
  if (job.isError) return <Problem problem={errorMessageOf(job.error)} />;
  if (!job.data) {
    return (
      <p data-part="portability.loading" role="status">
        {t("common-loading")}
      </p>
    );
  }
  const report = job.data.report;
  if (job.data.state === "running" || !report) return <JobProgressView job={job.data} />;
  return <JobReportView report={report}>{children?.(report)}</JobReportView>;
}
