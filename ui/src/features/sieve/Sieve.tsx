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
  type Address,
  type LocalizedMessage,
  type ManageSieveConfig,
  type SieveAccountStatus,
  type VacationSettings,
} from "../../api/generated";
import { useLabel, useLocale, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { queryKeys, useAccountsConfig, useClient } from "../../state/queries";
import { CheckField, NumberField, TextAreaField, TextField } from "../../widgets/forms";
import { useLeaveDrafts, useSettingsForm } from "../settings/draft";
import {
  AccountChoiceField,
  NullablePickerField,
  PickerListField,
  identityAddresses,
} from "../../widgets/pickers";
import { manageSieveIssues, vacationIssues } from "./model";
import { DATE_TIME, formatDateTime } from "../../i18n/format";
import { PlacementTable } from "../rules/PlacementTable";

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

/** What the ManageSieve form edits: on / off and the connection settings. */
interface ConnectionDraft {
  on: boolean;
  config: ManageSieveConfig;
}

/** The ManageSieve switch and connection settings of one account. */
function ConnectionEditor({ account }: { account: AccountConfig }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const sieve = account.sieve;
  const [failure, setFailure] = useState<LocalizedMessage | null>(null);
  const [saved, setSaved] = useState(false);
  // R6 (ADR 0239): the edits wait for the settings page's Save.
  const form = useSettingsForm<ConnectionDraft>(
    {
      on: sieve !== undefined && sieve.enabled !== false,
      config: sieve ?? { enabled: true, host: null, port: null, tls: null, script_name: null },
    },
    {
      canSave: ({ on, config }) => !on || manageSieveIssues(config).length === 0,
      save: async ({ on, config }) => {
        const next: AccountConfig = on
          ? { ...account, sieve: { ...config, enabled: true } }
          : sieve === undefined
            ? account
            : { ...account, sieve: { ...config, enabled: false } };
        await client.request({ type: "UpdateAccount", config: next, secrets: [] }, "AccountSaved");
        await queryClient.invalidateQueries({ queryKey: queryKeys.accountsConfig });
        void queryClient.invalidateQueries({ queryKey: sieveKeys.all });
      },
      onSaved: () => {
        setSaved(true);
      },
      onError: (error) => {
        setFailure(errorMessageOf(error));
      },
    },
  );
  const { on, config: draft } = form.value;
  const issues: LocalizedMessage[] = on
    ? manageSieveIssues(draft).map((i) => ({ id: `sieve-issue-${i}` }))
    : [];
  const problems = failure === null ? issues : [...issues, failure];

  const change = (patch: Partial<ConnectionDraft>) => {
    form.set((current) => ({ ...current, ...patch }));
    setSaved(false);
    setFailure(null);
  };
  const changeConfig = (patch: Partial<ManageSieveConfig>) => {
    form.set((current) => ({ ...current, config: { ...current.config, ...patch } }));
    setSaved(false);
    setFailure(null);
  };

  return (
    <fieldset data-part="form.section" data-section="managesieve">
      <legend data-part="form.section.title">{t("sieve-connection-title")}</legend>
      <CheckField
        labelId="sieve-enabled"
        name="sieve-enabled"
        checked={on}
        onChange={(value) => {
          change({ on: value });
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
              changeConfig({ host: host === "" ? null : host });
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
              changeConfig({ port });
            }}
          />
          <TextField
            labelId="sieve-script-name"
            hintId="sieve-script-name-hint"
            name="sieve-script-name"
            value={draft.script_name ?? ""}
            onChange={(name) => {
              changeConfig({ script_name: name === "" ? null : name });
            }}
          />
        </>
      )}
      <Messages items={problems} part="sieve-status.issues" />
      {saved && !form.dirty && (
        <p data-part="sieve-status.notice" role="status">
          {t("common-saved-to-accounts")}
        </p>
      )}
    </fieldset>
  );
}

function RulePlacements({ status }: { status: SieveAccountStatus }) {
  const t = useT();
  return (
    <PlacementTable
      rules={status.rules}
      parts={{
        table: "sieve-status.rules",
        caption: "sieve-status.rules.caption",
        head: "sieve-status.rules.head",
        row: "sieve-status.rule",
        empty: "sieve-status.rules-empty",
      }}
      ids={{
        caption: "sieve-rules-title",
        placement: "sieve-rules-placement",
        empty: "sieve-rules-empty",
      }}
      placementLabel={(placement) => t(`sieve-placement-${placement}`)}
    />
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
        <dd data-part="sieve-status.value">{uploaded ?? t("common-never")}</dd>
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
  const leave = useLeaveDrafts();
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
          <AccountChoiceField
            labelId="sieve-account"
            name="account"
            accounts={imap}
            value={account?.id ?? null}
            onChange={(id) => {
              // R6: another account asks first when this one has unsaved changes.
              leave(() => {
                setChosen(id);
              });
            }}
          />
          {account && <ConnectionEditor key={account.id} account={account} />}
          {statuses.isError && (
            <Messages items={[errorMessageOf(statuses.error)]} part="sieve-status.issues" />
          )}
          {status && <AccountStatus status={status} />}
        </>
      )}
    </section>
  );
}

function VacationForm({
  accountId,
  own,
  saved,
}: {
  accountId: string;
  /** The account's own addresses: the only ones "Reply from" and "Also my addresses" offer. */
  own: readonly Address[];
  saved: VacationSettings;
}) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const ids = useId();
  const [failure, setFailure] = useState<LocalizedMessage | null>(null);
  const [notice, setNotice] = useState(false);
  // R6 (ADR 0239): the edits wait for the settings page's Save.
  const form = useSettingsForm(saved, {
    canSave: (settings) => vacationIssues(settings).length === 0,
    save: async (settings) => {
      const answer = await client.request(
        { type: "UpdateVacation", account_id: accountId, settings },
        "Vacation",
      );
      queryClient.setQueryData(sieveKeys.vacation(accountId), answer.settings);
      void queryClient.invalidateQueries({ queryKey: sieveKeys.status });
    },
    onSaved: () => {
      setNotice(true);
    },
    onError: (error) => {
      setFailure(errorMessageOf(error));
    },
  });
  const draft = form.value;
  const issues: LocalizedMessage[] = vacationIssues(draft).map((i) => ({
    id: VACATION_ISSUE_MESSAGE_IDS[i],
  }));
  const problems = failure === null ? issues : [...issues, failure];

  const change = (patch: Partial<VacationSettings>) => {
    form.set((current) => ({ ...current, ...patch }));
    setNotice(false);
    setFailure(null);
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
      <PickerListField
        kind="address"
        labelId="vacation-addresses"
        hintId="vacation-addresses-hint"
        name="vacation-addresses"
        ownAddresses={own}
        values={draft.addresses}
        onChange={(addresses) => {
          change({ addresses });
        }}
      />
      <NullablePickerField
        kind="address"
        labelId="vacation-from"
        hintId="vacation-from-hint"
        name="vacation-from"
        ownAddresses={own}
        value={draft.from}
        onChange={(from) => {
          change({ from });
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
      <VacationForm
        key={account.id}
        accountId={account.id}
        own={identityAddresses(account.identities)}
        saved={vacation.data}
      />
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
  const leave = useLeaveDrafts();
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
          <AccountChoiceField
            labelId="vacation-account"
            name="account"
            accounts={imap}
            value={account?.id ?? null}
            onChange={(id) => {
              // R6: another account asks first when this one has unsaved changes.
              leave(() => {
                setChosen(id);
              });
            }}
          />
          {account && <Vacation key={account.id} account={account} />}
        </>
      )}
    </section>
  );
}
