/**
 * Backups (ADR 0094, "Backup and restore"):
 *
 * - **Backup wizard** (dialog `backup-wizard`, action `backup.create`): what
 *   goes in (everything but the encryption keys by default; the keys only
 *   when ticked, with the warning), a passphrase typed twice (at least 12
 *   characters; never stored), the `.nailbackup` file, the job and its
 *   report with the archive's contents, then "Check this backup" (reads
 *   every entry back: `VerifyBackup`).
 * - **Restore wizard** (dialog `restore-wizard`, action `backup.restore`):
 *   open an archive with its passphrase (`InspectBackup`), optionally check
 *   it, choose the items, review the **dry run** (`PreviewRestore`: per
 *   item replace / add what is missing / add / leave as is, problems,
 *   accounts that need their passwords, restart) and only then restore.
 *   Nothing is deleted; replaced configuration files keep a backup.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import type {
  BackupItem,
  BackupManifest,
  LocalizedMessage,
  RestoreAction,
  RestorePlan,
  RestoreReport,
} from "../../api/generated";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { useDateFormat } from "../../i18n/format";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useClient } from "../../state/queries";
import { CheckGroup, StepHeader, TextField, WizardButtons } from "./Fields";
import { Counts, IssueList, JobView, Problem } from "./Job";
import {
  BACKUP_FILE_EXTENSION,
  BACKUP_FILTERS,
  BACKUP_ITEMS,
  backupContentsIssues,
  backupIssueId,
  backupItemId,
  defaultBackupContents,
  formatBytes,
  isAbsolutePath,
  passphraseIssues,
  plainText,
  restoreEffectId,
} from "./model";
import { PathField } from "./PathField";
import { rememberJob } from "./queries";

/** What an archive holds: when, by which Nail, items with sizes. */
export function ManifestSummary({ manifest }: { manifest: BackupManifest }) {
  const t = useT();
  const locale = useLocale();
  const format = useDateFormat();
  return (
    <section data-part="portability.manifest" aria-label={t("portability-manifest-title")}>
      <h4 data-part="portability.section.title">{t("portability-manifest-title")}</h4>
      <p data-part="portability.manifest.created">
        {t("portability-manifest-created", {
          when: format.dateTime(manifest.created_at),
          version: manifest.app_version,
        })}
      </p>
      <ul data-part="portability.manifest.items">
        {manifest.contents.items.map((item) => {
          const entries = manifest.entries.filter((e) => e.item === item);
          return (
            <li key={item} data-part="portability.manifest.item" data-item={item}>
              {t("portability-manifest-item", {
                item: t(backupItemId(item)),
                count: entries.length,
                size: formatBytes(
                  entries.reduce((n, e) => n + e.size_bytes, 0),
                  locale,
                ),
              })}
            </li>
          );
        })}
      </ul>
      <Counts counts={manifest.counts} />
      {manifest.accounts.length > 0 && (
        <p data-part="form.hint" data-field="accounts">
          {t("portability-manifest-accounts", { count: manifest.accounts.length })}
        </p>
      )}
    </section>
  );
}

function ItemChoices({
  items,
  selected,
  onChange,
  legend,
}: {
  items: readonly BackupItem[];
  selected: BackupItem[];
  onChange(items: BackupItem[]): void;
  legend: string;
}) {
  const t = useT();
  return (
    <CheckGroup
      legend={legend}
      field="items"
      selected={selected}
      onChange={onChange}
      choices={items.map((item) => ({
        value: item,
        label: t(backupItemId(item)),
        hint: t(`portability-backup-item-hint-${item}`),
      }))}
    />
  );
}

function defaultBackupName(t: (id: string, args?: Record<string, string>) => string): string {
  const day = new Date().toISOString().slice(0, 10);
  return `${plainText(t("portability-backup-default-name", { date: day }))}.${BACKUP_FILE_EXTENSION}`;
}

/** The backup wizard's content. */
export function BackupWizard({ close }: { close(): void }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const [items, setItems] = useState<BackupItem[]>(() => defaultBackupContents().items);
  const [passphrase, setPassphrase] = useState("");
  const [repeat, setRepeat] = useState("");
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [jobId, setJobId] = useState<string | null>(null);
  const [verifyId, setVerifyId] = useState<string | null>(null);
  const contentIssue = backupContentsIssues({ items })[0];
  const passIssue = passphrase === "" ? undefined : passphraseIssues(passphrase)[0];
  const mismatch = repeat !== "" && repeat !== passphrase;
  const ready =
    !contentIssue &&
    passphrase !== "" &&
    passphraseIssues(passphrase).length === 0 &&
    repeat === passphrase &&
    isAbsolutePath(path);

  const run = async (verify: boolean) => {
    setBusy(true);
    setProblem(null);
    try {
      const answer = await client.request(
        verify
          ? { type: "VerifyBackup", path: path.trim(), passphrase }
          : { type: "CreateBackup", path: path.trim(), contents: { items }, passphrase },
        "PortabilityJob",
      );
      rememberJob(queryClient, answer.job);
      if (verify) setVerifyId(answer.job.job_id);
      else setJobId(answer.job.job_id);
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };

  if (jobId) {
    return (
      <div data-part="wizard" data-wizard="backup" data-step="job">
        <div data-part="wizard.step-body" data-step="job">
          <StepHeader step={2} steps={2} titleId="portability-backup-job-title" />
          <JobView jobId={jobId}>
            {(report) => (
              <>
                {report.backup && <ManifestSummary manifest={report.backup} />}
                {report.state !== "failed" && report.state !== "cancelled" && !verifyId && (
                  <button
                    type="button"
                    data-part="form.button"
                    data-action="verify"
                    disabled={busy}
                    onClick={() => void run(true)}
                  >
                    {t("portability-backup-verify")}
                  </button>
                )}
              </>
            )}
          </JobView>
          {verifyId && (
            <section data-part="portability.verify" aria-label={t("portability-verify-title")}>
              <h4 data-part="portability.section.title">{t("portability-verify-title")}</h4>
              <JobView jobId={verifyId} />
            </section>
          )}
          {problem && <Problem problem={problem} />}
          <WizardButtons>
            <button type="button" data-part="form.button" data-action="close" onClick={close}>
              {t("portability-close")}
            </button>
          </WizardButtons>
        </div>
      </div>
    );
  }

  return (
    <div data-part="wizard" data-wizard="backup" data-step="options">
      <div data-part="wizard.step-body" data-step="options">
        <StepHeader step={1} steps={2} titleId="portability-backup-title" />
        <p data-part="wizard.intro">{t("portability-backup-intro")}</p>
        <ItemChoices
          items={BACKUP_ITEMS}
          selected={items}
          onChange={setItems}
          legend={t("portability-backup-items")}
        />
        {items.includes("keys") && (
          <p data-part="form.hint" data-tone="warning" data-field="keys-warning">
            {t("portability-backup-keys-warning")}
          </p>
        )}
        {contentIssue && (
          <p data-part="form.hint" data-tone="danger">
            {t(backupIssueId(contentIssue))}
          </p>
        )}
        <TextField
          label={t("portability-backup-passphrase")}
          field="passphrase"
          type="password"
          autoComplete="new-password"
          value={passphrase}
          onChange={setPassphrase}
          invalid={passIssue !== undefined}
          hint={t(passIssue ? backupIssueId(passIssue) : "portability-backup-passphrase-hint")}
        />
        <TextField
          label={t("portability-backup-passphrase-repeat")}
          field="passphrase-repeat"
          type="password"
          autoComplete="new-password"
          value={repeat}
          onChange={setRepeat}
          invalid={mismatch}
          {...(mismatch ? { hint: t("portability-backup-passphrase-mismatch") } : {})}
        />
        <PathField
          labelId="portability-backup-file"
          field="backup-path"
          value={path}
          onChange={setPath}
          kind="save-file"
          titleId="portability-backup-pick-title"
          filters={BACKUP_FILTERS}
          defaultPath={defaultBackupName(t)}
        />
        {problem && <Problem problem={problem} />}
        <WizardButtons>
          <button
            type="button"
            data-part="form.button"
            data-variant="primary"
            data-action="start"
            disabled={busy || !ready}
            onClick={() => void run(false)}
          >
            {t("portability-backup-start")}
          </button>
        </WizardButtons>
      </div>
    </div>
  );
}

function PlanRow({ action }: { action: RestoreAction }) {
  const t = useT();
  return (
    <tr data-part="portability.plan.row" data-effect={action.effect} data-item={action.item}>
      <th scope="row" data-part="portability.plan.item">
        {t(backupItemId(action.item))}
      </th>
      <td data-part="portability.plan.effect">{t(restoreEffectId(action.effect))}</td>
      <td data-part="portability.plan.numbers">
        {t("portability-plan-numbers", {
          added: action.added,
          replaced: action.replaced,
          unchanged: action.unchanged,
        })}
      </td>
      <td data-part="portability.plan.note">
        {action.note ? t(action.note.id, action.note.args) : ""}
      </td>
    </tr>
  );
}

/** The dry run's per-item table (also the restore report's). */
export function PlanTable({
  actions,
  captionId,
}: {
  actions: readonly RestoreAction[];
  captionId: string;
}) {
  const t = useT();
  return (
    <table data-part="portability.plan">
      <caption data-part="portability.plan.caption">{t(captionId)}</caption>
      <thead>
        <tr>
          <th scope="col" data-part="portability.plan.head">
            {t("portability-plan-item")}
          </th>
          <th scope="col" data-part="portability.plan.head">
            {t("portability-plan-effect")}
          </th>
          <th scope="col" data-part="portability.plan.head">
            {t("portability-plan-what")}
          </th>
          <th scope="col" data-part="portability.plan.head">
            {t("portability-plan-note")}
          </th>
        </tr>
      </thead>
      <tbody>
        {actions.map((a) => (
          <PlanRow key={a.item} action={a} />
        ))}
      </tbody>
    </table>
  );
}

function RestoreSummary({ report }: { report: RestoreReport }) {
  const t = useT();
  return (
    <section data-part="portability.restore-report">
      <PlanTable actions={report.actions} captionId="portability-restore-report-caption" />
      {report.config_backups.length > 0 && (
        <p data-part="form.hint" data-field="config-backups">
          {t("portability-restore-config-backups", { count: report.config_backups.length })}
        </p>
      )}
      {report.accounts_needing_secrets.length > 0 && (
        <p data-part="form.hint" data-tone="warning" data-field="passwords">
          {t("portability-restore-passwords", {
            accounts: report.accounts_needing_secrets.join(", "),
          })}
        </p>
      )}
      {report.restart_required && (
        <p data-part="form.hint" data-tone="warning" data-field="restart">
          {t("portability-restore-restart")}
        </p>
      )}
    </section>
  );
}

type RestoreStep = "open" | "choose" | "plan" | "job";

/** The restore wizard's content. */
export function RestoreWizard({ close }: { close(): void }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const [step, setStep] = useState<RestoreStep>("open");
  const [path, setPath] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [manifest, setManifest] = useState<BackupManifest | null>(null);
  const [items, setItems] = useState<BackupItem[]>([]);
  const [plan, setPlan] = useState<RestorePlan | null>(null);
  const [verifyId, setVerifyId] = useState<string | null>(null);
  const [jobId, setJobId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);

  const guarded = async (work: () => Promise<void>) => {
    setBusy(true);
    setProblem(null);
    try {
      await work();
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };

  const open = () =>
    guarded(async () => {
      const answer = await client.request(
        { type: "InspectBackup", path: path.trim(), passphrase },
        "BackupManifest",
      );
      setManifest(answer.manifest);
      setItems([...answer.manifest.contents.items]);
      setVerifyId(null);
      setStep("choose");
    });

  const verify = () =>
    guarded(async () => {
      const answer = await client.request(
        { type: "VerifyBackup", path: path.trim(), passphrase },
        "PortabilityJob",
      );
      rememberJob(queryClient, answer.job);
      setVerifyId(answer.job.job_id);
    });

  const dryRun = () =>
    guarded(async () => {
      const answer = await client.request(
        { type: "PreviewRestore", path: path.trim(), passphrase, contents: { items } },
        "RestorePlan",
      );
      setPlan(answer.plan);
      setStep("plan");
    });

  const restore = () =>
    guarded(async () => {
      const answer = await client.request(
        { type: "StartRestore", path: path.trim(), passphrase, contents: { items } },
        "PortabilityJob",
      );
      rememberJob(queryClient, answer.job);
      setJobId(answer.job.job_id);
      setStep("job");
    });

  return (
    <div data-part="wizard" data-wizard="restore" data-step={step}>
      {step === "open" && (
        <div data-part="wizard.step-body" data-step="open">
          <StepHeader step={1} steps={4} titleId="portability-restore-open-title" />
          <p data-part="wizard.intro">{t("portability-restore-intro")}</p>
          <PathField
            labelId="portability-restore-file"
            field="backup-path"
            value={path}
            onChange={setPath}
            kind="open-file"
            titleId="portability-restore-pick-title"
            filters={BACKUP_FILTERS}
          />
          <TextField
            label={t("portability-restore-passphrase")}
            field="passphrase"
            type="password"
            autoComplete="current-password"
            value={passphrase}
            onChange={setPassphrase}
          />
          {problem && <Problem problem={problem} />}
          <WizardButtons>
            <button
              type="button"
              data-part="form.button"
              data-variant="primary"
              data-action="open"
              disabled={busy || passphrase === "" || !isAbsolutePath(path)}
              onClick={() => void open()}
            >
              {t("portability-restore-open")}
            </button>
          </WizardButtons>
        </div>
      )}
      {step === "choose" && manifest && (
        <div data-part="wizard.step-body" data-step="choose">
          <StepHeader step={2} steps={4} titleId="portability-restore-choose-title" />
          <ManifestSummary manifest={manifest} />
          {verifyId ? (
            <section data-part="portability.verify" aria-label={t("portability-verify-title")}>
              <h4 data-part="portability.section.title">{t("portability-verify-title")}</h4>
              <JobView jobId={verifyId} />
            </section>
          ) : (
            <button
              type="button"
              data-part="form.button"
              data-action="verify"
              disabled={busy}
              onClick={() => void verify()}
            >
              {t("portability-restore-verify")}
            </button>
          )}
          <ItemChoices
            items={manifest.contents.items}
            selected={items}
            onChange={setItems}
            legend={t("portability-restore-items")}
          />
          {backupContentsIssues({ items })[0] && (
            <p data-part="form.hint" data-tone="danger">
              {t(backupIssueId("nothing-chosen"))}
            </p>
          )}
          {problem && <Problem problem={problem} />}
          <WizardButtons>
            <button
              type="button"
              data-part="form.button"
              data-action="back"
              disabled={busy}
              onClick={() => setStep("open")}
            >
              {t("portability-back")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-variant="primary"
              data-action="dry-run"
              disabled={busy || items.length === 0}
              onClick={() => void dryRun()}
            >
              {t("portability-restore-dry-run")}
            </button>
          </WizardButtons>
        </div>
      )}
      {step === "plan" && plan && (
        <div data-part="wizard.step-body" data-step="plan">
          <StepHeader step={3} steps={4} titleId="portability-restore-plan-title" />
          <p data-part="wizard.intro">{t("portability-restore-plan-intro")}</p>
          <PlanTable actions={plan.actions} captionId="portability-restore-plan-caption" />
          <IssueList issues={plan.issues} truncated={0} titleId="portability-restore-plan-issues" />
          {plan.accounts_needing_secrets.length > 0 && (
            <p data-part="form.hint" data-tone="warning" data-field="passwords">
              {t("portability-restore-passwords", {
                accounts: plan.accounts_needing_secrets.join(", "),
              })}
            </p>
          )}
          {plan.restart_required && (
            <p data-part="form.hint" data-tone="warning" data-field="restart">
              {t("portability-restore-restart-planned")}
            </p>
          )}
          <p data-part="form.hint" data-field="nothing-deleted">
            {t("portability-restore-nothing-deleted")}
          </p>
          {problem && <Problem problem={problem} />}
          <WizardButtons>
            <button
              type="button"
              data-part="form.button"
              data-action="back"
              disabled={busy}
              onClick={() => setStep("choose")}
            >
              {t("portability-back")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-variant="danger"
              data-action="restore"
              disabled={busy}
              onClick={() => void restore()}
            >
              {t("portability-restore-start")}
            </button>
          </WizardButtons>
        </div>
      )}
      {step === "job" && jobId && (
        <div data-part="wizard.step-body" data-step="job">
          <StepHeader step={4} steps={4} titleId="portability-restore-job-title" />
          <JobView jobId={jobId}>
            {(report) => (report.restore ? <RestoreSummary report={report.restore} /> : null)}
          </JobView>
          <WizardButtons>
            <button type="button" data-part="form.button" data-action="close" onClick={close}>
              {t("portability-close")}
            </button>
          </WizardButtons>
        </div>
      )}
    </div>
  );
}

export function BackupWizardDialog({ close }: DialogProps) {
  return <BackupWizard close={close} />;
}

export function RestoreWizardDialog({ close }: DialogProps) {
  return <RestoreWizard close={close} />;
}

export { RestoreSummary };
