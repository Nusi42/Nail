/**
 * Junk mail: the reader's verdict bar (widget `junk-verdict`: who decided,
 * the classifier's score, "Not junk") and the classifier settings with its
 * training status per account (widget `junk-settings`). Nail never deletes
 * junk; the classifier is optional and off by default.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import {
  JUNK_ISSUE_MESSAGE_IDS,
  type JunkSettings as Settings,
  type JunkStatus,
  type LocalizedMessage,
} from "../../api/generated";
import { useActions } from "../../actions/ActionsProvider";
import { useLabel, useLocale, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useAccountsConfig, useClient, useMessage } from "../../state/queries";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { askSettingsQuestion } from "../settings/confirm";
import { useFormNotice, useLeaveDrafts, useSettingsForm } from "../settings/draft";
import { CheckField, NumberField } from "../../widgets/forms";
import { AccountChoiceField } from "../../widgets/pickers";
import { hamCutoffOf, junkIssues, MAX_MIN_TRAINING } from "./model";

/** The confirmation window of "Reset the filter…". */
export const JUNK_RESET_DIALOG = "reset-junk-filter";
import { triageKeys, useJunkStatus } from "./queries";
import { DATE_TIME, formatDateTime } from "../../i18n/format";

/** The junk verdict of the message in the reader. */
export function JunkVerdictBar({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const { dispatch } = useActions();
  const selectedId = useUi((s) => s.selectedMessageId);
  const message = useMessage(selectedId).data;
  const verdict = message?.junk;
  if (!message || !verdict?.junk) return null;
  return (
    <div
      data-part="junk-verdict"
      data-source={verdict.source}
      data-node-id={node.id ?? undefined}
      role="status"
      aria-label={node.label ? label(node.label) : t("junk-verdict-label")}
    >
      <span data-part="junk-verdict.text">
        {verdict.score === null
          ? t("junk-verdict-junk")
          : t("junk-verdict-score", { score: verdict.score })}
      </span>
      <span data-part="junk-verdict.source">{t(`junk-source-${verdict.source}`)}</span>
      <button
        type="button"
        data-part="form.button"
        data-action="junk.not-junk"
        // This message alone, also inside a card of a conversation (P3).
        onClick={() => void dispatch("junk.not-junk", [message.id])}
      >
        {t("junk-action-not-junk")}
      </button>
    </div>
  );
}

function Issues({ issues }: { issues: readonly LocalizedMessage[] }) {
  const t = useT();
  if (issues.length === 0) return null;
  return (
    <ul data-part="junk-settings.issues" role="alert">
      {issues.map((issue) => (
        <li key={issue.id} data-part="junk-settings.issue">
          {t(issue.id, issue.args)}
        </li>
      ))}
    </ul>
  );
}

function SettingsForm({ accountId }: { accountId: string }) {
  const t = useT();
  const status = useJunkStatus(accountId);
  if (status.isError) {
    const message = errorMessageOf(status.error);
    return (
      <p data-part="junk-settings.error" role="alert">
        {t(message.id, message.args)}
      </p>
    );
  }
  if (!status.data) {
    return <p data-part="junk-settings.loading">{t("junk-settings-loading")}</p>;
  }
  return <SettingsEditor status={status.data} />;
}

/** The problems of junk settings, as messages. */
function issuesOf(settings: Settings): LocalizedMessage[] {
  return junkIssues(settings).map((i) => ({ id: JUNK_ISSUE_MESSAGE_IDS[i] }));
}

function SettingsEditor({ status: s }: { status: JunkStatus }) {
  const t = useT();
  const locale = useLocale();
  const client = useClient();
  const ui = useUiStoreApi();
  const queryClient = useQueryClient();
  const accountId = s.account_id;
  const { notice, setNotice, outcome, clear, problems: withFailure } = useFormNotice();
  // R6 (ADR 0239): the edits wait for the settings page's Save.
  const form = useSettingsForm(s.settings, {
    ...outcome("junk-settings-saved"),
    canSave: (settings) => junkIssues(settings).length === 0,
    save: async (settings) => {
      const answer = await client.request(
        { type: "UpdateJunkSettings", account_id: accountId, settings },
        "JunkStatus",
      );
      queryClient.setQueryData(triageKeys.junk(accountId), answer.status);
    },
  });
  const draft = form.value;
  const problems = withFailure(issuesOf(draft));

  const update = (patch: Partial<Settings>) => {
    form.set((current) => ({ ...current, ...patch }));
    clear();
  };

  const reset = () => {
    askSettingsQuestion(ui, JUNK_RESET_DIALOG, {
      question: t("junk-reset-question"),
      detail: t("junk-reset-detail"),
      action: t("junk-reset-action"),
      run: async () => {
        const answer = await client.request(
          { type: "ResetJunkClassifier", account_id: accountId },
          "JunkStatus",
        );
        queryClient.setQueryData(triageKeys.junk(accountId), answer.status);
        setNotice("junk-settings-reset-done");
      },
    });
  };

  const last = s.last_trained_at === null ? null : new Date(s.last_trained_at);
  return (
    <div data-part="junk-settings.account">
      <dl data-part="junk-settings.status" aria-label={t("junk-status-label")}>
        <dt data-part="junk-settings.status.term">{t("common-state")}</dt>
        <dd data-part="junk-settings.status.value" data-state={s.ready ? "ready" : "learning"}>
          {s.ready
            ? t("junk-status-ready")
            : t("junk-status-learning", { needed: s.settings.min_training })}
        </dd>
        <dt data-part="junk-settings.status.term">{t("junk-status-trained")}</dt>
        <dd data-part="junk-settings.status.value">
          {t("junk-status-trained-counts", { junk: s.trained_junk, ham: s.trained_ham })}
        </dd>
        <dt data-part="junk-settings.status.term">{t("junk-status-tokens")}</dt>
        <dd data-part="junk-settings.status.value">
          {new Intl.NumberFormat(locale).format(s.tokens)}
        </dd>
        <dt data-part="junk-settings.status.term">{t("junk-status-last-trained")}</dt>
        <dd data-part="junk-settings.status.value">
          {last === null || Number.isNaN(last.getTime())
            ? t("common-never")
            : formatDateTime(last, locale, DATE_TIME)}
        </dd>
      </dl>
      <fieldset data-part="form.section">
        <legend data-part="form.section.title">{t("junk-settings-classifier")}</legend>
        <CheckField
          labelId="junk-settings-enabled"
          name="enabled"
          checked={draft.enabled}
          onChange={(enabled) => {
            update({ enabled });
          }}
        />
        <NumberField
          labelId="junk-settings-threshold"
          hintId="junk-settings-threshold-hint"
          name="threshold"
          min={50}
          max={99}
          value={draft.threshold}
          onChange={(threshold) => {
            update({ threshold: threshold ?? 0 });
          }}
        />
        <NumberField
          labelId="junk-settings-ham-cutoff"
          hintId="junk-settings-ham-cutoff-hint"
          name="ham-cutoff"
          min={0}
          max={Math.max(0, draft.threshold - 1)}
          value={hamCutoffOf(draft)}
          onChange={(ham_cutoff) => {
            update({ ham_cutoff: ham_cutoff ?? 0 });
          }}
        />
        <NumberField
          labelId="junk-settings-min-training"
          hintId="junk-settings-min-training-hint"
          name="min-training"
          min={1}
          max={MAX_MIN_TRAINING}
          value={draft.min_training}
          onChange={(value) => {
            update({ min_training: value ?? 0 });
          }}
        />
        <CheckField
          labelId="junk-settings-honor-server"
          name="honor-server-flags"
          checked={draft.honor_server_flags}
          onChange={(honor_server_flags) => {
            update({ honor_server_flags });
          }}
        />
        <CheckField
          labelId="junk-settings-move"
          name="move-to-junk"
          checked={draft.move_to_junk}
          onChange={(move_to_junk) => {
            update({ move_to_junk });
          }}
        />
        <p data-part="form.hint">{t("junk-settings-never-deletes")}</p>
      </fieldset>
      <Issues issues={problems} />
      {notice && (
        <p data-part="junk-settings.notice" role="status">
          {t(notice)}
        </p>
      )}
      <div data-part="junk-settings.actions">
        <button type="button" data-part="form.button" data-action="junk-reset" onClick={reset}>
          {t("junk-reset")}
        </button>
      </div>
    </div>
  );
}

/** Settings → Junk mail (widget `junk-settings`). */
export function JunkSettingsWidget({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const config = useAccountsConfig();
  const accounts = config.data?.accounts ?? [];
  const [chosen, setChosen] = useState<string | null>(null);
  const leave = useLeaveDrafts();
  const accountId = chosen ?? accounts[0]?.id ?? null;
  return (
    <section
      data-part="junk-settings"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("junk-settings-label")}
    >
      <p data-part="junk-settings.intro">{t("junk-settings-intro")}</p>
      {accounts.length === 0 ? (
        <p data-part="junk-settings.empty">{t("common-no-accounts-yet")}</p>
      ) : (
        <>
          <AccountChoiceField
            labelId="junk-settings-account"
            name="account"
            accounts={accounts}
            value={accountId}
            onChange={(id) => {
              // R6: another account asks first when this one has unsaved changes.
              leave(() => {
                setChosen(id);
              });
            }}
          />
          {accountId !== null && <SettingsForm key={accountId} accountId={accountId} />}
        </>
      )}
    </section>
  );
}
