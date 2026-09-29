import { errorMessageIdOf } from "../api/client";
import { useLabel, useT } from "../i18n/I18nProvider";
import type { WidgetProps } from "../layout/widgetRegistry";
import { useAccounts } from "../state/queries";
import { useUi } from "../state/uiStore";

/** Lists the accounts; clicking one makes it active. */
export function AccountSwitcher({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const accounts = useAccounts();
  const activeId = useUi((s) => s.activeAccountId);
  const selectAccount = useUi((s) => s.selectAccount);

  return (
    <div
      data-part="account-switcher"
      data-node-id={node.id ?? undefined}
      role="group"
      aria-label={node.label ? label(node.label) : t("account-switcher-label")}
    >
      {accounts.isError && (
        <div data-part="account-switcher.error">{t(errorMessageIdOf(accounts.error))}</div>
      )}
      {accounts.data?.map((account) => {
        const selected = account.id === activeId;
        return (
          <button
            key={account.id}
            type="button"
            data-part="account-switcher.item"
            data-state={selected ? "selected" : undefined}
            data-account={account.id}
            data-kind={account.kind}
            aria-pressed={selected}
            title={account.email}
            onClick={() => selectAccount(account.id)}
          >
            {account.name}
          </button>
        );
      })}
    </div>
  );
}
