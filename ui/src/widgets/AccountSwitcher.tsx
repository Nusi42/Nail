import { errorMessageIdOf } from "../api/client";
import type { Account } from "../api/generated";
import { useLabel, useT } from "../i18n/I18nProvider";
import { optionBool, type WidgetProps } from "../layout/widgetRegistry";
import { useAccounts, useAccountStatuses } from "../state/queries";
import { useUi } from "../state/uiStore";
import { ALL_ACCOUNTS } from "../api/contract";

/** Display name of an account; the built-in "All accounts" has none of its own. */
export function useAccountName(): (account: Account) => string {
  const t = useT();
  return (account) =>
    account.name !== ""
      ? account.name
      : account.id === ALL_ACCOUNTS
        ? t("account-all")
        : account.id;
}

/**
 * Lists the accounts (real and virtual); clicking one makes it active. Real
 * accounts show their connection state.
 */
export function AccountSwitcher({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const accounts = useAccounts();
  const statuses = useAccountStatuses();
  const activeId = useUi((s) => s.activeAccountId);
  const selectAccount = useUi((s) => s.selectAccount);
  const showStatus = optionBool(node, "show-status", true);
  const nameOf = useAccountName();

  return (
    <div
      data-part="account-switcher"
      data-node-id={node.id ?? undefined}
      role="group"
      aria-label={node.label ? label(node.label) : t("common-accounts")}
    >
      {accounts.isError && (
        <div data-part="account-switcher.error">{t(errorMessageIdOf(accounts.error))}</div>
      )}
      {accounts.data?.map((account) => {
        const selected = account.id === activeId;
        const status = statuses.data?.find((s) => s.account_id === account.id);
        const stateText = status ? t(`account-state-${status.state}`) : null;
        return (
          <button
            key={account.id}
            type="button"
            data-part="account-switcher.item"
            data-state={selected ? "selected" : undefined}
            data-account={account.id}
            data-kind={account.kind}
            data-status={status?.state}
            aria-pressed={selected}
            title={
              stateText && account.email
                ? `${account.email} (${stateText})`
                : (stateText ?? (account.email || undefined))
            }
            onClick={() => {
              selectAccount(account.id);
            }}
          >
            <span data-part="account-switcher.item.name">{nameOf(account)}</span>
            {showStatus && status && stateText && (
              <span
                data-part="account-switcher.item.status"
                data-state={status.state}
                aria-hidden="true"
              />
            )}
          </button>
        );
      })}
    </div>
  );
}
