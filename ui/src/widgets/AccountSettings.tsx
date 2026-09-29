import { useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { errorMessageIdOf } from "../api/client";
import type { AccountConfig, Identity, SecretInput } from "../api/generated";
import { useLabel, useT } from "../i18n/I18nProvider";
import { type WidgetProps } from "../layout/widgetRegistry";
import { isEmail } from "../state/addresses";
import { queryKeys, useAccountsConfig, useAccountStatuses, useClient } from "../state/queries";
import { useUi } from "../state/uiStore";
import { accountSignIn, secretsFor, SETUP_WINDOW } from "./AccountSetup";
import {
  CheckField,
  IncomingFields,
  NumberField,
  OutgoingFields,
  Section,
  TextAreaField,
  TextField,
} from "./forms";

/** A new, empty identity for an account. */
export function newIdentity(accountId: string): Identity {
  return {
    id: "",
    account_id: accountId,
    email: "",
    name: null,
    reply_to: null,
    bcc: [],
    signature: null,
    composer_mode: null,
  };
}

/** Problems that keep an account from being saved (Fluent ids). */
export function accountProblems(config: AccountConfig): string[] {
  const problems: string[] = [];
  if (!isEmail(config.email)) problems.push("settings-error-email");
  if (config.incoming.server.host === "") problems.push("settings-error-host");
  if (config.outgoing && config.outgoing.server.host === "") problems.push("settings-error-host");
  if (config.identities.length === 0) problems.push("settings-error-no-identity");
  if (config.identities.some((i) => !isEmail(i.email))) problems.push("settings-error-identity");
  return [...new Set(problems)];
}

function IdentityEditor({
  identity,
  index,
  onChange,
  onRemove,
}: {
  identity: Identity;
  index: number;
  onChange: (identity: Identity) => void;
  onRemove: (() => void) | null;
}) {
  const t = useT();
  const signature = identity.signature?.text ?? "";
  return (
    <li data-part="account-settings.identity" data-index={index}>
      <TextField
        labelId="identity-email"
        name={`identity-${index}-email`}
        type="email"
        value={identity.email}
        onChange={(email) => onChange({ ...identity, email: email.trim() })}
      />
      <TextField
        labelId="identity-name"
        name={`identity-${index}-name`}
        value={identity.name ?? ""}
        onChange={(name) => onChange({ ...identity, name: name === "" ? null : name })}
      />
      <TextField
        labelId="identity-reply-to"
        name={`identity-${index}-reply-to`}
        type="email"
        value={identity.reply_to ?? ""}
        onChange={(v) => onChange({ ...identity, reply_to: v.trim() === "" ? null : v.trim() })}
      />
      <TextAreaField
        labelId="identity-signature"
        name={`identity-${index}-signature`}
        value={signature}
        onChange={(text) =>
          onChange({
            ...identity,
            signature: text === "" ? null : { text, html: null, markdown: null },
          })
        }
      />
      {onRemove && (
        <button
          type="button"
          data-part="form.button"
          data-action="remove-identity"
          onClick={onRemove}
        >
          {t("identity-remove")}
        </button>
      )}
    </li>
  );
}

/** A result message shown under the form (Fluent id). */
export type Notice = { id: string; error: boolean } | null;

function NoticeView({ part, notice }: { part: string; notice: Notice }) {
  const t = useT();
  if (!notice) return null;
  return (
    <div
      data-part={part}
      data-state={notice.error ? "error" : "ok"}
      role={notice.error ? "alert" : "status"}
    >
      {t(notice.id)}
    </div>
  );
}

export { NoticeView };

function AccountForm({
  account,
  onRemoved,
  setMessage,
}: {
  account: AccountConfig;
  onRemoved: () => void;
  setMessage: (notice: Notice) => void;
}) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const [config, setConfig] = useState(account);
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const problems = accountProblems(config);

  const refresh = () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.accountsConfig }),
      queryClient.invalidateQueries({ queryKey: queryKeys.accounts }),
      queryClient.invalidateQueries({ queryKey: queryKeys.allIdentities }),
    ]);

  const run = async (work: () => Promise<string>) => {
    setBusy(true);
    setMessage(null);
    try {
      setMessage({ id: await work(), error: false });
    } catch (e) {
      setMessage({ id: errorMessageIdOf(e), error: true });
    } finally {
      setBusy(false);
    }
  };

  const save = (event: FormEvent) => {
    event.preventDefault();
    if (problems.length > 0) return;
    void run(async () => {
      const secrets: SecretInput[] =
        accountSignIn(config).kind === "password" ? secretsFor(config, password, null) : [];
      // New identities are identified by their address (IdentityId rules).
      const identities = config.identities.map((i) => ({
        ...i,
        id: i.id || i.email,
        account_id: config.id,
      }));
      const { config: stored } = await client.request(
        { type: "UpdateAccount", config: { ...config, identities }, secrets },
        "AccountSaved",
      );
      setConfig(stored);
      setPassword("");
      await refresh();
      return "settings-saved";
    });
  };

  const remove = () =>
    run(async () => {
      await client.request({ type: "RemoveAccount", account_id: config.id }, "Done");
      await refresh();
      onRemoved();
      return "settings-removed";
    });

  const setIdentity = (index: number, identity: Identity) =>
    setConfig({
      ...config,
      identities: config.identities.map((old, i) => (i === index ? identity : old)),
    });

  const incoming = config.incoming;
  return (
    <form data-part="account-settings.form" data-account={config.id} noValidate onSubmit={save}>
      <Section titleId="settings-general">
        <TextField
          labelId="settings-name"
          name="name"
          value={config.name}
          onChange={(name) => setConfig({ ...config, name })}
        />
        <TextField
          labelId="settings-email"
          name="email"
          type="email"
          value={config.email}
          onChange={(email) => setConfig({ ...config, email: email.trim() })}
        />
        <CheckField
          labelId="settings-enabled"
          name="enabled"
          checked={config.enabled}
          onChange={(enabled) => setConfig({ ...config, enabled })}
        />
      </Section>
      <IncomingFields
        incoming={incoming}
        onChange={(next) => setConfig({ ...config, incoming: next })}
      />
      {incoming.protocol === "imap" && (
        <NumberField
          labelId="settings-poll-interval"
          name="poll-interval"
          hintId="settings-poll-interval-hint"
          min={30}
          value={incoming.options.poll_interval_secs}
          onChange={(v) =>
            setConfig({
              ...config,
              incoming: { ...incoming, options: { ...incoming.options, poll_interval_secs: v } },
            })
          }
        />
      )}
      {incoming.protocol === "pop3" && (
        <Section titleId="settings-pop3">
          <CheckField
            labelId="settings-leave-on-server"
            name="leave-on-server"
            checked={incoming.options.leave_on_server ?? true}
            onChange={(v) =>
              setConfig({
                ...config,
                incoming: { ...incoming, options: { ...incoming.options, leave_on_server: v } },
              })
            }
          />
          <NumberField
            labelId="settings-delete-after-days"
            name="delete-after-days"
            min={1}
            value={incoming.options.delete_after_days}
            onChange={(v) =>
              setConfig({
                ...config,
                incoming: { ...incoming, options: { ...incoming.options, delete_after_days: v } },
              })
            }
          />
          <CheckField
            labelId="settings-delete-when-deleted"
            name="delete-when-deleted"
            checked={incoming.options.delete_when_deleted_locally ?? false}
            onChange={(v) =>
              setConfig({
                ...config,
                incoming: {
                  ...incoming,
                  options: { ...incoming.options, delete_when_deleted_locally: v },
                },
              })
            }
          />
        </Section>
      )}
      {config.outgoing && (
        <OutgoingFields
          outgoing={config.outgoing}
          onChange={(outgoing) => setConfig({ ...config, outgoing })}
        />
      )}
      {accountSignIn(config).kind === "password" && (
        <TextField
          labelId="settings-new-password"
          name="password"
          type="password"
          hintId="settings-new-password-hint"
          autoComplete="new-password"
          value={password}
          onChange={setPassword}
        />
      )}
      <Section titleId="settings-identities">
        <ul data-part="account-settings.identities">
          {config.identities.map((identity, index) => (
            <IdentityEditor
              key={index}
              identity={identity}
              index={index}
              onChange={(next) => setIdentity(index, next)}
              onRemove={
                config.identities.length > 1
                  ? () =>
                      setConfig({
                        ...config,
                        identities: config.identities.filter((_, i) => i !== index),
                      })
                  : null
              }
            />
          ))}
        </ul>
        <button
          type="button"
          data-part="form.button"
          data-action="add-identity"
          onClick={() =>
            setConfig({ ...config, identities: [...config.identities, newIdentity(config.id)] })
          }
        >
          {t("identity-add")}
        </button>
      </Section>
      {problems.map((problem) => (
        <div key={problem} data-part="account-settings.problem" role="alert">
          {t(problem)}
        </div>
      ))}
      <div data-part="account-settings.actions">
        <button
          type="submit"
          data-part="form.button"
          data-action="save"
          data-variant="primary"
          disabled={busy || problems.length > 0}
        >
          {t("settings-save")}
        </button>
        {!confirmRemove ? (
          <button
            type="button"
            data-part="form.button"
            data-action="remove"
            data-variant="danger"
            disabled={busy}
            onClick={() => setConfirmRemove(true)}
          >
            {t("settings-remove")}
          </button>
        ) : (
          <span data-part="account-settings.confirm" role="group">
            <span data-part="account-settings.confirm.text">
              {t("settings-remove-confirm", { name: config.name })}
            </span>
            <button
              type="button"
              data-part="form.button"
              data-action="remove"
              data-variant="danger"
              disabled={busy}
              onClick={() => void remove()}
            >
              {t("settings-remove-yes")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-action="cancel"
              onClick={() => setConfirmRemove(false)}
            >
              {t("settings-cancel")}
            </button>
          </span>
        )}
      </div>
    </form>
  );
}

/** Real accounts: server settings, sign-in, identities; add and remove accounts. */
export function AccountSettings({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const config = useAccountsConfig();
  const statuses = useAccountStatuses();
  const openWindow = useUi((s) => s.openWindow);
  const [selected, setSelected] = useState<string | null>(null);
  const [notice, setNotice] = useState<Notice>(null);
  const accounts = config.data?.accounts ?? [];
  const current = accounts.find((a) => a.id === selected) ?? accounts[0];

  return (
    <section
      data-part="account-settings"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("settings-accounts-label")}
    >
      <div data-part="account-settings.sidebar">
        <ul data-part="account-settings.list" aria-label={t("settings-accounts-label")}>
          {accounts.map((account) => {
            const status = statuses.data?.find((s) => s.account_id === account.id);
            const active = account.id === current?.id;
            return (
              <li key={account.id} data-part="account-settings.list.item">
                <button
                  type="button"
                  data-part="account-settings.account"
                  data-state={active ? "selected" : undefined}
                  data-status={status?.state}
                  aria-pressed={active}
                  onClick={() => {
                    setSelected(account.id);
                    setNotice(null);
                  }}
                >
                  <span data-part="account-settings.account.name">{account.name}</span>
                  <span data-part="account-settings.account.email">{account.email}</span>
                  {status && (
                    <span data-part="account-settings.account.status" data-state={status.state}>
                      {t(`account-state-${status.state}`)}
                    </span>
                  )}
                </button>
              </li>
            );
          })}
        </ul>
        <button
          type="button"
          data-part="form.button"
          data-action="add-account"
          onClick={() => openWindow(SETUP_WINDOW)}
        >
          {t("settings-add-account")}
        </button>
      </div>
      {config.isError && (
        <div data-part="account-settings.error" role="alert">
          {t(errorMessageIdOf(config.error))}
        </div>
      )}
      {config.isSuccess && accounts.length === 0 && (
        <div data-part="account-settings.empty">{t("settings-no-accounts")}</div>
      )}
      {current && (
        <AccountForm
          key={current.id}
          account={current}
          onRemoved={() => setSelected(null)}
          setMessage={setNotice}
        />
      )}
      <NoticeView part="account-settings.message" notice={notice} />
    </section>
  );
}
