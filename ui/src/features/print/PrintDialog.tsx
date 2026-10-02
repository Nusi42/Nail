/**
 * The print dialog (dialog `print`; actions `print.message`,
 * `print.thread`, `print.event`, `print.agenda`, `print.save-pdf`): what is
 * printed, how (paper size, orientation, headers, quoted text, attachment
 * list, remote images, backgrounds; agendas: the days, the layout and the
 * calendars), then
 *
 * - **Print** — `CreatePrintJob`, the print view opens and with it the
 *   system print dialog (in a browser "Save as PDF" is one of its
 *   printers);
 * - **Save as PDF** — on the desktop the file is written by the shell
 *   (`SavePrintPdf` → WebView2 `PrintToPdf`) to a path typed or chosen with
 *   the system dialog; elsewhere the core answers `unsupported` and the
 *   dialog says to use the print dialog's PDF printer instead.
 */
import { useQuery } from "@tanstack/react-query";
import { useId, useState } from "react";
import { NailError, errorMessageOf } from "../../api/client";
import type {
  AgendaLayout,
  HeaderDetail,
  LocalizedMessage,
  PageOrientation,
  PageSize,
  PdfFile,
  PrintOptions,
  PrintTarget,
} from "../../api/generated";
import { useLocale, useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useClient, useMessage, useThread } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { Check, CheckGroup, SelectField, WizardButtons } from "../portability/Fields";
import { formatBytes, isAbsolutePath, plainText } from "../portability/model";
import { PathField } from "../portability/PathField";
import { useCalendars } from "../portability/queries";
import { AGENDA_LAYOUTS, HEADER_DETAILS, ORIENTATIONS, PAGE_SIZES } from "./messages";
import { defaultAgenda, defaultPrintOptions, targetIssue } from "./model";
import { openPrintView } from "./PrintView";
import { printStoreFor, usePrintState } from "./store";

const PDF_FILTERS = [{ label_id: "print-filter-pdf", extensions: ["pdf"] }];

/** A file name from a title (no characters Windows refuses). */
export function pdfName(title: string, fallback: string): string {
  const clean = plainText(title)
    .replace(/[\\/:*?"<>|\p{Cc}]+/gu, " ")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 80);
  return `${clean === "" ? fallback : clean}.pdf`;
}

function useTargetTitle(target: PrintTarget | null): string {
  const t = useT();
  const message = useMessage(target?.type === "message" ? target.message_id : null);
  const thread = useThread(target?.type === "thread" ? target.thread_id : null);
  const client = useClient();
  const event = useQuery({
    queryKey: ["print", "event", target?.type === "event" ? target.event_id : ""],
    enabled: target?.type === "event",
    retry: false,
    queryFn: async () =>
      (
        await client.request(
          { type: "GetEvent", event_id: target?.type === "event" ? target.event_id : "" },
          "PimEvent",
        )
      ).event,
  });
  switch (target?.type) {
    case "message":
      return message.data?.headers.subject || t("print-no-subject");
    case "thread":
      return thread.data?.thread.subject || t("print-no-subject");
    case "event":
      return event.data?.fields.title || t("print-no-title");
    case "agenda":
      return t("print-agenda-name", { start: target.start, end: target.end });
    default:
      return "";
  }
}

function AgendaFields({
  target,
  onChange,
}: {
  target: Extract<PrintTarget, { type: "agenda" }>;
  onChange(target: Extract<PrintTarget, { type: "agenda" }>): void;
}) {
  const t = useT();
  const id = useId();
  const calendars = useCalendars();
  const issue = targetIssue(target);
  return (
    <fieldset data-part="form.section" data-field="agenda">
      <legend data-part="form.section.title">{t("print-agenda-days")}</legend>
      <div data-part="form.field" data-field="agenda-start">
        <label data-part="form.label" htmlFor={`${id}-start`}>
          {t("print-agenda-start")}
        </label>
        <input
          id={`${id}-start`}
          data-part="form.input"
          type="date"
          value={target.start}
          onChange={(e) => onChange({ ...target, start: e.target.value })}
        />
      </div>
      <div data-part="form.field" data-field="agenda-end">
        <label data-part="form.label" htmlFor={`${id}-end`}>
          {t("print-agenda-end")}
        </label>
        <input
          id={`${id}-end`}
          data-part="form.input"
          type="date"
          value={target.end}
          aria-invalid={issue !== null || undefined}
          aria-describedby={issue ? `${id}-issue` : undefined}
          onChange={(e) => onChange({ ...target, end: e.target.value })}
        />
      </div>
      {issue && (
        <p data-part="form.hint" data-tone="danger" id={`${id}-issue`}>
          {t(issue)}
        </p>
      )}
      <SelectField
        label={t("print-agenda-layout")}
        field="agenda-layout"
        value={target.layout}
        onChange={(v) => onChange({ ...target, layout: v as AgendaLayout })}
      >
        {AGENDA_LAYOUTS.map((l) => (
          <option key={l} value={l}>
            {t(`print-agenda-layout-${l}`)}
          </option>
        ))}
      </SelectField>
      {(calendars.data?.length ?? 0) > 0 && (
        <CheckGroup
          legend={t("print-agenda-calendars")}
          field="agenda-calendars"
          selected={target.calendar_ids}
          onChange={(calendar_ids) => onChange({ ...target, calendar_ids })}
          choices={(calendars.data ?? []).map((c) => ({ value: c.id, label: c.settings.name }))}
        />
      )}
      <p data-part="form.hint">{t("print-agenda-calendars-hint")}</p>
    </fieldset>
  );
}

function OptionFields({
  options,
  onChange,
  mail,
}: {
  options: PrintOptions;
  onChange(options: PrintOptions): void;
  mail: boolean;
}) {
  const t = useT();
  const set = <K extends keyof PrintOptions>(key: K, value: PrintOptions[K]) =>
    onChange({ ...options, [key]: value });
  return (
    <fieldset data-part="form.section" data-field="options">
      <legend data-part="form.section.title">{t("print-options")}</legend>
      <SelectField
        label={t("print-page-size")}
        field="page-size"
        value={options.page_size}
        onChange={(v) => set("page_size", v as PageSize)}
      >
        {PAGE_SIZES.map((s) => (
          <option key={s} value={s}>
            {t(`print-page-size-${s}`)}
          </option>
        ))}
      </SelectField>
      <SelectField
        label={t("print-orientation")}
        field="orientation"
        value={options.orientation}
        onChange={(v) => set("orientation", v as PageOrientation)}
      >
        {ORIENTATIONS.map((o) => (
          <option key={o} value={o}>
            {t(`print-orientation-${o}`)}
          </option>
        ))}
      </SelectField>
      {mail && (
        <>
          <SelectField
            label={t("print-headers")}
            field="headers"
            value={options.headers}
            onChange={(v) => set("headers", v as HeaderDetail)}
          >
            {HEADER_DETAILS.map((h) => (
              <option key={h} value={h}>
                {t(`print-headers-${h}`)}
              </option>
            ))}
          </SelectField>
          <Check
            label={t("print-quoted-text")}
            field="quoted-text"
            checked={options.quoted_text}
            onChange={(v) => set("quoted_text", v)}
          />
          <Check
            label={t("print-attachment-list")}
            field="attachment-list"
            checked={options.attachment_list}
            onChange={(v) => set("attachment_list", v)}
          />
          <Check
            label={t("print-remote-images")}
            field="remote-images"
            checked={options.remote_images}
            onChange={(v) => set("remote_images", v)}
            hint={t("print-remote-images-hint")}
          />
        </>
      )}
      <Check
        label={t("print-backgrounds")}
        field="backgrounds"
        checked={options.backgrounds}
        onChange={(v) => set("backgrounds", v)}
      />
    </fieldset>
  );
}

/** The print dialog's content. */
export function PrintSetup({ close }: { close(): void }) {
  const t = useT();
  const locale = useLocale();
  const client = useClient();
  const ui = useUiStoreApi();
  const initialTarget = usePrintState((s) => s.target);
  const mode = usePrintState((s) => s.mode);
  const [target, setTarget] = useState<PrintTarget | null>(initialTarget);
  const [options, setOptions] = useState<PrintOptions>(defaultPrintOptions);
  const [pdfStep, setPdfStep] = useState(mode === "pdf");
  const [path, setPath] = useState("");
  const [saved, setSaved] = useState<PdfFile | null>(null);
  const [noDesktop, setNoDesktop] = useState(false);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const title = useTargetTitle(target);
  const issue = target ? targetIssue(target) : null;
  const mail = target?.type === "message" || target?.type === "thread";

  const createJob = async () => {
    if (!target) throw new Error("nothing to print");
    return (await client.request({ type: "CreatePrintJob", target, options }, "PrintJob")).job;
  };

  const print = async () => {
    setBusy(true);
    setProblem(null);
    try {
      const job = await createJob();
      printStoreFor(ui).getState().setAutoPrint(job.job_id);
      close();
      openPrintView(job.view);
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };

  const savePdf = async () => {
    setBusy(true);
    setProblem(null);
    try {
      const job = await createJob();
      const answer = await client.request(
        { type: "SavePrintPdf", job_id: job.job_id, path: path.trim() },
        "PdfSaved",
      );
      setSaved(answer.file);
    } catch (error) {
      if (error instanceof NailError && error.code === "unsupported") setNoDesktop(true);
      else setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div data-part="print-setup" data-target={target?.type ?? "none"}>
      {target === null ? (
        <section data-part="print-setup.nothing">
          <p data-part="form.hint">{t("print-nothing-selected")}</p>
          <button
            type="button"
            data-part="form.button"
            data-action="agenda"
            onClick={() => setTarget(defaultAgenda())}
          >
            {t("print-choose-agenda")}
          </button>
        </section>
      ) : (
        <>
          <p data-part="print-setup.what">{t(`print-what-${target.type}`, { title })}</p>
          {target.type === "agenda" && <AgendaFields target={target} onChange={setTarget} />}
          <OptionFields options={options} onChange={setOptions} mail={mail} />
          {pdfStep && !saved && (
            <PathField
              labelId="print-pdf-file"
              field="pdf-path"
              value={path}
              onChange={setPath}
              kind="save-file"
              titleId="print-pdf-pick-title"
              filters={PDF_FILTERS}
              defaultPath={pdfName(title, t("print-pdf-default-name"))}
            />
          )}
          {noDesktop && (
            <p data-part="form.hint" data-tone="warning" data-field="no-desktop" role="status">
              {t("print-pdf-browser")}
            </p>
          )}
          {saved && (
            <p data-part="print-setup.saved" role="status">
              {t("print-pdf-saved", {
                path: saved.path,
                size: formatBytes(saved.size_bytes, locale),
              })}
            </p>
          )}
          {problem && (
            <p data-part="form.error" role="alert">
              {t(problem.id, problem.args)}
            </p>
          )}
          <WizardButtons>
            {saved ? (
              <button type="button" data-part="form.button" data-action="close" onClick={close}>
                {t("print-close")}
              </button>
            ) : (
              <>
                <button
                  type="button"
                  data-part="form.button"
                  data-variant={pdfStep && !noDesktop ? undefined : "primary"}
                  data-action="print"
                  disabled={busy || issue !== null}
                  onClick={() => void print()}
                >
                  {t("print-now")}
                </button>
                {!noDesktop &&
                  (pdfStep ? (
                    <button
                      type="button"
                      data-part="form.button"
                      data-variant="primary"
                      data-action="save-pdf"
                      disabled={busy || issue !== null || !isAbsolutePath(path)}
                      onClick={() => void savePdf()}
                    >
                      {t("print-pdf-save")}
                    </button>
                  ) : (
                    <button
                      type="button"
                      data-part="form.button"
                      data-action="pdf"
                      disabled={busy}
                      onClick={() => setPdfStep(true)}
                    >
                      {t("print-pdf")}
                    </button>
                  ))}
              </>
            )}
          </WizardButtons>
        </>
      )}
    </div>
  );
}

export function PrintDialog({ close }: DialogProps) {
  return <PrintSetup close={close} />;
}
