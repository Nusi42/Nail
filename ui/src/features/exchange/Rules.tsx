/**
 * Exchange inbox rules (ADR 0072, `exchange::rules`): Nail rules are placed
 * as inbox rules where Exchange can express them (`RulePlacement` reused).
 *
 * - mount point `ExchangeRuleBadges`: in the rule list, where one Nail rule
 *   runs for each Exchange account (on the server / in Nail / Nail only),
 *   with the reasons it stays in Nail.
 * - widget `exchange-rules` (settings): per Exchange account the rules
 *   state, "Sync now", the "run rules on the server" switch (the account's
 *   `server-rules` option), where each Nail rule runs, and every inbox rule
 *   on the server — the user's own ones can be switched on / off or deleted
 *   (rules with conditions Nail does not model only that), broken ones are
 *   marked.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { errorMessageId, errorMessageOf } from "../../api/client";
import type {
  Account,
  AccountConfig,
  ExchangeInboxRule,
  ExchangeRulesStatus,
  LocalizedMessage,
  RuleId,
} from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { queryKeys, useAccountsConfig, useClient } from "../../state/queries";
import { useRules } from "../rules/api";
import { formatInstant } from "../pim/fmt";
import { localZone } from "../pim/time";
import { Problem, useWriteContext } from "./common";
import {
  deleteServerRule,
  exchangeKeys,
  setServerRuleEnabled,
  syncRules,
  useExchangeAccounts,
  useExchangeRules,
} from "./queries";

function AccountBadge({ account, ruleId }: { account: Account; ruleId: RuleId }) {
  const t = useT();
  const reasonsId = useId();
  const rules = useExchangeRules(account.id);
  const status = rules.data?.rules.find((r) => r.rule_id === ruleId);
  if (!status || rules.data?.state === "disabled") return null;
  const reasons = status.reasons.map((r) => t(r.id, r.args)).join(" ");
  return (
    <li
      data-part="exchange.rule-badge"
      data-placement={status.placement}
      title={reasons || undefined}
      aria-describedby={reasons ? reasonsId : undefined}
    >
      <span data-part="exchange.rule-badge.text">
        {t(`exchange-placement-${status.placement}`, { account: account.name })}
      </span>
      {reasons && (
        <span id={reasonsId} data-part="exchange.rule-badge.reasons">
          {reasons}
        </span>
      )}
    </li>
  );
}

/**
 * Mount point `ExchangeRuleBadges`: where a Nail rule runs on each
 * Exchange account; nothing without Exchange accounts.
 */
export function ExchangeRuleBadges({ ruleId }: { ruleId: RuleId }) {
  const t = useT();
  const accounts = useExchangeAccounts();
  if (accounts.length === 0) return null;
  return (
    <ul data-part="exchange.rule-badges" aria-label={t("exchange-rule-badges-label")}>
      {accounts.map((a) => (
        <AccountBadge key={a.id} account={a} ruleId={ruleId} />
      ))}
    </ul>
  );
}

function ServerSwitch({ config }: { config: AccountConfig }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const incoming = config.incoming;
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  if (incoming.protocol !== "exchange") return null;
  const on = incoming.options.server_rules !== false;
  const toggle = async (value: boolean) => {
    setProblem(null);
    const next: AccountConfig = {
      ...config,
      incoming: {
        ...incoming,
        options: { ...incoming.options, server_rules: value ? null : false },
      },
    };
    try {
      await client.request({ type: "UpdateAccount", config: next, secrets: [] }, "AccountSaved");
      void queryClient.invalidateQueries({ queryKey: queryKeys.accountsConfig });
      void queryClient.invalidateQueries({ queryKey: exchangeKeys.area(config.id, "rules") });
    } catch (error) {
      setProblem(errorMessageOf(error));
    }
  };
  return (
    <>
      <label data-part="form.check" data-field="exchange-server-rules">
        <input
          data-part="form.checkbox"
          type="checkbox"
          checked={on}
          onChange={(e) => void toggle(e.target.checked)}
        />
        <span data-part="form.check.label">{t("exchange-rules-server-switch")}</span>
      </label>
      <Problem message={problem} />
    </>
  );
}

function Placements({ status }: { status: ExchangeRulesStatus }) {
  const t = useT();
  const rules = useRules();
  const names = new Map((rules.data ?? []).map((r) => [r.id, r.name]));
  if (status.rules.length === 0) {
    return <p data-part="exchange-rules.empty">{t("exchange-rules-none")}</p>;
  }
  return (
    <table data-part="exchange-rules.placements">
      <caption data-part="exchange-rules.caption">{t("exchange-rules-placements")}</caption>
      <thead>
        <tr data-part="exchange-rules.head">
          <th scope="col">{t("exchange-rules-rule")}</th>
          <th scope="col">{t("exchange-rules-placement")}</th>
          <th scope="col">{t("exchange-rules-why")}</th>
        </tr>
      </thead>
      <tbody>
        {status.rules.map((rule) => (
          <tr key={rule.rule_id} data-part="exchange-rules.rule" data-placement={rule.placement}>
            <th scope="row" data-part="exchange-rules.rule.name">
              {names.get(rule.rule_id) ?? rule.rule_id}
            </th>
            <td data-part="exchange-rules.rule.placement">
              {t(`exchange-placement-short-${rule.placement}`)}
            </td>
            <td data-part="exchange-rules.rule.reasons">
              {rule.reasons.map((r) => t(r.id, r.args)).join(" ")}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function ServerRule({ accountId, rule }: { accountId: string; rule: ExchangeInboxRule }) {
  const t = useT();
  const ctx = useWriteContext();
  const [confirm, setConfirm] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const run = async (work: () => Promise<void>) => {
    setProblem(null);
    try {
      await work();
    } catch (error) {
      setProblem(errorMessageOf(error));
    }
  };
  return (
    <li
      data-part="exchange-rules.server.item"
      data-origin={rule.nail_rule_id ? "nail" : "server"}
      data-state={rule.in_error ? "error" : rule.enabled ? "on" : "off"}
    >
      <label data-part="form.check" data-field="enabled">
        <input
          data-part="form.checkbox"
          type="checkbox"
          checked={rule.enabled}
          disabled={rule.in_error}
          onChange={(e) =>
            void run(() => setServerRuleEnabled(ctx, accountId, rule.id, e.target.checked))
          }
        />
        <span data-part="form.check.label">{rule.name}</span>
      </label>
      <span data-part="exchange-rules.server.origin">
        {t(rule.nail_rule_id ? "exchange-rules-from-nail" : "exchange-rules-from-server")}
      </span>
      {rule.has_unsupported && (
        <span data-part="exchange-rules.server.note">{t("exchange-rules-unsupported")}</span>
      )}
      {rule.in_error && (
        <span data-part="exchange-rules.server.note" role="alert">
          {t("exchange-rules-in-error")}
        </span>
      )}
      {confirm ? (
        <span data-part="exchange-rules.server.confirm">
          <span data-part="exchange.question">
            {t(
              rule.nail_rule_id
                ? "exchange-rules-delete-placed-question"
                : "exchange-rules-delete-question",
              { name: rule.name },
            )}
          </span>
          <button type="button" data-part="form.button" onClick={() => setConfirm(false)}>
            {t("exchange-cancel")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-variant="danger"
            onClick={() => void run(() => deleteServerRule(ctx, accountId, rule.id))}
          >
            {t("exchange-rules-delete")}
          </button>
        </span>
      ) : (
        <button
          type="button"
          data-part="form.button"
          data-action="delete"
          aria-label={t("exchange-rules-delete-named", { name: rule.name })}
          onClick={() => setConfirm(true)}
        >
          {t("exchange-rules-delete")}
        </button>
      )}
      <Problem message={problem} />
    </li>
  );
}

function AccountRules({
  account,
  config,
}: {
  account: Account;
  config: AccountConfig | undefined;
}) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useWriteContext();
  const rules = useExchangeRules(account.id);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const status = rules.data;
  if (rules.isError) return <Problem message={errorMessageOf(rules.error)} />;
  if (!status) {
    return (
      <p data-part="exchange.hint" aria-busy="true">
        {t("exchange-loading")}
      </p>
    );
  }
  const failure: LocalizedMessage | null =
    status.localized ?? (status.error === null ? null : { id: errorMessageId(status.error) });
  const sync = async () => {
    setProblem(null);
    try {
      await syncRules(ctx, account.id);
    } catch (error) {
      setProblem(errorMessageOf(error));
    }
  };
  return (
    <div data-part="exchange-rules.account" data-state={status.state}>
      {config && <ServerSwitch config={config} />}
      <p data-part="exchange-rules.state" role="status">
        {t(`exchange-rules-state-${status.state}`)}
        {status.synced_at &&
          ` · ${t("exchange-rules-synced-at", {
            time: formatInstant(format, status.synced_at, localZone(), {
              dateStyle: "medium",
              timeStyle: "short",
            }),
          })}`}
      </p>
      {failure && <Problem message={failure} />}
      <Problem message={problem} />
      <div data-part="exchange.actions">
        <button
          type="button"
          data-part="form.button"
          disabled={status.state === "disabled" || status.state === "syncing"}
          onClick={() => void sync()}
        >
          {t("exchange-rules-sync")}
        </button>
      </div>
      <Placements status={status} />
      <h3 data-part="exchange-rules.server.title">{t("exchange-rules-server-title")}</h3>
      {status.server_rules.length === 0 ? (
        <p data-part="exchange-rules.empty">{t("exchange-rules-server-none")}</p>
      ) : (
        <ul data-part="exchange-rules.server.items">
          {[...status.server_rules]
            .sort((a, b) => a.sequence - b.sequence)
            .map((rule) => (
              <ServerRule key={rule.id} accountId={account.id} rule={rule} />
            ))}
        </ul>
      )}
    </div>
  );
}

/** Widget `exchange-rules`. */
export function ExchangeRulesWidget({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const accounts = useExchangeAccounts();
  const config = useAccountsConfig();
  const [chosen, setChosen] = useState<string | null>(null);
  const ids = { account: useId() };
  const account = accounts.find((a) => a.id === chosen) ?? accounts[0];
  return (
    <section
      data-part="exchange-rules"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("exchange-rules-label")}
    >
      <p data-part="exchange-rules.intro">{t("exchange-rules-intro")}</p>
      {!account ? (
        <p data-part="exchange-rules.empty">{t("exchange-no-accounts")}</p>
      ) : (
        <>
          {accounts.length > 1 && (
            <div data-part="form.field" data-field="account">
              <label data-part="form.label" htmlFor={ids.account}>
                {t("exchange-account")}
              </label>
              <select
                id={ids.account}
                data-part="form.select"
                value={account.id}
                onChange={(e) => setChosen(e.target.value)}
              >
                {accounts.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </select>
            </div>
          )}
          <AccountRules
            key={account.id}
            account={account}
            config={config.data?.accounts.find((a) => a.id === account.id)}
          />
        </>
      )}
    </section>
  );
}
