/**
 * The permission list of the install dialog and the plug-in manager: every
 * permission the manifest declares, each with "Allow" / "Deny", and for the
 * mail permissions a choice of accounts.
 */
import { useId } from "react";
import type { Account, PluginGrant, PluginPermission } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { ALL_ACCOUNTS, useAccountName } from "../../widgets/AccountSwitcher";
import { isWritePermission } from "./extensions";
import { grantOf, requiresRead, setAccounts, setGranted, takesAccounts } from "./grants";
import { permissionArgs, permissionMessageId } from "./messages";

interface Props {
  permissions: readonly PluginPermission[];
  grants: readonly PluginGrant[];
  accounts: readonly Account[];
  onChange: (grants: PluginGrant[]) => void;
  disabled?: boolean;
}

function PermissionRow({
  permission,
  grants,
  accounts,
  onChange,
  disabled,
}: Omit<Props, "permissions"> & { permission: PluginPermission }) {
  const t = useT();
  const name = useId();
  const accountName = useAccountName();
  const grant = grantOf(grants, permission);
  const needsRead = requiresRead(permission) && !grantOf(grants, "mail.read");
  const text = t(permissionMessageId(permission), permissionArgs(permission));
  const write = isWritePermission(permission);
  return (
    <li
      data-part="plugin-permissions.item"
      data-permission={permission}
      data-state={grant ? "allowed" : "denied"}
      data-write={write ? "" : undefined}
    >
      <fieldset data-part="plugin-permissions.choice" disabled={disabled}>
        <legend data-part="plugin-permissions.label">
          {text}
          {write && (
            <span data-part="plugin-permissions.write">{t("plugin-permission-write")}</span>
          )}
        </legend>
        <label data-part="plugin-permissions.option">
          <input
            type="radio"
            name={`${name}-grant`}
            data-grant="allow"
            checked={grant !== undefined}
            disabled={needsRead}
            onChange={() => {
              onChange(setGranted(grants, permission, true));
            }}
          />
          {t("plugin-permission-allow")}
        </label>
        <label data-part="plugin-permissions.option">
          <input
            type="radio"
            name={`${name}-grant`}
            data-grant="deny"
            checked={grant === undefined}
            onChange={() => {
              onChange(setGranted(grants, permission, false));
            }}
          />
          {t("plugin-permission-deny")}
        </label>
        {needsRead && (
          <span data-part="plugin-permissions.hint">{t("plugin-permission-needs-read")}</span>
        )}
      </fieldset>
      {grant && takesAccounts(permission) && accounts.length > 0 && (
        <fieldset data-part="plugin-permissions.accounts" disabled={disabled}>
          <legend data-part="plugin-permissions.accounts.legend">
            {t("plugin-permission-accounts", { permission: text })}
          </legend>
          <label data-part="plugin-permissions.option">
            <input
              type="radio"
              name={`${name}-scope`}
              data-scope="all"
              checked={grant.accounts === null}
              onChange={() => {
                onChange(setAccounts(grants, permission, null));
              }}
            />
            {t("plugin-permission-all-accounts")}
          </label>
          <label data-part="plugin-permissions.option">
            <input
              type="radio"
              name={`${name}-scope`}
              data-scope="some"
              checked={grant.accounts !== null}
              onChange={() => {
                onChange(
                  setAccounts(
                    grants,
                    permission,
                    accounts[0] === undefined ? null : [accounts[0].id],
                  ),
                );
              }}
            />
            {t("plugin-permission-some-accounts")}
          </label>
          {grant.accounts !== null && (
            <ul data-part="plugin-permissions.account-list">
              {accounts.map((account) => {
                const on = grant.accounts?.includes(account.id) ?? false;
                return (
                  <li key={account.id} data-part="plugin-permissions.account">
                    <label data-part="plugin-permissions.option">
                      <input
                        type="checkbox"
                        data-account={account.id}
                        checked={on}
                        onChange={(event) => {
                          const current = grant.accounts ?? [];
                          const next = event.target.checked
                            ? [...current, account.id]
                            : current.filter((a) => a !== account.id);
                          onChange(setAccounts(grants, permission, next));
                        }}
                      />
                      {accountName(account)}
                    </label>
                  </li>
                );
              })}
            </ul>
          )}
          {grant.accounts !== null && grant.accounts.length === 0 && (
            <span data-part="plugin-permissions.error" role="alert">
              {t("plugin-permission-no-accounts")}
            </span>
          )}
        </fieldset>
      )}
    </li>
  );
}

/** The permissions of a plug-in with their grants. */
export function Permissions(props: Props) {
  const t = useT();
  // "All accounts" is what an unlimited grant means already.
  const accounts = props.accounts.filter((a) => a.id !== ALL_ACCOUNTS);
  if (props.permissions.length === 0) {
    return <p data-part="plugin-permissions.none">{t("plugin-permissions-none")}</p>;
  }
  return (
    <ul data-part="plugin-permissions" aria-label={t("plugin-permissions-label")}>
      {props.permissions.map((permission) => (
        <PermissionRow key={permission} {...props} accounts={accounts} permission={permission} />
      ))}
    </ul>
  );
}
