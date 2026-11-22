/**
 * Exchange inbox rules (ADR 0072, `exchange::rules`): Nail rules are placed
 * as inbox rules where Exchange can express them (`RulePlacement` reused).
 *
 * - mount point `ExchangeRuleBadges`: in the rule list, where one Nail rule
 *   runs for each Exchange account (on the server / in Nail / Nail only),
 *   with the reasons it stays in Nail — one compact ⓘ per rule with the
 *   number of reasons over all accounts; each account's line is in its
 *   hover / focus popover (R3).
 * - widget `exchange-rules` (settings): per Exchange account the rules
 *   state, "Sync now", the "run rules on the server" switch (the account's
 *   `server-rules` option), where each Nail rule runs, and every inbox rule
 *   on the server — the user's own ones can be switched on / off or deleted
 *   (rules with conditions Nail does not model only that), broken ones are
 *   marked. R6 (ADR 0239): the switches are settings drafts (written with
 *   the section's Save); Delete… asks in the shared question window.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useId, useState } from "react";
import { errorMessageId, errorMessageOf } from "../../api/client";
import { RULE_PLACEMENT_VALUES } from "../../api/generated";
import type {
  Account,
  AccountConfig,
  AccountId,
  ExchangeInboxRule,
  ExchangeRulesStatus,
  LocalizedMessage,
  RuleId,
  RulePlacement,
} from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { queryKeys, useAccountsConfig, useClient } from "../../state/queries";
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
import { PlacementTable } from "../rules/PlacementTable";
import { useFormNotice, useSettingsForm } from "../settings/draft";
import { askQuestion } from "../../widgets/dialog";
import { SERVER_RULE_DELETE_DIALOG } from "./names";
import { Icon } from "../../widgets/Icon";

/** One account's line of the badge: where the rule runs there, and why. */
interface AccountLine {
  placement: RulePlacement;
  /** "Contoso: in Nail" and the reasons, localized. */
  text: string;
  reasons: number;
}

/**
 * Placements from the server's to Nail's only (the core declares them in
 * that order): the badge shows the most local one.
 */
const PLACEMENT_ORDER: readonly RulePlacement[] = RULE_PLACEMENT_VALUES;

/**
 * What the badge shows for the accounts' lines: the reasons summed over
 * every account (the number beside the ⓘ) and the most local placement
 * (where its colour comes from).
 */
export function badgeSummary(lines: readonly Omit<AccountLine, "text">[]): {
  total: number;
  placement: RulePlacement;
} {
  return {
    total: lines.reduce((n, l) => n + l.reasons, 0),
    placement: lines.reduce<RulePlacement>(
      (most, l) =>
        PLACEMENT_ORDER.indexOf(l.placement) > PLACEMENT_ORDER.indexOf(most) ? l.placement : most,
      "server",
    ),
  };
}

/**
 * Reads where the rule runs on one account and reports its line (`null`:
 * nothing to show there); renders nothing itself, so the badge can sum
 * every account's reasons.
 */
function AccountProbe({
  account,
  ruleId,
  report,
}: {
  account: Account;
  ruleId: RuleId;
  report: (accountId: AccountId, line: AccountLine | null) => void;
}) {
  const t = useT();
  const rules = useExchangeRules(account.id);
  const status = rules.data?.rules.find((r) => r.rule_id === ruleId);
  const shown = status && rules.data?.state !== "disabled" ? status : null;
  const placement = shown
    ? t(`exchange-placement-${shown.placement}`, { account: account.name })
    : "";
  const reasons = shown ? shown.reasons.map((r) => t(r.id, r.args)).join(" ") : "";
  const text =
    reasons === "" ? placement : t("exchange-placement-with-reasons", { placement, reasons });
  useEffect(() => {
    report(
      account.id,
      shown ? { placement: shown.placement, text, reasons: shown.reasons.length } : null,
    );
  }, [report, account.id, shown, text]);
  useEffect(
    () => () => {
      report(account.id, null);
    },
    [report, account.id],
  );
  return null;
}

/**
 * Mount point `ExchangeRuleBadges` (R3, ADR 0227): one compact ⓘ per rule
 * with the number of reasons the rule stays in Nail, summed over every
 * Exchange account; its popover (hover, and while the rule list's row has
 * the focus) lists each account's line. The popover is positioned over the
 * list, so showing it moves nothing. Inside the list's row (an option, whose
 * content is not interactive) the badge is not a control: the row names its
 * lines as its description. Nothing without Exchange accounts.
 */
export function ExchangeRuleBadges({ ruleId }: { ruleId: RuleId }) {
  const accounts = useExchangeAccounts();
  const [lines, setLines] = useState<Readonly<Record<AccountId, AccountLine>>>({});
  const report = useCallback((accountId: AccountId, line: AccountLine | null) => {
    setLines((all) => {
      const old = all[accountId];
      if (line === null) {
        if (old === undefined) return all;
        return Object.fromEntries(Object.entries(all).filter(([id]) => id !== accountId));
      }
      if (old?.text === line.text && old.placement === line.placement) return all;
      return { ...all, [accountId]: line };
    });
  }, []);
  if (accounts.length === 0) return null;
  const shown = accounts.flatMap((a) => {
    const line = lines[a.id];
    return line ? [{ id: a.id, ...line }] : [];
  });
  const { total, placement } = badgeSummary(shown);
  return (
    <>
      {accounts.map((a) => (
        <AccountProbe key={a.id} account={a} ruleId={ruleId} report={report} />
      ))}
      {shown.length > 0 && (
        <div data-part="exchange.rule-badge" data-placement={placement}>
          <Icon name="info" part="exchange.rule-badge.icon" />
          {total > 0 && (
            <span data-part="exchange.rule-badge.count" aria-hidden="true">
              {total}
            </span>
          )}
          {/* No name of its own: as the row's description, the lines are read as they are. */}
          <ul data-part="exchange.rule-badge.reasons">
            {shown.map((l) => (
              <li key={l.id} data-part="exchange.rule-badge.reason" data-placement={l.placement}>
                {l.text}
              </li>
            ))}
          </ul>
        </div>
      )}
    </>
  );
}

/** The account's `server_rules` option, switched (R6: a settings draft, written with Save). */
function withServerRules(config: AccountConfig, on: boolean): AccountConfig {
  const incoming = config.incoming;
  if (incoming.protocol !== "exchange") return config;
  return {
    ...config,
    incoming: { ...incoming, options: { ...incoming.options, server_rules: on ? null : false } },
  };
}

function ServerSwitch({ config }: { config: AccountConfig }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const incoming = config.incoming;
  const { failed, problems, clear } = useFormNotice();
  const form = useSettingsForm(
    incoming.protocol === "exchange" && incoming.options.server_rules !== false,
    {
      save: async (on) => {
        await client.request(
          { type: "UpdateAccount", config: withServerRules(config, on), secrets: [] },
          "AccountSaved",
        );
        await queryClient.invalidateQueries({ queryKey: queryKeys.accountsConfig });
        void queryClient.invalidateQueries({ queryKey: exchangeKeys.area(config.id, "rules") });
      },
      onError: failed,
    },
  );
  if (incoming.protocol !== "exchange") return null;
  return (
    <>
      <label data-part="form.check" data-field="exchange-server-rules">
        <input
          data-part="form.checkbox"
          type="checkbox"
          checked={form.value}
          onChange={(e) => {
            clear();
            form.set(e.target.checked);
          }}
        />
        <span data-part="form.check.label">{t("exchange-rules-server-switch")}</span>
      </label>
      <Problem message={problems([])[0] ?? null} />
    </>
  );
}

function Placements({ status }: { status: ExchangeRulesStatus }) {
  const t = useT();
  return (
    <PlacementTable
      rules={status.rules}
      parts={{
        table: "exchange-rules.placements",
        caption: "exchange-rules.caption",
        head: "exchange-rules.head",
        row: "exchange-rules.rule",
        empty: "exchange-rules.empty",
      }}
      ids={{
        caption: "exchange-rules-placements",
        placement: "exchange-rules-placement",
        empty: "exchange-rules-none",
      }}
      placementLabel={(placement) => t(`exchange-placement-short-${placement}`)}
    />
  );
}

function ServerRule({ accountId, rule }: { accountId: string; rule: ExchangeInboxRule }) {
  const t = useT();
  const ctx = useWriteContext();
  const { failed, problems, clear } = useFormNotice();
  // On / off waits for the settings page's Save (R6); deleting asks first.
  const enabled = useSettingsForm(rule.enabled, {
    save: (on) => setServerRuleEnabled(ctx, accountId, rule.id, on),
    onError: failed,
  });
  const askDelete = () => {
    askQuestion(ctx.store, SERVER_RULE_DELETE_DIALOG, {
      question: t(
        rule.nail_rule_id
          ? "exchange-rules-delete-placed-question"
          : "exchange-rules-delete-question",
        { name: rule.name },
      ),
      action: t("exchange-rules-delete-rule"),
      run: () => deleteServerRule(ctx, accountId, rule.id),
    });
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
          checked={enabled.value}
          disabled={rule.in_error}
          onChange={(e) => {
            clear();
            enabled.set(e.target.checked);
          }}
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
      <button
        type="button"
        data-part="form.button"
        data-action="delete"
        aria-label={t("exchange-rules-delete-named", { name: rule.name })}
        onClick={askDelete}
      >
        {t("common-delete-ask")}
      </button>
      <Problem message={problems([])[0] ?? null} />
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
        {t("common-loading")}
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
          {t("common-sync-now")}
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
                {t("common-account")}
              </label>
              <select
                id={ids.account}
                data-part="form.select"
                value={account.id}
                onChange={(e) => {
                  setChosen(e.target.value);
                }}
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
