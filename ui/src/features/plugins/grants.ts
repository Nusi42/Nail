/**
 * Grants chosen in the install dialog and the plug-in manager (ADR 0050):
 * a subset of the manifest's permissions; the two mail permissions may be
 * limited to accounts (`null` = every account, never empty); reading
 * bodies needs reading mail.
 */
import {
  ACCOUNT_PERMISSIONS,
  REQUIRED_PERMISSIONS,
  type AccountId,
  type PluginGrant,
  type PluginPermission,
} from "../../api/generated";

/** Permissions that can be limited to accounts (`PluginPermission::takes_accounts`, generated). */
export { ACCOUNT_PERMISSIONS } from "../../api/generated";

const REQUIRES: Readonly<Record<string, readonly string[] | undefined>> = REQUIRED_PERMISSIONS;

/** Permissions that need `mail.read` (`PluginPermission::requires`, generated). */
export const NEEDS_READ: readonly PluginPermission[] = Object.keys(REQUIRES).filter((p) =>
  REQUIRES[p]?.includes("mail.read"),
) as PluginPermission[];

export function requiresRead(permission: PluginPermission): boolean {
  return NEEDS_READ.includes(permission);
}

export function takesAccounts(permission: PluginPermission): boolean {
  return (ACCOUNT_PERMISSIONS as readonly PluginPermission[]).includes(permission);
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

/** Grants or denies a permission (denying `mail.read` also denies what needs it). */
export function setGranted(
  grants: readonly PluginGrant[],
  permission: PluginPermission,
  on: boolean,
): PluginGrant[] {
  const rest = grants.filter(
    (g) =>
      g.permission !== permission &&
      !(!on && permission === "mail.read" && requiresRead(g.permission)),
  );
  if (!on) return rest;
  if (requiresRead(permission) && !grantOf(grants, "mail.read")) return [...grants];
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
