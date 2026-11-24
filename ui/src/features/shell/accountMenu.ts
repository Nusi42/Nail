/**
 * The account menu of the rail's avatar (R8): the accounts to show in the
 * mail view ("All accounts" first when there is one, the shown account
 * marked), then the account settings and "Add account".
 *
 * The accounts are the user's data, so the menu is built here at run time
 * (like the rules submenu), not in `layout.kdl`; its actions are the
 * layout's own (`settings.section "accounts"`, `account.add`) plus
 * `SHOW_ACCOUNT_ACTION`.
 */
import { ALL_ACCOUNTS } from "../../api/contract";
import type {
  Account,
  AccountId,
  JsonValue,
  ResolvedLayout,
  ResolvedMenuEntry,
} from "../../api/generated";
import { usesWidget } from "../../layout/views";
import { literalMenuLabel } from "../menus";

/** Shows an account (argument: its id) in the mail view. */
export const SHOW_ACCOUNT_ACTION = "shell.show-account";

/** The widget that lists the accounts' folders: its view shows an account. */
const ACCOUNTS_WIDGET = "folder-tree";

/** The view `SHOW_ACCOUNT_ACTION` shows: the first whose window lists folders. */
export function accountView(layout: ResolvedLayout): string | null {
  for (const [id, def] of Object.entries(layout.views)) {
    const breakpoints = Object.hasOwn(layout.windows, def.window) ? layout.windows[def.window] : [];
    if ((breakpoints ?? []).some((bp) => usesWidget(bp.root, ACCOUNTS_WIDGET))) return id;
  }
  return null;
}

/**
 * An item of one of the layout's actions: `label` / `icon` when given, else
 * the layout's own.
 */
function layoutAction(
  layout: ResolvedLayout,
  action: string,
  args: JsonValue[],
  look: { label?: string; icon?: string } = {},
): ResolvedMenuEntry {
  const def = Object.hasOwn(layout.actions, action) ? layout.actions[action] : undefined;
  return {
    type: "action",
    action,
    args,
    label: look.label ?? def?.label ?? null,
    icon: look.icon ?? def?.icon ?? null,
    shortcut: null,
  };
}

/** The menu's entries for `accounts` while `active` is shown. */
export function accountMenuItems(
  layout: ResolvedLayout,
  accounts: readonly Account[],
  active: AccountId | null,
): ResolvedMenuEntry[] {
  const entry = (account: Account): ResolvedMenuEntry => ({
    type: "action",
    action: SHOW_ACCOUNT_ACTION,
    args: [account.id],
    label:
      account.id === ALL_ACCOUNTS
        ? "@shell.account-menu.all"
        : literalMenuLabel(account.name || account.email),
    icon: account.id === active ? "check" : null,
    shortcut: null,
  });
  const all = accounts.filter((a) => a.id === ALL_ACCOUNTS);
  const rest = accounts.filter((a) => a.id !== ALL_ACCOUNTS);
  return [
    ...all.map(entry),
    ...rest.map(entry),
    { type: "separator" },
    layoutAction(layout, "settings.section", ["accounts"], {
      label: "@settings.section.accounts",
      icon: "user-round",
    }),
    layoutAction(layout, "account.add", []),
  ];
}
