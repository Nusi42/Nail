/**
 * Server-side rules (Sieve) and vacation replies:
 *
 * - `sieve-status`: per IMAP account, whether rules run on the server
 *   (ManageSieve on / off, host / port / script name), the script's state
 *   (uploaded, failed, extensions, other scripts left alone), "Upload now",
 *   "Show script", and where each rule runs (server / Nail / Nail only, with
 *   the reasons).
 * - `vacation-editor`: the out-of-office reply of an account, compiled
 *   into the same script by the core.
 */
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { errorMessageId, errorMessageOf } from "../../api/client";
import {
  VACATION_ISSUE_MESSAGE_IDS,
  type AccountConfig,
  type LocalizedMessage,
  type ManageSieveConfig,
  type SieveAccountStatus,
  type VacationSettings,
} from "../../api/generated";
import { useLabel, useLocale, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useAccountsConfig, useClient } from "../../state/queries";
import {
  CheckField,
  NumberField,
  SelectField,
  TextAreaField,
  TextField,
} from "../../widgets/forms";
import { useRules } from "../rules/api";
import { manageSieveIssues, parseAddressList, vacationIssues } from "./model";
import { DATE_TIME, formatDateTime } from "../../i18n/format";

export const sieveKeys = {
  status: ["sieve", "status"] as const,
  vacation: (accountId: string | null) => ["sieve", "vacation", accountId] as const,
  all: ["sieve"] as const,
};

function useSieveStatus() {
  const client = useClient();
  return useQuery({
    queryKey: sieveKeys.status,
    queryFn: async () =>
      (await client.request({ type: "GetSieveStatus", account_id: null }, "SieveStatus")).accounts,
  });
}

function Messages({ items, part }: { items: readonly LocalizedMessage[]; part: string }) {
  const t = useT();
  if (items.length === 0) return null;
  return (
    <ul data-part={part} role="alert">
      {items.map((m, index) => (
        <li key={`${m.id}-${String(index)}`} data-part={`${part}.item`}>
          {t(m.id, m.args)}
        </li>
      ))}
    </ul>
  );
}

function formatTime(value: string | null, locale: string): string | null {
  if (value === null) return null;
  return formatDateTime(value, locale, DATE_TIME);
}

/** The ManageSieve switch and connection settings of one account. */
function ConnectionEditor({ account }: { account: AccountConfig }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const sieve = account.sieve;
  const [draft, setDraft] = useState<ManageSieveConfig>(
    () => sieve ?? { enabled: true, host: null, port: null, tls: null, script_name: null },
  );
  const [on, setOn] = useState(sieve !== undefined && sieve.enabled !== false);
  const [problems, setProblems] = useState<LocalizedMessage[]>([]);
  const [saved, setSaved] = useState(false);

  const save = async () => {
    const issues = on ? manageSieveIssues(draft) : [];
    if (issues.length > 0) {
      setProblems(issues.map((i) => ({ id: `sieve-issue-${i}` })));
      return;
    }
    setProblems([]);
    const next: AccountConfig = on
      ? { ...account, sieve: { ...draft, enabled: true } }
      : sieve === undefined
        ? account
        : { ...account, sieve: { ...draft, enabled: false } };
    try {
      await client.request({ type: "UpdateAccount", config: next, secrets: [] }, "AccountSaved");
      setSaved(true);
      void queryClient.invalidateQueries({ queryKey: sieveKeys.all });
    } catch (error) {
      setProblems([errorMessageOf(error)]);
    }
  };

  const change = (patch: Partial<ManageSieveConfig>) => {
    setDraft({ ...draft, ...patch });
    setSaved(false);
  };

  return (
    <fieldset data-part="form.section" data-section="managesieve">
      <legend data-part="form.section.title">{t("sieve-connection-title")}</legend>
      <CheckField
        labelId="sieve-enabled"
        name="sieve-enabled"
        checked={on}
        onChange={(value) => {
          setOn(value);
          setSaved(false);
        }}
      />
      {on && (
        <>
          <TextField
            labelId="sieve-host"
            hintId="sieve-host-hint"
            name="sieve-host"
            value={draft.host ?? ""}
            onChange={(host) => {
              change({ host: host === "" ? null : host });
            }}
          />
          <NumberField
            labelId="sieve-port"
            hintId="sieve-port-hint"
            name="sieve-port"
            min={1}
            max={65535}
            value={draft.port}
            onChange={(port) => {
              change({ port });
            }}
          />
          <TextField
            labelId="sieve-script-name"
            hintId="sieve-script-name-hint"
            name="sieve-script-name"
            value={draft.script_name ?? ""}
            onChange={(name) => {
              change({ script_name: name === "" ? null : name });
            }}
          />
        </>
      )}
      <Messages items={problems} part="sieve-status.issues" />
      {saved && (
        <p data-part="sieve-status.notice" role="status">
          {t("sieve-saved")}
        </p>
      )}
      <div data-part="sieve-status.actions">
        <button type="button" data-part="form.button" onClick={() => void save()}>
          {t("sieve-connection-save")}
        </button>
      </div>
    </fieldset>
  );
}

function RulePlacements({ status }: { status: SieveAccountStatus }) {
  const t = useT();
  const rules = useRules();
  const names = new Map((rules.data ?? []).map((r) => [r.id, r.name]));
  if (status.rules.length === 0) {
    return <p data-part="sieve-status.rules-empty">{t("sieve-rules-empty")}</p>;
  }
  return (
    <table data-part="sieve-status.rules">
      <caption data-part="sieve-status.rules.caption">{t("sieve-rules-title")}</caption>
      <thead>
        <tr data-part="sieve-status.rules.head">
          <th scope="col">{t("sieve-rules-rule")}</th>
          <th scope="col">{t("sieve-rules-placement")}</th>
          <th scope="col">{t("sieve-rules-why")}</th>
        </tr>
      </thead>
      <tbody>
        {status.rules.map((rule) => (
          <tr key={rule.rule_id} data-part="sieve-status.rule" data-placement={rule.placement}>
            <th scope="row" data-part="sieve-status.rule.name">
              {names.get(rule.rule_id) ?? rule.rule_id}
            </th>
            <td data-part="sieve-status.rule.placement">
              {t(`sieve-placement-${rule.placement}`)}
            </td>
            <td data-part="sieve-status.rule.reasons">
              {rule.reasons.map((r) => t(r.id, r.args)).join(" ")}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function AccountStatus({ status }: { status: SieveAccountStatus }) {
  const t = useT();
  const locale = useLocale();
  const client = useClient();
  const queryClient = useQueryClient();
  const [script, setScript] = useState<string | null>(null);
  const [problems, setProblems] = useState<LocalizedMessage[]>([]);
  const [busy, setBusy] = useState(false);
  const scriptId = useId();

  const upload = async () => {
    setBusy(true);
    setProblems([]);
    try {
      const answer = await client.request(
        { type: "SyncSieve", account_id: status.account_id },
        "SieveStatus",
      );
      queryClient.setQueryData<SieveAccountStatus[]>(sieveKeys.status, (old = []) => [
        ...old.filter((s) => s.account_id !== status.account_id),
        ...answer.accounts,
      ]);
    } catch (error) {
      setProblems([errorMessageOf(error)]);
    } finally {
      setBusy(false);
    }
  };

  const preview = async () => {
    if (script !== null) {
      setScript(null);
      return;
    }
    setProblems([]);
    try {
      const answer = await client.request(
        { type: "PreviewSieveScript", account_id: status.account_id },
        "SieveScript",
      );
      setScript(answer.script);
    } catch (error) {
      setProblems([errorMessageOf(error)]);
    }
  };

  const failure: LocalizedMessage | null =
    status.localized ?? (status.error === null ? null : { id: errorMessageId(status.error) });
  const uploaded = formatTime(status.uploaded_at, locale);
  return (
    <div data-part="sieve-status.state" data-state={status.state}>
      <p data-part="sieve-status.state.text" role="status">
        {t(`sieve-state-${status.state}`)}
      </p>
      {failure && (
        <p data-part="sieve-status.error" role="alert">
          {t(failure.id, failure.args)}
        </p>
      )}
      <dl data-part="sieve-status.details">
        <dt data-part="sieve-status.term">{t("sieve-script")}</dt>
        <dd data-part="sieve-status.value">{status.script_name}</dd>
        <dt data-part="sieve-status.term">{t("sieve-uploaded")}</dt>
        <dd data-part="sieve-status.value">{uploaded ?? t("sieve-uploaded-never")}</dd>
        {status.extensions.length > 0 && (
          <>
            <dt data-part="sieve-status.term">{t("sieve-extensions")}</dt>
            <dd data-part="sieve-status.value">{status.extensions.join(", ")}</dd>
          </>
        )}
        {status.other_scripts.length > 0 && (
          <>
            <dt data-part="sieve-status.term">{t("sieve-other-scripts")}</dt>
            <dd data-part="sieve-status.value">{status.other_scripts.join(", ")}</dd>
          </>
        )}
        <dt data-part="sieve-status.term">{t("sieve-vacation-state")}</dt>
        <dd data-part="sieve-status.value">
          {t(status.vacation_active ? "sieve-vacation-on" : "sieve-vacation-off")}
        </dd>
      </dl>
      <Messages items={problems} part="sieve-status.issues" />
      <div data-part="sieve-status.actions">
        <button
          type="button"
          data-part="form.button"
          disabled={busy || status.state === "disabled"}
          onClick={() => void upload()}
        >
          {t("sieve-upload")}
        </button>
        <button
          type="button"
          data-part="form.button"
          aria-expanded={script !== null}
          aria-controls={scriptId}
          disabled={status.state === "disabled"}
          onClick={() => void preview()}
        >
          {t(script === null ? "sieve-show-script" : "sieve-hide-script")}
        </button>
      </div>
      {script !== null && (
        <pre
          id={scriptId}
          data-part="sieve-status.script"
          // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- the script scrolls: keyboard users must be able to reach it (axe scrollable-region-focusable).
          tabIndex={0}
          aria-label={t("sieve-script-label")}
        >
          {script}
        </pre>
      )}
      <RulePlacements status={status} />
    </div>
  );
}

/** Settings → Server rules (widget `sieve-status`). */
export function SieveStatusWidget({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const config = useAccountsConfig();
  const statuses = useSieveStatus();
  const imap = (config.data?.accounts ?? []).filter((a) => a.incoming.protocol === "imap");
  const [chosen, setChosen] = useState<string | null>(null);
  const account = imap.find((a) => a.id === chosen) ?? imap[0];
  const status = statuses.data?.find((s) => s.account_id === account?.id);
  return (
    <section
      data-part="sieve-status"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("sieve-status-label")}
    >
      <p data-part="sieve-status.intro">{t("sieve-intro")}</p>
      {imap.length === 0 ? (
        <p data-part="sieve-status.empty">{t("sieve-no-imap")}</p>
      ) : (
        <>
          <SelectField
            labelId="sieve-account"
            name="account"
            value={account?.id ?? ""}
            choices={imap.map((a) => ({ value: a.id, label: a.name, literal: true }))}
            onChange={setChosen}
          />
          {account && (
            <ConnectionEditor
              key={`${account.id}-${JSON.stringify(account.sieve)}`}
              account={account}
            />
          )}
          {statuses.isError && (
            <Messages items={[errorMessageOf(statuses.error)]} part="sieve-status.issues" />
          )}
          {status && <AccountStatus status={status} />}
        </>
      )}
    </section>
  );
}

function VacationForm({ accountId, saved }: { accountId: string; saved: VacationSettings }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const ids = useId();
  const [draft, setDraft] = useState(saved);
  const [addresses, setAddresses] = useState(saved.addresses.join(", "));
  const [problems, setProblems] = useState<LocalizedMessage[]>([]);
  const [notice, setNotice] = useState(false);

  const change = (patch: Partial<VacationSettings>) => {
    setDraft({ ...draft, ...patch });
    setNotice(false);
  };

  const save = async () => {
    const settings = { ...draft, addresses: parseAddressList(addresses) };
    const issues = vacationIssues(settings);
    if (issues.length > 0) {
      setProblems(issues.map((i) => ({ id: VACATION_ISSUE_MESSAGE_IDS[i] })));
      return;
    }
    setProblems([]);
    try {
      const answer = await client.request(
        { type: "UpdateVacation", account_id: accountId, settings },
        "Vacation",
      );
      setDraft(answer.settings);
      setNotice(true);
      queryClient.setQueryData(sieveKeys.vacation(accountId), answer.settings);
      void queryClient.invalidateQueries({ queryKey: sieveKeys.status });
    } catch (error) {
      setProblems([errorMessageOf(error)]);
    }
  };

  const dateField = (key: "start_date" | "end_date", labelId: string) => (
    <div data-part="form.field" data-field={key}>
      <label data-part="form.label" htmlFor={`${ids}-${key}`}>
        {t(labelId)}
      </label>
      <input
        id={`${ids}-${key}`}
        data-part="form.input"
        type="date"
        value={draft[key] ?? ""}
        onChange={(event) => {
          change({ [key]: event.target.value === "" ? null : event.target.value });
        }}
      />
    </div>
  );

  return (
    <div data-part="vacation-editor.form">
      <CheckField
        labelId="vacation-enabled"
        name="vacation-enabled"
        checked={draft.enabled}
        onChange={(enabled) => {
          change({ enabled });
        }}
      />
      <TextField
        labelId="vacation-subject"
        hintId="vacation-subject-hint"
        name="vacation-subject"
        value={draft.subject}
        onChange={(subject) => {
          change({ subject });
        }}
      />
      <TextAreaField
        labelId="vacation-body"
        name="vacation-body"
        value={draft.body}
        onChange={(body) => {
          change({ body });
        }}
      />
      <div data-part="vacation-editor.dates">
        {dateField("start_date", "vacation-start")}
        {dateField("end_date", "vacation-end")}
      </div>
      <NumberField
        labelId="vacation-days"
        hintId="vacation-days-hint"
        name="vacation-days"
        min={1}
        max={365}
        value={draft.days}
        onChange={(days) => {
          change({ days: days ?? 0 });
        }}
      />
      <TextField
        labelId="vacation-addresses"
        hintId="vacation-addresses-hint"
        name="vacation-addresses"
        value={addresses}
        onChange={(value) => {
          setAddresses(value);
          setNotice(false);
        }}
      />
      <TextField
        labelId="vacation-from"
        hintId="vacation-from-hint"
        name="vacation-from"
        type="email"
        value={draft.from ?? ""}
        onChange={(from) => {
          change({ from: from === "" ? null : from });
        }}
      />
      <CheckField
        labelId="vacation-skip-lists"
        name="vacation-skip-lists"
        checked={draft.skip_lists}
        onChange={(skip_lists) => {
          change({ skip_lists });
        }}
      />
      <Messages items={problems} part="vacation-editor.issues" />
      {notice && (
        <p data-part="vacation-editor.notice" role="status">
          {t("vacation-saved")}
        </p>
      )}
      <div data-part="vacation-editor.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          onClick={() => void save()}
        >
          {t("vacation-save")}
        </button>
      </div>
    </div>
  );
}

function Vacation({ account }: { account: AccountConfig }) {
  const t = useT();
  const client = useClient();
  const vacation = useQuery({
    queryKey: sieveKeys.vacation(account.id),
    queryFn: async () =>
      (await client.request({ type: "GetVacation", account_id: account.id }, "Vacation")).settings,
  });
  if (vacation.isError) {
    return <Messages items={[errorMessageOf(vacation.error)]} part="vacation-editor.issues" />;
  }
  if (!vacation.data) return <p data-part="vacation-editor.loading">{t("vacation-loading")}</p>;
  return (
    <>
      {account.incoming.protocol === "exchange" ? (
        <p data-part="vacation-editor.hint">{t("vacation-exchange-hint")}</p>
      ) : account.sieve === undefined || account.sieve.enabled === false ? (
        <p data-part="vacation-editor.hint">{t("vacation-needs-sieve")}</p>
      ) : null}
      <VacationForm key={account.id} accountId={account.id} saved={vacation.data} />
    </>
  );
}

/** Settings → Vacation reply (widget `vacation-editor`). */
export function VacationEditorWidget({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const config = useAccountsConfig();
  // M3c: an Exchange account's vacation reply is its automatic replies.
  const imap = (config.data?.accounts ?? []).filter(
    (a) => a.incoming.protocol === "imap" || a.incoming.protocol === "exchange",
  );
  const [chosen, setChosen] = useState<string | null>(null);
  const account = imap.find((a) => a.id === chosen) ?? imap[0];
  return (
    <section
      data-part="vacation-editor"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("vacation-label")}
    >
      <p data-part="vacation-editor.intro">{t("vacation-intro")}</p>
      {imap.length === 0 ? (
        <p data-part="vacation-editor.empty">{t("sieve-no-imap")}</p>
      ) : (
        <>
          <SelectField
            labelId="vacation-account"
            name="account"
            value={account?.id ?? ""}
            choices={imap.map((a) => ({ value: a.id, label: a.name, literal: true }))}
            onChange={setChosen}
          />
          {account && <Vacation key={account.id} account={account} />}
        </>
      )}
    </section>
  );
}
