/**
 * The connection log's places in Settings (ADR 0190):
 *
 * - Accounts → an account: "Connection log" and the account's recent
 *   problems (`ListSyncErrors`: the last error of the account and of each
 *   folder, the messages folders skipped), each with "Show details"
 *   (`AccountConnection`, rendered by the `account-settings` widget);
 * - Data & backup → Diagnostics: "Keep connection log on disk" (off by
 *   default; turning it off deletes the files) and the application log
 *   (`ConnectionLogSettingsCard`, rendered by the `diagnostics` widget);
 * - the application log's recent lines (window `application-log`).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { errorMessageIdOf } from "../../api/client";
import type { AccountId, SkippedMessage, SyncErrorRecord } from "../../api/generated";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { DATE_TIME, formatDateTime } from "../../i18n/format";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { Switch } from "../../widgets/primitives";
import { ShowDetailsButton } from "./ErrorDetails";
import { stepId } from "./model";
import { ACCOUNT_PART, APP_LOG_KIND, APP_LOG_PART, SETTINGS_PART } from "./names";
import {
  saveConnectionLogSettings,
  useAppLog,
  useConnectionLogSettings,
  useSyncErrors,
} from "./queries";
import { openConnectionLog } from "./store";

/** Skipped messages grouped by folder (one line each). */
function byFolder(skipped: readonly SkippedMessage[]): SkippedMessage[][] {
  const groups = new Map<string, SkippedMessage[]>();
  for (const message of skipped) {
    const group = groups.get(message.folder_id);
    if (group) group.push(message);
    else groups.set(message.folder_id, [message]);
  }
  return [...groups.values()];
}

/** One stored error of the account. */
function ProblemItem({ record }: { record: SyncErrorRecord }) {
  const t = useT();
  const locale = useLocale();
  const { details } = record;
  return (
    <li data-part="connection-log-account.problem" data-step={details.step}>
      <span data-part="connection-log-account.problem.text">
        {details.folder
          ? t("connlog-account-problem-folder", {
              step: t(stepId(details.step)),
              folder: details.folder,
              time: formatDateTime(details.at, locale, DATE_TIME),
            })
          : t("connlog-account-problem", {
              step: t(stepId(details.step)),
              time: formatDateTime(details.at, locale, DATE_TIME),
            })}
      </span>
      <ShowDetailsButton
        report={{ accountId: record.account_id, code: details.code, localized: null, details }}
      />
    </li>
  );
}

/** "Connection log" and the recent problems of one account. */
export function AccountConnection({ accountId }: { accountId: AccountId }) {
  const t = useT();
  const ui = useUiStoreApi();
  const headingId = useId();
  const problems = useSyncErrors(accountId);
  const errors = problems.data?.errors ?? [];
  const skipped = byFolder(problems.data?.skipped ?? []);
  return (
    <section data-part={ACCOUNT_PART} data-account={accountId} aria-labelledby={headingId}>
      <h3 data-part="form.section.title" id={headingId}>
        {t("connlog-account-title")}
      </h3>
      <p data-part="form.hint">{t("connlog-account-hint")}</p>
      <button
        type="button"
        data-part="form.button"
        data-action="connection-log"
        onClick={() => {
          openConnectionLog(ui, { kind: "account", account_id: accountId });
        }}
      >
        {t("connlog-account-open")}
      </button>
      <h4 data-part="connection-log-account.heading">{t("connlog-account-problems")}</h4>
      {problems.isError && (
        <p data-part="form.hint" data-tone="danger">
          {t(errorMessageIdOf(problems.error))}
        </p>
      )}
      {problems.isSuccess && errors.length === 0 && skipped.length === 0 ? (
        <p data-part="connection-log-account.empty">{t("connlog-account-no-problems")}</p>
      ) : (
        <ul data-part="connection-log-account.problems">
          {errors.map((record) => (
            <ProblemItem key={record.folder_id ?? "account"} record={record} />
          ))}
          {skipped.map((group) => {
            const [first] = group;
            if (!first) return null;
            return (
              <li
                key={first.folder_id}
                data-part="connection-log-account.problem"
                data-step="skipped"
              >
                <span data-part="connection-log-account.problem.text">
                  {t("sync-skipped-messages", {
                    count: group.length,
                    folder: first.details.folder ?? first.folder_id,
                  })}
                </span>
                <ShowDetailsButton
                  report={{
                    accountId: first.account_id,
                    code: first.details.code,
                    localized: null,
                    details: first.details,
                  }}
                />
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

/** Settings → Diagnostics: disk logging and the application log. */
export function ConnectionLogSettingsCard() {
  const t = useT();
  const ui = useUiStoreApi();
  const client = useClient();
  const queryClient = useQueryClient();
  const settings = useConnectionLogSettings();
  const appLog = useAppLog();
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const keep = settings.data?.keep_on_disk ?? false;
  return (
    <section data-part={SETTINGS_PART} data-widget="connection-log-settings">
      <h3 data-part="form.section.title">{t("connlog-settings-title")}</h3>
      <p data-part="form.hint">{t("connlog-settings-intro")}</p>
      <Switch
        name="keep-on-disk"
        label={t("connlog-settings-disk")}
        hint={t("connlog-settings-disk-hint")}
        checked={keep}
        disabled={busy || !settings.isSuccess}
        onChange={(keep_on_disk) => {
          setBusy(true);
          setProblem(null);
          saveConnectionLogSettings(client, queryClient, { keep_on_disk })
            .catch((error: unknown) => {
              setProblem(errorMessageIdOf(error));
            })
            .finally(() => {
              setBusy(false);
            });
        }}
      />
      {problem !== null && (
        <p data-part="form.hint" data-tone="danger" role="alert">
          {t(problem)}
        </p>
      )}
      <h4 data-part="connection-log-settings.heading">{t("connlog-settings-app-log")}</h4>
      <p data-part="connection-log-settings.path">
        {appLog.data?.path
          ? t("connlog-settings-app-log-path", { path: appLog.data.path })
          : t("connlog-settings-app-log-none")}
      </p>
      <button
        type="button"
        data-part="form.button"
        data-action="show-app-log"
        onClick={() => {
          ui.getState().openWindow(APP_LOG_KIND);
        }}
      >
        {t("connlog-settings-app-log-show")}
      </button>
    </section>
  );
}

/** The application log's recent lines (redacted by the core). */
export function AppLogWindow({ close }: DialogProps) {
  const t = useT();
  const appLog = useAppLog();
  const lines = appLog.data?.lines ?? [];
  return (
    <div data-part={APP_LOG_PART}>
      <p data-part="application-log.path">
        {appLog.data?.path
          ? t("connlog-settings-app-log-path", { path: appLog.data.path })
          : t("connlog-settings-app-log-none")}
      </p>
      {appLog.isError && (
        <p data-part="form.hint" data-tone="danger" role="alert">
          {t(errorMessageIdOf(appLog.error))}
        </p>
      )}
      {appLog.isSuccess && lines.length === 0 ? (
        <p data-part="application-log.empty">{t("connlog-app-log-empty")}</p>
      ) : (
        <pre
          data-part="application-log.lines"
          aria-label={t("connlog-app-log-lines")}
          // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- the lines scroll: keyboard users must be able to reach them (axe scrollable-region-focusable).
          tabIndex={0}
        >
          {lines.join("\n")}
        </pre>
      )}
      <div data-part="application-log.actions">
        <button
          type="button"
          data-part="form.button"
          data-action="refresh"
          onClick={() => void appLog.refetch()}
        >
          {t("connlog-app-log-refresh")}
        </button>
        <button type="button" data-part="form.button" data-action="close" onClick={close}>
          {t("connlog-details-close")}
        </button>
      </div>
    </div>
  );
}
