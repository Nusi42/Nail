/**
 * The Settings widgets of this feature (future widgets of ADR 0094):
 * `import-export` and `backup-restore` — buttons that open the wizards and
 * the recent jobs of those kinds (running ones with progress, finished ones
 * with their outcome and "Show report") — and the report dialog the lists
 * open (`portability-report`).
 */
import type { JobKind, JobStatus } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useDateFormat } from "../../i18n/format";
import type { DialogProps } from "../../layout/dialogRegistry";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { useUiStoreApi } from "../../state/uiStore";
import { RestoreSummary } from "./BackupWizard";
import { Proposals } from "./ImportWizard";
import { JobView, Problem } from "./Job";
import { jobKindId, jobPhaseId, jobStateId, progressFraction } from "./model";
import {
  BACKUP_DIALOG,
  EXPORT_DIALOG,
  IMPORT_DIALOG,
  REPORT_DIALOG,
  RESTORE_DIALOG,
} from "./names";
import { useJobs } from "./queries";
import { portabilityStoreFor, usePortabilityState } from "./store";

function JobRow({ job }: { job: JobStatus }) {
  const t = useT();
  const format = useDateFormat();
  const ui = useUiStoreApi();
  const fraction = progressFraction(job.progress);
  return (
    <li data-part="portability.job" data-state={job.state} data-kind={job.kind}>
      <span data-part="portability.job.kind">{t(jobKindId(job.kind))}</span>
      <span data-part="portability.job.path">{job.path}</span>
      <span data-part="portability.job.when">{format.dateTime(job.started_at)}</span>
      {job.state === "running" ? (
        <>
          <span data-part="portability.job.state">{t(jobPhaseId(job.progress.phase))}</span>
          <progress
            data-part="portability.job.progress"
            aria-label={t("portability-progress-label")}
            max={1}
            {...(fraction === null ? {} : { value: fraction })}
          />
        </>
      ) : (
        <span data-part="portability.job.state">{t(jobStateId(job.state))}</span>
      )}
      <button
        type="button"
        data-part="form.button"
        data-action="show-report"
        onClick={() => {
          portabilityStoreFor(ui).getState().showReport(job.job_id);
          ui.getState().openWindow(REPORT_DIALOG);
        }}
      >
        {t(
          job.state === "running" ? "portability-job-show-progress" : "portability-job-show-report",
        )}
      </button>
    </li>
  );
}

/** The recent jobs of some kinds. */
export function JobList({ kinds, titleId }: { kinds: readonly JobKind[]; titleId: string }) {
  const t = useT();
  const jobs = useJobs();
  const shown = (jobs.data ?? []).filter((j) => kinds.includes(j.kind));
  return (
    <section data-part="portability.jobs" aria-label={t(titleId)}>
      <h4 data-part="form.section.title">{t(titleId)}</h4>
      {jobs.isError ? (
        <p data-part="form.hint">{t("portability-jobs-unavailable")}</p>
      ) : shown.length === 0 ? (
        <p data-part="form.hint">{t("portability-jobs-none")}</p>
      ) : (
        <ul data-part="portability.jobs.list">
          {shown.map((job) => (
            <JobRow key={job.job_id} job={job} />
          ))}
        </ul>
      )}
    </section>
  );
}

function OpenButton({
  dialog,
  labelId,
  action,
}: {
  dialog: string;
  labelId: string;
  action: string;
}) {
  const t = useT();
  const ui = useUiStoreApi();
  return (
    <button
      type="button"
      data-part="form.button"
      data-action={action}
      onClick={() => {
        ui.getState().openWindow(dialog);
      }}
    >
      {t(labelId)}
    </button>
  );
}

/** Widget `import-export` (option `show-jobs`, default on). */
export function ImportExportWidget({ node }: WidgetProps) {
  const t = useT();
  return (
    <section data-part="import-export" data-widget="import-export">
      <h3 data-part="form.section.title">{t("portability-import-export-title")}</h3>
      <p data-part="form.hint">{t("portability-import-export-text")}</p>
      <div data-part="portability.buttons">
        <OpenButton dialog={IMPORT_DIALOG} labelId="portability-open-import" action="open-import" />
        <OpenButton dialog={EXPORT_DIALOG} labelId="portability-open-export" action="open-export" />
      </div>
      {optionBool(node, "show-jobs", true) && (
        <JobList kinds={["import", "export"]} titleId="portability-jobs-import-export" />
      )}
    </section>
  );
}

/** Widget `backup-restore` (option `show-jobs`, default on). */
export function BackupRestoreWidget({ node }: WidgetProps) {
  const t = useT();
  return (
    <section data-part="backup-restore" data-widget="backup-restore">
      <h3 data-part="form.section.title">{t("portability-backup-restore-title")}</h3>
      <p data-part="form.hint">{t("portability-backup-restore-text")}</p>
      <div data-part="portability.buttons">
        <OpenButton dialog={BACKUP_DIALOG} labelId="portability-open-backup" action="open-backup" />
        <OpenButton
          dialog={RESTORE_DIALOG}
          labelId="portability-open-restore"
          action="open-restore"
        />
      </div>
      {optionBool(node, "show-jobs", true) && (
        <JobList kinds={["backup", "verify-backup", "restore"]} titleId="portability-jobs-backup" />
      )}
    </section>
  );
}

/** Dialog `portability-report`: the job the lists chose. */
export function ReportDialog({ close }: DialogProps) {
  const t = useT();
  const jobId = usePortabilityState((s) => s.reportJobId);
  return (
    <div data-part="portability.report-dialog">
      {jobId === null ? (
        <Problem problem={{ id: "portability-job-gone" }} part="form.hint" />
      ) : (
        <JobView jobId={jobId}>
          {(report) => (
            <>
              <Proposals accounts={report.accounts} rules={report.rules} />
              {report.restore && <RestoreSummary report={report.restore} />}
            </>
          )}
        </JobView>
      )}
      <div data-part="wizard.buttons">
        <button type="button" data-part="form.button" data-action="close" onClick={close}>
          {t("portability-close")}
        </button>
      </div>
    </div>
  );
}
