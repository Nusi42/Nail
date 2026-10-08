/**
 * The import wizard (dialog `import-wizard`, action `import.open`):
 *
 * 1. **Source** — a Thunderbird profile found on this computer
 *    (`FindImportSources`) or a format and a path (typed or chosen with the
 *    system dialog).
 * 2. **What and where** — the preview (`PreviewImport`, nothing written):
 *    folders and collections with counts to choose from, problems found so
 *    far, where mail goes ("Local Folders" or under a folder of an
 *    account, through its operation queue), duplicate skipping and flags.
 * 3. **Import** — the job with progress, then its report with every
 *    per-item issue; for Thunderbird the proposed accounts and filters to
 *    add (`AddAccount` without passwords / `SaveRule`, never automatic).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import type {
  AccountProposal,
  ImportFormat,
  ImportPreview,
  JobReport,
  LocalizedMessage,
  RuleProposal,
} from "../../api/generated";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { DATE_ONLY, useDateFormat } from "../../i18n/format";
import type { DialogProps } from "../../layout/dialogRegistry";
import { queryKeys, useAccounts, useClient, useFolders } from "../../state/queries";
import {
  Check,
  CheckGroup,
  RadioGroup,
  SelectField,
  StepHeader,
  TextField,
  WizardButtons,
} from "./Fields";
import { Counts, IssueList, JobView, Problem } from "./Job";
import {
  IMPORT_FORMATS,
  baseName,
  formatBytes,
  importFilters,
  importFormatId,
  importHasMail,
  importIsDirectory,
  isAbsolutePath,
} from "./model";
import { PathField } from "./PathField";
import { rememberJob, useAddressBooks, useCalendars, useImportSources } from "./queries";
import { useBusyAction } from "../../state/busyAction";
import { CloseButton } from "../../widgets/CloseButton";

type Step = "source" | "options" | "job";

/** `null` = "Local Folders". */
type MailTarget = string | null;

function SourceStep({
  format,
  setFormat,
  path,
  setPath,
  busy,
  problem,
  onNext,
  onUseFound,
}: {
  format: ImportFormat;
  setFormat: (f: ImportFormat) => void;
  path: string;
  setPath: (p: string) => void;
  busy: boolean;
  problem: LocalizedMessage | null;
  onNext: () => void;
  onUseFound: (format: ImportFormat, path: string) => void;
}) {
  const t = useT();
  const dates = useDateFormat();
  const found = useImportSources();
  const sources = found.data ?? [];
  return (
    <div data-part="wizard.step-body" data-step="source">
      <StepHeader step={1} steps={3} titleId="portability-import-source-title" />
      <p data-part="wizard.intro">{t("portability-import-intro")}</p>
      {sources.length > 0 && (
        <section data-part="form.section" aria-label={t("portability-import-found")}>
          <h4 data-part="form.section.title">{t("portability-import-found")}</h4>
          <ul data-part="portability.found">
            {sources.map((s) => (
              <li key={s.source.path} data-part="portability.found.item">
                <span data-part="portability.found.label">
                  {t("portability-import-found-profile", { name: s.label })}
                </span>
                {s.is_default && (
                  <span data-part="portability.found.default">
                    {t("portability-import-found-default")}
                  </span>
                )}
                {s.last_used_at && (
                  <span data-part="portability.found.used">
                    {t("portability-import-found-used", {
                      when: dates.dateTime(s.last_used_at, DATE_ONLY),
                    })}
                  </span>
                )}
                <button
                  type="button"
                  data-part="form.button"
                  data-action="use-found"
                  disabled={busy}
                  onClick={() => {
                    onUseFound(s.source.format, s.source.path);
                  }}
                >
                  {t("portability-import-found-use")}
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      <RadioGroup
        legend={t("portability-import-format")}
        field="format"
        value={format}
        onChange={setFormat}
        choices={IMPORT_FORMATS.map((f) => ({
          value: f,
          label: t(importFormatId(f)),
          hint: t(`portability-import-format-hint-${f}`),
        }))}
      />
      <PathField
        labelId={
          importIsDirectory(format) ? "portability-import-folder" : "portability-import-file"
        }
        field="source-path"
        value={path}
        onChange={setPath}
        kind={importIsDirectory(format) ? "open-folder" : "open-file"}
        titleId="portability-import-pick-title"
        filters={importFilters(format)}
      />
      {problem && <Problem problem={problem} />}
      <WizardButtons>
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          data-action="next"
          disabled={busy || !isAbsolutePath(path)}
          onClick={onNext}
        >
          {t(busy ? "portability-reading" : "portability-import-read")}
        </button>
      </WizardButtons>
    </div>
  );
}

function TargetFields({
  target,
  setTarget,
  parentId,
  setParentId,
  folderName,
  setFolderName,
}: {
  target: MailTarget;
  setTarget: (t: MailTarget) => void;
  parentId: string | null;
  setParentId: (id: string | null) => void;
  folderName: string;
  setFolderName: (name: string) => void;
}) {
  const t = useT();
  const accounts = useAccounts();
  const real = (accounts.data ?? []).filter((a) => a.kind !== "virtual");
  const folders = useFolders(target);
  return (
    <fieldset data-part="form.section" data-field="mail-target">
      <legend data-part="form.section.title">{t("portability-import-target")}</legend>
      <SelectField
        label={t("common-account")}
        field="target-account"
        value={target ?? ""}
        onChange={(v) => {
          setTarget(v === "" ? null : v);
          setParentId(null);
        }}
      >
        <option value="">{t("portability-import-target-local")}</option>
        {real.map((a) => (
          <option key={a.id} value={a.id}>
            {t("portability-account-option", {
              name: a.name,
              email: a.email,
              hasEmail: a.email ? "yes" : "no",
            })}
          </option>
        ))}
      </SelectField>
      <p data-part="form.hint">
        {t(
          target === null
            ? "portability-import-target-local-hint"
            : "portability-import-target-account-hint",
        )}
      </p>
      {target !== null && (
        <SelectField
          label={t("portability-import-target-parent")}
          field="target-parent"
          value={parentId ?? ""}
          onChange={(v) => {
            setParentId(v === "" ? null : v);
          }}
        >
          <option value="">{t("portability-import-target-inbox")}</option>
          {(folders.data ?? []).map((f) => (
            <option key={f.id} value={f.id}>
              {f.path}
            </option>
          ))}
        </SelectField>
      )}
      {target === null && (
        <TextField
          label={t("portability-import-folder-name")}
          field="folder-name"
          value={folderName}
          onChange={setFolderName}
          hint={t("portability-import-folder-name-hint")}
        />
      )}
    </fieldset>
  );
}

/** What the options step hands to `onStart`. */
type StartOptions = Parameters<Parameters<typeof OptionsStep>[0]["onStart"]>[0];

function OptionsStep({
  preview,
  busy,
  problem,
  onBack,
  onStart,
}: {
  preview: ImportPreview;
  busy: boolean;
  problem: LocalizedMessage | null;
  onBack: () => void;
  onStart: (options: {
    folders: string[] | null;
    collections: string[] | null;
    target: MailTarget;
    parentId: string | null;
    folderName: string;
    skipDuplicates: boolean;
    keepFlags: boolean;
    calendarId: string | null;
    addressBookId: string | null;
  }) => void;
}) {
  const t = useT();
  const locale = useLocale();
  const [folders, setFolders] = useState(() => preview.folders.map((f) => f.path));
  const [collections, setCollections] = useState(() => preview.collections.map((c) => c.key));
  const [target, setTarget] = useState<MailTarget>(null);
  const [parentId, setParentId] = useState<string | null>(null);
  const [folderName, setFolderName] = useState(() =>
    t("portability-import-folder-default", { name: baseName(preview.source.path) }),
  );
  const [skipDuplicates, setSkipDuplicates] = useState(true);
  const [keepFlags, setKeepFlags] = useState(true);
  const [calendarId, setCalendarId] = useState<string | null>(null);
  const [addressBookId, setAddressBookId] = useState<string | null>(null);
  const hasEvents = preview.collections.some((c) => c.kind !== "address-book");
  const hasContacts = preview.collections.some((c) => c.kind === "address-book");
  const calendars = useCalendars();
  const books = useAddressBooks();
  const nothing = folders.length === 0 && collections.length === 0;
  const mail = importHasMail(preview.source.format) && preview.folders.length > 0;

  return (
    <div data-part="wizard.step-body" data-step="options">
      <StepHeader step={2} steps={3} titleId="portability-import-options-title" />
      <p data-part="wizard.intro">
        {t("portability-import-preview-intro", {
          format: t(importFormatId(preview.source.format)),
        })}
      </p>
      <Counts counts={preview.counts} part="portability.preview-counts" />
      {preview.folders.length > 0 && (
        <CheckGroup
          legend={t("common-folders")}
          field="folders"
          selected={folders}
          onChange={setFolders}
          choices={preview.folders.map((f) => ({
            value: f.path,
            label: f.path,
            hint: t("portability-import-folder-size", {
              count: f.messages,
              size: formatBytes(f.size_bytes, locale),
            }),
          }))}
        />
      )}
      {preview.collections.length > 0 && (
        <CheckGroup
          legend={t("common-pim-sources")}
          field="collections"
          selected={collections}
          onChange={setCollections}
          choices={preview.collections.map((c) => ({
            value: c.key,
            label: c.name,
            hint: t("portability-import-collection-size", {
              kind: t(`import-collection-${c.kind}`),
              count: c.items,
            }),
          }))}
        />
      )}
      {mail && (
        <>
          <TargetFields
            target={target}
            setTarget={setTarget}
            parentId={parentId}
            setParentId={setParentId}
            folderName={folderName}
            setFolderName={setFolderName}
          />
          <Check
            label={t("portability-import-skip-duplicates")}
            field="skip-duplicates"
            checked={skipDuplicates}
            onChange={setSkipDuplicates}
            hint={t("portability-import-skip-duplicates-hint")}
          />
          <Check
            label={t("portability-import-keep-flags")}
            field="keep-flags"
            checked={keepFlags}
            onChange={setKeepFlags}
          />
        </>
      )}
      {hasEvents && (
        <SelectField
          label={t("portability-import-calendar")}
          field="calendar"
          value={calendarId ?? ""}
          onChange={(v) => {
            setCalendarId(v === "" ? null : v);
          }}
        >
          <option value="">{t("common-default-calendar")}</option>
          {(calendars.data ?? [])
            .filter((c) => !c.read_only)
            .map((c) => (
              <option key={c.id} value={c.id}>
                {c.settings.name}
              </option>
            ))}
        </SelectField>
      )}
      {hasContacts && (
        <SelectField
          label={t("portability-import-address-book")}
          field="address-book"
          value={addressBookId ?? ""}
          onChange={(v) => {
            setAddressBookId(v === "" ? null : v);
          }}
        >
          <option value="">{t("portability-import-default-address-book")}</option>
          {(books.data ?? [])
            .filter((b) => !b.read_only)
            .map((b) => (
              <option key={b.id} value={b.id}>
                {b.settings.name}
              </option>
            ))}
        </SelectField>
      )}
      {(preview.accounts.length > 0 || preview.rules.length > 0) && (
        <p data-part="form.hint" data-field="proposals">
          {t("portability-import-proposals-later", {
            accounts: preview.accounts.length,
            rules: preview.rules.length,
          })}
        </p>
      )}
      <IssueList
        issues={preview.issues}
        truncated={preview.issues_truncated}
        titleId="portability-preview-issues-title"
      />
      {problem && <Problem problem={problem} />}
      <WizardButtons>
        <button
          type="button"
          data-part="form.button"
          data-action="back"
          disabled={busy}
          onClick={onBack}
        >
          {t("common-back")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          data-action="start"
          disabled={busy || nothing}
          onClick={() => {
            onStart({
              folders: folders.length === preview.folders.length ? null : folders,
              collections: collections.length === preview.collections.length ? null : collections,
              target,
              parentId,
              folderName,
              skipDuplicates,
              keepFlags,
              calendarId,
              addressBookId,
            });
          }}
        >
          {t("common-import")}
        </button>
      </WizardButtons>
    </div>
  );
}

/** Thunderbird accounts and filters: chosen ones are added on request. */
export function Proposals({
  accounts,
  rules,
}: {
  accounts: readonly AccountProposal[];
  rules: readonly RuleProposal[];
}) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const [chosenAccounts, setChosenAccounts] = useState(() =>
    accounts.filter((a) => a.duplicate_of === null).map((a) => a.key),
  );
  const [chosenRules, setChosenRules] = useState(() => rules.map((r) => r.key));
  const [added, setAdded] = useState<string[]>([]);
  const [saved, setSaved] = useState<string[]>([]);
  const { busy, problem, run } = useBusyAction();
  if (accounts.length === 0 && rules.length === 0) return null;

  const addAccounts = async () => {
    await run(async () => {
      for (const proposal of accounts.filter(
        (a) => chosenAccounts.includes(a.key) && !added.includes(a.key),
      )) {
        // Passwords are never imported: the user signs in afterwards.
        await client.request(
          { type: "AddAccount", config: proposal.config, secrets: [] },
          "AccountSaved",
        );
        setAdded((list) => [...list, proposal.key]);
      }
      void queryClient.invalidateQueries({ queryKey: queryKeys.accounts });
    });
  };

  const saveRules = async () => {
    await run(async () => {
      for (const proposal of rules.filter(
        (r) => chosenRules.includes(r.key) && !saved.includes(r.key),
      )) {
        await client.request(
          {
            type: "SaveRule",
            // Incomplete filters start disabled (the core proposes them so).
            rule: { ...proposal.rule, enabled: proposal.complete && proposal.rule.enabled },
            position: null,
          },
          "RuleSaved",
        );
        setSaved((list) => [...list, proposal.key]);
      }
      void queryClient.invalidateQueries({ queryKey: ["rules"] });
    });
  };

  return (
    <section data-part="portability.proposals">
      {accounts.length > 0 && (
        <section data-part="form.section" aria-label={t("portability-proposals-accounts")}>
          <h4 data-part="form.section.title">{t("portability-proposals-accounts")}</h4>
          <p data-part="form.hint">{t("portability-proposals-accounts-hint")}</p>
          <ul data-part="portability.proposal-list">
            {accounts.map((a) => (
              <li
                key={a.key}
                data-part="portability.proposal"
                data-state={
                  added.includes(a.key) ? "added" : a.duplicate_of ? "duplicate" : undefined
                }
              >
                <label data-part="form.check">
                  <input
                    data-part="form.checkbox"
                    type="checkbox"
                    checked={chosenAccounts.includes(a.key)}
                    disabled={busy || added.includes(a.key)}
                    onChange={(e) => {
                      setChosenAccounts((list) =>
                        e.target.checked ? [...list, a.key] : list.filter((k) => k !== a.key),
                      );
                    }}
                  />
                  <span data-part="form.check.label">
                    {t("portability-account-option", {
                      name: a.config.name,
                      email: a.config.email,
                      hasEmail: a.config.email ? "yes" : "no",
                    })}
                  </span>
                </label>
                {a.duplicate_of && (
                  <p data-part="form.hint" data-tone="warning">
                    {t("portability-proposal-duplicate", { account: a.duplicate_of })}
                  </p>
                )}
                {added.includes(a.key) && (
                  <p data-part="form.hint" data-tone="success">
                    {t("portability-proposal-added")}
                  </p>
                )}
                <IssueList issues={a.issues} truncated={0} titleId="portability-proposal-issues" />
              </li>
            ))}
          </ul>
          <button
            type="button"
            data-part="form.button"
            data-action="add-accounts"
            disabled={busy || chosenAccounts.every((k) => added.includes(k))}
            onClick={() => void addAccounts()}
          >
            {t("portability-proposals-add-accounts")}
          </button>
        </section>
      )}
      {rules.length > 0 && (
        <section data-part="form.section" aria-label={t("portability-proposals-rules")}>
          <h4 data-part="form.section.title">{t("portability-proposals-rules")}</h4>
          <ul data-part="portability.proposal-list">
            {rules.map((r) => (
              <li
                key={r.key}
                data-part="portability.proposal"
                data-state={saved.includes(r.key) ? "added" : r.complete ? undefined : "incomplete"}
              >
                <label data-part="form.check">
                  <input
                    data-part="form.checkbox"
                    type="checkbox"
                    checked={chosenRules.includes(r.key)}
                    disabled={busy || saved.includes(r.key)}
                    onChange={(e) => {
                      setChosenRules((list) =>
                        e.target.checked ? [...list, r.key] : list.filter((k) => k !== r.key),
                      );
                    }}
                  />
                  <span data-part="form.check.label">{r.rule.name}</span>
                </label>
                <p data-part="form.hint" data-tone={r.complete ? undefined : "warning"}>
                  {t(
                    r.complete
                      ? "portability-proposal-rule-complete"
                      : "portability-proposal-rule-incomplete",
                  )}
                </p>
                {saved.includes(r.key) && (
                  <p data-part="form.hint" data-tone="success">
                    {t("portability-proposal-saved")}
                  </p>
                )}
                <IssueList issues={r.issues} truncated={0} titleId="portability-proposal-issues" />
              </li>
            ))}
          </ul>
          <button
            type="button"
            data-part="form.button"
            data-action="save-rules"
            disabled={busy || chosenRules.every((k) => saved.includes(k))}
            onClick={() => void saveRules()}
          >
            {t("portability-proposals-save-rules")}
          </button>
        </section>
      )}
      {problem && <Problem problem={problem} />}
    </section>
  );
}

/** The import wizard's content. */
export function ImportWizard({ close }: { close: () => void }) {
  const client = useClient();
  const queryClient = useQueryClient();
  const [step, setStep] = useState<Step>("source");
  const [format, setFormat] = useState<ImportFormat>("mbox");
  const [path, setPath] = useState("");
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [jobId, setJobId] = useState<string | null>(null);
  const { busy, problem, setProblem, run } = useBusyAction();

  const read = async (f: ImportFormat, p: string) => {
    await run(async () => {
      const answer = await client.request(
        { type: "PreviewImport", source: { format: f, path: p.trim() } },
        "ImportPreview",
      );
      setPreview(answer.preview);
      setStep("options");
    });
  };

  const start = async (o: StartOptions) => {
    if (!preview) return;
    await run(async () => {
      const answer = await client.request(
        {
          type: "StartImport",
          source: preview.source,
          options: {
            mail: {
              account_id: o.target,
              parent_folder_id: o.parentId,
              // New folders only in Local Folders: into an account, mail
              // goes into its existing folders (ADR 0099).
              folder_name:
                o.target !== null || o.folderName.trim() === "" ? null : o.folderName.trim(),
            },
            folders: o.folders,
            collections: o.collections,
            calendar_id: o.calendarId,
            address_book_id: o.addressBookId,
            skip_duplicates: o.skipDuplicates,
            keep_flags: o.keepFlags,
          },
        },
        "PortabilityJob",
      );
      rememberJob(queryClient, answer.job);
      setJobId(answer.job.job_id);
      setStep("job");
    });
  };

  return (
    <div data-part="wizard" data-wizard="import" data-step={step}>
      {step === "source" && (
        <SourceStep
          format={format}
          setFormat={setFormat}
          path={path}
          setPath={setPath}
          busy={busy}
          problem={problem}
          onNext={() => void read(format, path)}
          onUseFound={(f, p) => {
            setFormat(f);
            setPath(p);
            // "Use this profile" reads it at once.
            void read(f, p);
          }}
        />
      )}
      {step === "options" && preview && (
        <OptionsStep
          preview={preview}
          busy={busy}
          problem={problem}
          onBack={() => {
            setProblem(null);
            setStep("source");
          }}
          onStart={(o) => {
            void start(o);
          }}
        />
      )}
      {step === "job" && jobId && (
        <div data-part="wizard.step-body" data-step="job">
          <StepHeader step={3} steps={3} titleId="portability-import-job-title" />
          <JobView jobId={jobId}>
            {(report: JobReport) => <Proposals accounts={report.accounts} rules={report.rules} />}
          </JobView>
          <WizardButtons>
            <CloseButton onClose={close} action="close" />
          </WizardButtons>
        </div>
      )}
    </div>
  );
}

export function ImportWizardDialog({ close }: DialogProps) {
  return <ImportWizard close={close} />;
}
