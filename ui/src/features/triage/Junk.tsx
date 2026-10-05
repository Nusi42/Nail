/**
 * Junk mail: the reader's verdict bar (widget `junk-verdict`: who decided,
 * the classifier's score, "Not junk") and the classifier settings with its
 * training status per account (widget `junk-settings`). Nail never deletes
 * junk; the classifier is optional and off by default.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { JunkSettings as Settings, JunkStatus, LocalizedMessage } from "../../api/generated";
import { useActions } from "../../actions/ActionsProvider";
import { useLabel, useLocale, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useAccountsConfig, useClient, useMessage } from "../../state/queries";
import { useUi } from "../../state/uiStore";
import { CheckField, NumberField, SelectField } from "../../widgets/forms";
import { junkIssues } from "./model";
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
  const [notice, setNotice] = useState<string | null>(null);
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
  return (
    <SettingsEditor
      key={JSON.stringify(status.data.settings)}
      status={status.data}
      notice={notice}
      onNotice={setNotice}
    />
  );
}

function SettingsEditor({
  status: s,
  notice,
  onNotice,
}: {
  status: JunkStatus;
  notice: string | null;
  onNotice: (id: string | null) => void;
}) {
  const t = useT();
  const locale = useLocale();
  const client = useClient();
  const queryClient = useQueryClient();
  const accountId = s.account_id;
  const [draft, setDraft] = useState<Settings>(s.settings);
  const [problems, setProblems] = useState<LocalizedMessage[]>([]);
  const [confirmReset, setConfirmReset] = useState(false);

  const update = (patch: Partial<Settings>) => {
    setDraft({ ...draft, ...patch });
    onNotice(null);
  };

  const save = async () => {
    const issues = junkIssues(draft);
    if (issues.length > 0) {
      setProblems(issues.map((i) => ({ id: `junk-issue-${i}` })));
      return;
    }
    setProblems([]);
    try {
      const answer = await client.request(
        { type: "UpdateJunkSettings", account_id: accountId, settings: draft },
        "JunkStatus",
      );
      onNotice("junk-settings-saved");
      queryClient.setQueryData(triageKeys.junk(accountId), answer.status);
    } catch (error) {
      setProblems([errorMessageOf(error)]);
    }
  };

  const reset = async () => {
    setConfirmReset(false);
    try {
      const answer = await client.request(
        { type: "ResetJunkClassifier", account_id: accountId },
        "JunkStatus",
      );
      onNotice("junk-settings-reset-done");
      queryClient.setQueryData(triageKeys.junk(accountId), answer.status);
    } catch (error) {
      setProblems([errorMessageOf(error)]);
    }
  };

  const last = s.last_trained_at === null ? null : new Date(s.last_trained_at);
  return (
    <div data-part="junk-settings.account">
      <dl data-part="junk-settings.status" aria-label={t("junk-status-label")}>
        <dt data-part="junk-settings.status.term">{t("junk-status-state")}</dt>
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
            ? t("junk-status-never")
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
          labelId="junk-settings-min-training"
          hintId="junk-settings-min-training-hint"
          name="min-training"
          min={1}
          max={10000}
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
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          onClick={() => void save()}
        >
          {t("junk-settings-save")}
        </button>
        {confirmReset ? (
          <div data-part="junk-settings.confirm" role="group" aria-label={t("junk-reset-confirm")}>
            <span data-part="junk-settings.confirm.text">{t("junk-reset-confirm")}</span>
            <button
              type="button"
              data-part="form.button"
              data-variant="danger"
              onClick={() => void reset()}
            >
              {t("junk-reset-yes")}
            </button>
            <button
              type="button"
              data-part="form.button"
              onClick={() => {
                setConfirmReset(false);
              }}
            >
              {t("junk-reset-no")}
            </button>
          </div>
        ) : (
          <button
            type="button"
            data-part="form.button"
            onClick={() => {
              setConfirmReset(true);
            }}
          >
            {t("junk-reset")}
          </button>
        )}
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
  const accountId = chosen ?? accounts[0]?.id ?? null;
  return (
    <section
      data-part="junk-settings"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("junk-settings-label")}
    >
      <p data-part="junk-settings.intro">{t("junk-settings-intro")}</p>
      {accounts.length === 0 ? (
        <p data-part="junk-settings.empty">{t("junk-settings-no-accounts")}</p>
      ) : (
        <>
          <SelectField
            labelId="junk-settings-account"
            name="account"
            value={accountId ?? ""}
            choices={accounts.map((a) => ({ value: a.id, label: a.name, literal: true }))}
            onChange={setChosen}
          />
          {accountId !== null && <SettingsForm key={accountId} accountId={accountId} />}
        </>
      )}
    </section>
  );
}
