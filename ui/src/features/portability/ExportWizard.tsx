/**
 * The export wizard (dialog `export-wizard`, action `export.open`): what
 * (folders of an account with or without subfolders, the selected
 * message, calendars, address books), the format (only the ones that can
 * write it: `ExportFormat::accepts`), the target file or folder, then the
 * job with progress and its report. An existing target is refused unless
 * "Replace" is ticked (`conflict`).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import type {
  ExportFormat,
  ExportSource,
  LocalizedMessage,
  MboxVariant,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useAccounts, useClient, useFolders } from "../../state/queries";
import { useUi } from "../../state/uiStore";
import { Check, CheckGroup, RadioGroup, SelectField, StepHeader, WizardButtons } from "./Fields";
import { JobView, Problem } from "./Job";
import {
  EXPORT_FORMATS,
  MBOX_VARIANTS,
  exportAccepts,
  exportExtension,
  exportFilters,
  exportFormatId,
  exportIsDirectory,
  isAbsolutePath,
  mboxVariantId,
} from "./model";
import { PathField, useSavePath } from "./PathField";
import { rememberJob, useAddressBooks, useCalendars } from "./queries";

type SourceKind = ExportSource["type"];
const SOURCE_KINDS: readonly SourceKind[] = ["folders", "messages", "calendars", "address-books"];

/** The export wizard's content. */
export function ExportWizard({ close }: { close(): void }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const selectedMessage = useUi((s) => s.selectedMessageId);
  const activeAccount = useUi((s) => s.activeAccountId);
  const activeFolder = useUi((s) => s.activeFolderId);
  const accounts = useAccounts();
  const real = (accounts.data ?? []).filter((a) => a.kind !== "virtual");
  const [kind, setKind] = useState<SourceKind>("folders");
  const [accountId, setAccountId] = useState<string | null>(
    activeAccount && real.some((a) => a.id === activeAccount) ? activeAccount : null,
  );
  const account = accountId ?? real[0]?.id ?? null;
  const folders = useFolders(account);
  const [folderIds, setFolderIds] = useState<string[]>(() => (activeFolder ? [activeFolder] : []));
  const [subfolders, setSubfolders] = useState(true);
  const calendars = useCalendars();
  const books = useAddressBooks();
  const [calendarIds, setCalendarIds] = useState<string[]>([]);
  const [bookIds, setBookIds] = useState<string[]>([]);
  const [format, setFormat] = useState<ExportFormat>("mbox");
  const [variant, setVariant] = useState<MboxVariant>("mboxrd");
  // `confirmed`: the Save dialog confirmed replacing an existing file.
  const { path, overwrite: confirmed, onChange: setPath } = useSavePath();
  const [overwrite, setOverwrite] = useState(false);
  const [headersOnly, setHeadersOnly] = useState(false);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [jobId, setJobId] = useState<string | null>(null);

  const accountFolders = (folders.data ?? []).map((f) => f.id);
  const chosenFolders = folderIds.filter((id) => accountFolders.includes(id));
  const source: ExportSource =
    kind === "folders"
      ? { type: "folders", folder_ids: chosenFolders, include_subfolders: subfolders }
      : kind === "messages"
        ? { type: "messages", message_ids: selectedMessage ? [selectedMessage] : [] }
        : kind === "calendars"
          ? { type: "calendars", calendar_ids: calendarIds }
          : { type: "address-books", address_book_ids: bookIds };
  const count =
    source.type === "folders"
      ? source.folder_ids.length
      : source.type === "messages"
        ? source.message_ids.length
        : source.type === "calendars"
          ? source.calendar_ids.length
          : source.address_book_ids.length;
  const formats = EXPORT_FORMATS.filter((f) => exportAccepts(f, kind));
  const chosenFormat = formats.includes(format) ? format : (formats[0] ?? "mbox");
  // Several folders / collections, or a folder format: the target is a folder.
  const toFolder =
    exportIsDirectory(chosenFormat) || count > 1 || (kind === "folders" && subfolders);
  const ext = exportExtension(chosenFormat);

  const start = async () => {
    setBusy(true);
    setProblem(null);
    try {
      const answer = await client.request(
        {
          type: "StartExport",
          source,
          format: chosenFormat,
          path: path.trim(),
          options: {
            overwrite: overwrite || (confirmed && !toFolder),
            mbox_variant: variant,
            allow_headers_only: headersOnly,
          },
        },
        "PortabilityJob",
      );
      rememberJob(queryClient, answer.job);
      setJobId(answer.job.job_id);
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };

  if (jobId) {
    return (
      <div data-part="wizard" data-wizard="export" data-step="job">
        <div data-part="wizard.step-body" data-step="job">
          <StepHeader step={2} steps={2} titleId="portability-export-job-title" />
          <JobView jobId={jobId} />
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
    <div data-part="wizard" data-wizard="export" data-step="options">
      <div data-part="wizard.step-body" data-step="options">
        <StepHeader step={1} steps={2} titleId="portability-export-title" />
        <RadioGroup<SourceKind>
          legend={t("portability-export-what")}
          field="source-kind"
          value={kind}
          onChange={setKind}
          choices={SOURCE_KINDS.map((k) => ({
            value: k,
            label: t(`portability-export-source-${k}`),
            disabled: k === "messages" && selectedMessage === null,
          }))}
        />
        {kind === "folders" && (
          <>
            <SelectField
              label={t("portability-export-account")}
              field="account"
              value={account ?? ""}
              onChange={(v) => setAccountId(v)}
            >
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
            <CheckGroup
              legend={t("portability-export-folders")}
              field="folders"
              selected={chosenFolders}
              onChange={setFolderIds}
              choices={(folders.data ?? []).map((f) => ({
                value: f.id,
                label: f.path,
                hint: t("portability-export-folder-count", { count: f.total }),
              }))}
            />
            <Check
              label={t("portability-export-subfolders")}
              field="subfolders"
              checked={subfolders}
              onChange={setSubfolders}
            />
          </>
        )}
        {kind === "messages" && (
          <p data-part="form.hint" data-field="messages">
            {t("portability-export-selected-message")}
          </p>
        )}
        {kind === "calendars" && (
          <CheckGroup
            legend={t("portability-export-calendars")}
            field="calendars"
            selected={calendarIds}
            onChange={setCalendarIds}
            choices={(calendars.data ?? []).map((c) => ({ value: c.id, label: c.settings.name }))}
          />
        )}
        {kind === "address-books" && (
          <CheckGroup
            legend={t("portability-export-address-books")}
            field="address-books"
            selected={bookIds}
            onChange={setBookIds}
            choices={(books.data ?? []).map((b) => ({ value: b.id, label: b.settings.name }))}
          />
        )}
        <RadioGroup<ExportFormat>
          legend={t("portability-export-format")}
          field="format"
          value={chosenFormat}
          onChange={setFormat}
          choices={formats.map((f) => ({ value: f, label: t(exportFormatId(f)) }))}
        />
        {chosenFormat === "mbox" && (
          <SelectField
            label={t("portability-export-mbox-variant")}
            field="mbox-variant"
            value={variant}
            onChange={(v) => setVariant(v as MboxVariant)}
          >
            {MBOX_VARIANTS.map((v) => (
              <option key={v} value={v}>
                {t(mboxVariantId(v))}
              </option>
            ))}
          </SelectField>
        )}
        <PathField
          labelId={toFolder ? "portability-export-target-folder" : "portability-export-target-file"}
          field="target-path"
          value={path}
          onChange={setPath}
          kind={toFolder ? "open-folder" : "save-file"}
          titleId="portability-export-pick-title"
          filters={toFolder ? [] : exportFilters(chosenFormat)}
          defaultPath={
            toFolder || ext === null ? null : `${t("portability-export-default-name")}.${ext}`
          }
        />
        <Check
          label={t("portability-export-overwrite")}
          field="overwrite"
          checked={overwrite}
          onChange={setOverwrite}
          hint={t("portability-export-overwrite-hint")}
        />
        {(kind === "folders" || kind === "messages") && (
          <Check
            label={t("portability-export-headers-only")}
            field="headers-only"
            checked={headersOnly}
            onChange={setHeadersOnly}
            hint={t("portability-export-headers-only-hint")}
          />
        )}
        {problem && <Problem problem={problem} />}
        <WizardButtons>
          <button
            type="button"
            data-part="form.button"
            data-variant="primary"
            data-action="start"
            disabled={busy || count === 0 || !isAbsolutePath(path)}
            onClick={() => void start()}
          >
            {t("portability-export-start")}
          </button>
        </WizardButtons>
      </div>
    </div>
  );
}

export function ExportWizardDialog({ close }: DialogProps) {
  return <ExportWizard close={close} />;
}
