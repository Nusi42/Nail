/**
 * "Show details" (ADR 0190): the precise cause of a sync, sign-in or send
 * failure in a window of its own (`error-details:<n>`) — what Nail was
 * doing, the protocol and folder, the command and the server's answer
 * verbatim (both redacted by the core) — with Copy details and Show in
 * connection log. `ShowDetailsButton` opens it from anywhere an error is
 * shown.
 */
import { useState } from "react";
import { errorMessageId, errorMessageIdOf } from "../../api/client";
import type { ErrorDetails } from "../../api/generated";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { DATE_TIME_SECONDS, formatDateTime } from "../../i18n/format";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useUiStoreApi } from "../../state/uiStore";
import { protocolId, stepId } from "./model";
import { DETAILS_PART } from "./names";
import { openConnectionLog, openErrorDetails, useConnlogState, type ErrorReport } from "./store";

/** One labelled field: label message id and value. */
interface Field {
  key: string;
  labelId: string;
  value: string;
  /** Shown verbatim in monospace (commands, server answers). */
  code?: boolean;
}

/** The fields of a report, in display order (empty ones left out). */
function useFields(details: ErrorDetails): Field[] {
  const t = useT();
  const locale = useLocale();
  const fields: (Field | null)[] = [
    { key: "step", labelId: "connlog-details-step", value: t(stepId(details.step)) },
    details.protocol
      ? {
          key: "protocol",
          labelId: "connlog-details-protocol",
          value: t(protocolId(details.protocol)),
        }
      : null,
    details.folder
      ? { key: "folder", labelId: "connlog-details-folder", value: details.folder }
      : null,
    details.command
      ? { key: "command", labelId: "connlog-details-command", value: details.command, code: true }
      : null,
    details.server_text
      ? {
          key: "server",
          labelId: "connlog-details-server",
          value: details.server_text,
          code: true,
        }
      : null,
    {
      key: "at",
      labelId: "connlog-details-at",
      value: formatDateTime(details.at, locale, DATE_TIME_SECONDS),
    },
    { key: "code", labelId: "connlog-details-code", value: details.code, code: true },
    details.detail
      ? { key: "detail", labelId: "connlog-details-detail", value: details.detail, code: true }
      : null,
  ];
  return fields.filter((f): f is Field => f !== null);
}

/** The wrapper text of a report: the core's localized text, else its class's. */
function useSummary(report: ErrorReport): string {
  const t = useT();
  return report.localized
    ? t(report.localized.id, report.localized.args)
    : t(errorMessageId(report.code));
}

/** The details of one report. */
export function ErrorDetailsView({ report, close }: { report: ErrorReport; close: () => void }) {
  const t = useT();
  const ui = useUiStoreApi();
  const summary = useSummary(report);
  const fields = useFields(report.details);
  const [copied, setCopied] = useState<string | null>(null);
  const copy = () => {
    const text = [summary, ...fields.map((f) => `${t(f.labelId)}: ${f.value}`)].join("\n");
    navigator.clipboard
      .writeText(text)
      .then(() => {
        setCopied("connlog-details-copied");
      })
      .catch((error: unknown) => {
        setCopied(errorMessageIdOf(error));
      });
  };
  const accountId = report.accountId;
  return (
    <div data-part={DETAILS_PART} data-step={report.details.step} data-code={report.details.code}>
      <p data-part="error-details.summary">{summary}</p>
      <dl data-part="error-details.fields">
        {fields.map((field) => (
          <div key={field.key} data-part="error-details.field" data-field={field.key}>
            <dt data-part="error-details.label">{t(field.labelId)}</dt>
            <dd data-part="error-details.value">
              {field.code ? <code data-part="error-details.code">{field.value}</code> : field.value}
            </dd>
          </div>
        ))}
      </dl>
      <p data-part="error-details.hint">{t("connlog-details-redacted")}</p>
      <p data-part="error-details.status" role="status">
        {copied ? t(copied) : ""}
      </p>
      <div data-part="error-details.actions">
        <button type="button" data-part="form.button" data-action="copy" onClick={copy}>
          {t("connlog-details-copy")}
        </button>
        {accountId !== null && (
          <button
            type="button"
            data-part="form.button"
            data-action="show-log"
            onClick={() => {
              openConnectionLog(
                ui,
                { kind: "account", account_id: accountId },
                report.details.log_seq ?? null,
              );
            }}
          >
            {t("connlog-details-show-log")}
          </button>
        )}
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          data-action="close"
          onClick={close}
        >
          {t("connlog-details-close")}
        </button>
      </div>
    </div>
  );
}

/** The details window: its report comes from the store. */
export function ErrorDetailsWindow({ name, close }: DialogProps) {
  const t = useT();
  const report = useConnlogState((s) => s.reports[name]);
  if (!report) {
    return (
      <div data-part={DETAILS_PART}>
        <p data-part="error-details.summary">{t("connlog-details-gone")}</p>
        <div data-part="error-details.actions">
          <button type="button" data-part="form.button" data-action="close" onClick={close}>
            {t("connlog-details-close")}
          </button>
        </div>
      </div>
    );
  }
  return <ErrorDetailsView report={report} close={close} />;
}

/** "Show details" next to an error: opens its details window. */
export function ShowDetailsButton({
  report,
  part = "form.button",
}: {
  report: ErrorReport;
  /** The button's part (`status-bar.button` in the status bar). */
  part?: string;
}) {
  const t = useT();
  const ui = useUiStoreApi();
  return (
    <button
      type="button"
      data-part={part}
      data-action="show-details"
      onClick={() => {
        openErrorDetails(ui, report);
      }}
    >
      {t("connlog-show-details")}
    </button>
  );
}
