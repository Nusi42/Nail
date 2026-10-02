/**
 * Grants chosen in the install dialog and the plug-in manager (ADR 0050):
 * a subset of the manifest's permissions; the two mail permissions may be
 * limited to accounts (`null` = every account, never empty); reading
 * bodies needs reading mail.
 */
import type { AccountId, PluginGrant, PluginPermission } from "../../api/generated";

/** Permissions that can be limited to accounts (`PluginPermission::takes_accounts`; M4a adds the mail writes, mail hooks and `attachments`). */
export const ACCOUNT_PERMISSIONS: readonly PluginPermission[] = [
  "mail.read",
  "mail.read.body",
  "mail.organize",
  "mail.send",
  "hooks.arrival",
  "hooks.beforeSend",
  "hooks.open",
  "attachments",
];

export function takesAccounts(permission: PluginPermission): boolean {
  return ACCOUNT_PERMISSIONS.includes(permission);
}

/** Every declared permission granted for every account (the install default). */
export function allGranted(permissions: readonly PluginPermission[]): PluginGrant[] {
  return permissions.map((permission) => ({ permission, accounts: null }));
}

export function grantOf(
  grants: readonly PluginGrant[],
  permission: PluginPermission,
): PluginGrant | undefined {
  return grants.find((g) => g.permission === permission);
}

/** Grants or denies a permission (denying `mail.read` also denies bodies). */
export function setGranted(
  grants: readonly PluginGrant[],
  permission: PluginPermission,
  on: boolean,
): PluginGrant[] {
  const rest = grants.filter(
    (g) =>
      g.permission !== permission &&
      !(!on && permission === "mail.read" && g.permission === "mail.read.body"),
  );
  if (!on) return rest;
  if (permission === "mail.read.body" && !grantOf(grants, "mail.read")) return [...grants];
  return [...rest, grantOf(grants, permission) ?? { permission, accounts: null }];
}

/** Limits a mail permission to accounts (`null` = every account). */
export function setAccounts(
  grants: readonly PluginGrant[],
  permission: PluginPermission,
  accounts: AccountId[] | null,
): PluginGrant[] {
  return grants.map((g) => (g.permission === permission ? { ...g, accounts } : g));
}

/** Whether the grants can be sent (no permission limited to no account). */
export function grantsValid(grants: readonly PluginGrant[]): boolean {
  return grants.every((g) => g.accounts === null || g.accounts.length > 0);
}

/** The grants in the manifest's order, for requests and comparisons. */
export function orderedGrants(
  permissions: readonly PluginPermission[],
  grants: readonly PluginGrant[],
): PluginGrant[] {
  return permissions.flatMap((p) => {
    const grant = grantOf(grants, p);
    return grant ? [{ permission: p, accounts: takesAccounts(p) ? grant.accounts : null }] : [];
  });
}
